"""Captura de áudio do microfone com parada automática por silêncio.

Backends, nesta ordem: pw-record (PipeWire, já presente em qualquer desktop
moderno) e sounddevice (PortAudio, que exige libportaudio2 instalada). O
primeiro evita uma dependência de sistema, então é o padrão quando existe.
"""

from __future__ import annotations

import _thread
import contextlib
import math
import os
import queue
import select
import shutil
import subprocess
import sys
import threading
import wave
from collections.abc import Callable
from dataclasses import dataclass
from pathlib import Path

import numpy as np

SAMPLE_RATE = 16_000  # o que o Whisper espera
BLOCK = 1280  # 80 ms — o quadro que o openWakeWord espera
_BYTES_PER_BLOCK = BLOCK * 4  # float32
_BLOCK_SECS = BLOCK / SAMPLE_RATE

# Blocos seguidos acima do limiar para valer como fala. Um estalo de teclado ou
# uma batida na mesa passa fácil de 0,015 num bloco isolado — este microfone tem
# mediana 0,006 e picos de 0,031 numa sala em silêncio. Exigir 240 ms contínuos
# é o que separa "alguém falou" de "algo fez barulho".
_FALA_MINIMA = 3


@dataclass
class Recording:
    audio: np.ndarray  # float32 mono em [-1, 1]
    path: Path | None = None  # cópia em disco, se pedida

    @property
    def seconds(self) -> float:
        return len(self.audio) / SAMPLE_RATE

    def discard(self) -> None:
        """Apaga a cópia em disco. Só depois que o texto foi entregue."""
        if self.path:
            self.path.unlink(missing_ok=True)


class _Sink:
    """Despeja o áudio em disco enquanto ele chega.

    A fala é a única coisa insubstituível do processo: transcrição, limpeza e
    entrega podem ser refeitas a partir dela, ela não pode ser refeita a partir
    de nada. Então ela toca o disco antes de qualquer modelo ser carregado.

    Durante a captura o formato é float32 cru — assim um travamento no meio
    deixa um arquivo íntegro, sem cabeçalho para consertar. No fim vira WAV.
    """

    def __init__(self, path: Path):
        path.parent.mkdir(parents=True, exist_ok=True)
        self.raw_path = path.with_suffix(".raw")
        self.wav_path = path.with_suffix(".wav")
        self.handle = self.raw_path.open("wb")

    def write(self, block: np.ndarray) -> None:
        self.handle.write(block.tobytes())

    def finish(self) -> Path | None:
        self.handle.close()
        samples = np.fromfile(self.raw_path, dtype=np.float32)
        if samples.size == 0:
            self.raw_path.unlink(missing_ok=True)
            return None
        with wave.open(str(self.wav_path), "wb") as out:
            out.setnchannels(1)
            out.setsampwidth(2)
            out.setframerate(SAMPLE_RATE)
            out.writeframes((np.clip(samples, -1, 1) * 32767).astype("<i2").tobytes())
        self.raw_path.unlink(missing_ok=True)
        return self.wav_path

    def abort(self) -> None:
        self.handle.close()
        self.raw_path.unlink(missing_ok=True)


class AudioError(RuntimeError):
    pass


def backend() -> str:
    if shutil.which("pw-record"):
        return "pipewire"
    try:
        import sounddevice  # noqa: F401

        return "portaudio"
    except (ImportError, OSError):
        raise AudioError(
            "nenhum backend de áudio disponível — instale pipewire-bin "
            "(pw-record) ou libportaudio2"
        ) from None


# -- produtores de blocos ------------------------------------------------


_AU_MAGIC = b".snd"


def _skip_au_header(stream) -> None:
    """Descarta o cabeçalho AU que o pw-record escreve antes das amostras.

    Apesar de `--format f32`, o pw-record não emite PCM cru: ele escreve um
    arquivo AU (.snd). Ler esse cabeçalho como áudio injeta seis amostras
    falsas, e uma delas é 0xFFFFFFFF — NaN em float32.

    Um único NaN é catastrófico: o Whisper normaliza o espectrograma por
    `log_spec.max()`, então o NaN contamina o máximo e toda a matriz de
    features vira NaN. A transcrição sai como um fragmento sem sentido.
    """
    head = stream.read(8)
    if len(head) < 8:
        return
    # O magic sai invertido porque o campo é escrito em little-endian.
    if head[:4] not in (_AU_MAGIC, _AU_MAGIC[::-1]):
        # Sem cabeçalho: uma versão futura pode emitir PCM cru. Devolve os
        # bytes ao fluxo em vez de descartá-los.
        return head
    offset = int.from_bytes(head[4:8], "little")
    remaining = offset - 8
    if remaining > 0:
        stream.read(remaining)
    return b""


