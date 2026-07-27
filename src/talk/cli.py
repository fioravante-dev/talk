"""Interface de linha de comando."""

from __future__ import annotations

import argparse
import json
import sys
import time

from . import __version__, audio, config, daemon, output, polish, stt


def build_parser() -> argparse.ArgumentParser:
    p = argparse.ArgumentParser(
        prog="talk",
        description="Ditado por voz local: grava, transcreve com Whisper, limpa com um LLM.",
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog="""A gravação vai até você apertar Enter ou Espaço (ou o atalho de
teclado de novo, quando disparada por tecla).

exemplos:
  talk                          grava e imprime o texto limpo
  talk -m prompt --to clipboard vira um prompt e copia
  talk -m raw                   sem LLM, só a transcrição
  talk --loop --to type         ditado contínuo direto no app focado
  talk -f reuniao.m4a -m note   transcreve um arquivo e organiza como nota
  talk --silence-secs 1.5       volta a cortar sozinho no silêncio
""",
    )
    p.add_argument("--version", action="version", version=f"talk {__version__}")

    g = p.add_argument_group("entrada")
    g.add_argument("-f", "--file", help="transcreve um arquivo de áudio em vez do microfone")
    g.add_argument("-d", "--input-device", help="dispositivo de captura (nome ou índice)")
    g.add_argument("--silence-secs", type=float,
                   help="segundos de silêncio que encerram a gravação (0 = desligado, padrão)")
    g.add_argument("--max-secs", type=float, help="duração máxima da gravação")
    g.add_argument("--devices", action="store_true", help="lista os dispositivos de áudio e sai")
    g.add_argument("--last", action="store_true",
                   help="reprocessa a última gravação guardada (ditada que falhou)")
    g.add_argument("--recordings", action="store_true",
                   help="lista as gravações guardadas e sai")

    g = p.add_argument_group("transcrição")
    g.add_argument("--model", help="modelo Whisper (padrão: large-v3-turbo)")
    g.add_argument("--device", choices=["cuda", "cpu", "auto"], help="onde rodar o Whisper")
    g.add_argument("--compute-type", help="int8_float16, float16, int8…")
    g.add_argument("-l", "--language", help="idioma da fala; 'auto' para detectar")
    g.add_argument("--vocab", help="termos que o Whisper costuma errar")

    g = p.add_argument_group("pós-processamento")
    g.add_argument(
        "-m", "--mode",
        choices=["raw", "clean", "prompt", "note", "summary"],
        help="raw não usa LLM (padrão: clean)",
    )
    g.add_argument("--llm", help="modelo do Ollama para a limpeza")
    g.add_argument("--system", help="instrução própria, substitui o modo")

    g = p.add_argument_group("saída")
    g.add_argument("--to", help="auto, stdout, clipboard, paste, type — separados por vírgula")
    g.add_argument("--paste-key", help="combinação de colar (padrão ctrl+v; terminais usam ctrl+shift+v)")
    g.add_argument("--json", action="store_true", help="imprime metadados em JSON no stdout")
    g.add_argument("-q", "--quiet", action="store_true", help="sem medidor nem tempos")
    g.add_argument("--loop", action="store_true", help="fica ditando até Ctrl-C")
    g.add_argument("--warmup", action="store_true", help="carrega os modelos e sai")
    g.add_argument("--init", action="store_true", help="escreve um config.toml de exemplo")

    g = p.add_argument_group("daemon (para atalhos de teclado)")
    g.add_argument("--serve", action="store_true", help="mantém o modelo na VRAM e escuta no socket")
    g.add_argument("--no-daemon", action="store_true", help="ignora o daemon mesmo se estiver rodando")
    g.add_argument("--stop", action="store_true", help="encerra a gravação em andamento no daemon")
    g.add_argument("--status", action="store_true", help="diz se o daemon está de pé")
    g.add_argument("--quit", action="store_true", help="desliga o daemon")

    return p


def _merge(cfg: config.Config, args: argparse.Namespace) -> config.Config:
    for name in (
        "model", "device", "compute_type", "language", "vocab", "mode",
        "llm", "to", "paste_key", "input_device", "silence_secs", "max_secs",
    ):
        value = getattr(args, name, None)
        if value is not None:
            setattr(cfg, name, value)
    if cfg.language in ("auto", ""):
        cfg.language = None
    if cfg.device == "auto":
        cfg.device = "cuda"
    return cfg


