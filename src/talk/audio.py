"""Captura de áudio do microfone com parada automática por silêncio.

Backends, nesta ordem: pw-record (PipeWire, já presente em qualquer desktop
moderno) e sounddevice (PortAudio, que exige libportaudio2 instalada). O
primeiro evita uma dependência de sistema, então é o padrão quando existe.
"""

from __future__ import annotations

import _thread
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
BLOCK = 1600  # 100 ms
_BYTES_PER_BLOCK = BLOCK * 4  # float32


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
            blocks.put(np.frombuffer(chunk, dtype=np.float32))
    finally:
        proc.terminate()
        try:
            proc.wait(timeout=1)
        except subprocess.TimeoutExpired:
            proc.kill()
        blocks.put(None)  # sentinela: acabou


def _feed_portaudio(blocks: queue.Queue, stop: threading.Event, device: str | None):
    import sounddevice as sd

    def on_block(indata, _frames, _time, status):
        if status:
            print(f"[audio] {status}", file=sys.stderr)
        blocks.put(indata[:, 0].copy())

    with sd.InputStream(
        samplerate=SAMPLE_RATE, channels=1, dtype="float32",
        blocksize=BLOCK, device=device, callback=on_block,
    ):
        stop.wait()
    blocks.put(None)


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
    stop_on_enter: bool = True,
    meter: bool = True,
    stop_event: threading.Event | None = None,
    save_to: Path | None = None,
    on_live: Callable[[], None] | None = None,
) -> Recording:
    """Grava até você apertar Enter/Espaço, ou até o limite de tempo.

    Com `silence_secs` maior que zero, um silêncio dessa duração também
    encerra — desligado por padrão, porque cortar sozinho no meio de uma
    pausa para pensar é pior do que apertar uma tecla.
    """
    blocks: queue.Queue = queue.Queue()
    stop = stop_event if stop_event is not None else threading.Event()
    stop.clear()

    feed = _feed_pipewire if backend() == "pipewire" else _feed_portaudio
    producer = threading.Thread(target=feed, args=(blocks, stop, device), daemon=True)
    producer.start()

    if stop_on_enter and sys.stdin.isatty():
        threading.Thread(target=_wait_key, args=(stop,), daemon=True).start()
        hint_idle = "fale… (Enter ou Espaço encerra)"
    else:
        hint_idle = "fale…"

    sink = _Sink(save_to) if save_to else None
    collected: list[np.ndarray] = []
    speech_started = False
    silence_blocks = 0
    silence_needed = int(silence_secs * SAMPLE_RATE / BLOCK)
    max_blocks = int(max_secs * SAMPLE_RATE / BLOCK)
    ui = _Meter(meter)

    try:
        while len(collected) < max_blocks:
            try:
                block = blocks.get(timeout=0.5)
            except queue.Empty:
                if stop.is_set():
                    break
                continue
            if block is None:  # produtor encerrou
                break

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
                speech_started = True
                silence_blocks = 0
            elif speech_started:
                silence_blocks += 1

            if speech_started:
                tail = ""
                if silence_needed and silence_blocks:
                    left = (silence_needed - silence_blocks) * BLOCK / SAMPLE_RATE
                    tail = f"  (corta em {left:.1f}s)"
                ui.update(rms, f"gravando  {len(collected) * 0.1:5.1f}s{tail}")
            else:
                ui.update(rms, hint_idle)

            if silence_needed and speech_started and silence_blocks >= silence_needed:
                break
    finally:
        stop.set()
        ui.clear()

    audio = np.concatenate(collected) if collected else np.zeros(0, dtype=np.float32)

    if len(audio) / SAMPLE_RATE < min_secs:
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
