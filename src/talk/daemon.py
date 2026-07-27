"""Daemon que mantém o Whisper na VRAM, para uso via atalho de teclado.

Carregar o large-v3-turbo leva ~10 s; por tecla isso é inviável. O daemon paga
esse custo uma vez e responde em socket unix. O cliente é um processo curto que
o atalho do sistema dispara.

Protocolo: uma linha JSON por requisição, uma linha JSON de resposta.
  {"cmd": "ping"}
  {"cmd": "dictate", "language": "pt", "mode": "clean", "to": "type"}
  {"cmd": "stop"}     encerra a gravação em andamento
  {"cmd": "quit"}
"""

from __future__ import annotations

import json
import os
import re
import shutil
import socket
import subprocess
import sys
import threading
from dataclasses import replace
from datetime import datetime
from pathlib import Path

from . import audio, config, output, polish, stt


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

    # -- comandos ---------------------------------------------------------

    def dictate(self, req: dict) -> dict:
        # Segunda batida no atalho enquanto grava: encerra e deixa o pedido
        # original seguir. É o que dá o comportamento de liga/desliga.
        if self.recording.is_set():
            self.stop_recording.set()
            return {"status": "stopping"}

        if not self.lock.acquire(blocking=False):
            return {"status": "busy"}
        try:
            cfg = replace(
                self.cfg,
                language=req.get("language", self.cfg.language),
                mode=req.get("mode", self.cfg.mode),
                to=req.get("to", self.cfg.to),
                paste_key=req.get("paste_key", self.cfg.paste_key),
            )
            if cfg.language in ("auto", ""):
                cfg.language = None

            label = (cfg.language or "auto").upper()
            notify("⏳ Abrindo o microfone…", tag="talk-live")
            self.recording.set()
            try:
                rec = audio.record(
                    device=cfg.input_device,
                    silence_secs=cfg.silence_secs,
                    silence_threshold=cfg.silence_threshold,
                    max_secs=cfg.max_secs,
                    stop_on_enter=False,
                    meter=False,
                    stop_event=self.stop_recording,
                    save_to=recordings_dir() / _stamp(),
                    on_live=lambda: notify(
                        f"🎤 Pode falar · {label}", "aperte o atalho de novo para encerrar",
                        tag="talk-live",
                    ),
                )
            finally:
                self.recording.clear()
                self.stop_recording.clear()

            if rec.seconds == 0:
                notify("⃠ Nada capturado")
                return {"status": "empty"}

            notify("✍ Transcrevendo…", f"{rec.seconds:.0f}s")
            result = stt.transcribe(  # a partir daqui, o áudio já está salvo
                rec.audio,
                model=cfg.model,
                device=cfg.device,
                compute_type=cfg.compute_type,
                language=cfg.language,
                initial_prompt=cfg.initial_prompt,
                beam_size=cfg.beam_size,
            )
            if not result.text:
                notify("⃠ Nenhuma fala reconhecida", _kept(rec))
                return {"status": "empty", "recording": str(rec.path or "")}

            final = polish.polish(
                result.text, mode=cfg.mode, model=cfg.llm,
                # o detectado, não o pedido: se você ditou em inglês com -l pt,
                # o texto é inglês e é nele que a limpeza tem de ficar
                language=result.language, keep_alive=cfg.keep_alive,
            )

            try:
                delivered = output.deliver(final, cfg.targets, paste_key=cfg.paste_key)
            except output.OutputError as exc:
                notify("⚠ Falha na saída", f"{exc}\n{_kept(rec)}")
                return {"status": "error", "error": str(exc), "text": final,
                        "recording": str(rec.path or "")}

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
            return {"status": "ok", "text": final, "raw": result.text}
        finally:
            self.lock.release()

    def handle(self, req: dict) -> dict:
        cmd = req.get("cmd", "dictate")
        if cmd == "ping":
            return {"status": "ok", "recording": self.recording.is_set()}
        if cmd == "stop":
            self.stop_recording.set()
            return {"status": "ok"}
        if cmd == "dictate":
            return self.dictate(req)
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

    print(f"[talk] carregando {cfg.model}…", file=sys.stderr)
    stt.warmup(cfg.model, cfg.device, cfg.compute_type)

    daemon = Daemon(cfg)
    server = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    server.bind(str(path))
    path.chmod(0o600)
    server.listen(8)
    print(f"[talk] pronto em {path}", file=sys.stderr)
    notify("talk pronto", f"{cfg.model} na VRAM")

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
            resp = {"status": "error", "error": repr(exc)}
        try:
            conn.sendall((json.dumps(resp, ensure_ascii=False) + "\n").encode())
        except OSError:
            pass


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
