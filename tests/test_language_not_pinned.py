"""Slice 1 — the spoken language is detected, not pinned.

The reported bug: English speech came back as Portuguese text. Whisper is
multilingual and conditioned on the language token, so a "pt" pin turns English
speech into a translation instead of a transcription. The pin lives in
config.Config.language, and cli/daemon only leave the pin when the value is
literally "auto".
"""

from talk import config


def test_default_config_does_not_pin_a_language():
    """A fresh install must not force one language on the model."""
    assert config.Config().language is None


def test_auto_is_the_same_as_no_pin():
    """Writing auto in the TOML must mean the same thing as omitting the key."""
    assert config.Config(language="auto").language is None
