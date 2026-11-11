//! O texto no cursor, no Windows: área de transferência (`arboard`) + Ctrl+V
//! pelo `SendInput` (`enigo`). O mesmo contrato do `output.rs`.
//!
//! No Windows a área de transferência é do sistema: quando `set_text` volta, o
//! texto já vale, e não há a espera que o `wl-copy` exige.

use std::thread::sleep;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use enigo::{Direction, Enigo, Key, Keyboard as _, Settings};

/// Um `enigo` por colagem: no Windows criar é instantâneo (não há teclado
/// virtual a registrar, como no uinput), e assim nada precisa ser `Send`.
pub struct Keyboard;

/// "ctrl+v", "ctrl+shift+v": as teclas na ordem de apertar.
pub fn parse_combo(combo: &str) -> Result<Vec<Key>> {
    combo
        .split('+')
        .map(|k| match k.trim().to_lowercase().as_str() {
            "ctrl" | "control" => Ok(Key::Control),
            "shift" => Ok(Key::Shift),
            "alt" => Ok(Key::Alt),
            "super" => Ok(Key::Meta),
            "v" => Ok(Key::V),
            "insert" => Ok(Key::Insert),
            other => bail!("tecla desconhecida em paste_key: {other:?}"),
        })
        .collect()
}

impl Keyboard {
    pub fn new() -> Result<Self> {
        Ok(Self)
    }

    pub fn press(&mut self, combo: &[Key]) -> Result<()> {
        let mut enigo = Enigo::new(&Settings::default()).context("o Windows recusou o teclado simulado")?;
        for k in combo {
            enigo.key(*k, Direction::Press)?;
        }
        sleep(Duration::from_millis(8));
        for k in combo.iter().rev() {
            enigo.key(*k, Direction::Release)?;
        }
        Ok(())
    }
}

pub fn copy(text: &str) -> Result<()> {
    arboard::Clipboard::new()
        .and_then(|mut c| c.set_text(text))
        .context("a área de transferência do Windows recusou o texto")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn combos_from_the_config() {
        assert_eq!(parse_combo("ctrl+v").unwrap(), [Key::Control, Key::V]);
        assert_eq!(parse_combo("Ctrl+Shift+V").unwrap(), [Key::Control, Key::Shift, Key::V]);
        assert!(parse_combo("ctrl+x").is_err());
    }
}
