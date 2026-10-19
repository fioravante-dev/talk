"""Interface grafic: configurador do config.toml + panel do daemon.

Two things live here, and they are deliberately different shapes:

* the patching of the config file is pure text work, testable without a
  display, because the GUI and the CLI must never disagree about what the file
  says — the loader is the proof;
* the window itself needs PySide6, an optional extra, so it is imported lazily:
  importing this module costs nothing, and only opening the window says which
  extra is missing (same shape as the STT engines).

The configurator offers a handful of knobs, never the whole file: the rest stays
reachable in the TOML, whose comments are the documentation of each knob. Every
knob is a dropdown of the values the loader accepts, because a user does not
know what to type.

The configurator edits the user's file in place, one key at a time. It never
regenerates it from a template, because the comments are the documentation of
each knob and a user may have added keys this project never heard of.
"""

from __future__ import annotations

import json
import re
import shutil
import sys
import tomllib
from dataclasses import fields as dataclass_fields
from pathlib import Path

from . import config, history

GUI_PATH = config.CONFIG_PATH


class GuiError(Exception):
    """The window cannot open, or the value typed cannot reach the file."""


# The knobs the configurator offers, in the order they read on screen. Every
# name here must be a real Config field: a read-only property (active_model)
# would look editable and do nothing.
FIELDS = (
    ("transcription", "model", "Model"),
    ("transcription", "device", "Runs on"),
    ("transcription", "compute_type", "Precision"),
    ("transcription", "language", "Spoken language"),
    ("transcription", "translate", "Translate to English"),
    ("output", "to", "Where the text goes"),
)

# One dropdown for the weights, with the engine in its label: the two catalogs
# have nothing in common, so a model carries the engine that owns it and writes
# stt_motor along with it. Two free knobs would be two chances to save a whisper
# name under the qwen engine, which is weights nobody asked to load.
_MODEL_KEY = {"qwen": "qwen_model", "whisper": "model"}
_MODELS = (
    ("qwen - Qwen3-ASR-1.7B", {"stt_motor": "qwen", "qwen_model": "Qwen/Qwen3-ASR-1.7B"}),
    ("whisper - large-v3-turbo", {"stt_motor": "whisper", "model": "large-v3-turbo"}),
    ("whisper - large-v3", {"stt_motor": "whisper", "model": "large-v3"}),
    ("whisper - medium", {"stt_motor": "whisper", "model": "medium"}),
    ("whisper - small", {"stt_motor": "whisper", "model": "small"}),
    ("whisper - base", {"stt_motor": "whisper", "model": "base"}),
    ("whisper - tiny", {"stt_motor": "whisper", "model": "tiny"}),
)

# A dropdown item is a label a user reads plus the config keys it writes. The
# labels say what each value does, because the bare names are not a choice.
CHOICES = {
    "device": (
        ("GPU", {"device": "cuda"}),
        ("CPU", {"device": "cpu"}),
    ),
    "compute_type": (
        ("int8_float16 - GPU, the fast one", {"compute_type": "int8_float16"}),
        ("bfloat16 - GPU", {"compute_type": "bfloat16"}),
        ("float32 - GPU", {"compute_type": "float32"}),
        ("int8 - CPU", {"compute_type": "int8"}),
    ),
    "language": (
        ("detect it", {"language": ""}),
        ("Portuguese", {"language": "pt"}),
        ("English", {"language": "en"}),
        ("Spanish", {"language": "es"}),
        ("French", {"language": "fr"}),
        ("German", {"language": "de"}),
    ),
    "translate": (
        ("off", {"translate": False}),
        ("on - whisper only", {"translate": True}),
    ),
    "to": (
        ("auto - paste at the cursor", {"to": "auto"}),
        ("paste at the cursor", {"to": "paste"}),
        ("clipboard", {"to": "clipboard"}),
        ("print it", {"to": "stdout"}),
        ("type it", {"to": "type"}),
    ),
}

# The window's own palette, taken from the tray glyph rather than from a
# generic dark theme: a blue slate that sits under COSMIC's panel, the spoken
# words on a warm paper tone, and three hues that only ever mean a state
# (ready / listening / not running). _PETROL is read as text or a mark on the
# slate; _DEEP fills a button under white words - one hue cannot do both.
_INK = "#1a2129"
_RISEN = "#222b35"
_RAISED = "#2a3440"
_EDGE = "#2f3a46"
_PAPER = "#ece7dc"
_BODY = "#c3cad3"
_QUIET = "#8d9aa8"
_PETROL = "#6fa7bb"
_DEEP = "#35687b"
_LISTEN = "#8cc0a5"
_HALT = "#d6a35c"

# Chrome in the system's own sans, so the window sits in Pop!_OS like a native
# tool; the spoken words in a book face (P052, a Palatino), because what you
# said becomes prose. Both are installed on this box (fc-list); Qt cannot reach
# a webfont.
_CHROME = '"Fira Sans"'
_SPOKEN = '"P052", "Noto Serif"'

