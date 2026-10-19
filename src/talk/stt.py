"""Dispatcher de motores de transcrição.

One public interface, many engines. The caller never asks an engine how to run
— it says what it wants (this motor, this language, translate or not) and the
dispatcher resolves the motor, checks the capability table, and builds the
Transcript that the rest of the program sees.

Each engine owns its lifecycle hooks (obter/is_loaded/unload/warmup/download)
because CTranslate2 and torch release memory in different ways; the dispatcher
stays engine-agnostic and only delegates.
"""

from __future__ import annotations

import time

import numpy as np

from dataclasses import dataclass

from . import audio

MOTORES = ("qwen", "whisper")


@dataclass
class Transcript:
    text: str
    language: str
    language_probability: float
    duration: float
    elapsed: float


class SttError(Exception):
    """The motor cannot honour what the dictation asks for."""


# Plugins and tests register engines here; the built-ins below are imported
# lazily, because their packages are optional extras and importing this module
# must not require any of them.
ENGINES: dict[str, object] = {}


def _classe(motor: str):
    """Resolve the motor name to the engine object implementing the contract."""
    engine = ENGINES.get(motor)
    if engine is not None:
        return engine
    if motor == "qwen":
        from . import stt_qwen

        return stt_qwen
    if motor == "whisper":
        from . import stt_whisper

        return stt_whisper
    raise SttError(f"motor desconhecido: {motor!r} (use qwen ou whisper)")


def _check(motor: str, engine, *, translate: bool) -> None:
    """Fail loud when an engine cannot honour what the dictation asks.

    A translate that quietly does nothing is the same class of bug as a
    language pin that quietly mistranslates: the text looks fine and is wrong.
    Asking for a capability the motor lacks must raise, naming one that has it.
    """
    if not translate or "translate" in engine.CAPABILITIES:
        return
    able = [m for m in MOTORES if "translate" in _classe(m).CAPABILITIES]
    raise SttError(
        f"translate não disponível para motor {motor!r}"
        + (f" (use {' or '.join(able)})" if able else "")
    )


def transcribe(
    samples: np.ndarray,
    *,
    motor: str = "whisper",
    model: str | None = None,
    device: str = "cuda",
    compute_type: str = "int8_float16",
    language: str | None = None,
    translate: bool = False,
    initial_prompt: str | None = None,
    beam_size: int = 5,
    vad: bool = True,
) -> Transcript:
    """Transcribe a audio with the motor requested. Devolve the same shape
    regardless of the engine: the caller never branches on the motor."""
    engine = _classe(motor)
    _check(motor, engine, translate=translate)
    started = time.monotonic()

    text, spoken, probability = engine.run(
        samples,
        model=model or engine.DEFAULT_MODEL,
        device=device,
        compute_type=compute_type,
        language=language,
        translate=translate,
        initial_prompt=initial_prompt,
        beam_size=beam_size,
        vad=vad,
    )

    return Transcript(
        text=text,
        # The language the voice actually spoke, even when the text came back
        # translated: `language` is what was heard, not what was written.
        language=spoken,
        language_probability=probability,
        duration=samples.size / audio.SAMPLE_RATE,
        elapsed=time.monotonic() - started,
    )


# -- lifecycle, delegated to the active engine -----------------------------


def download(model: str | None = None, *, motor: str = "whisper") -> str:
    engine = _classe(motor)
    return engine.download(model or engine.DEFAULT_MODEL)


def warmup(
    model: str | None = None,
    device: str = "cuda",
    compute_type: str = "int8_float16",
    *,
    motor: str = "whisper",
) -> bool:
    engine = _classe(motor)
    return engine.warmup(model or engine.DEFAULT_MODEL, device, compute_type)


def is_loaded(*, motor: str = "whisper") -> bool:
    return _classe(motor).is_loaded()


def unload(*, motor: str = "whisper") -> None:
    _classe(motor).unload()
