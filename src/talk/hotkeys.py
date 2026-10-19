"""Os atalhos de teclado que abrem uma ditada, em qualquer plataforma.

The window never asks which desktop it runs on: it asks this layer for the
dictation shortcuts and edits them through one small surface (list, add,
set_language, remove). Behind it there is one engine per platform, because who
owns a global key differs everywhere:

* COSMIC owns its keys and exposes no portal for them, so the engine writes
  COSMIC's own shortcuts file and COSMIC fires the command (hotkeys_cosmic);
* Windows lets a process own a key itself (RegisterHotKey), so its engine will
  live inside the tray app - not built yet;
* KDE and GNOME offer the GlobalShortcuts portal - not built yet.

The engines not built yet are still named here, so asking for one says what is
missing instead of failing somewhere deeper.
"""

from __future__ import annotations

import os
import shutil
import sys
from dataclasses import dataclass, field
from pathlib import Path

# The order a person reads them in, and the order every engine writes them in.
MODIFIERS = ("Super", "Ctrl", "Alt", "Shift")
_ALIASES = {"super": "Super", "meta": "Super", "ctrl": "Ctrl", "control": "Ctrl",
            "alt": "Alt", "shift": "Shift"}


class HotkeyError(Exception):
    """The shortcut cannot be read or written on this machine, and why."""


@dataclass(frozen=True)
class Hotkey:
    mods: tuple[str, ...]
    key: str
    language: str  # "" means detect it
    description: str = ""
    broken: str = ""  # an argument the CLI no longer accepts: the key does nothing
    ref: int = field(default=0, compare=False)  # the engine's own handle

    @property
    def keys(self) -> str:
        return label(self.mods, self.key)


def _is_function_key(key: str) -> bool:
    return key[:1] == "F" and key[1:].isdigit()


def combo(mods, key: str) -> tuple[tuple[str, ...], str]:
    """A combination in its one canonical form: known modifiers in reading
    order, a letter in lower case. Modifier order and the letter's case are
    not part of a shortcut, so two spellings of one must compare equal."""
    names = set()
    for mod in mods:
        name = _ALIASES.get(str(mod).lower())
        if name is None:
            raise HotkeyError(f"unknown modifier {mod!r}; use {', '.join(MODIFIERS)}")
        names.add(name)
    key = key.lower() if len(key) == 1 else key
    # A bare letter fires whenever that letter is typed, anywhere.
    if not names and not _is_function_key(key):
        raise HotkeyError("a shortcut needs a modifier (Super, Ctrl, Alt or Shift) "
                          "unless it is a function key")
    return tuple(m for m in MODIFIERS if m in names), key


def label(mods, key: str) -> str:
    """The combination as a person says it: "Super + Alt + D"."""
    return " + ".join(list(mods) + [key.upper() if len(key) == 1 else key])


def talk_command(language: str) -> str:
    """The command a new shortcut runs: this very install's `talk`, so the
    key keeps working whatever PATH the desktop session happens to have."""
    beside = Path(sys.executable).with_name("talk")
    program = str(beside) if beside.is_file() else shutil.which("talk")
    if not program:
        raise HotkeyError("cannot find the talk command to bind")
    return f"{program} -l {language or 'auto'}"


def engine(platform: str | None = None, desktop: str | None = None):
    """The engine for this machine. Arguments exist for the tests; at run time
    they come from sys.platform and XDG_CURRENT_DESKTOP."""
    platform = sys.platform if platform is None else platform
    desktop = os.environ.get("XDG_CURRENT_DESKTOP", "") if desktop is None else desktop

    if platform == "win32":
        raise HotkeyError("shortcuts on Windows are not built yet: they will be "
                          "registered by the tray app itself (RegisterHotKey)")
    if "cosmic" in desktop.lower():
        from .hotkeys_cosmic import CosmicEngine

        return CosmicEngine()
    raise HotkeyError(f"no shortcut engine for {desktop or 'this desktop'} yet; it "
                      "would use the GlobalShortcuts portal (KDE, GNOME)")