_STYLE = f"""
QWidget {{ background: {_INK}; color: {_BODY};
          font-family: {_CHROME}; font-size: 13px; }}
QLabel {{ background: transparent; }}
QToolTip {{ background: {_RAISED}; color: {_PAPER}; border: 1px solid {_EDGE}; padding: 4px; }}

QTabWidget::pane {{ border: 0; border-top: 1px solid {_EDGE}; top: -1px; }}
QTabBar::tab {{ background: transparent; color: {_QUIET}; padding: 9px 0;
               margin-right: 24px; border: 0; border-bottom: 2px solid transparent;
               font-size: 14px; }}
QTabBar::tab:selected {{ color: {_PAPER}; border-bottom: 2px solid {_PETROL}; }}
QTabBar::tab:hover:!selected {{ color: {_BODY}; }}
QTabBar::tab:focus {{ color: {_PAPER}; }}

QScrollArea {{ border: 0; }}
QScrollBar:vertical {{ background: transparent; width: 10px; margin: 2px; }}
QScrollBar::handle:vertical {{ background: {_EDGE}; border-radius: 3px; min-height: 32px; }}
QScrollBar::handle:vertical:hover {{ background: {_QUIET}; }}
QScrollBar::add-line, QScrollBar::sub-line, QScrollBar::add-page, QScrollBar::sub-page
    {{ background: none; height: 0; }}

QFrame#row {{ background: transparent; border-radius: 6px; }}
QFrame#row:hover {{ background: {_RISEN}; }}
QFrame#bar {{ background: {_PETROL}; border-radius: 1px; }}

QComboBox {{ background: {_RISEN}; color: {_PAPER}; border: 1px solid {_EDGE};
            border-radius: 6px; padding: 6px 10px; min-height: 18px; }}
QComboBox:hover {{ border-color: {_QUIET}; }}
QComboBox:focus {{ border-color: {_PETROL}; }}
QComboBox:disabled {{ color: {_QUIET}; background: {_INK}; }}
QComboBox::drop-down {{ border: 0; width: 28px; }}
QComboBox::down-arrow {{ image: url("{{chevron}}"); width: 10px; height: 6px; }}
QComboBox QAbstractItemView {{ background: {_RAISED}; color: {_PAPER}; border: 1px solid {_EDGE};
                              padding: 4px; outline: 0;
                              selection-background-color: {_DEEP}; selection-color: {_PAPER}; }}

QPushButton {{ background: transparent; color: {_QUIET}; border: 1px solid transparent;
               border-radius: 6px; padding: 3px 9px; font-size: 12px; }}
QPushButton:hover {{ background: {_RAISED}; color: {_PAPER}; }}
QPushButton:focus {{ border-color: {_PETROL}; color: {_PAPER}; }}
QFrame#row QPushButton {{ color: {_EDGE}; padding: 1px 8px; }}
QFrame#row:hover QPushButton {{ color: {_QUIET}; }}
QFrame#row QPushButton:hover, QFrame#row QPushButton:focus {{ color: {_PAPER}; }}
QPushButton#primary {{ background: {_DEEP}; color: white; padding: 8px 18px; font-size: 13px; }}
QPushButton#primary:hover {{ background: #3f7a90; }}
QPushButton#primary:focus {{ border-color: {_PAPER}; }}
QPushButton#primary:disabled {{ background: {_RISEN}; color: {_QUIET}; }}
"""

_KEY = re.compile(r"^(?P<indent>\s*)(?P<hash>#\s*)?(?P<key>[A-Za-z_][A-Za-z0-9_]*)\s*=")


def _value(value) -> str:
    """A Python value as the TOML that the loader reads back."""
    return json.dumps(value, ensure_ascii=False)


def _patch(text: str, values: dict) -> str:
    """One key at a time, keeping comments, blank lines and unknown keys.

    A key the template ships commented out is a knob being turned on: the line
    is uncommented in place, not appended, or the file would carry the key
    twice and tomllib would refuse to parse it at all.

    The comment column is kept, so the file still reads as an aligned table
    after a value changes length.
    """
    pending = dict(values)
    out: list[str] = []

    for line in text.splitlines():
        match = _KEY.match(line)
        if match is None or (key := match.group("key")) not in pending:
            out.append(line)
            continue

        prefix = match.group("indent") + f"{key} = {_value(pending.pop(key))}"
        if (comment_at := line.find("#", match.end())) >= 0:
            pad = max(1, comment_at - len(prefix))
            prefix += " " * pad + line[comment_at:]
        out.append(prefix)

    for key, value in pending.items():
        out.append(f"{key} = {_value(value)}")

    return "\n".join(out) + ("\n" if text.endswith("\n") else "")


def _matches(writes: dict, today: dict) -> bool:
    """Is this dropdown item what the file says today?"""
    return all(k in today and today[k] == v for k, v in writes.items())


