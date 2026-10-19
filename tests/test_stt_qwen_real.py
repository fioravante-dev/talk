"""Opt-in smoke test for the real Qwen3-ASR engine: `uv run pytest -m asr`.

The only test that touches real weights. It downloads ~4 GB and needs a GPU, so
pyproject deselects the `asr` marker by default and `-m asr` is what reaches
this file.

What it proves is the engine contract, not the model: that the packages resolve,
that the weights load on the requested device, and that the dispatcher still
builds the same Transcript shape. Assertions about the *words* the model
produces belong to a recorded dictation, not to a synthetic buffer — a model
named for what it hears is free to answer anything to zeros.
"""

import numpy as np

import pytest

from talk import audio, stt


@pytest.mark.asr
def test_real_engine_loads_and_reports_the_shape():
    """Weights load, and the Transcript keeps its five fields."""
    samples = np.zeros(audio.SAMPLE_RATE, dtype=np.float32)
    result = stt.transcribe(samples, motor="qwen")

    assert result.duration == 1.0
    assert result.elapsed > 0
    assert isinstance(result.text, str)
    assert isinstance(result.language, str)


@pytest.mark.asr
def test_real_engine_rejects_a_capability_it_lacks():
    """The gate is the dispatcher's, so it holds for the real engine too."""
    try:
        stt.transcribe(
            np.zeros(audio.SAMPLE_RATE, dtype=np.float32),
            motor="qwen",
            translate=True,
        )
    except stt.SttError as exc:
        assert "translate" in str(exc).lower()
    else:
        raise AssertionError("no error raised")


@pytest.mark.asr
def test_real_engine_accepts_the_language_the_config_carries():
    """The config pins an ISO code, and the weights want a canonical name.

    This is the failure the user hit: qwen_asr raises ValueError deep in its
    own utils for "Pt", which the daemon shows as "Erro interno". The engine
    has to translate the code before the weights see it.
    """
    result = stt.transcribe(
        np.zeros(audio.SAMPLE_RATE, dtype=np.float32), motor="qwen", language="pt"
    )
    assert isinstance(result.text, str)
    assert result.language_probability == 1.0
