"""O motor de atalhos do COSMIC: escreve no arquivo de atalhos do próprio COSMIC.

COSMIC owns its global keys and its portal has no GlobalShortcuts, so the only
way for talk to have a key is to ask COSMIC to run `talk` when it is pressed.
COSMIC's compositor watches this file (inotify), so a change applies on the
next press, with no restart.

The file is not ours: it holds bindings for other tools too. So this module
never regenerates it. It finds the `Spawn` of each binding whose program is
`talk` and edits that binding in place - the same discipline as gui._patch for
config.toml. Everything else comes back byte for byte.

The language matters more than it looks: a binding that pins `-l pt` makes
Whisper decode English speech as Portuguese, a translation nobody asked for.
"""

from __future__ import annotations

import re
import shlex
import time
from dataclasses import dataclass
from pathlib import Path

from . import hotkeys

_DIR = "com.system76.CosmicSettings.Shortcuts/v1"
SHORTCUTS_PATH = Path.home() / ".config/cosmic" / _DIR / "custom"
# What COSMIC binds by itself: a new key must not land on one of these, or
# one of the two bindings silently never fires.
DEFAULTS_PATHS = (
    Path.home() / ".config/cosmic" / _DIR / "defaults",
    Path("/usr/share/cosmic") / _DIR / "defaults",
)

# One binding of the custom file: modifiers, key, optional description, Spawn.
_BINDING = re.compile(
    r"\(\s*modifiers:\s*\[(?P<mods>[^\]]*)\],\s*"
    r'key:\s*"(?P<key>[^"]*)",\s*'
    r'(?:description:\s*(?:Some\("(?P<desc>[^"]*)"\)|None),\s*)?'
    r'\):\s*Spawn\("(?P<cmd>[^"]*)"\)'
)
# Any binding at all, in either layout COSMIC writes (one per line in
# defaults, spread over lines in custom), for the "already taken" check.
_ANY = re.compile(
    r"\(\s*modifiers:\s*\[(?P<mods>[^\]]*)\],\s*"
    r'key:\s*"(?P<key>[^"]*)"\s*,?\s*'
    r'(?:description:\s*(?:Some\("(?P<desc>[^"]*)"\)|None),?\s*)?'
    r"\)\s*:\s*(?P<action>\w+(?:\([^)]*\))?)"
)
_LANGUAGE = re.compile(r"(?P<flag>(?:-l|--language)(?:=|\s+))(?P<value>\S+)")


@dataclass(frozen=True)
class Shortcut:
    mods: tuple[str, ...]
    key: str
    description: str
    command: str
    language: str  # "" when the command pins none; "auto" reads as ""
    broken: str  # the first argument the CLI no longer accepts, or ""
    start: int  # the command's span, to rewrite only it
    end: int
    block: tuple[int, int]  # the whole binding's lines, to remove it

    @property
    def keys(self) -> str:
        return hotkeys.label(self.mods, self.key)


def _mods(text: str) -> tuple[str, ...]:
    names = [m.strip() for m in text.split(",") if m.strip()]
    try:
        return hotkeys.combo(names, "F1")[0]
    except hotkeys.HotkeyError:
        return tuple(names)


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


def _block(text: str, match: re.Match) -> tuple[int, int]:
    """The binding's own lines: from its indentation to the comma and the
    newline after it, so removing it leaves no blank line or stray comma."""
    start = text.rfind("\n", 0, match.start()) + 1
    end = match.end()
    if text[end:end + 1] == ",":
        end += 1
    if text[end:end + 1] == "\n":
        end += 1
    return start, end


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
            mods=_mods(match.group("mods")),
            key=match.group("key"),
            description=match.group("desc") or "",
            command=command,
            language="" if language == "auto" else language,
            broken=_broken(command),
            start=match.start("cmd"),
            end=match.end("cmd"),
            block=_block(text, match),
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