def _options(key: str, today: dict) -> list[tuple[str, dict]]:
    """The items a knob offers, plus the file's own value when it is not one of them.

    A value set by hand that this list does not carry must stay selectable, or
    opening the window and saving would silently rewrite it.
    """
    if key == "model":
        items = list(_MODELS)
        if not any(_matches(writes, today) for _label, writes in items):
            motor = today.get("stt_motor", config.Config.stt_motor)
            field = _MODEL_KEY.get(motor, "model")
            value = today.get(field, getattr(config.Config, field))
            items.append((f"{motor} - {value}", {"stt_motor": motor, field: value}))
        return items

    if key == "language":
        # Absent and empty are the same thing to the loader: let the model hear.
        today = {**today, "language": today.get("language") or ""}

    items = list(CHOICES[key])
    if not any(_matches(writes, today) for _label, writes in items):
        value = today.get(key, getattr(config.Config, key))
        items.append((str(value), {key: value}))
    return items


def _index(items: list[tuple[str, dict]], today: dict) -> int:
    """Which item the file selects today: the dropdown opens on the current value."""
    for position, (_label, writes) in enumerate(items):
        if _matches(writes, today):
            return position
    return 0


def _read_config_text() -> str:
    """The user's file, or the template when there is none yet."""
    if config.CONFIG_PATH.is_file():
        return config.CONFIG_PATH.read_text()
    return config.DEFAULT_TOML


def _current(text: str) -> dict:
    """What the loader would see today — the same view the CLI has.

    The defaults are part of the view: a key the file never mentions is still a
    knob with a value, and leaving it out would make the dropdown offer a third
    item that only repeats that default under a raw label.
    """
    known = {f.name: getattr(config.Config, f.name) for f in dataclass_fields(config.Config)}
    try:
        data = tomllib.loads(text)
    except tomllib.TOMLDecodeError:
        return known
    return {**known, **{k: v for k, v in data.items() if k in known}}


def _state_lines(state: dict | None) -> list[str]:
    """The daemon's own ping, in the daemon's own words.

    The state comes from daemon.ping(), so the panel renders a fact the daemon
    already owns; a second status source here would be a second notion of what
    the daemon is doing. The words a user reads are a mapping of these lines,
    never a second notion.
    """
    if state is None:
        return ["daemon stopped"]
    if state.get("recording"):
        head = "daemon recording"
    elif state.get("reading"):
        head = "daemon reading aloud"
    else:
        head = "daemon ready"

    out = [head]
    if state.get("wake"):
        out.append(f"listening for {state.get('wake_model', '')!r}")
    return out


# The daemon's vocabulary is the daemon's: a user reads whether the machine is
# listening to them, and the hue says it before the word is read.
_STATE_WORDS = {
    "daemon recording": ("listening", _LISTEN),
    "daemon reading aloud": ("reading aloud", _LISTEN),
    "daemon ready": ("ready", _PETROL),
    "daemon stopped": ("not running", _HALT),
}


def _language_label(value: str) -> str:
    """The language a history entry carries, in the words a user reads.

    Whisper stores ISO codes and Qwen stores canonical names, so both go through
    the engine's own mapping rather than a second table here that can drift.
    """
    from . import stt_qwen

    try:
        return stt_qwen.language_name(value) or value
    except stt_qwen.SttError:
        return value


def _phrase_line(entry: dict) -> str:
    """One phrase as the user reads it: the time first, the text last and verbatim.

    The text is what a copy button hands back, so it stays exactly as the entry
    stored it.
    """
    stamp = entry.get("at", "")[11:]
    text = entry.get("text", "")
    return f"{stamp}  {text}" if stamp else text


def _details_lines(entry: dict) -> list[str]:
    """How a phrase was captured, from the facts the entry already carries."""
    out: list[str] = []
    language = entry.get("language") or ""
    if language:
        out.append(f"heard in {_language_label(language)}")
    if motor := entry.get("motor") or "":
        out.append(f"by {motor}")
    audio = entry.get("audio_seconds")
    took = entry.get("stt_seconds")
    if audio is not None and took is not None:
        out.append(f"{audio}s of audio, {took}s to transcribe")
    if delivered := ", ".join(entry.get("delivered") or []):
        out.append(f"went to {delivered}")
    return out


def _panel(state: dict | None, entries: list[dict]) -> list[str]:
    """State lines, then the phrase lines, newest last.

    The history renders even with the daemon stopped: the re-copy exists
    precisely for when nothing is running.
    """
    out = _state_lines(state)
    for entry in entries:
        out.append(_phrase_line(entry))
    return out


def _shortcut() -> tuple[str, str | None]:
    """The exact command, and where a desktop shortcut would go, only when the
    desktop is known well enough to trust it.

    A shortcut on a machine that cannot emit a paste key is an icon that does
    nothing, which is worse than the command the user can copy.
    """
    command = "talk --loop --to paste"
    if not any(shutil.which(tool) for tool in ("ydotool", "xdotool", "wtype")):
        return command, None
    return command, str(Path.home() / "Desktop" / "talk-dictate.desktop")


# Qt draws a stylesheet's arrow only from a file, so the chevron is written
# next to the history the first time the window opens.
_CHEVRON = (
    '<svg xmlns="http://www.w3.org/2000/svg" width="10" height="6" viewBox="0 0 10 6">'
    f'<path d="M1 1l4 4 4-4" fill="none" stroke="{_QUIET}" stroke-width="1.5" '
    'stroke-linecap="round" stroke-linejoin="round"/></svg>'
)


