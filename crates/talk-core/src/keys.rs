//! Um atalho de teclado como o talk o entende: modificadoras e uma tecla.
//!
//! É a mesma ideia nos dois lados, escrita de dois jeitos: no Windows o
//! `[hotkeys]` do config guarda `"ctrl+alt+d"`; no Linux o arquivo do COSMIC
//! guarda `modifiers: [Ctrl, Alt], key: "d"`. A janela edita um `Combo` e cada
//! lado o escreve no seu formato.

use anyhow::{Result, bail};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Combo {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    /// A tecla do Windows, que no Linux se chama Super.
    pub win: bool,
    /// Minúscula: "d", "5", "f9".
    pub key: String,
}

/// Letra, dígito ou F1 a F24, em minúscula; `None` para o resto.
fn normal_key(key: &str) -> Option<String> {
    let k = key.trim().to_lowercase();
    let mut chars = k.chars();
    match (chars.next(), chars.next()) {
        (Some(c), None) if c.is_ascii_alphanumeric() => Some(k),
        (Some('f'), Some(_)) => k[1..].parse::<u8>().ok().filter(|n| (1..=24).contains(n)).map(|_| k),
        _ => None,
    }
}

impl Combo {
    /// Valida o que veio da janela. Uma letra ou um dígito precisa de Ctrl,
    /// Alt ou Win: sozinho (ou só com Shift) ele roubaria a digitação.
    pub fn new(ctrl: bool, alt: bool, shift: bool, win: bool, key: &str) -> Result<Self> {
        let Some(key) = normal_key(key) else {
            bail!("a tecla tem que ser uma letra, um número ou de F1 a F24");
        };
        let function = key.len() > 1;
        if !function && !(ctrl || alt || win) {
            bail!("junto com uma letra ou um número, marque Ctrl, Alt ou {}", if cfg!(windows) { "Win" } else { "Super" });
        }
        Ok(Self { ctrl, alt, shift, win, key })
    }

    /// `"ctrl+alt+d"`, em qualquer ordem e caixa.
    pub fn parse(text: &str) -> Option<Self> {
        let parts: Vec<String> = text.split('+').map(|p| p.trim().to_lowercase()).collect();
        let (key, mods) = parts.split_last()?;
        let mut c = Self { key: normal_key(key)?, ..Self::default() };
        for m in mods {
            match m.as_str() {
                "ctrl" | "control" => c.ctrl = true,
                "alt" => c.alt = true,
                "shift" => c.shift = true,
                "win" | "windows" | "super" | "logo" => c.win = true,
                _ => return None,
            }
        }
        Some(c)
    }

    /// Como o `[hotkeys]` guarda: sempre na mesma ordem, para que
    /// "alt+ctrl+d" e "ctrl+alt+d" sejam o mesmo atalho.
    pub fn to_config(&self) -> String {
        let mut parts = self.mods("win");
        parts.push(&self.key);
        parts.join("+")
    }

    /// Ctrl, Alt, Shift, Win/Super (o nome que se passa), nesta ordem.
    fn mods<'a>(&self, win: &'a str) -> Vec<&'a str> {
        [(self.ctrl, "ctrl"), (self.alt, "alt"), (self.shift, "shift"), (self.win, win)]
            .into_iter()
            .filter_map(|(on, name)| on.then_some(name))
            .collect()
    }

    /// As modificadoras com os nomes do COSMIC.
    pub fn cosmic_mods(&self) -> Vec<&'static str> {
        [(self.win, "Super"), (self.ctrl, "Ctrl"), (self.alt, "Alt"), (self.shift, "Shift")]
            .into_iter()
            .filter_map(|(on, name)| on.then_some(name))
            .collect()
    }

    /// As teclas como se leem: Ctrl, Alt, D. `win` é o nome da tecla do
    /// sistema: "Win" no Windows, "Super" no Linux.
    pub fn labels(&self, win: &str) -> Vec<String> {
        let mut out: Vec<String> = [(self.ctrl, "Ctrl"), (self.alt, "Alt"), (self.shift, "Shift"), (self.win, win)]
            .into_iter()
            .filter(|(on, _)| *on)
            .map(|(_, name)| name.to_owned())
            .collect();
        out.push(self.key.to_uppercase());
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_order_and_case_do_not_matter() {
        let a = Combo::parse("Alt+CTRL+d").unwrap();
        assert_eq!(a, Combo::parse("ctrl+alt+D").unwrap());
        assert_eq!(a.to_config(), "ctrl+alt+d");
        assert_eq!(Combo::parse("super+shift+f9").unwrap().to_config(), "shift+win+f9");
    }

    #[test]
    fn reads_as_on_the_keyboard() {
        let c = Combo::parse("ctrl+alt+shift+d").unwrap();
        assert_eq!(c.labels("Win"), ["Ctrl", "Alt", "Shift", "D"]);
        assert_eq!(Combo::parse("win+alt+d").unwrap().cosmic_mods(), ["Super", "Alt"]);
    }

    #[test]
    fn a_letter_alone_would_steal_typing() {
        assert!(Combo::new(false, false, true, false, "d").is_err());
        assert!(Combo::new(true, false, false, false, "d").is_ok());
        assert!(Combo::new(false, false, false, false, "F9").is_ok(), "F9 sozinha não atrapalha");
        assert!(Combo::new(true, false, false, false, "ç").is_err());
        assert!(Combo::new(true, false, false, false, "f25").is_err());
        assert!(Combo::parse("ctrl+banana+d").is_none());
    }
}
