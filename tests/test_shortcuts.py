"""The keyboard shortcuts live in COSMIC's own file, and the GUI edits them there.

A shortcut that pins `-l pt` turns English speech into Portuguese text: the
language a shortcut carries is a setting, so it has to be reachable from the
window, not only from a RON file nobody opens. The file also holds shortcuts
that are not ours (a screenshot tool here), and those must come back untouched.
"""

from talk import shortcuts

COSMIC = '''{
    (
        modifiers: [
            Super,
        ],
        key: "d",
        description: Some("voice-pt"),
    ): Spawn("/home/dev/projetos/talk/.venv/bin/talk -l pt"),
    (
        modifiers: [
            Super,
            Alt,
        ],
        key: "d",
        description: Some("voice-en"),
    ): Spawn("/home/dev/projetos/talk/.venv/bin/talk --language=en --to clipboard"),
    (
        modifiers: [
            Super,
            Ctrl,
        ],
        key: "d",
        description: Some("voice-read"),
    ): Spawn("/home/dev/projetos/talk/.venv/bin/talk --read"),
    (
        modifiers: [],
        key: "Print",
        description: Some("imgcpy-shot"),
    ): Spawn("/home/dev/imgcpy/imgcpy shot"),
}
'''


def test_only_the_dictation_shortcuts_are_offered():
    """Reading aloud has no spoken language, and imgcpy is not ours."""
    found = shortcuts.parse(COSMIC)
    assert [s.keys for s in found] == ["Super + D", "Super + Alt + D"]
    assert [s.language for s in found] == ["pt", "en"]
    assert found[0].description == "voice-pt"


def test_detect_is_written_as_auto():
    """`-l auto` says what it does; dropping the flag would hand the choice to
    whatever config.toml says, which is the thing that hid this bug."""
    out = shortcuts.set_language(COSMIC, shortcuts.parse(COSMIC)[0], "")
    assert 'Spawn("/home/dev/projetos/talk/.venv/bin/talk -l auto")' in out


def test_the_long_form_is_rewritten_in_place():
    out = shortcuts.set_language(COSMIC, shortcuts.parse(COSMIC)[1], "pt")
    assert "talk --language=pt --to clipboard" in out


def test_everything_else_comes_back_byte_identical():
    out = shortcuts.set_language(COSMIC, shortcuts.parse(COSMIC)[0], "en")
    assert out.replace("talk -l en\")", "talk -l pt\")") == COSMIC


def test_a_shortcut_without_a_language_gets_one():
    text = COSMIC.replace("talk -l pt", "talk --to paste")
    found = shortcuts.parse(text)
    assert found[0].language == ""
    out = shortcuts.set_language(text, found[0], "en")
    assert "talk -l en --to paste" in out


def test_a_flag_the_cli_no_longer_has_is_reported():
    """`-m email` died with the LLM cleanup; the shortcut fails on every press."""
    text = COSMIC.replace("talk -l pt", "talk -l pt -m email")
    assert shortcuts.parse(text)[0].broken == "-m"
    assert shortcuts.parse(COSMIC)[0].broken == ""


def test_save_keeps_a_backup_next_to_the_file(tmp_path):
    path = tmp_path / "custom"
    path.write_text(COSMIC)
    shortcuts.save(COSMIC.replace("-l pt", "-l auto"), path=path)
    assert "-l auto" in path.read_text()
    backups = list(tmp_path.glob("custom.bak-*"))
    assert len(backups) == 1 and backups[0].read_text() == COSMIC