def _style() -> str:
    chevron = history.HISTORY_PATH.parent / "gui-chevron.svg"
    try:
        chevron.parent.mkdir(parents=True, exist_ok=True)
        if not chevron.is_file() or chevron.read_text() != _CHEVRON:
            chevron.write_text(_CHEVRON)
    except OSError:
        pass  # without the file Qt draws no arrow; the dropdown still opens
    return _STYLE.replace("{chevron}", chevron.as_posix())


def _qt():
    """Lazy importer: the app must stay importable without the gui extra."""
    try:
        from PySide6 import QtWidgets
    except ImportError as exc:
        raise GuiError(
            "PySide6 not installed - run `uv sync --extra gui` in the "
            "project directory"
        ) from exc
    return QtWidgets


def _label(QtWidgets, text: str, style: str = ""):
    label = QtWidgets.QLabel()
    label.setText(text)
    if style:
        label.setStyleSheet(style)
    return label


def _quiet(QtWidgets, text: str, size: int = 12):
    return _label(QtWidgets, text, f"color: {_QUIET}; font-size: {size}px;")


def _button(QtWidgets, text: str, press, name: str = ""):
    button = QtWidgets.QPushButton()
    button.setText(text)
    if name:
        button.setObjectName(name)
    button.clicked.connect(press)
    return button


def _summary(today: dict) -> str:
    """The engine, said as a sentence: "Whisper large-v3-turbo, on the GPU".

    Read off the same dropdown labels the settings show, so the header and the
    knob never name the same model two ways.
    """
    models = _options("model", today)
    device = _options("device", today)
    motor, _, name = models[_index(models, today)][0].partition(" - ")
    return f"{motor.capitalize()} {name}, on the {device[_index(device, today)][0]}"


def _details_sentence(entry: dict) -> str:
    """The capture facts as one sentence, for the details line under a phrase."""
    lines = _details_lines(entry)
    if not lines:
        return ""
    sentence = ", ".join(lines)
    return sentence[0].upper() + sentence[1:] + "."


_MONTHS = ("Jan", "Feb", "Mar", "Apr", "May", "Jun",
           "Jul", "Aug", "Sep", "Oct", "Nov", "Dec")


def _day(stamp: str) -> str:
    """The heading a group of phrases sits under: Today, Yesterday, or the date.

    Month names are spelled here, not by strftime: the locale would put a
    Portuguese "out" inside an English window.
    """
    from datetime import date, timedelta

    try:
        day = date.fromisoformat(stamp[:10])
    except ValueError:
        return ""
    today = date.today()
    if day == today:
        return "Today"
    if day == today - timedelta(days=1):
        return "Yesterday"
    label = f"{day.day} {_MONTHS[day.month - 1]}"
    return label if day.year == today.year else f"{label} {day.year}"


def _when(stamp: str) -> str:
    """The clock time a phrase was said; the day is its group's heading."""
    _, _, clock = stamp.partition("T")
    return clock[:5] if clock else stamp


def _bar_width(seconds, longest: float = 60.0, span: int = 44) -> int:
    """How long the speech was, as a length: a minute fills the gutter."""
    try:
        seconds = float(seconds)
    except (TypeError, ValueError):
        return 0
    return max(3, min(span, round(span * seconds / longest)))


def _home(path: str) -> str:
    home = str(Path.home())
    return "~" + path[len(home):] if path.startswith(home) else path


# What each knob changes, in the words of what it does to the dictation.
_HINTS = {
    "model": "Bigger models hear better and take longer to load.",
    "device": "The GPU is much faster. The CPU leaves the card free.",
    "compute_type": "Lower precision uses less video memory.",
    "language": "Pin one only when detection keeps guessing wrong.",
    "translate": "Whisper only. Your speech comes out as English text.",
    "to": "Auto pastes at the cursor when it can.",
}


