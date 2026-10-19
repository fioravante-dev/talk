"""Daemon que mantém o Whisper quente entre ditadas, para uso por atalho.

Carregar o large-v3-turbo leva ~4 s; pagar isso a cada tecla seria demais. O
daemon carrega na primeira ditada, mantém o modelo por `whisper_keep_alive` e o
libera quando fica ocioso — em repouso ele não custa VRAM nenhuma.

Não precisa de serviço do systemd: o próprio cliente o sobe quando não encontra
nenhum, herdando o ambiente gráfico de quem chamou.

A ditada tem dois gatilhos, e eles convivem: o atalho de teclado, que chega pelo
socket, e a palavra-chave, que roda numa thread daqui de dentro. Os dois caem no
mesmo `lock`, no mesmo `_capturar` e no mesmo `_processar`.

Protocolo: uma linha JSON por requisição, uma linha JSON de resposta.
  {"cmd": "ping"}
  {"cmd": "dictate", "language": "pt", "to": "type"}
  {"cmd": "stop"}     encerra a gravação em andamento (de qualquer gatilho)
  {"cmd": "wake", "enable": false}   pausa/retoma o ouvinte de palavra-chave
  {"cmd": "quit"}
"""

from __future__ import annotations

import hashlib
import json
import os
import re
import shutil
import socket
import subprocess
import sys
import threading
import time
from dataclasses import replace
from datetime import datetime
from pathlib import Path

from . import audio, config, history, output, stt, voz, wake


def _duracao(segundos: float) -> str:
    """'~2 min' em vez de '~118 s': a unidade certa para decidir se vale ouvir."""
    if segundos < 90:
        return f"{segundos:.0f}s"
    return f"{segundos / 60:.0f} min"


def socket_path() -> Path:
    runtime = os.environ.get("XDG_RUNTIME_DIR") or f"/tmp/talk-{os.getuid()}"
    Path(runtime).mkdir(parents=True, exist_ok=True)
    return Path(runtime) / "talk.sock"


def recordings_dir() -> Path:
    base = os.environ.get("XDG_CACHE_HOME") or (Path.home() / ".cache")
    return Path(base) / "talk" / "recordings"


def _stamp() -> str:
    return datetime.now().strftime("%Y-%m-%d_%H-%M-%S")


def _kept(rec) -> str:
    return f"Áudio preservado em {rec.path}" if rec.path else ""


def _suspicious(text: str, seconds: float) -> bool:
    """Transcrição curta demais para a duração do áudio.

    Entregar com sucesso não é o mesmo que transcrever certo: uma frase de
    quinze segundos que vira "E aí." foi entregue perfeitamente e está errada.
    Nesses casos a gravação precisa sobreviver, senão a evidência do defeito
    some junto. Fala corrida rende ~2 palavras por segundo.
    """
    if seconds < 3.0:
        return False
    return len(re.findall(r"\w+", text)) < seconds * 0.5


def last_recording() -> Path | None:
    wavs = sorted(recordings_dir().glob("*.wav"))
    return wavs[-1] if wavs else None


def prune(keep: int = 20) -> None:
    """Mantém apenas as gravações mais recentes que sobraram de falhas."""
    wavs = sorted(recordings_dir().glob("*.wav"))
    for old in wavs[:-keep] if keep > 0 else wavs:
        old.unlink(missing_ok=True)


def notify(title: str, body: str = "", *, tag: str = "talk") -> None:
    """Feedback visual — disparado por atalho, não há terminal para olhar."""
    if not shutil.which("notify-send"):
        return
    subprocess.run(
        ["notify-send", "-a", "talk", "-h", f"string:x-canonical-private-synchronous:{tag}",
         "-t", "2500", title, body],
        check=False,
    )