def _put(blocks: queue.Queue, bloco) -> None:
    """Enfileira derrubando o mais velho quando a fila enche.

    A fila só enche quando ninguém está consumindo — o ouvinte de palavra-chave
    parado enquanto o Whisper trabalha. Nesse estado o áudio de trinta
    segundos atrás não serve para nada e o de agora, sim. Sem isto a fila cresce
    sem teto enquanto a ditada é processada.
    """
    try:
        blocks.put_nowait(bloco)
        return
    except queue.Full:
        pass
    try:
        blocks.get_nowait()
    except queue.Empty:
        pass
    with contextlib.suppress(queue.Full):
        blocks.put_nowait(bloco)


def _feed_pipewire(blocks: queue.Queue, stop: threading.Event, device: str | None):
    argv = ["pw-record", "--rate", str(SAMPLE_RATE), "--channels", "1", "--format", "f32"]
    if device:
        argv += ["--target", str(device)]
    argv.append("-")

    proc = subprocess.Popen(argv, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL)
    try:
        pending = _skip_au_header(proc.stdout) or b""
        while not stop.is_set():
            chunk = pending + proc.stdout.read(_BYTES_PER_BLOCK - len(pending))
            pending = b""
            if not chunk or len(chunk) < _BYTES_PER_BLOCK:
                break
            _put(blocks, np.frombuffer(chunk, dtype=np.float32))
    finally:
        proc.terminate()
        try:
            proc.wait(timeout=1)
        except subprocess.TimeoutExpired:
            proc.kill()
        _put(blocks, None)  # sentinela: acabou


def _feed_portaudio(blocks: queue.Queue, stop: threading.Event, device: str | None):
    import sounddevice as sd

    def on_block(indata, _frames, _time, status):
        if status:
            print(f"[audio] {status}", file=sys.stderr)
        _put(blocks, indata[:, 0].copy())

    with sd.InputStream(
        samplerate=SAMPLE_RATE, channels=1, dtype="float32",
        blocksize=BLOCK, device=device, callback=on_block,
    ):
        stop.wait()
    _put(blocks, None)


# -- fluxo de captura ----------------------------------------------------


class Stream:
    """Uma captura viva, entregando blocos por uma fila.

    Existe para que o ouvinte de palavra-chave e a gravação que ele dispara
    sejam a MESMA captura. Subir um segundo pw-record no instante do disparo
    custa 200-300 ms, e é justamente aí que está o começo da frase: quem falou a
    palavra-chave já emendou o resto. Pelo atalho isso não acontece, porque
    entre apertar a tecla e falar existe um instante — por isso `record()`
    continua abrindo a sua própria captura quando ninguém lhe entrega uma.
    """

    def __init__(self, device: str | int | None = None, *, buffer_secs: float = 30.0):
        self.blocks: queue.Queue = queue.Queue(
            maxsize=max(1, int(buffer_secs / _BLOCK_SECS))
        )
        self.stop = threading.Event()
        self.vivo = True
        feed = _feed_pipewire if backend() == "pipewire" else _feed_portaudio
        self.thread = threading.Thread(
            target=feed, args=(self.blocks, self.stop, device), daemon=True
        )
        self.thread.start()

    def get(self, timeout: float = 0.5) -> np.ndarray | None:
        """Próximo bloco, ou None se nada chegou no prazo.

        None não distingue "ainda não veio" de "acabou" de propósito: quase todo
        laço só quer continuar esperando. Quem precisa da diferença olha `vivo`.
        """
        try:
            bloco = self.blocks.get(timeout=timeout)
        except queue.Empty:
            return None
        if bloco is None:  # sentinela do produtor
            self.vivo = False
            return None
        return bloco

    def drain(self) -> None:
        """Descarta o que se acumulou enquanto ninguém consumia.

        Sem isto o ouvinte volta a escutar a fala que ele acabou de ditar — e a
        palavra-chave que a iniciou está no meio dela, então ele redispara.
        """
        while True:
            try:
                if self.blocks.get_nowait() is None:
                    self.vivo = False
                    return
            except queue.Empty:
                return

    def close(self) -> None:
        self.stop.set()
        self.thread.join(timeout=2.0)


