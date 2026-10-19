"""One shortcut layer, one engine per platform.

The window never asks which desktop it runs on: it asks the layer for the
dictation shortcuts and edits them. COSMIC is the engine that works today
(it owns the keys, we write its file); Windows and the GlobalShortcuts portal
are named, so asking for them says what is missing instead of failing late.
"""

import pytest

from talk import hotkeys


def test_cosmic_is_picked_on_cosmic():
    engine = hotkeys.engine(platform="linux", desktop="COSMIC")
    assert engine.name == "cosmic"


def test_windows_says_it_is_not_built_yet():
    with pytest.raises(hotkeys.HotkeyError, match="Windows"):
        hotkeys.engine(platform="win32", desktop="")


def test_another_linux_desktop_says_what_it_needs():
    with pytest.raises(hotkeys.HotkeyError, match="GlobalShortcuts"):
        hotkeys.engine(platform="linux", desktop="KDE")


def test_a_combination_is_normalised_the_same_way_everywhere():
    """Order of modifiers and the letter's case are not part of a shortcut."""
    assert hotkeys.combo({"shift", "Super"}, "d") == (("Super", "Shift"), "d")
    assert hotkeys.combo({"Ctrl"}, "F9") == (("Ctrl",), "F9")


def test_a_combination_needs_a_modifier_unless_it_is_a_function_key():
    """A bare letter would fire while typing that letter anywhere."""
    with pytest.raises(hotkeys.HotkeyError, match="modifier"):
        hotkeys.combo(set(), "d")
    assert hotkeys.combo(set(), "F9") == ((), "F9")


def test_the_combination_reads_as_a_person_says_it():
    assert hotkeys.label(("Super", "Alt"), "d") == "Super + Alt + D"