class Daemon:
    def __init__(self, cfg: config.Config):
        self.cfg = cfg
        self.recording = threading.Event()  # há uma gravação em andamento
        self.stop_recording = threading.Event()
        self.lock = threading.Lock()
        self.last_used = time.monotonic()
        self.wake_stop = threading.Event()  # pede ao ouvinte que solte o mic
        self.wake_on = threading.Event()  # o ouvinte está de fato escutando
        self.wake_thread: threading.Thread | None = None
        # A leitura em voz alta NÃO usa self.lock: ela não é uma ditada e não
        # pode segurar uma. O que ela faz é ceder — qualquer ditada que comece
        # a interrompe, porque falar por cima da máquina nunca é a intenção.
        self.lendo = threading.Event()
        self.parar_leitura = threading.Event()
        self._leitor = None  # carregado na primeira leitura, como o Whisper
        # Congelado na subida: é o código REALMENTE carregado neste processo.
        # Recalcular a cada ping leria os arquivos já editados no disco, e um
        # daemon obsoleto se declararia atual — que era o defeito original.
        self.code = code_stamp()

    # -- ciclo de vida do modelo -----------------------------------------

    def reap_idle(self) -> None:
        """Devolve a VRAM quando o ditado fica parado.

        O daemon existe para o modelo estar quente quando você aperta a tecla,
        mas segurar 1,2 GB a noite inteira para uma ditada de manhã é caro numa
        placa pequena. Ocioso além do TTL, o modelo sai; a próxima tecla o traz
        de volta em ~4 s.
        """
        ttl = self.cfg.whisper_ttl
        if ttl == float("inf"):
            return
        while True:
            time.sleep(min(max(ttl / 4, 2.0), 30.0))
            if self.recording.is_set() or not stt.is_loaded(motor=self.cfg.stt_motor):
                continue
            if time.monotonic() - self.last_used < ttl:
                continue
            # Não arranque o modelo debaixo de uma transcrição em curso.
            if not self.lock.acquire(blocking=False):
                continue
            try:
                stt.unload(motor=self.cfg.stt_motor)
                print("[talk] ocioso: modelo liberado da VRAM", file=sys.stderr)
            finally:
                self.lock.release()

    # -- comandos ---------------------------------------------------------

    def dictate(self, req: dict) -> dict:
        # Segunda batida no atalho enquanto grava: encerra e deixa o pedido
        # original seguir. É o que dá o comportamento de liga/desliga — e vale
        # também para uma ditada que a palavra-chave abriu, porque quem grava
        # marca o mesmo `recording` e escuta o mesmo `stop_recording`.
        if self.recording.is_set():
            self.stop_recording.set()
            return {"status": "stopping"}

        # Ditar por cima da máquina lendo não é o que ninguém quer — e o
        # microfone captaria o próprio Piper.
        self.calar()

        if not self.lock.acquire(blocking=False):
            return {"status": "busy"}
        try:
            cfg = replace(
                self.cfg,
                language=req.get("language", self.cfg.language),
                to=req.get("to", self.cfg.to),
                paste_key=req.get("paste_key", self.cfg.paste_key),
            )

            rec = self._capturar(cfg)
            if rec.seconds == 0:
                notify("⃠ Nada capturado")
                return {"status": "empty"}
            return self._processar(cfg, rec)
        finally:
            # O relógio de ociosidade conta a partir do fim do uso, não do
            # início: uma ditada longa não pode fazer o modelo vencer sozinha.
            self.last_used = time.monotonic()
            self.lock.release()

    # -- as duas metades, compartilhadas pelos dois gatilhos ---------------

    def _capturar(
        self,
        cfg,
        *,
        source=None,
        silence_secs: float | None = None,
        max_secs: float | None = None,
        wait_speech_secs: float = 0.0,
        min_speech_secs: float = 0.0,
        skip_secs: float = 0.0,
        aquecer: bool = False,
        dica: str = "aperte o atalho de novo para encerrar",
        anunciar_abertura: bool = True,
    ):
        """Grava, marcando o estado que o toggle e o `talk --stop` consultam."""
        label = (cfg.language or "auto").upper()
        if anunciar_abertura:
            notify("⏳ Abrindo o microfone…", tag="talk-live")
        self.recording.set()
        try:
            if aquecer:
                self._aquecer(cfg)
            return audio.record(
                device=cfg.input_device,
                silence_secs=cfg.silence_secs if silence_secs is None else silence_secs,
                silence_threshold=cfg.silence_threshold,
                max_secs=cfg.max_secs if max_secs is None else max_secs,
                wait_speech_secs=wait_speech_secs,
                min_speech_secs=min_speech_secs,
                skip_secs=skip_secs,
                stop_on_enter=False,
                meter=False,
                stop_event=self.stop_recording,
                save_to=recordings_dir() / _stamp(),
                source=source,
                on_live=lambda: notify(
                    f"🎤 Pode falar · {label}", dica, tag="talk-live",
                ),
            )
        finally:
            self.recording.clear()
            self.stop_recording.clear()

    def _processar(self, cfg, rec) -> dict:
        """Transcreve, limpa, entrega, e decide se a gravação sobrevive.

        É tudo o que a tecla e a voz têm em comum depois que o áudio existe.
        O lock e o relógio de ociosidade são de quem chamou, não daqui.
        """
        if stt.is_loaded(motor=cfg.stt_motor):
            notify("✍ Transcrevendo…", f"{rec.seconds:.0f}s")
        else:
            notify("✍ Carregando o modelo…", f"{rec.seconds:.0f}s de áudio guardados")
        result = stt.transcribe(  # a partir daqui, o áudio já está salvo
            rec.audio,
            motor=cfg.stt_motor,
            model=cfg.active_model,
            device=cfg.device,
            compute_type=cfg.compute_type,
            language=cfg.language,
            translate=cfg.translate,
            initial_prompt=cfg.initial_prompt,
            beam_size=cfg.beam_size,
        )
        if not result.text:
            notify("⃠ Nenhuma fala reconhecida", _kept(rec))
            return {"status": "empty", "recording": str(rec.path or "")}

        final = result.text

        try:
            delivered = output.deliver(final, cfg.targets, paste_key=cfg.paste_key)
        except output.OutputError as exc:
            notify("⚠ Falha na saída", f"{exc}\n{_kept(rec)}")
            return {"status": "error", "error": str(exc), "text": final,
                    "recording": str(rec.path or "")}

        history.record(
            final,
            delivered=delivered,
            language=result.language,
            motor=cfg.stt_motor,
            audio_seconds=result.duration,
            stt_seconds=result.elapsed,
        )

        preview = final[:120] + ("…" if len(final) > 120 else "")
        wanted = {"type", "paste"} & set(cfg.targets)
        suspect = _suspicious(final, rec.seconds)

        if wanted and not wanted & set(delivered):
            notify("✓ Copiado (não deu para colar)", "Cole com Ctrl+V — " + preview)
        elif suspect:
            notify("⚠ Saiu curto demais", f"{preview}\n{_kept(rec)} — talk --last")
        else:
            notify("✓ Pronto", preview)

        # O texto chegou ao destino E parece plausível: só agora a gravação
        # é dispensável.
        if not suspect:
            rec.discard()
        prune(keep=cfg.keep_recordings)
        return {"status": "ok", "text": final}

    def _aquecer(self, cfg) -> None:
        """Sobe o Whisper enquanto a pessoa fala.

        No atalho existe um instante entre apertar a tecla e começar a falar. Na
        voz não existe: o disparo e a fala são o mesmo evento, e com o modelo
        frio os ~4 s de carregamento apareceriam inteiros depois que ela
        terminou. Carregar em paralelo à gravação esconde a espera dentro da
        própria fala.
        """
        if stt.is_loaded(motor=cfg.stt_motor):
            return

        def tarefa():
            try:
                stt.warmup(
                    cfg.active_model, cfg.device, cfg.compute_type, motor=cfg.stt_motor
                )
            except Exception as exc:  # aquecer é melhor esforço, nunca fatal
                print(f"[talk] aquecimento falhou: {exc!r}", file=sys.stderr)

        threading.Thread(target=tarefa, daemon=True).start()

    # -- ouvinte de palavra-chave -----------------------------------------

    def escutar(self) -> None:
        """Fica com o microfone aberto, esperando a palavra-chave.

        Um só fluxo de captura serve o ouvinte E a gravação que ele dispara: o
        `record` recebe o MESMO Stream. Abrir um segundo pw-record no instante
        do disparo custaria 200-300 ms, e quem falou "hey jarvis" já emendou a
        frase — esse é exatamente o pedaço que se perderia.
        """
        while not self.wake_stop.is_set():
            try:
                det = wake.Detector(
                    modelo=self.cfg.wake_model,
                    threshold=self.cfg.wake_threshold,
                    patience=self.cfg.wake_patience,
                    cooldown=self.cfg.wake_cooldown,
                )
            except wake.WakeError as exc:
                # Falta de biblioteca ou de modelo não se resolve tentando de
                # novo: desiste e diz por quê, uma vez. O atalho segue inteiro.
                print(f"[talk] palavra-chave desligada: {exc}", file=sys.stderr)
                notify("⚠ Palavra-chave indisponível", str(exc))
                return

            ocupado = False
            try:
                with audio.stream(self.cfg.input_device) as fonte:
                    print(f"[talk] ouvindo '{det.nome}' "
                          f"(limiar {self.cfg.wake_threshold})", file=sys.stderr)
                    self.wake_on.set()
                    while not self.wake_stop.is_set():
                        bloco = fonte.get(timeout=0.5)
                        if bloco is None:
                            if not fonte.vivo:
                                raise audio.AudioError("a captura encerrou sozinha")
                            continue

                        # Enquanto o atalho grava, o microfone é dele. Rodar
                        # inferência aqui só gastaria CPU — o disparo esbarraria
                        # no lock e seria descartado de qualquer jeito. Seguir
                        # consumindo, porém, é obrigatório: é o que impede a
                        # fila de encher e começar a derrubar blocos.
                        if self.recording.is_set():
                            ocupado = True
                            continue
                        if ocupado:
                            ocupado = False
                            det.reset()
                            fonte.drain()
                            continue

                        if not det.disparou(bloco):
                            continue

                        # Enquanto lê, o disparo significa CALAR, não ditar.
                        # Duas razões: interromper por voz é o que importa num
                        # recurso usado sem olhar para a tela; e o alto-falante
                        # alimenta o microfone, então o próprio Piper pode
                        # disparar o detector. Com esta regra essa falha se
                        # autolimita — ao calar, a fonte do disparo some.
                        if self.lendo.is_set():
                            self.parar_leitura.set()
                            det.reset()
                            fonte.drain()
                            continue

                        self._ditar_por_voz(fonte)
                        det.reset()
                        fonte.drain()
            except (audio.AudioError, OSError) as exc:
                print(f"[talk] captura caiu ({exc}); tentando de novo", file=sys.stderr)
            finally:
                self.wake_on.clear()
            # Espera antes de reabrir: um microfone desconectado ou o PipeWire
            # reiniciando não podem virar um laço quente de subprocessos.
            self.wake_stop.wait(2.0)

    def _ditar_por_voz(self, fonte) -> None:
        """Uma ditada inteira, do disparo à entrega, na thread do ouvinte."""
        if not self.lock.acquire(blocking=False):
            return  # já há uma ditada em curso; este disparo se perde
        try:
            cfg = self.cfg
            rec = self._capturar(
                cfg,
                source=fonte,
                silence_secs=cfg.wake_silence_secs,
                max_secs=cfg.wake_max_secs,
                wait_speech_secs=cfg.wake_grace_secs,
                min_speech_secs=cfg.wake_min_speech_secs,
                skip_secs=cfg.wake_skip_secs,
                aquecer=True,
                dica=f"a pausa de {cfg.wake_silence_secs:.0f}s encerra",
                # Com o fluxo já aberto o áudio entra no mesmo instante: um
                # "abrindo o microfone" piscaria e sumiria antes de ser lido.
                anunciar_abertura=False,
            )
            # Disparou e ninguém falou. Sai calado — notificar cada ruído que
            # soa como a palavra-chave é pior do que o ruído. Mas calado na
            # tela, não no log: sem esta linha um descarte indevido fica
            # indistinguível de "não escutou", que foi como ele apareceu.
            if rec.seconds == 0:
                print(
                    f"[talk] disparo descartado: menos de "
                    f"{cfg.wake_min_speech_secs}s de fala em "
                    f"{cfg.wake_grace_secs}s", file=sys.stderr,
                )
                return
            self._processar(cfg, rec)
        except Exception as exc:
            # Aqui não existe o _respond para transformar exceção em resposta:
            # uma exceção solta mataria esta thread, e a palavra-chave pararia
            # de funcionar em silêncio até o próximo reinício do daemon.
            notify("⚠ Erro na ditada por voz", repr(exc))
            print(f"[talk] ditada por voz falhou: {exc!r}", file=sys.stderr)
        finally:
            self.last_used = time.monotonic()
            self.lock.release()

    # -- leitura em voz alta ----------------------------------------------

    def calar(self) -> None:
        """Interrompe a leitura em curso, se houver. Barato e idempotente."""
        if self.lendo.is_set():
            self.parar_leitura.set()

    def ler(self, req: dict) -> dict:
        """Lê em voz alta o que estiver selecionado.

        Segunda batida no atalho enquanto lê: cala. É o mesmo liga/desliga da
        ditada, e o único jeito de descobrir que era o texto errado é ouvir os
        primeiros segundos — então a saída precisa estar a um toque.
        """
        if self.lendo.is_set():
            self.parar_leitura.set()
            return {"status": "stopping"}

        try:
            texto = output.selecao()
        except output.OutputError as exc:
            notify("⚠ Não deu para ler a seleção", str(exc))
            return {"status": "error", "error": str(exc)}

        if not texto:
            # Silêncio aqui faz o atalho parecer quebrado. Ele não está: não há
            # nada destacado.
            notify("⃠ Nada selecionado", "destaque um texto e acione de novo")
            return {"status": "empty"}

        cfg = self.cfg
        lang = req.get("language") or cfg.read_language
        if lang in ("auto", "", None):
            lang = voz.idioma(texto, padrao=(cfg.language or "pt"))
        motor = cfg.voz_motor
        nome = voz.voz_para(motor, lang, cfg.voz_en if lang == "en" else cfg.voz_pt)

        try:
            # O modelo fica carregado entre leituras, como o Whisper: recarregar
            # o Kokoro a cada acionamento custaria mais que a própria fala curta.
            if self._leitor is None or self._leitor.chave != (motor, nome, lang):
                self._leitor = voz.Leitor(
                    motor, nome, lang=lang,
                    velocidade=cfg.voz_velocidade, cadencia=cfg.voz_cadencia,
                )
        except voz.VozError as exc:
            notify("⚠ Leitura indisponível", str(exc))
            return {"status": "error", "error": str(exc)}

        segundos = voz.duracao_estimada(texto, motor)
        previa = texto[:60].replace("\n", " ") + ("…" if len(texto) > 60 else "")
        notify(f"🔊 Lendo · {lang.upper()} · ~{_duracao(segundos)}", previa, tag="talk-read")

        self.parar_leitura.clear()
        self.lendo.set()
        threading.Thread(
            target=self._ler_ate_o_fim, args=(texto,), daemon=True
        ).start()
        return {"status": "ok", "seconds": round(segundos, 1), "language": lang}

    def _ler_ate_o_fim(self, texto: str) -> None:
        try:
            completo = self._leitor.falar(texto, parar=self.parar_leitura)
            if not completo:
                notify("⏹ Leitura interrompida", tag="talk-read")
        except Exception as exc:
            # Sem o _respond aqui: uma exceção solta deixaria `lendo` preso e o
            # atalho pareceria morto até o daemon reiniciar.
            notify("⚠ Erro na leitura", repr(exc))
            print(f"[talk] leitura falhou: {exc!r}", file=sys.stderr)
        finally:
            self.lendo.clear()
            self.parar_leitura.clear()

    def alternar_escuta(self, req: dict) -> dict:
        """Liga e desliga o ouvinte sem reiniciar o daemon.

        Existe por causa do `--wake-test`: sem isto, calibrar o limiar com um
        daemon de pé abriria uma ditada de verdade a cada acerto.
        """
        antes = self.wake_on.is_set()
        querer = req.get("enable")
        if querer is None:
            return {"status": "ok", "wake": antes, "wake_antes": antes}
        if querer:
            self.wake_stop.clear()
            if self.wake_thread is None or not self.wake_thread.is_alive():
                self.wake_thread = threading.Thread(target=self.escutar, daemon=True)
                self.wake_thread.start()
        else:
            self.wake_stop.set()  # o ouvinte solta o microfone em até 0,5 s
        return {"status": "ok", "wake": bool(querer), "wake_antes": antes}

    def handle(self, req: dict) -> dict:
        cmd = req.get("cmd", "dictate")
        if cmd == "ping":
            return {"status": "ok", "recording": self.recording.is_set(),
                    "code": self.code, "wake": self.wake_on.is_set(),
                    "wake_model": self.cfg.wake_model if self.cfg.wake else "",
                    "reading": self.lendo.is_set()}
        if cmd == "stop":
            # Um só comando para as duas coisas: no momento em que você quer
            # parar algo, não é hora de lembrar qual comando era.
            self.stop_recording.set()
            self.calar()
            return {"status": "ok"}
        if cmd == "dictate":
            return self.dictate(req)
        if cmd == "read":
            return self.ler(req)
        if cmd == "wake":
            return self.alternar_escuta(req)
        if cmd == "quit":
            return {"status": "bye"}
        return {"status": "error", "error": f"comando desconhecido: {cmd}"}


