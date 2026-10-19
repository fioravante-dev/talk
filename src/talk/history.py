"""Persistent text history — what was dictated, and where it went.

JSONL, one line per successful delivery, under the same cache directory the
daemon already uses for recordings. The GUI reads it to offer a re-copy of
something the clipboard has already forgotten.

The "only when it actually arrived" rule lives here rather than at the call
sites, so the CLI and the daemon cannot drift apart on what counts as success.
"""

from __future__ import annotations

import json
import os
from datetime import datetime
from pathlib import Path

HISTORY_KEEP = 500

HISTORY_PATH = (
    Path(os.environ.get("XDG_CACHE_HOME") or (Path.home() / ".cache"))
    / "talk"
    / "history.jsonl"
)


def record(
    text: str,
    *,
    delivered: list[str],
    language: str = "",
    motor: str = "",
    audio_seconds: float = 0.0,
    stt_seconds: float = 0.0,
) -> bool:
    """Appends only when something reached a destination and there is text."""
    if not text or not delivered:
        return False

    HISTORY_PATH.parent.mkdir(parents=True, exist_ok=True)
    entry = {
        "at": datetime.now().strftime("%Y-%m-%dT%H:%M:%S"),
        "text": text,
        "language": language,
        "motor": motor,
        "delivered": list(delivered),
        "audio_seconds": round(audio_seconds, 2),
        "stt_seconds": round(stt_seconds, 2),
    }
    with HISTORY_PATH.open("a") as fh:
        fh.write(json.dumps(entry, ensure_ascii=False) + "\n")

    _prune()
    return True


def recent(limit: int = 20) -> list[dict]:
    """Newest entries, oldest first. A torn last line is skipped, not fatal.

    The file is appended by one process and read by another (the GUI), and a
    kill mid-write leaves exactly that: half a line. Failing here would mean a
    dictation lost its history entry for the next one.
    """
    try:
        lines = HISTORY_PATH.read_text().splitlines()
    except FileNotFoundError:
        return []

    out: list[dict] = []
    for line in reversed(lines):
        try:
            entry = json.loads(line)
        except json.JSONDecodeError:
            continue
        if isinstance(entry, dict):
            out.append(entry)
        if len(out) >= limit:
            break

    out.reverse()
    return out


def _prune() -> None:
    """Keeps the file bounded: a year of dictation must not grow unbounded."""
    lines = HISTORY_PATH.read_text().splitlines()
    if len(lines) <= HISTORY_KEEP:
        return
    HISTORY_PATH.write_text("\n".join(lines[-HISTORY_KEEP:]) + "\n")
