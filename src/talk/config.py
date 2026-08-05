"""Configuração: padrões < arquivo TOML < variáveis de ambiente < flags."""

from __future__ import annotations

import os
import re
import sys
import tomllib
from dataclasses import dataclass, fields
from pathlib import Path

CONFIG_PATH = Path(
    os.environ.get("XDG_CONFIG_HOME", Path.home() / ".config")
) / "talk" / "config.toml"


@dataclass
class Config:
    # transcrição
    model: str = "large-v3-turbo"
    device: str = "cuda"
    compute_type: str = "int8_float16"
    language: str | None = "pt"
    beam_size: int = 5
    # vocabulário que o Whisper tende a errar: nomes próprios, siglas, jargão
    vocab: str = ""

    # pós-processamento
    mode: str = "clean"
    llm: str = "qwen3:4b-instruct"
    # quanto tempo o LLM fica na VRAM depois de usado (repassado ao Ollama)
    keep_alive: str = "5m"
    # quanto tempo o Whisper fica na VRAM sem uso, antes de o daemon liberá-lo.
    # "never" o mantém para sempre; "0" libera logo após cada ditada.
    whisper_keep_alive: str = "5m"

    # áudio
    input_device: str | None = None
    # 0 desliga o corte por silêncio: a gravação vai até você mandar parar
    silence_secs: float = 0.0
    silence_threshold: float = 0.015
    max_secs: float = 300.0

    # palavra-chave (wake word) — desligada por padrão.
    # Ela SOMA ao atalho de teclado, não o substitui: os dois gatilhos caem no
    # mesmo daemon e no mesmo processamento.
    wake: bool = False
    wake_model: str = "hey_jarvis"
    wake_threshold: float = 0.5
    wake_patience: int = 2  # quadros de 80 ms seguidos acima do limiar
    wake_cooldown: float = 3.0  # tempo mudo do detector depois de disparar
    # A ditada aberta por voz não tem tecla para encerrar, então ela para
    # sozinha no silêncio. Estes campos são separados dos de cima de propósito:
    # o caminho do atalho continua em silence_secs = 0, sem corte automático.
    wake_silence_secs: float = 2.0
    wake_max_secs: float = 60.0
    # Fala somada que precisa entrar antes de o corte por silêncio poder agir.
    # Existe porque o rabo da palavra-chave, sozinho, não pode contar como fala:
    # senão a pausa de quem espera o "pode falar" encerra a ditada antes da
    # primeira palavra.
    wake_min_speech_secs: float = 0.5
    # Sem essa fala mínima dentro deste prazo, foi ruído: desiste calado.
    wake_grace_secs: float = 5.0
    wake_skip_secs: float = 0.0  # suba se o rabo da palavra vazar na transcrição

    # leitura em voz alta — o caminho inverso do resto do projeto.
    # Lê o que você SELECIONOU, não a tela. O Piper roda na CPU, então isto não
    # disputa VRAM com o Whisper nem com o LLM.
    # kokoro soa mais natural; piper é o leve (61 MB por voz contra 347 MB, e
    # ~25x o tempo real contra ~4,5x) para máquina fraca.
    voz_motor: str = "kokoro"
    # Vazio usa a voz padrão do motor para o idioma — assim trocar de motor não
    # obriga a trocar os nomes, que não têm nada em comum entre os catálogos.
    voz_pt: str = ""
    voz_en: str = ""
    voz_velocidade: float = 1.0  # 1.5 fala mais rápido; 0.8 mais devagar
    voz_cadencia: float = 1.2  # só piper: variação de duração dos fonemas
    read_language: str = "auto"  # auto detecta pelo texto; ou "pt"/"en" fixo

    # saída
    to: str = "auto"
    # Combinação de colar. Terminais costumam usar ctrl+shift+v.
    paste_key: str = "ctrl+v"
    # Quantas gravações de ditadas que falharam guardar em ~/.cache/talk
    keep_recordings: int = 20

    @property
    def initial_prompt(self) -> str | None:
        return self.vocab or None

    @property
    def whisper_ttl(self) -> float:
        return parse_duration(self.whisper_keep_alive)

    @property
    def targets(self) -> list[str]:
        """Resolve 'auto': num terminal, imprime; disparado por tecla de
        atalho (sem TTY), cola onde o cursor estiver."""
        out = []
        for t in (t.strip() for t in self.to.split(",")):
            if t == "auto":
                t = "stdout" if sys.stdout.isatty() else "paste"
            if t:
                out.append(t)
        return out


_DURATION = re.compile(r"^\s*(\d+(?:\.\d+)?)\s*([smh]?)\s*$")
_UNIT = {"": 1, "s": 1, "m": 60, "h": 3600}