def serve(cfg: config.Config) -> int:
    path = socket_path()

    if path.exists():
        if ping(path) is not None:
            print(f"[talk] já há um daemon em {path}", file=sys.stderr)
            return 1
        path.unlink()  # socket órfão de um processo morto

    daemon = Daemon(cfg)
    server = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    try:
        server.bind(str(path))
    except OSError:  # outro daemon ganhou a corrida da criação
        return 1
    path.chmod(0o600)
    server.listen(8)

    # O modelo NÃO sobe aqui. Ele carrega na primeira ditada e sai sozinho
    # depois de ocioso: em repouso o daemon não custa VRAM nenhuma, só o
    # processo. O áudio já está salvo em disco quando o carregamento começa,
    # então a espera atrasa o texto, nunca a fala.
    threading.Thread(target=daemon.reap_idle, daemon=True).start()
    print(f"[talk] pronto em {path} (modelo carrega na primeira ditada)", file=sys.stderr)

    # A palavra-chave é um segundo gatilho: ela sobe aqui, mas o atalho funciona
    # exatamente igual com ela ligada ou desligada.
    if cfg.wake:
        daemon.wake_thread = threading.Thread(target=daemon.escutar, daemon=True)
        daemon.wake_thread.start()

    try:
        while True:
            conn, _ = server.accept()
            with conn:
                try:
                    data = conn.makefile("r").readline()
                    req = json.loads(data) if data.strip() else {}
                except (json.JSONDecodeError, OSError):
                    continue
                # Cada pedido roda em sua própria thread: senão um "stop"
                # nunca chegaria enquanto a gravação segura o loop.
                threading.Thread(
                    target=_respond, args=(daemon, conn.dup(), req), daemon=True
                ).start()
                if req.get("cmd") == "quit":
                    break
    except KeyboardInterrupt:
        pass
    finally:
        server.close()
        path.unlink(missing_ok=True)
    return 0


