"""Slice 5 — the history is written only when the text actually arrived.

The history exists so the GUI can re-copy a dictation the clipboard already
forgot. A failed delivery must not add an entry: the recording file is kept for
that case already, and two overlapping notions of "what went wrong" is one too
many. The rule lives in the module, not at the call sites, so both the CLI and
the daemon get the same rule for free.
"""

import json

from talk import history


def test_record_appends_one_line_per_successful_delivery(tmp_path):
    history.HISTORY_PATH = tmp_path / "history.jsonl"

    history.record(
        "first",
        delivered=["paste"],
        language="pt",
        motor="qwen",
        audio_seconds=1.5,
        stt_seconds=0.4,
    )
    history.record(
        "second",
        delivered=["clipboard", "type"],
        language="en",
        motor="whisper",
        audio_seconds=2.0,
        stt_seconds=1.0,
    )

    lines = history.HISTORY_PATH.read_text().splitlines()
    assert len(lines) == 2
    first = json.loads(lines[0])
    assert first["text"] == "first"
    assert first["delivered"] == ["paste"]
    assert first["motor"] == "qwen"


def test_failed_delivery_writes_nothing(tmp_path):
    """Nothing reached a destination: there is nothing to re-copy."""
    history.HISTORY_PATH = tmp_path / "history.jsonl"
    history.record("lost", delivered=[], language="pt", motor="qwen")
    assert not history.HISTORY_PATH.exists()


def test_empty_text_writes_nothing(tmp_path):
    history.HISTORY_PATH = tmp_path / "history.jsonl"
    history.record("", delivered=["clipboard"], language="pt", motor="qwen")
    assert not history.HISTORY_PATH.exists()


def test_recent_is_newest_last_and_never_crashes_on_a_partial_line(tmp_path):
    """A torn last line is a crash waiting for the next dictation."""
    history.HISTORY_PATH = tmp_path / "history.jsonl"
    for n in range(3):
        history.record(f"entry {n}", delivered=["paste"], language="pt", motor="qwen")
    history.HISTORY_PATH.write_text(
        history.HISTORY_PATH.read_text() + '{"text": "torn'
    )

    entries = history.recent(limit=2)
    assert [e["text"] for e in entries] == ["entry 1", "entry 2"]


def test_history_stays_bounded(tmp_path):
    """A year of dictation must not grow into a megabyte the GUI reads at startup."""
    history.HISTORY_PATH = tmp_path / "history.jsonl"
    for n in range(history.HISTORY_KEEP + 10):
        history.record(f"entry {n}", delivered=["paste"], language="pt", motor="qwen")

    lines = history.HISTORY_PATH.read_text().splitlines()
    assert len(lines) == history.HISTORY_KEEP
    assert json.loads(lines[0])["text"] == f"entry {10}"