def parse_duration(value: str | float) -> float:
    """'5m' → 300 · '30s' → 30 · '0' → 0 · 'never'/'-1' → infinito."""
    text = str(value).strip().lower()
    if text in ("never", "-1", "forever", "inf"):
        return float("inf")
    match = _DURATION.match(text)
    if not match:
        raise ValueError(f"duração inválida: {value!r} (use 30s, 5m, 1h, 0 ou never)")
    return float(match.group(1)) * _UNIT[match.group(2)]


def _bool(texto: str) -> bool:
    """Coerção de bool vinda do ambiente.

    Sem isto `TALK_WAKE=0` chegaria como a string "0", que é verdadeira em
    Python — a variável ligaria justamente o que ela pede para desligar.
    """
    return texto.strip().lower() not in ("", "0", "false", "no", "off", "nao", "não")


def load() -> Config:
    cfg = Config()

    if CONFIG_PATH.is_file():
        data = tomllib.loads(CONFIG_PATH.read_text())
        known = {f.name for f in fields(Config)}
        for key, value in data.items():
            if key in known:
                setattr(cfg, key, value)

    for f in fields(Config):
        env = os.environ.get(f"TALK_{f.name.upper()}")
        if env is None:
            continue
        if f.type in ("float", "float | None"):
            setattr(cfg, f.name, float(env))
        elif f.type == "bool":
            setattr(cfg, f.name, _bool(env))
        elif f.type in ("int", "int | None"):
            setattr(cfg, f.name, int(env))
        else:
            setattr(cfg, f.name, env)

    return cfg


DEFAULT_TOML = """\
# ~/.config/talk/config.toml

model = "large-v3-turbo"   # tiny, base, small, medium, large-v3, large-v3-turbo
device = "cuda"
compute_type = "int8_float16"
language = "pt"            # remova para detecção automática

mode = "clean"             # raw, clean, prompt, note, summary
llm = "qwen3:4b-instruct"
keep_alive = "5m"          # quanto tempo o LLM fica na VRAM depois do uso
whisper_keep_alive = "5m"  # ocioso por esse tempo, o Whisper sai da VRAM
                           # ("never" mantém sempre; "0" libera após cada ditada)

# 0 = a gravação só para quando você mandar (Enter/Espaço, ou o atalho de novo).
# Ponha 1.5, por exemplo, para voltar a cortar sozinho após 1,5 s de silêncio.
silence_secs = 0.0
silence_threshold = 0.015

# --- palavra-chave ---------------------------------------------------------
# Um segundo gatilho, que SOMA ao atalho de teclado em vez de substituí-lo.
# Com wake = true o daemon fica com o microfone aberto o tempo todo, ouvindo só
# pela palavra-chave (~3 % de um núcleo). Exige `uv sync --extra wake` no
# diretório do projeto e depois `talk --download`.
# Calibre o limiar com `talk --wake-test`.
wake = false
wake_model = "hey_jarvis"  # hey_jarvis, alexa, hey_mycroft, hey_rhasspy,
                           # ou o caminho de um .onnx que você treinou
wake_threshold = 0.5       # maior = menos falso positivo, mais falso negativo
wake_patience = 2          # quadros de 80 ms seguidos acima do limiar
wake_cooldown = 3.0        # tempo mudo do detector depois de disparar

# A ditada aberta por voz não tem tecla para encerrar: ela para sozinha no
# silêncio. Estes valores valem SÓ para o caminho da voz — o atalho segue
# governado por silence_secs, logo acima.
wake_silence_secs = 2.0
wake_max_secs = 60.0
wake_min_speech_secs = 0.5 # fala somada antes de o corte por silêncio valer —
                           # o rabo da palavra-chave sozinho não conta como fala
wake_grace_secs = 5.0      # sem essa fala mínima nesse prazo: era ruído
wake_skip_secs = 0.0       # suba para 0.2 se "Jarvis" vazar na transcrição

# --- leitura em voz alta ---------------------------------------------------
# Lê o texto que você SELECIONOU (destacado com o mouse), não a tela. Exige
# `uv sync --extra voz` e `talk --download`. O Piper roda na CPU: nada de VRAM.
voz_motor = "kokoro"       # kokoro (mais natural, 347 MB) ou piper (leve, 61 MB)
voz_pt = ""                # vazio = a voz padrão do motor. kokoro: pm_alex,
voz_en = ""                # pf_dora · piper: pt_BR-faber-medium, cadu, jeff
voz_velocidade = 1.0       # 1.5 fala mais rápido; 0.8 mais devagar
voz_cadencia = 1.2         # só piper: acima de ~1.5 dá tremido nas vogais
read_language = "auto"     # auto escolhe a voz pelo texto; ou "pt"/"en" fixo

to = "auto"                # auto, stdout, clipboard, paste, type
paste_key = "ctrl+v"       # ctrl+shift+v se o alvo for um terminal
keep_recordings = 20       # gravações de ditadas falhas mantidas em ~/.cache/talk

# Palavras que o Whisper costuma errar. Vira initial_prompt do modelo.
vocab = ""
"""