def _respond(daemon: Daemon, conn: socket.socket, req: dict) -> None:
    with conn:
        try:
            resp = daemon.handle(req)
        except Exception as exc:  # nunca derruba o daemon por causa de um pedido
            # Sem terminal, um erro só existe se for notificado.
            notify("⚠ Erro interno", repr(exc))
            resp = {"status": "error", "error": repr(exc)}
        try:
            conn.sendall((json.dumps(resp, ensure_ascii=False) + "\n").encode())
        except OSError:
            pass


def code_stamp() -> str:
    """Impressão digital do código-fonte carregado.

    O daemon é um processo longo: ele congela o código de quando subiu. Editar
    um módulo e apertar o atalho executa a versão velha, e a falha aparece como
    "não fez nada" — sem terminal, o erro do cliente não é visto por ninguém.
    Comparar isto deixa o cliente reiniciar um daemon obsoleto sozinho.
    """
    raiz = Path(__file__).parent
    marcas = sorted(f"{p.name}:{p.stat().st_mtime_ns}" for p in raiz.glob("*.py"))
    return hashlib.sha256("|".join(marcas).encode()).hexdigest()[:16]


def ensure_running(timeout: float = 15.0) -> bool:
    """Sobe o daemon se não houver um. Devolve True se há daemon respondendo.

    Agora que o daemon não segura VRAM em repouso, ele não precisa de serviço
    do systemd nem de nada instalado: a primeira ditada o traz à vida, e ele
    herda o ambiente gráfico de quem o chamou — que é justamente o que o
    clipboard e as notificações precisam.
    """
    estado = ping()
    if estado is not None:
        if estado.get("code") == code_stamp():
            return True
        # Daemon obsoleto: derruba e sobe de novo com o código atual. Barato,
        # porque parado ele não segura VRAM.
        print("[talk] daemon rodando código antigo; reiniciando.", file=sys.stderr)
        request({"cmd": "quit"}, timeout=5.0)
        for _ in range(50):
            if ping() is None:
                break
            time.sleep(0.1)

    log = recordings_dir().parent / "daemon.log"
    log.parent.mkdir(parents=True, exist_ok=True)
    with log.open("ab") as handle:
        subprocess.Popen(
            [sys.executable, "-m", "talk.cli", "--serve"],
            stdout=handle, stderr=handle, stdin=subprocess.DEVNULL,
            start_new_session=True,  # sobrevive ao fim do cliente
        )

    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if ping() is not None:
            return True
        time.sleep(0.1)
    return False


def request(req: dict, *, path: Path | None = None, timeout: float = 600.0) -> dict | None:
    """Envia um pedido ao daemon. Devolve None se não houver daemon."""
    path = path or socket_path()
    try:
        client = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        client.settimeout(timeout)
        client.connect(str(path))
    except (OSError, socket.timeout):
        return None
    with client:
        client.sendall((json.dumps(req) + "\n").encode())
        line = client.makefile("r").readline()
    return json.loads(line) if line.strip() else None


def ping(path: Path | None = None) -> dict | None:
    return request({"cmd": "ping"}, path=path, timeout=2.0)