class Configurator:
    """Rows of knobs: a label and a dropdown of the values the loader accepts.

    Nothing here is free text. A user does not know what to type, so every knob
    offers the choices that exist and opens on the one the file already carries.
    """

    _SECTIONS = {"transcription": "Transcription", "output": "Output"}

    def __init__(self, parent):
        self.QtWidgets = _qt()
        self.text = _read_config_text()
        self.rows: dict[str, tuple] = {}
        self.saved = _quiet(self.QtWidgets, "")
        self.saved.setWordWrap(True)
        self._build(parent)

    def _build(self, parent):
        Qw = self.QtWidgets
        today = _current(self.text)
        outer = Qw.QVBoxLayout(parent)
        outer.setContentsMargins(2, 22, 2, 2)
        outer.setSpacing(0)

        grid = Qw.QGridLayout()
        grid.setHorizontalSpacing(28)
        grid.setVerticalSpacing(14)
        grid.setColumnStretch(0, 1)
        row = 0
        section = None
        for where, key, label in FIELDS:
            if where != section:
                section = where
                heading = _label(Qw, self._SECTIONS.get(where, where),
                                 f"color: {_PAPER}; font-size: 15px; font-weight: 600;")
                if row:
                    grid.setRowMinimumHeight(row, 14)
                    row += 1
                grid.addWidget(heading, row, 0, 1, 2)
                row += 1

            words = Qw.QWidget()
            stack = Qw.QVBoxLayout(words)
            stack.setContentsMargins(0, 0, 0, 0)
            stack.setSpacing(2)
            stack.addWidget(_label(Qw, label, f"color: {_PAPER};"))
            if hint := _HINTS.get(key):
                note = _quiet(Qw, hint)
                note.setWordWrap(True)
                stack.addWidget(note)

            combo = Qw.QComboBox()
            combo.setFixedWidth(250)
            items = _options(key, today)
            combo.addItems([item[0] for item in items])
            combo.setCurrentIndex(_index(items, today))
            combo.currentIndexChanged.connect(self._changed)
            self.rows[key] = (combo, items)
            grid.addWidget(words, row, 0)
            grid.addWidget(combo, row, 1)
            row += 1
        outer.addLayout(grid)
        outer.addStretch(1)

        self.button = _button(Qw, "Save changes", self._save, name="primary")
        self.button.setEnabled(False)
        foot = Qw.QHBoxLayout()
        foot.addWidget(self.saved, 1)
        foot.addWidget(self.button)
        outer.addLayout(foot)
        self._guard()

    def _guard(self) -> None:
        """Translate is a Whisper-only knob: under Qwen it can only raise, so
        it is shown off and locked instead of offered."""
        if "model" not in self.rows or "translate" not in self.rows:
            return
        combo, items = self.rows["model"]
        qwen = items[combo.currentIndex()][1].get("stt_motor") == "qwen"
        translate, _items = self.rows["translate"]
        if qwen:
            translate.setCurrentIndex(0)
        translate.setEnabled(not qwen)

    def _changed(self) -> None:
        self._guard()
        dirty = _patch(self.text, self._typed()) != self.text
        self.button.setEnabled(dirty)
        self.saved.setText("You have unsaved changes." if dirty else "")

    def _save(self) -> None:
        """The button's press: the window goes back over the file, and says where."""
        try:
            where = self.save()
        except OSError as exc:
            # The words say what happened and what is still true: a silent
            # failure would leave someone convinced the knobs took effect.
            self.saved.setText(f"Could not write the file: {exc}. It stays as it is.")
            return
        # The daemon reads the file once, at start: saying "saved" alone would
        # promise a change the running dictation does not see yet.
        self.text = _read_config_text()
        self.button.setEnabled(False)
        self.saved.setText(f"Saved to {_home(where)}. Restart the daemon to apply it.")

    def _typed(self) -> dict:
        """The dropdown selections, expanded to the config keys each one writes."""
        out: dict = {}
        for combo, items in self.rows.values():
            out.update(items[combo.currentIndex()][1])
        return out

    def save(self) -> str:
        """Writes the window back over the file, and says where."""
        config.CONFIG_PATH.parent.mkdir(parents=True, exist_ok=True)
        config.CONFIG_PATH.write_text(_patch(self.text, self._typed()))
        return str(config.CONFIG_PATH)


def _ClickCopy(label, copy):
    """Watches a label: a left click that selected nothing is a copy.

    Built lazily because QObject is PySide6's, an optional extra. The caller
    keeps the returned object alive; Qt only borrows an event filter.
    """
    from PySide6 import QtCore

    class Filter(QtCore.QObject):
        def eventFilter(self, watched, event):
            if (
                event.type() == QtCore.QEvent.MouseButtonRelease
                and event.button() == QtCore.Qt.LeftButton
                and not watched.hasSelectedText()
            ):
                copy()
            return False  # the label still handles the press: drags select

    watcher = Filter(label)
    label.installEventFilter(watcher)
    return watcher


