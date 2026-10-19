"""Slice 7/8 — the panel renders the daemon's own state, and never invents a shortcut.

The status lines come from daemon.ping(), so the panel renders a fact the daemon
already owns; a second status source here would be a second notion of what the
daemon is doing, and those two notions are where a stale panel comes from.

The history lines are the point of the panel: the clipboard has already
forgotten the dictation, so the text must arrive verbatim for a re-copy to be
worth anything.

The shortcut is the other half: writing a desktop shortcut on a machine that
cannot emit a paste key leaves an icon that does nothing, which is worse than
the exact command the user can copy instead.
"""

import shutil
import pytest

from talk import gui, history


def test_panel_says_stopped_when_there_is_no_daemon():
    assert gui._panel(None, []) == ["daemon stopped"]


def test_panel_shows_the_state_the_daemon_reports():
    """The payload shape is the daemon's own ping, copied from its handle()."""
    state = {"status": "ok", "recording": True, "code": 0, "wake": False,
             "wake_model": "", "reading": False}
    assert gui._panel(state, []) == ["daemon recording"]


def test_panel_names_the_wake_word_only_when_it_listens():
    quiet = {"status": "ok", "recording": False, "code": 0, "wake": False,
             "wake_model": "", "reading": False}
    assert gui._panel(quiet, []) == ["daemon ready"]

    listening = dict(quiet, wake=True, wake_model="hey_jarvis")
    assert gui._panel(listening, []) == ["daemon ready", "listening for 'hey_jarvis'"]


def test_history_renders_even_with_the_daemon_stopped():
    """The re-copy exists precisely for when nothing is running."""
    entries = [{"at": "2026-10-15T10:00:00", "text": "hello there",
                "language": "en", "motor": "qwen", "delivered": ["paste"],
                "audio_seconds": 1.0, "stt_seconds": 0.5}]
    lines = gui._panel(None, entries)
    assert lines[0] == "daemon stopped"
    assert lines[-1].endswith("hello there")


def test_the_phrase_line_keeps_the_text_verbatim():
    """A copy button hands this line's tail back, so nothing may decorate it."""
    entry = {"at": "2026-10-15T10:00:00", "text": "  spaced  ", "language": "pt"}
    assert gui._phrase_line(entry) == "10:00:00    spaced  "


def test_the_capture_facts_live_behind_the_details_button():
    """The metadata moved off the phrase line, it did not vanish with it."""
    entry = {"at": "2026-10-15T10:00:00", "text": "hello there",
             "language": "en", "motor": "qwen", "delivered": ["paste"],
             "audio_seconds": 1.0, "stt_seconds": 0.5}
    details = gui._details_lines(entry)
    assert "went to paste" in details
    assert "by qwen" in details
    assert "1.0s of audio, 0.5s to transcribe" in details
    assert "heard in English" in details


def test_details_survive_an_entry_missing_every_fact():
    """An old entry may carry none of it: the button says nothing, not "None"."""
    assert gui._details_lines({"text": "hello"}) == []


def test_the_panel_translates_the_daemon_words():
    """A user reads whether the machine is listening, not the daemon's vocabulary."""
    heads = {"daemon recording", "daemon transcribing", "daemon reading aloud",
             "daemon ready", "daemon stopped"}
    assert set(gui._STATE_WORDS) == heads
    assert all(not words.startswith("daemon") for words, _color in gui._STATE_WORDS.values())


def test_the_command_is_exact_enough_to_copy():
    command, _ = gui._shortcut()
    assert command == "talk --loop --to paste"


def test_no_shortcut_without_the_tools():
    """No typing tool here: the panel must hand over the command instead."""
    if any(shutil.which(t) for t in ("ydotool", "xdotool", "wtype")):
        pytest.skip("a typing tool is installed on this machine")
    _, shortcut = gui._shortcut()
    assert shortcut is None


def test_a_recording_says_how_long_it_has_been_listening():
    """A red light raises one question, and the stopwatch answers it."""
    state = {"status": "ok", "recording": True, "recording_secs": 67.4,
             "transcribing": False, "wake": False, "reading": False}
    words, color, recording = gui._state_words(state)
    assert (words, recording) == ("recording 1:07", True)
    assert color == gui._REC


def test_an_older_daemon_without_the_stopwatch_still_says_recording():
    state = {"status": "ok", "recording": True, "wake": False, "reading": False}
    assert gui._state_words(state)[0] == "recording"


def test_transcribing_is_its_own_state_between_recording_and_ready():
    """The gap after the key and before the text is where it looked frozen."""
    state = {"status": "ok", "recording": False, "transcribing": True,
             "wake": False, "reading": False}
    assert gui._state_lines(state) == ["daemon transcribing"]
    assert gui._state_words(state)[:1] == ("transcribing",)


def test_the_clock_reads_like_a_stopwatch():
    assert [gui._clock(s) for s in (0, 7.9, 60, 92)] == ["0:00", "0:07", "1:00", "1:32"]
    assert gui._clock(None) == ""
