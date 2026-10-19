"""Slice 11 — one config field, two language vocabularies.

`language` is a single knob in the config, but the engines do not speak it the
same way: Whisper takes ISO codes ("pt"), Qwen3-ASR takes canonical names
("Portuguese") and raises ValueError deep inside its own utils for anything
else. The user's config still carries the Whisper-era code, so the translation
belongs to the engine, and an unknown value must raise naming what it accepts —
before from_pretrained buys 4.4 GB of weights to die in a forward pass.
"""

import numpy as np

from talk import stt, stt_qwen


def test_iso_codes_reach_the_engine_vocabulary():
    """The public vocabulary: the codes the config can carry, in engine names."""
    assert stt_qwen.LANGUAGE_NAMES["pt"] == "Portuguese"
    assert stt_qwen.LANGUAGE_NAMES["en"] == "English"


def test_the_knob_resolves_to_one_canonical_name():
    """A code, a name, or any casing all land on the same canonical name.

    qwen_asr lower-cases whatever it gets, so "pOrTuGuEsE" already works inside
    the package; the codes are what this project adds.
    """
    for value in ("pt", "Portuguese", "pOrTuGuEsE", " pt "):
        assert stt_qwen.language_name(value) == "Portuguese"
    # Empty means "let the model name what it heard", not "no language".
    assert stt_qwen.language_name(None) is None
    assert stt_qwen.language_name("") is None


def test_unknown_language_raises_naming_the_accepted_ones():
    """A typo must raise here, not as a ValueError from inside qwen_asr."""
    try:
        stt_qwen.language_name("klingon")
    except stt.SttError as exc:
        assert "klingon" in str(exc)
        assert "Portuguese" in str(exc)
    else:
        raise AssertionError("no error raised")


def test_the_bad_value_never_reaches_the_weights():
    """Validation happens before the model, so the config typo is free.

    is_loaded() is the observable proof: had the check lived after obter(),
    this call would have downloaded and loaded a 4.4 GB model first.
    """
    audio = np.zeros(16_000, dtype=np.float32)
    try:
        stt.transcribe(
            audio,
            motor="qwen",
            model="x",
            device="cpu",
            compute_type="float32",
            language="klingon",
        )
    except stt.SttError as exc:
        assert "klingon" in str(exc)
    else:
        raise AssertionError("no error raised")
    assert stt_qwen.is_loaded() is False