class Panel:
    """Live view: the phrases you said, read as a transcript.

    A phrase is the only thing a user reads; everything else about it — the
    engine, the language heard, where it went — is behind its details button,
    because metadata on every line is a wall nobody scans. The one exception
    is how long you spoke, drawn as a bar in the gutter: a length is read at a
    glance, a number is not. Newest on top: the phrase someone opens this
    window for is the one they just said.
    """

    LIMIT = 60
    GUTTER = 58

    def __init__(self, parent):
        self.QtWidgets = _qt()
        self._refresh()
        self._build(parent)

    def _refresh(self) -> None:
        from . import daemon

        self.state = daemon.ping()
        self.entries = history.recent(limit=self.LIMIT)

    def _recopy(self, text: str) -> list[str]:
        """Sends an old dictation back to the clipboard, through the same
        delivery path the CLI uses."""
        from . import output

        return output.deliver(text, ["clipboard"])

    def _copy(self, button, text: str) -> None:
        """Copy, and say so on the button itself: the clipboard shows nothing."""
        from PySide6 import QtCore

        try:
            self._recopy(text)
        except Exception as exc:  # the clipboard tool is outside our control
            button.setToolTip(str(exc))
            button.setText("Copy failed")
        else:
            button.setText("Copied")
        QtCore.QTimer.singleShot(1400, lambda: button.setText("Copy"))

    def _entry(self, entry: dict):
        """One phrase: the gutter says when and for how long, the column says
        what. The phrase wraps, keeps a book face's leading, and is selectable,
        because part of a dictation is sometimes all that is wanted back."""
        import html

        from PySide6 import QtCore

        Qw = self.QtWidgets
        row = Qw.QFrame()
        row.setObjectName("row")
        line = Qw.QHBoxLayout(row)
        line.setContentsMargins(8, 8, 8, 4)
        line.setSpacing(14)

        gutter = Qw.QVBoxLayout()
        gutter.setSpacing(5)
        gutter.addWidget(_quiet(Qw, _when(entry.get("at", "")), 12))
        if width := _bar_width(entry.get("audio_seconds")):
            bar = Qw.QFrame()
            bar.setObjectName("bar")
            bar.setFixedSize(width, 3)
            bar.setToolTip(f"{entry.get('audio_seconds')} s of speech")
            gutter.addWidget(bar)
        gutter.addStretch(1)
        holder = Qw.QWidget()
        holder.setFixedWidth(self.GUTTER)
        holder.setLayout(gutter)
        gutter.setContentsMargins(0, 3, 0, 0)
        line.addWidget(holder, 0, QtCore.Qt.AlignTop)

        column = Qw.QVBoxLayout()
        column.setSpacing(0)
        phrase = _label(
            Qw,
            f'<p style="line-height: 122%;">{html.escape(entry.get("text", ""))}</p>',
            f"font-family: {_SPOKEN}; font-size: 16px; color: {_PAPER};",
        )
        phrase.setTextFormat(QtCore.Qt.RichText)
        phrase.setWordWrap(True)
        phrase.setTextInteractionFlags(QtCore.Qt.TextSelectableByMouse)
        column.addWidget(phrase)

        details = _quiet(Qw, _details_sentence(entry))
        details.setWordWrap(True)
        details.setVisible(False)
        column.addWidget(details)

        actions = Qw.QHBoxLayout()
        actions.setSpacing(2)
        actions.addStretch(1)
        if details.text():
            more = Qw.QPushButton("Details")

            def toggle(_checked=False, details=details, more=more):
                details.setVisible(not details.isVisible())
                more.setText("Hide details" if details.isVisible() else "Details")

            more.clicked.connect(toggle)
            actions.addWidget(more)
        copy = Qw.QPushButton("Copy")
        copy.clicked.connect(lambda: self._copy(copy, entry.get("text", "")))
        actions.addWidget(copy)
        column.addLayout(actions)
        line.addLayout(column, 1)

        # A click on the words copies them; a drag still selects a part. The
        # Copy button stays, because it is what the keyboard can reach, and
        # it is where the click says "Copied".
        phrase.setCursor(QtCore.Qt.PointingHandCursor)
        phrase.setToolTip("Click to copy, or drag to select part of it")
        row.click_copy = _ClickCopy(phrase, lambda: self._copy(copy, entry.get("text", "")))
        return row

    def _build(self, parent):
        from PySide6 import QtCore

        Qw = self.QtWidgets
        outer = Qw.QVBoxLayout(parent)
        outer.setContentsMargins(0, 8, 0, 0)

        self.scroll = Qw.QScrollArea()
        self.scroll.setWidgetResizable(True)
        self.list = Qw.QWidget()
        self.cards = Qw.QVBoxLayout(self.list)
        self.cards.setContentsMargins(0, 0, 8, 0)
        self.cards.setSpacing(2)
        self.scroll.setWidget(self.list)
        outer.addWidget(self.scroll, 1)

        command, where = _shortcut()
        hint = f"To dictate from a terminal, run {command}."
        if where is not None:
            hint += f" A desktop shortcut can go in {_home(where)}."
        foot = _quiet(Qw, hint, 11)
        foot.setWordWrap(True)
        foot.setTextInteractionFlags(QtCore.Qt.TextSelectableByMouse)
        outer.addWidget(foot)
        self._fill()

    def _fill(self) -> None:
        from PySide6 import QtCore

        Qw = self.QtWidgets
        while (item := self.cards.takeAt(0)) is not None:
            if item.widget() is not None:
                item.widget().deleteLater()
        if not self.entries:
            empty = _label(
                Qw,
                "Nothing dictated yet.\nPress your dictation shortcut and speak; the text shows up here.",
                f"color: {_QUIET}; font-family: {_SPOKEN}; font-size: 15px;",
            )
            empty.setWordWrap(True)
            empty.setAlignment(QtCore.Qt.AlignCenter)
            self.cards.addStretch(1)
            self.cards.addWidget(empty)
        day = None
        for entry in reversed(self.entries):
            if (heading := _day(entry.get("at", ""))) != day:
                day = heading
                label = _label(Qw, heading, f"color: {_BODY}; font-size: 13px; font-weight: 600;")
                label.setContentsMargins(8, 18 if self.cards.count() else 6, 0, 2)
                self.cards.addWidget(label)
            self.cards.addWidget(self._entry(entry))
        self.cards.addStretch(1)

    def poll(self) -> None:
        """Re-reads the daemon and the history; rebuilds the list only when a
        phrase arrived, so a scroll position or an open details line survives."""
        before = [entry.get("at") for entry in self.entries], len(self.entries)
        self._refresh()
        if ([entry.get("at") for entry in self.entries], len(self.entries)) != before:
            self._fill()
            self.scroll.verticalScrollBar().setValue(0)