@contextlib.contextmanager
def stream(device: str | int | None = None, *, buffer_secs: float = 30.0):
    fonte = Stream(device, buffer_secs=buffer_secs)
    try:
        yield fonte
    finally:
        fonte.close()


# -- medidor -------------------------------------------------------------


class _Meter:
    """Barra de nível no stderr, pra você ver que o mic está pegando."""

    def __init__(self, enabled: bool):
        self.enabled = enabled and sys.stderr.isatty()

    def update(self, rms: float, hint: str) -> None:
        if not self.enabled:
            return
        # dBFS -> 0..20 blocos, com piso em -60 dB
        db = 20 * math.log10(max(rms, 1e-6))
        filled = max(0, min(20, int((db + 60) / 3)))
        bar = "█" * filled + "·" * (20 - filled)
        sys.stderr.write(f"\r  \033[36m{bar}\033[0m  {hint}\033[K")
        sys.stderr.flush()

    def clear(self) -> None:
        if self.enabled:
            sys.stderr.write("\r\033[K")
            sys.stderr.flush()


# -- gravação ------------------------------------------------------------


def record(
    *,
    device: str | int | None = None,
    silence_secs: float = 0.0,
    silence_threshold: float = 0.015,
    max_secs: float = 300.0,
    min_secs: float = 0.4,
    wait_speech_secs: float = 0.0,
    min_speech_secs: float = 0.0,
    skip_secs: float = 0.0,
    stop_on_enter: bool = True,
    meter: bool = True,
    stop_event: threading.Event | None = None,
    save_to: Path | None = None,
    on_live: Callable[[], None] | None = None,
    source: Stream | None = None,
) -> Recording:
    """Grava até você apertar Enter/Espaço, ou até o limite de tempo.

    Com `silence_secs` maior que zero, um silêncio dessa duração também
    encerra — desligado por padrão, porque cortar sozinho no meio de uma
    pausa para pensar é pior do que apertar uma tecla. A ditada aberta por
    palavra-chave é a exceção: lá não há tecla para apertar, e é ela quem
    passa `silence_secs`, `wait_speech_secs`, `min_speech_secs` e `skip_secs`.

    `min_speech_secs` é o quanto de fala precisa ter entrado, somada, antes de
    o corte por silêncio poder agir — e também o que a guarda de falso positivo
    exige para não descartar. Com 0 (o caminho do atalho) nada muda.

    Com `source`, consome uma captura já aberta em vez de abrir a sua — é o
    que faz o ouvinte de palavra-chave e esta gravação serem o mesmo fluxo.
    """
    parar = stop_event if stop_event is not None else threading.Event()
    parar.clear()

    fonte = source if source is not None else Stream(device)
    proprio = source is None

    if stop_on_enter and sys.stdin.isatty():
        threading.Thread(target=_wait_key, args=(parar,), daemon=True).start()
        hint_idle = "fale… (Enter ou Espaço encerra)"
    else:
        hint_idle = "fale…"

    sink = _Sink(save_to) if save_to else None
    collected: list[np.ndarray] = []
    speech_started = False
    mudo = False
    acima = 0
    fala_blocos = 0
    silence_blocks = 0
    silence_needed = int(silence_secs / _BLOCK_SECS)
    max_blocks = int(max_secs / _BLOCK_SECS)
    espera_max = int(wait_speech_secs / _BLOCK_SECS)
    fala_minima = int(min_speech_secs / _BLOCK_SECS)
    pular = int(skip_secs / _BLOCK_SECS)
    ui = _Meter(meter)

    try:
        while len(collected) < max_blocks:
            # Com a captura compartilhada o produtor sobrevive à gravação, então
            # a sentinela dele não encerra mais este laço. Quem manda parar
            # (o atalho de novo, o `talk --stop`) é este teste.
            if parar.is_set():
                break

            block = fonte.get(timeout=0.5)
            if block is None:
                if not fonte.vivo:  # a captura caiu
                    break
                continue

            # Descarta o rabo da palavra-chave, quando ele vaza na transcrição.
            if pular:
                pular -= 1
                continue

            # Só agora o áudio está realmente entrando. Avisar antes disso é o
            # que fazia a pessoa começar a falar contra um stream que ainda não
            # existia, e perder o início da frase.
            if not collected and on_live is not None:
                on_live()

            collected.append(block)
            if sink is not None:
                sink.write(block)

            rms = float(np.sqrt(np.mean(block**2)))
            if rms > silence_threshold:
                acima += 1
                fala_blocos += 1
                if acima >= _FALA_MINIMA:
                    speech_started = True
                silence_blocks = 0
            else:
                acima = 0
                if speech_started:
                    silence_blocks += 1

            if speech_started:
                tail = ""
                if silence_needed and silence_blocks:
                    left = (silence_needed - silence_blocks) * _BLOCK_SECS
                    tail = f"  (corta em {left:.1f}s)"
                ui.update(rms, f"gravando  {len(collected) * _BLOCK_SECS:5.1f}s{tail}")
            else:
                ui.update(rms, hint_idle)

            # O corte por silêncio só arma depois de fala DE VERDADE. Sem o
            # acumulado, o rabo da palavra-chave — 240 ms que sobram na fila
            # quando o detector dispara — já ligava `speech_started`, e a pausa
            # natural de quem espera o "pode falar" estourava os 2 s de silêncio
            # antes da primeira palavra. A gravação saía com a sobra do "jarvis"
            # e nada mais, e era descartada por curta. Pelo atalho isso nunca
            # apareceu porque lá o corte por silêncio vem desligado.
            if (silence_needed and speech_started
                    and fala_blocos >= fala_minima
                    and silence_blocks >= silence_needed):
                break

            # Abriu o microfone e ninguém falou: foi um ruído que soou como a
            # palavra-chave. Mede pelo mesmo acumulado, senão o rabo da palavra
            # desarmaria esta guarda também — e aí não sobra nenhuma saída.
            if espera_max and fala_blocos < fala_minima and len(collected) >= espera_max:
                mudo = True
                break
    finally:
        parar.set()  # solta o _wait_key, que fica em cbreak até ver isto
        if proprio:
            fonte.close()
        ui.clear()

    audio = np.concatenate(collected) if collected else np.zeros(0, dtype=np.float32)

    # Curta demais para ser fala, ou disparo que não virou fala nenhuma: nos
    # dois casos não há o que transcrever, e o arquivo em disco seria só lixo.
    if mudo or len(audio) / SAMPLE_RATE < min_secs:
        if sink is not None:
            sink.abort()
        return Recording(np.zeros(0, dtype=np.float32))

    return Recording(audio, sink.finish() if sink is not None else None)