def add_binding(text: str, mods, key: str, command: str, description: str = "talk") -> str:
    """The file with one more binding before its closing brace, written in
    the same layout COSMIC's settings app writes."""
    mods, key = hotkeys.combo(mods, key)
    lines = "".join(f"            {m},\n" for m in mods)
    entry = (
        "    (\n"
        f"        modifiers: [\n{lines}        ],\n"
        f'        key: "{key}",\n'
        f'        description: Some("{description}"),\n'
        f'    ): Spawn("{command}"),\n'
    )
    close = text.rfind("}")
    if close < 0:
        return "{\n" + entry + "}\n"
    head = text[:close]
    if head.strip() != "{" and not head.rstrip().endswith(","):
        head = head.rstrip() + ",\n"
    return head + entry + text[close:]


def remove_binding(text: str, shortcut: Shortcut) -> str:
    start, end = shortcut.block
    return text[:start] + text[end:]


def taken(custom: str, defaults: str, mods, key: str) -> str:
    """What already answers this combination - a description, or COSMIC's own
    action - or "" when it is free."""
    wanted = hotkeys.combo(mods, key) if mods else ((), key.lower() if len(key) == 1 else key)
    for text in (custom, defaults):
        for match in _ANY.finditer(text):
            have = (_mods(match.group("mods")), match.group("key"))
            have = (have[0], have[1].lower() if len(have[1]) == 1 else have[1])
            if have == wanted:
                return match.group("desc") or match.group("action")
    return ""


def read(path: Path | None = None) -> str:
    path = path or SHORTCUTS_PATH
    return path.read_text() if path.is_file() else ""


def save(text: str, path: Path | None = None) -> Path:
    """Writes the file, keeping the previous one beside it as COSMIC's own
    tools here already do (custom.bak-<epoch>)."""
    path = path or SHORTCUTS_PATH
    path.parent.mkdir(parents=True, exist_ok=True)
    if path.is_file():
        # Several writes can land in one second (change, add, remove): a name
        # that repeats would overwrite the oldest backup, the one that matters.
        stamp = int(time.time())
        backup = path.with_name(f"{path.name}.bak-{stamp}")
        n = 1
        while backup.exists():
            n += 1
            backup = path.with_name(f"{path.name}.bak-{stamp}-{n}")
        backup.write_text(path.read_text())
    path.write_text(text)
    return path


class CosmicEngine:
    """The layer's surface over the file. Every call re-reads it, because
    COSMIC's own settings app may have changed it since the window opened."""

    name = "cosmic"
    live = True  # cosmic-comp watches the file: the next press uses it

    def __init__(self, path: Path | None = None, defaults: tuple[Path, ...] | None = None):
        self.path = path or SHORTCUTS_PATH
        self.defaults = DEFAULTS_PATHS if defaults is None else defaults

    def _found(self) -> list[Shortcut]:
        return parse(read(self.path))

    def list(self) -> list[hotkeys.Hotkey]:
        return [
            hotkeys.Hotkey(s.mods, s.key, s.language, s.description, s.broken, ref=n)
            for n, s in enumerate(self._found())
        ]

    def taken(self, mods, key: str) -> str:
        defaults = "\n".join(p.read_text() for p in self.defaults if p.is_file())
        return taken(read(self.path), defaults, mods, key)

    def add(self, mods, key: str, language: str) -> hotkeys.Hotkey:
        mods, key = hotkeys.combo(mods, key)
        if (owner := self.taken(mods, key)):
            raise hotkeys.HotkeyError(f"{hotkeys.label(mods, key)} is already used by {owner}")
        save(add_binding(read(self.path), mods, key, hotkeys.talk_command(language)), self.path)
        return self.list()[-1]

    def _one(self, hotkey: hotkeys.Hotkey) -> Shortcut:
        found = self._found()
        if hotkey.ref >= len(found) or (found[hotkey.ref].mods, found[hotkey.ref].key) != (hotkey.mods, hotkey.key):
            raise hotkeys.HotkeyError(f"{hotkey.keys} changed outside this window; reopen it")
        return found[hotkey.ref]

    def set_language(self, hotkey: hotkeys.Hotkey, language: str) -> None:
        save(set_language(read(self.path), self._one(hotkey), language), self.path)

    def remove(self, hotkey: hotkeys.Hotkey) -> None:
        save(remove_binding(read(self.path), self._one(hotkey)), self.path)
