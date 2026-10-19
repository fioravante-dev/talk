"""Slice 6 — the configurator edits one key at a time, comments and all.

The GUI must not regenerate the config from a template: the comments are the
documentation of each knob, and a user may have added their own keys. So the
edit is a line-level patch, and the proof that a patch is correct is that the
project's own loader still reads the value back — the GUI and the CLI must
never disagree about what the file says.

A key the template ships commented (`# language = "pt"`) is a knob being
turned on, not a new key: appending it would leave a duplicate that tomllib
rejects outright.
"""

import importlib.util
import tomllib
import pytest

from dataclasses import fields

from talk import config, gui


def test_patch_changes_one_value_and_keeps_the_comments():
    out = gui._patch(config.DEFAULT_TOML, {"stt_motor": "qwen"})
    assert 'stt_motor = "qwen"' in out
    assert out.count("#") == config.DEFAULT_TOML.count("#")
    # A neighbour untouched must stay byte-identical, alignment included.
    assert 'compute_type = "int8_float16"' in out


def test_patch_round_trips_through_the_loader(tmp_path):
    """The GUI writes the file; the loader is what the dictation reads."""
    path = tmp_path / "config.toml"
    path.write_text(
        gui._patch(config.DEFAULT_TOML, {"stt_motor": "whisper", "translate": True})
    )
    config.CONFIG_PATH = path

    cfg = config.load()
    assert cfg.stt_motor == "whisper"
    assert cfg.translate is True


def test_patch_uncomments_a_key_the_template_left_off():
    """Turning `language` on must uncomment, not append a duplicate."""
    out = gui._patch(config.DEFAULT_TOML, {"language": "en"})
    assert "# language" not in out
    assert tomllib.loads(out)["language"] == "en"


def test_patch_keeps_keys_the_template_does_not_know():
    """Someone else's config.toml may carry keys this project never heard of."""
    text = config.DEFAULT_TOML + '\nmy_own = "keep me"\n'
    out = gui._patch(text, {"device": "cpu"})
    assert 'my_own = "keep me"' in out
    assert tomllib.loads(out)["device"] == "cpu"


def test_every_editable_field_is_a_real_config_key():
    """A read-only property (active_model) must not be offered as editable."""
    names = {f.name for f in fields(config.Config)}
    assert {key for _, key, _ in gui.FIELDS} <= names


def test_gui_without_pyside6_says_how_to_install():
    if importlib.util.find_spec("PySide6") is not None:
        pytest.skip("PySide6 installed — the window itself is under test")
    try:
        gui._qt()
    except gui.GuiError as exc:
        assert "uv sync --extra gui" in str(exc)
    else:
        raise AssertionError("no error raised")


def test_every_dropdown_writes_only_real_config_keys():
    """A dropdown item is a patch: its keys must be keys the loader reads."""
    names = {f.name for f in fields(config.Config)}
    for _section, key, _label in gui.FIELDS:
        for _item, writes in gui._options(key, {}):
            assert set(writes) <= names


def test_the_model_dropdown_moves_the_engine_with_the_weights():
    """A whisper name saved under the qwen engine is weights nobody asked for."""
    writes = dict(gui._options("model", {}))
    assert writes["whisper - base"] == {"stt_motor": "whisper", "model": "base"}


def test_the_dropdown_opens_on_the_value_the_file_carries():
    today = {"stt_motor": "qwen", "qwen_model": "Qwen/Qwen3-ASR-1.7B", "device": "cpu"}
    items = gui._options("device", today)
    assert gui._index(items, today) == 1


def test_a_value_the_list_does_not_carry_stays_selectable():
    """Opening the window and saving must not silently rewrite a hand-set value."""
    today = {"compute_type": "qfloat8"}
    items = gui._options("compute_type", today)
    assert items[-1] == ("qfloat8", {"compute_type": "qfloat8"})
    assert gui._index(items, today) == len(items) - 1


def test_a_missing_language_opens_on_detect_it():
    """Absent and empty are the same thing to the loader: let the model hear."""
    today = {"stt_motor": "qwen"}
    assert gui._index(gui._options("language", today), today) == 0