def _wait_key(stop: threading.Event) -> None:
    """Encerra a gravação com Enter ou Espaço.

    Exige modo cbreak: em modo canônico o terminal só entrega a linha depois
    do Enter, e o Espaço nunca chegaria aqui.
    """
    import termios
    import tty

    fd = sys.stdin.fileno()
    try:
        saved = termios.tcgetattr(fd)
    except termios.error:  # stdin não é um terminal de verdade
        try:
            sys.stdin.readline()
        except Exception:
            pass
        stop.set()
        return

    try:
        tty.setcbreak(fd)
        while not stop.is_set():
            if not select.select([fd], [], [], 0.2)[0]:
                continue
            ch = os.read(fd, 1)
            if ch in (b"\r", b"\n", b" "):
                break
            if ch == b"\x03":  # Ctrl-C não é entregue como sinal em cbreak
                _thread.interrupt_main()
                break
    finally:
        termios.tcsetattr(fd, termios.TCSADRAIN, saved)
    stop.set()


def list_devices() -> str:
    if backend() == "pipewire":
        out = subprocess.run(
            ["pactl", "list", "short", "sources"], capture_output=True, text=True
        )
        return out.stdout or "(nenhuma fonte encontrada)"
    import sounddevice as sd

    return str(sd.query_devices())
