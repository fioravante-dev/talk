"""Slice 2 — one public interface, two engines, capabilities that fail loud.

The two engines do not accept the same options: Whisper has a translate task,
hotwords, beam size and VAD; Qwen has only the language and time stamps. A
request an engine cannot honour must raise, not be ignored — a translate that
quietly does nothing is the same class of bug as a language pin that quietly
mistranslates.

The engines here are fakes registered in the table: they implement the engine
contract (obter/is_loaded/unload/warmup/download/run) without weights or a GPU.
"""

import importlib.util

import numpy as np

import pytest

from inspect import signature

from talk import config, stt, stt_qwen, stt_whisper


class FakeEngine:
    """A registered engine that records what it was asked for."""

    def __init__(self, capabilities, text, language):
        self.CAPABILITIES = capabilities
        self.text = text
        self.language = language
        self.seen = []

    def obter(self, model, device, compute_type):
        return self

    def is_loaded(self):
        return True

    def unload(self):
        return None

    def warmup(self, model, device, compute_type):
        return True

    def download(self, model):
        return "/fake/weights"

    def run(self, samples, **kwargs):
        self.seen.append({"language": kwargs["language"], "translate": kwargs["translate"]})
        return self.text, self.language, 0.9


WHISHERY = frozenset({"translate", "hotwords", "beam_size", "vad"})
QWENISH = frozenset({"auto_language", "time_stamps"})


def _registered(motor, capabilities, text, language):
    fake = FakeEngine(capabilities, text, language)
    stt.ENGINES[motor] = fake
    return fake


def test_motor_switches_through_one_public_interface():
    """Both engines are reachable through stt.transcribe, no per-engine call
    sites in the caller."""
    whisper = _registered("fake_whisper", WHISHERY, "whisper said", "en")
    qwen = _registered("fake_qwen", QWENISH, "qwen said", "en")

    audio = np.zeros(16_000, dtype=np.float32)
    for motor, expected in (("fake_whisper", "whisper said"), ("fake_qwen", "qwen said")):
        out = stt.transcribe(audio, motor=motor, model="x", device="cpu", compute_type="int8")
        assert out.text == expected


def test_translate_is_opt_in():
    """The default path never asks an engine to translate."""
    whisper = _registered("fake_whisper", WHISHERY, "hola", "es")
    stt.transcribe(np.zeros(16_000, dtype=np.float32), motor="fake_whisper", model="x")
    assert whisper.seen[-1]["translate"] is False


def test_translate_without_the_capability_raises():
    """Asking Qwen to translate must raise, naming the engine that can."""
    _registered("fake_qwen", QWENISH, "hola", "es")
    try:
        stt.transcribe(
            np.zeros(16_000, dtype=np.float32),
            motor="fake_qwen",
            model="x",
            translate=True,
        )
    except stt.SttError as exc:
        assert "whisper" in str(exc).lower()
    else:
        raise AssertionError("no error raised")


def test_missing_extra_says_how_to_install():
    """An engine whose packages are absent must say so, not crash on import.

    Skipped once the qwen extra is installed: then the weights are what would
    be under test, and that is what the `-m asr` smoke test is for.
    """
    if importlib.util.find_spec("qwen_asr") is not None:
        pytest.skip("qwen_asr installed — the real engine is under test")
    try:
        stt.transcribe(np.zeros(16_000, dtype=np.float32), motor="qwen", model="x")
    except stt.SttError as exc:
        assert "uv sync" in str(exc)
    else:
        raise AssertionError("no error raised")


def test_real_engines_satisfy_the_dispatcher_contract():
    """The dispatcher calls engine.run with named kwargs, so an engine whose
    entry point is named otherwise is an AttributeError waiting for a real
    dictation — invisible to the fakes above, which are written to the contract
    by hand. Checked against the real modules, which import without weights,
    packages or a GPU.
    """
    asked = {
        "model",
        "device",
        "compute_type",
        "language",
        "translate",
        "initial_prompt",
        "beam_size",
        "vad",
    }
    for engine in (stt_whisper, stt_qwen):
        for hook in ("CAPABILITIES", "DEFAULT_MODEL", "obter", "is_loaded", "unload", "warmup", "download", "run"):
            assert hasattr(engine, hook), f"{engine.__name__} missing {hook}"
        assert asked <= set(signature(engine.run).parameters), engine.__name__
