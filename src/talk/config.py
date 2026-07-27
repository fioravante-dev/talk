"""Configuração: padrões < arquivo TOML < variáveis de ambiente < flags."""

from __future__ import annotations

import os
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
    keep_alive: str = "5m"

    # áudio
    input_device: str | None = None
    # 0 desliga o corte por silêncio: a gravação vai até você mandar parar
    silence_secs: float = 0.0
    silence_threshold: float = 0.015
    max_secs: float = 300.0

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

# 0 = a gravação só para quando você mandar (Enter/Espaço, ou o atalho de novo).
# Ponha 1.5, por exemplo, para voltar a cortar sozinho após 1,5 s de silêncio.
silence_secs = 0.0
silence_threshold = 0.015

to = "auto"                # auto, stdout, clipboard, paste, type
paste_key = "ctrl+v"       # ctrl+shift+v se o alvo for um terminal
keep_recordings = 20       # gravações de ditadas falhas mantidas em ~/.cache/talk

# Palavras que o Whisper costuma errar. Vira initial_prompt do modelo.
vocab = ""
"""
