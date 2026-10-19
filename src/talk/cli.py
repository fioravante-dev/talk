"""Interface de linha de comando."""

from __future__ import annotations

import argparse
import json
import sys
from dataclasses import replace

from . import __version__, audio, config, daemon, history, output, stt, voz, wake


def build_parser() -> argparse.ArgumentParser:
    p = argparse.ArgumentParser(
        prog="talk",
        description="Ditado por voz local: grava e transcreve com Whisper.",
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog="""A gravação vai até você apertar Enter ou Espaço (ou o atalho de
teclado de novo, quando disparada por tecla).

exemplos:
  talk                          grava, transcreve e imprime
  talk --to clipboard           copia o texto, sem colar
  talk --loop --to type         ditado contínuo direto no app focado
  talk -f reuniao.m4a           transcreve um arquivo de áudio
  talk --silence-secs 1.5       volta a cortar sozinho no silêncio
  talk --wake-test              calibra a palavra-chave, mostrando os scores
  talk --read                   lê em voz alta o texto que você selecionou
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
    g.add_argument("--stt-motor", choices=["qwen", "whisper"],
                   help="transcription engine (default: whisper)")
    g.add_argument("--translate", action=argparse.BooleanOptionalAction, default=None,
                   help="translate to English (whisper only; qwen raises)")
    g.add_argument("--model", help="Whisper weights (padrão: large-v3-turbo)")
    g.add_argument("--device", choices=["cuda", "cpu", "auto"], help="onde rodar o Whisper")
    g.add_argument("--compute-type", help="int8_float16, float16, int8…")
    g.add_argument("-l", "--language", help="idioma da fala; 'auto' para detectar")
    g.add_argument("--vocab", help="termos que o Whisper costuma errar")

    g = p.add_argument_group("saída")
    g.add_argument("--to", help="auto, stdout, clipboard, paste, type — separados por vírgula")
    g.add_argument("--paste-key", help="combinação de colar (padrão ctrl+v; terminais usam ctrl+shift+v)")
    g.add_argument("--json", action="store_true", help="imprime metadados em JSON no stdout")
    g.add_argument("-q", "--quiet", action="store_true", help="sem medidor nem tempos")
    g.add_argument("--loop", action="store_true", help="fica ditando até Ctrl-C")
    g.add_argument("--download", action="store_true",
                   help="baixa os modelos necessários e sai (não carrega na VRAM)")
    g.add_argument("--warmup", action="store_true", help="carrega o modelo na VRAM e sai")
    g.add_argument("--init", action="store_true", help="escreve um config.toml de exemplo")

    g = p.add_argument_group("daemon (para atalhos de teclado)")
    g.add_argument("--serve", action="store_true", help="mantém o modelo na VRAM e escuta no socket")
    g.add_argument("--no-daemon", action="store_true", help="ignora o daemon mesmo se estiver rodando")
    g.add_argument("--stop", action="store_true", help="encerra a gravação em andamento no daemon")
    g.add_argument("--status", action="store_true", help="diz se o daemon está de pé")
    g.add_argument("--quit", action="store_true", help="desliga o daemon")

    g = p.add_argument_group("palavra-chave (um segundo gatilho, além do atalho)")
    # default=None é obrigatório: _merge só sobrescreve o que não é None, e um
    # store_true comum entregaria False sempre, apagando o wake = true do TOML.
    g.add_argument("--wake", action=argparse.BooleanOptionalAction, default=None,
                   help="liga o ouvinte de palavra-chave no daemon")
    g.add_argument("--wake-model", help="hey_jarvis, alexa, hey_mycroft… ou um .onnx")
    g.add_argument("--wake-threshold", type=float, help="limiar de disparo (0 a 1)")
    g.add_argument("--wake-test", action="store_true",
                   help="mostra os scores ao vivo, para calibrar o limiar")

    g = p.add_argument_group("leitura em voz alta")
    g.add_argument("--read", action="store_true",
                   help="lê o texto selecionado; de novo, cala")
    g.add_argument("--read-language", choices=["auto", "pt", "en"],
                   help="voz a usar; auto escolhe pelo texto (padrão)")

    return p


def _merge(cfg: config.Config, args: argparse.Namespace) -> config.Config:
    """Flags over TOML/env. replace() rebuilds the Config, which re-runs
    __post_init__ — so "auto" from a flag normalizes the same way it does
    coming from the TOML or the environment."""
    overrides = {
        name: value
        for name in (
            "model", "device", "compute_type", "language", "vocab", "to",
            "stt_motor", "translate",
            "paste_key", "input_device", "silence_secs", "max_secs",
            "wake", "wake_model", "wake_threshold", "read_language",
        )
        if (value := getattr(args, name, None)) is not None
    }
    return replace(cfg, **overrides)


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
        if verbose:
            print("nenhuma fala reconhecida.", file=sys.stderr)
        return 1

    final = result.text

    if args.json:
        print(json.dumps({
            "text": final,
            "language": result.language,
            "language_probability": round(result.language_probability, 3),
            "audio_seconds": round(result.duration, 2),
            "stt_seconds": round(result.elapsed, 2),
        }, ensure_ascii=False))
        targets = [t for t in cfg.targets if t != "stdout"]
    else:
        targets = cfg.targets

    if targets:
        delivered = output.deliver(final, targets, paste_key=cfg.paste_key)
        history.record(
            final,
            delivered=delivered,
            language=result.language,
            motor=cfg.stt_motor,
            audio_seconds=result.duration,
            stt_seconds=result.elapsed,
        )

    if verbose:
        speed = result.duration / result.elapsed if result.elapsed else 0
        print(
            f"\033[2m  {result.duration:.1f}s de áudio · whisper {result.elapsed:.1f}s "
            f"({speed:.0f}x)\033[0m",
            file=sys.stderr,
        )
    return 0


def _wake_test(cfg: config.Config) -> int:
    """Mostra o score da palavra-chave ao vivo. É assim que se acha o limiar.

    Fale a palavra-chave umas dez vezes e anote o pico; depois converse, tussa,
    digite, e anote o teto do ruído. O limiar mora entre os dois — mais perto do
    ruído se você prefere repetir, mais perto do pico se prefere não ser
    interrompido no meio de uma conversa.
    """
    try:
        det = wake.Detector(
            modelo=cfg.wake_model, threshold=cfg.wake_threshold,
            patience=cfg.wake_patience, cooldown=cfg.wake_cooldown,
        )
    except wake.WakeError as exc:
        print(f"[talk] {exc}", file=sys.stderr)
        return 1

    # Um daemon ouvindo em paralelo abriria uma ditada de verdade a cada acerto
    # do teste. Cala a boca dele enquanto isso, e devolve como estava no fim.
    pausa = daemon.request({"cmd": "wake", "enable": False}, timeout=2.0)
    print(f"ouvindo '{det.nome}' · limiar {cfg.wake_threshold} · Ctrl-C encerra",
          file=sys.stderr)
    pico = 0.0
    try:
        with audio.stream(cfg.input_device) as fonte:
            while True:
                bloco = fonte.get(timeout=0.5)
                if bloco is None:
                    if not fonte.vivo:
                        break
                    continue
                disparo = det.disparou(bloco)
                pico = max(pico, det.ultimo)
                cheio = min(30, int(det.ultimo * 30))
                barra = "█" * cheio + "·" * (30 - cheio)
                cor = "32" if disparo else ("33" if det.ultimo >= cfg.wake_threshold else "2")
                sys.stderr.write(
                    f"\r  \033[{cor}m{barra}\033[0m {det.ultimo:5.3f}"
                    f"{'  ← DISPAROU' if disparo else ''}\033[K"
                )
                if disparo:
                    sys.stderr.write("\n")
                sys.stderr.flush()
    except KeyboardInterrupt:
        pass
    finally:
        sys.stderr.write(f"\r\033[K  pico observado: {pico:.3f}\n")
        if pausa is not None and pausa.get("wake_antes"):
            daemon.request({"cmd": "wake", "enable": True}, timeout=2.0)
    return 0


def _run_via_daemon(cfg: config.Config, args: argparse.Namespace) -> int:
    """Delega a ditada ao daemon; o stdout continua sendo do cliente."""
    targets = [t for t in cfg.targets if t != "stdout"]
    resp = daemon.request({
        "cmd": "dictate",
        "language": cfg.language or "auto",
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
        print(json.dumps({"text": text}, ensure_ascii=False))
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

    if args.download:
        cfg = _merge(config.load(), args)
        print(f"transcrição · {cfg.active_model}", file=sys.stderr)
        stt.download(cfg.active_model, motor=cfg.stt_motor)
        if cfg.wake:
            print(f"palavra-chave · {cfg.wake_model}", file=sys.stderr)
            try:
                print(f"  {wake.baixar(cfg.wake_model)}", file=sys.stderr)
            except wake.WakeError as exc:
                print(f"[talk] {exc}", file=sys.stderr)
                return 1
        # Só baixa as vozes se o extra estiver instalado: quem não usa leitura
        # não precisa de 121 MB parados no disco.
        if voz.disponivel(cfg.voz_motor):
            print(f"leitura em voz alta · {cfg.voz_motor}", file=sys.stderr)
            try:
                if cfg.voz_motor == "kokoro":
                    # Um modelo só serve as 54 vozes; não há o que escolher aqui.
                    print(f"  {voz.baixar('kokoro')}", file=sys.stderr)
                else:
                    for lang, escolhida in (("pt", cfg.voz_pt), ("en", cfg.voz_en)):
                        nome = voz.voz_para("piper", lang, escolhida)
                        print(f"  {voz.baixar('piper', nome)}", file=sys.stderr)
            except voz.VozError as exc:
                print(f"[talk] {exc}", file=sys.stderr)
                return 1

        print("pronto: os modelos necessários estão no disco.", file=sys.stderr)
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
        if state.get("recording"):
            linha = "gravando"
        elif state.get("reading"):
            linha = "lendo em voz alta"
        else:
            linha = "daemon pronto"
        if state.get("wake"):
            linha += f" · ouvindo '{state.get('wake_model', '')}'"
        print(linha)
        return 0

    if args.stop or args.quit:
        resp = daemon.request({"cmd": "quit" if args.quit else "stop"}, timeout=5.0)
        if resp is None:
            print("daemon parado", file=sys.stderr)
            return 1
        return 0

    cfg = _merge(config.load(), args)

    if args.wake_test:
        return _wake_test(cfg)

    if args.read:
        # Sempre pelo daemon: ele é quem tem o estado da leitura em curso, e
        # sem isso a segunda batida do atalho não teria o que calar.
        daemon.ensure_running()
        resp = daemon.request(
            {"cmd": "read", "language": args.read_language or cfg.read_language},
            timeout=15.0,
        )
        if resp is None:
            print("[talk] daemon não respondeu", file=sys.stderr)
            return 1
        if resp.get("status") == "error":
            print(f"[talk] {resp.get('error')}", file=sys.stderr)
            return 1
        return 0

    if args.serve:
        return daemon.serve(cfg)

    if not args.file and not args.loop and not args.warmup and not args.no_daemon:
        # Sobe o daemon na primeira ditada; ele não custa VRAM parado.
        daemon.ensure_running()
        code = _run_via_daemon(cfg, args)
        if code >= 0:
            return code  # o daemon cuidou; senão segue no modo local

    if args.warmup or args.loop:
        if not args.quiet:
            print(f"carregando {cfg.active_model}…", file=sys.stderr)
        stt.warmup(
            cfg.active_model, cfg.device, cfg.compute_type, motor=cfg.stt_motor
        )
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
