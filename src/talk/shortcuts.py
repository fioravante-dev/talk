"""Os atalhos de teclado do COSMIC que chamam o talk.

The shortcuts are not ours: COSMIC owns the file, and it holds bindings for
other tools too. So this module never regenerates it. It finds the `Spawn` of
each binding whose program is `talk`, reads the language that command pins,
and rewrites that one argument in place - the same discipline as gui._patch
for config.toml. Everything else comes back byte for byte.

The language matters more than it looks: a binding that pins `-l pt` makes
Whisper decode English speech as Portuguese, which is a translation the user
never asked for.
"""

from __future__ import annotations

import re
import shlex
import time
from dataclasses import dataclass
from pathlib import Path

SHORTCUTS_PATH = (
    Path.home() / ".config/cosmic/com.system76.CosmicSettings.Shortcuts/v1/custom"
)

# One binding of COSMIC's RON: modifiers, key, optional description, action.
_BINDING = re.compile(
    r"\(\s*modifiers:\s*\[(?P<mods>[^\]]*)\],\s*"
    r'key:\s*"(?P<key>[^"]*)",\s*'
    r'(?:description:\s*(?:Some\("(?P<desc>[^"]*)"\)|None),\s*)?'
    r'\):\s*Spawn\("(?P<cmd>[^"]*)"\)'
)
_LANGUAGE = re.compile(r"(?P<flag>(?:-l|--language)(?:=|\s+))(?P<value>\S+)")


@dataclass(frozen=True)
class Shortcut:
    keys: str  # "Super + Alt + D", as a person reads it
    description: str
    command: str
    language: str  # "" when the command pins none; "auto" reads as ""
    broken: str  # the first argument the CLI no longer accepts, or ""
    start: int  # where the command sits in the file, to rewrite only it
    end: int


def _is_talk(command: str) -> bool:
    try:
        words = shlex.split(command)
    except ValueError:
        return False
    return bool(words) and Path(words[0]).name == "talk"


def _broken(command: str) -> str:
    """Asks the real parser, so a flag removed from the CLI shows up here
    without a second list to keep in sync."""
    from . import cli

    _known, unknown = cli.build_parser().parse_known_args(shlex.split(command)[1:])
    return unknown[0] if unknown else ""


def _keys(mods: str, key: str) -> str:
    names = [m.strip() for m in mods.split(",") if m.strip()]
    return " + ".join(names + [key.upper() if len(key) == 1 else key])


def parse(text: str) -> list[Shortcut]:
    """The bindings that start a dictation. `--read` speaks text aloud and
    hears nothing, so it has no spoken language to choose."""
    out: list[Shortcut] = []
    for match in _BINDING.finditer(text):
        command = match.group("cmd")
        if not _is_talk(command) or "--read" in shlex.split(command):
            continue
        pinned = _LANGUAGE.search(command)
        language = pinned.group("value") if pinned else ""
        out.append(Shortcut(
            keys=_keys(match.group("mods"), match.group("key")),
            description=match.group("desc") or "",
            command=command,
            language="" if language == "auto" else language,
            broken=_broken(command),
            start=match.start("cmd"),
            end=match.end("cmd"),
        ))
    return out


def set_language(text: str, shortcut: Shortcut, language: str) -> str:
    """The same file with one binding's language changed. "" means detect it,
    written as `-l auto` so the binding says what it does."""
    value = language or "auto"
    command = text[shortcut.start:shortcut.end]
    if _LANGUAGE.search(command):
        command = _LANGUAGE.sub(lambda m: m.group("flag") + value, command, count=1)
    else:
        program, _, rest = command.partition(" ")
        command = f"{program} -l {value}" + (f" {rest}" if rest else "")
    return text[:shortcut.start] + command + text[shortcut.end:]


def read(path: Path | None = None) -> str:
    path = path or SHORTCUTS_PATH
    return path.read_text() if path.is_file() else ""


def save(text: str, path: Path | None = None) -> Path:
    """Writes the file, keeping the previous one beside it as COSMIC's own
    tools here already do (custom.bak-<epoch>)."""
    path = path or SHORTCUTS_PATH
    if path.is_file():
        backup = path.with_name(f"{path.name}.bak-{int(time.time())}")
        backup.write_text(path.read_text())
    path.write_text(text)
    return path