class Window:
    """One window: a header that says whether the machine is listening, then
    the phrases and the settings as two tabs."""

    POLL_MS = 1500

    def __init__(self):
        self.QtWidgets = _qt()
        from PySide6 import QtCore

        Qw = self.QtWidgets
        # The application instance owns the platform plugin and must outlive
        # the widgets, so it is kept on the window, not in a local.
        self.app = Qw.QApplication.instance() or Qw.QApplication(sys.argv)
        self.app.setStyle("Fusion")
        self.app.setWindowIcon(_svg_icon(APP_ICON_SVG))
        # Matches the .desktop file's name, so the dock groups the window
        # under the launcher icon instead of a generic one.
        self.app.setDesktopFileName("talk")
        self.window = Qw.QWidget(windowTitle="talk")
        self.window.setStyleSheet(_style())
        self.window.resize(640, 720)
        self.window.setMinimumSize(480, 480)

        spine = Qw.QVBoxLayout(self.window)
        spine.setContentsMargins(24, 20, 24, 16)
        spine.setSpacing(10)
        spine.addLayout(self._header())

        tabs = Qw.QTabWidget()
        tabs.setDocumentMode(True)
        tabs.tabBar().setDrawBase(False)
        # The live state opens the window: the first thing someone at a desk
        # wants to know is whether the machine is listening to them, and the
        # knobs are the rare thing this window is ever opened to change.
        self.panel = Panel(self._section(tabs, "History"))
        self.configurator = Configurator(self._section(tabs, "Settings"))
        spine.addWidget(tabs, 1)
        self._shown: tuple | None = None
        self._show_state()

        # The panel's tray (COSMIC's status area speaks StatusNotifierItem,
        # which Qt uses on its own). With it, closing the window only hides
        # it: the icon is what keeps saying whether the machine is listening.
        self.tray = Tray(self) if Qw.QSystemTrayIcon.isSystemTrayAvailable() else None
        self.app.setQuitOnLastWindowClosed(self.tray is None)

        self.timer = QtCore.QTimer(self.window)
        self.timer.timeout.connect(self._tick)
        self.timer.start(self.POLL_MS)

    def _header(self):
        """The glyph from the tray, the name, the engine; the state on the right.

        The glyph's dot is the same one the panel shows, so the window and the
        tray say the state the same way.
        """
        from PySide6 import QtCore

        Qw = self.QtWidgets
        head = Qw.QHBoxLayout()
        head.setSpacing(12)
        self.glyph = Qw.QLabel()
        self.glyph.setFixedSize(34, 34)
        head.addWidget(self.glyph, 0, QtCore.Qt.AlignTop)

        words = Qw.QVBoxLayout()
        words.setSpacing(0)
        words.addWidget(_label(Qw, "talk", f"color: {_PAPER}; font-family: {_SPOKEN}; "
                                           "font-size: 26px; font-style: italic;"))
        self.engine = _quiet(Qw, _summary(_current(_read_config_text())))
        words.addWidget(self.engine)
        head.addLayout(words)
        head.addStretch(1)

        self.state = _label(Qw, "")
        head.addWidget(self.state, 0, QtCore.Qt.AlignTop)
        return head

    def _section(self, tabs, title: str):
        """A container per tab: a widget owns exactly one layout, so the two
        sections never fight over the window's own."""
        holder = self.QtWidgets.QWidget()
        tabs.addTab(holder, title)
        return holder

    def _show_state(self) -> None:
        head, *rest = _state_lines(self.panel.state)
        words, color = _STATE_WORDS.get(head, (head, _QUIET))
        self.state.setToolTip("\n".join(rest))
        if self._shown == (words, color):
            return
        self._shown = (words, color)
        self.glyph.setPixmap(_svg_icon(_tray_svg(color), sizes=(34, 68)).pixmap(34, 34))
        self.state.setStyleSheet(f"color: {color}; font-size: 14px; font-weight: 600; padding-top: 8px;")
        self.state.setText(words.capitalize())

    def _tick(self) -> None:
        self.panel.poll()
        self._show_state()
        self.engine.setText(_summary(_current(_read_config_text())))
        if self.tray is not None:
            self.tray.show_state(self.panel.state, self.panel.entries)

    def raise_(self) -> None:
        """Brings the window back from the tray, in front of whatever is there."""
        self.window.showNormal()
        self.window.raise_()
        self.window.activateWindow()

    def toggle(self) -> None:
        if self.window.isVisible() and self.window.isActiveWindow():
            self.window.hide()
        else:
            self.raise_()

    def save(self) -> str:
        return self.configurator.save()


def _capsule(x: float, top: float, bottom: float, r: float = 1.9) -> str:
    """One waveform bar as a closed subpath, so it can punch a hole."""
    return (f"M{x - r},{top + r} a{r},{r} 0 0 1 {2 * r},0 V{bottom - r} "
            f"a{r},{r} 0 0 1 {-2 * r},0 Z")