def _run_once(cfg: config.Config, args: argparse.Namespace) -> int:
    verbose = not args.quiet and not args.json

    if args.file:
        from faster_whisper.audio import decode_audio

        samples = decode_audio(args.file, sampling_rate=audio.SAMPLE_RATE)
    else:
        rec = audio.record(
            device=cfg.input_device,
            silence_secs=cfg.silence_secs,
            silence_threshold=cfg.silence_threshold,
            max_secs=cfg.max_secs,
            meter=verbose,
        )
        if rec.seconds == 0:
            if verbose:
                print("nada capturado.", file=sys.stderr)
            return 1
        samples = rec.audio

    result = stt.transcribe(
        samples,
        model=cfg.model,
        device=cfg.device,
        compute_type=cfg.compute_type,
        language=cfg.language,
        initial_prompt=cfg.initial_prompt,
        beam_size=cfg.beam_size,
    )

    if not result.text:
        if verbose:
            print("nenhuma fala reconhecida.", file=sys.stderr)
        return 1

    started = time.monotonic()
    final = polish.polish(
        result.text,
        mode=cfg.mode,
        model=cfg.llm,
        system=args.system,
        language=result.language,
        keep_alive=cfg.keep_alive,
    )
    polish_elapsed = time.monotonic() - started

    if args.json:
        print(json.dumps({
            "text": final,
            "raw": result.text,
            "mode": cfg.mode,
            "language": result.language,
            "language_probability": round(result.language_probability, 3),
            "audio_seconds": round(result.duration, 2),
            "stt_seconds": round(result.elapsed, 2),
            "polish_seconds": round(polish_elapsed, 2),
        }, ensure_ascii=False))
        targets = [t for t in cfg.targets if t != "stdout"]
    else:
        targets = cfg.targets

    if targets:
        output.deliver(final, targets, paste_key=cfg.paste_key)

    if verbose:
        speed = result.duration / result.elapsed if result.elapsed else 0
        print(
            f"\033[2m  {result.duration:.1f}s de áudio · whisper {result.elapsed:.1f}s "
            f"({speed:.0f}x) · llm {polish_elapsed:.1f}s\033[0m",
            file=sys.stderr,
        )
    return 0


def _run_via_daemon(cfg: config.Config, args: argparse.Namespace) -> int:
    """Delega a ditada ao daemon; o stdout continua sendo do cliente."""
    targets = [t for t in cfg.targets if t != "stdout"]
    resp = daemon.request({
        "cmd": "dictate",
        "language": cfg.language or "auto",
        "mode": cfg.mode,
        "to": ",".join(targets),
        "paste_key": cfg.paste_key,
    })
    if resp is None:
        return -1  # sem daemon; o chamador decide o fallback

    status = resp.get("status")
    if status in ("busy", "stopping"):
        print(f"[talk] daemon {status}", file=sys.stderr)
        return 0
    if status != "ok":
        print(f"[talk] {resp.get('error', status)}", file=sys.stderr)
        # Uma falha de entrega não pode custar a ditada: se o texto voltou,
        # imprima. É a última cópia que existe dele.
        if resp.get("text"):
            print(resp["text"])
        return 1

    text = resp["text"]
    if args.json:
        print(json.dumps({"text": text, "raw": resp.get("raw", ""), "mode": cfg.mode},
                         ensure_ascii=False))
    elif "stdout" in cfg.targets:
        print(text)
    return 0


def main(argv: list[str] | None = None) -> int:
    args = build_parser().parse_args(argv)

    if args.init:
        config.CONFIG_PATH.parent.mkdir(parents=True, exist_ok=True)
        if config.CONFIG_PATH.exists():
            print(f"já existe: {config.CONFIG_PATH}", file=sys.stderr)
            return 1
        config.CONFIG_PATH.write_text(config.DEFAULT_TOML)
        print(f"escrito: {config.CONFIG_PATH}")
        return 0

    if args.devices:
        print(audio.list_devices())
        return 0

    if args.recordings:
        wavs = sorted(daemon.recordings_dir().glob("*.wav"))
        if not wavs:
            print("nenhuma gravação guardada — toda ditada foi entregue com sucesso")
            return 0
        for wav in wavs:
            print(f"{wav}  ({wav.stat().st_size / 32000:.1f}s)")
        return 0

    if args.last:
        wav = daemon.last_recording()
        if wav is None:
            print("nenhuma gravação guardada", file=sys.stderr)
            return 1
        print(f"[talk] reprocessando {wav}", file=sys.stderr)
        args.file = str(wav)

    if args.status:
        state = daemon.ping()
        if state is None:
            print("daemon parado")
            return 1
        print("gravando" if state.get("recording") else "daemon pronto")
        return 0

    if args.stop or args.quit:
        resp = daemon.request({"cmd": "quit" if args.quit else "stop"}, timeout=5.0)
        if resp is None:
            print("daemon parado", file=sys.stderr)
            return 1
        return 0

    cfg = _merge(config.load(), args)

    if args.serve:
        return daemon.serve(cfg)

    if not args.file and not args.loop and not args.warmup and not args.no_daemon:
        # Sobe o daemon na primeira ditada; ele não custa VRAM parado.
        daemon.ensure_running()
        code = _run_via_daemon(cfg, args)
        if code >= 0:
            return code  # o daemon cuidou; senão segue no modo local

    if cfg.mode != "raw" and not args.file and not polish.available():
        print(
            "[talk] Ollama não respondeu — usando modo raw. "
            "Inicie com `ollama serve` ou passe -m raw.",
            file=sys.stderr,
        )
        cfg.mode = "raw"

    if args.warmup or args.loop:
        if not args.quiet:
            print(f"carregando {cfg.model}…", file=sys.stderr)
        stt.warmup(cfg.model, cfg.device, cfg.compute_type)
        if args.warmup:
            return 0

    if not args.loop:
        return _run_once(cfg, args)

    if not args.quiet:
        print("pronto. Ctrl-C encerra.\n", file=sys.stderr)
    try:
        while True:
            _run_once(cfg, args)
    except KeyboardInterrupt:
        print(file=sys.stderr)
        return 0


if __name__ == "__main__":
    sys.exit(main())