# A speech bubble with a waveform cut out of it: "talk", said in one shape.
# The bars are holes (evenodd), not dark paint, so the panel shows through
# whatever its colour is - QtSvg speaks SVG Tiny, which has no masks.
_BARS = " ".join(
    _capsule(x, 27 - h / 2, 27 + h / 2)
    for x, h in ((20, 8), (26, 16), (32, 23), (38, 14), (44, 8))
)
_BUBBLE = (
    "M20,8 H44 A14,14 0 0 1 58,22 V32 A14,14 0 0 1 44,46 H29 L17,56 L19,46 "
    "H20 A14,14 0 0 1 6,32 V22 A14,14 0 0 1 20,8 Z " + _BARS
)


def _tray_svg(color: str) -> str:
    """The panel glyph: light ink like COSMIC's own symbolic icons, and the
    state as a dot in the hue the window's pill uses, so they never disagree."""
    return (
        '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 64">'
        f'<path fill="{_PAPER}" fill-rule="evenodd" d="{_BUBBLE}"/>'
        f'<circle cx="51" cy="51" r="11" fill="{_INK}"/>'
        f'<circle cx="51" cy="51" r="8" fill="{color}"/>'
        "</svg>"
    )


# The launcher icon: the same bubble, white on a petrol tile, for the places
# that show colour icons (app grid, window switcher, the autostart entry).
APP_ICON_SVG = (
    '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 64">'
    '<defs><linearGradient id="g" x1="0" y1="0" x2="0" y2="1">'
    '<stop offset="0" stop-color="#4f93a8"/><stop offset="1" stop-color="#2b5f71"/>'
    "</linearGradient></defs>"
    '<rect x="2" y="2" width="60" height="60" rx="15" fill="url(#g)"/>'
    '<g transform="translate(9.6 11.2) scale(0.7)">'
    f'<path fill="#ffffff" fill-rule="evenodd" d="{_BUBBLE}"/></g>'
    "</svg>"
)


def _svg_icon(svg: str, sizes=(16, 22, 24, 32, 48, 64, 128)):
    """Rendered per size, not scaled from one bitmap: the bars stay crisp at
    the panel's 22 px as well as in the app grid."""
    from PySide6 import QtCore, QtGui, QtSvg

    renderer = QtSvg.QSvgRenderer(QtCore.QByteArray(svg.encode()))
    icon = QtGui.QIcon()
    for size in sizes:
        image = QtGui.QImage(size, size, QtGui.QImage.Format_ARGB32_Premultiplied)
        image.fill(QtCore.Qt.transparent)
        painter = QtGui.QPainter(image)
        painter.setRenderHint(QtGui.QPainter.Antialiasing)
        renderer.render(painter)
        painter.end()
        icon.addPixmap(QtGui.QPixmap.fromImage(image))
    return icon


def _tray_icon(color: str):
    return _svg_icon(_tray_svg(color))


class Tray:
    """The talk icon in the panel: its dot is the daemon's state, a click
    opens the window, and the menu re-copies the last thing said."""

    def __init__(self, window: Window):
        Qw = window.QtWidgets
        self.window = window
        self.icon = Qw.QSystemTrayIcon(window.app)
        self.icon.activated.connect(self._activated)

        menu = Qw.QMenu()
        self.status = menu.addAction("")
        self.status.setEnabled(False)
        menu.addSeparator()
        menu.addAction("Open talk", window.raise_)
        self.last = menu.addAction("Copy last dictation", self._copy_last)
        menu.addSeparator()
        menu.addAction("Quit", window.app.quit)
        # The menu must outlive this call: the icon only borrows it.
        self.menu = menu
        self.icon.setContextMenu(menu)

        self._shown: tuple | None = None
        self.show_state(window.panel.state, window.panel.entries)
        self.icon.show()

    def show_state(self, state: dict | None, entries: list[dict]) -> None:
        head, *_rest = _state_lines(state)
        words, color = _STATE_WORDS.get(head, (head, _QUIET))
        self.last.setEnabled(bool(entries))
        if self._shown == (words, color):
            return  # repainting the icon every tick makes some panels flicker
        self._shown = (words, color)
        self.icon.setIcon(_tray_icon(color))
        self.icon.setToolTip(f"talk: {words}")
        self.status.setText(f"talk is {words}")

    def _copy_last(self) -> None:
        if self.window.panel.entries:
            self.window.panel._recopy(self.window.panel.entries[-1].get("text", ""))

    def _activated(self, reason) -> None:
        Qw = self.window.QtWidgets
        if reason in (Qw.QSystemTrayIcon.Trigger, Qw.QSystemTrayIcon.DoubleClick):
            self.window.toggle()


def main(argv: list[str] | None = None) -> int:
    argv = sys.argv[1:] if argv is None else argv
    try:
        app = Window()
    except GuiError as exc:
        print(f"[gui] {exc}", file=sys.stderr)
        return 1
    # --tray starts in the panel only, which is what a login autostart wants;
    # without a tray to live in, the window is the only thing there is to show.
    if "--tray" not in argv or app.tray is None:
        app.window.show()
    # show() only maps the window: a process that returns right away destroys it
    # before it ever reaches the screen, so the event loop has to run here and
    # end only when the window is closed.
    return app.app.exec()


if __name__ == "__main__":
    sys.exit(main())
