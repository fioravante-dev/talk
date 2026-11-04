//! O texto no cursor: área de transferência + Ctrl+V por um teclado virtual.
//!
//! O teclado é criado UMA vez, quando o serviço sobe. O ydotool 0.1 criava um
//! teclado novo a cada colagem e precisava esperar 250 ms o compositor
//! reconhecê-lo; um teclado que já existe não precisa.

use std::io::Write;
use std::process::{Command, Stdio};
use std::thread::sleep;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use evdev::uinput::VirtualDevice;
use evdev::{AttributeSet, EventType, InputEvent, KeyCode};

pub struct Keyboard {
    device: VirtualDevice,
}

/// "ctrl+v", "ctrl+shift+v": as teclas na ordem de apertar.
pub fn parse_combo(combo: &str) -> Result<Vec<KeyCode>> {
    combo
        .split('+')
        .map(|k| match k.trim().to_lowercase().as_str() {
            "ctrl" | "control" => Ok(KeyCode::KEY_LEFTCTRL),
            "shift" => Ok(KeyCode::KEY_LEFTSHIFT),
            "alt" => Ok(KeyCode::KEY_LEFTALT),
            "super" => Ok(KeyCode::KEY_LEFTMETA),
            "v" => Ok(KeyCode::KEY_V),
            "insert" => Ok(KeyCode::KEY_INSERT),
            other => bail!("tecla desconhecida em paste_key: {other:?}"),
        })
        .collect()
}

impl Keyboard {
    pub fn new() -> Result<Self> {
        let mut keys = AttributeSet::<KeyCode>::new();
        for k in [
            KeyCode::KEY_LEFTCTRL,
            KeyCode::KEY_LEFTSHIFT,
            KeyCode::KEY_LEFTALT,
            KeyCode::KEY_LEFTMETA,
            KeyCode::KEY_V,
            KeyCode::KEY_INSERT,
        ] {
            keys.insert(k);
        }
        let device = VirtualDevice::builder()
            .context("sem acesso ao /dev/uinput (veja o README, permissão do uinput)")?
            .name("talk virtual keyboard")
            .with_keys(&keys)?
            .build()?;
        Ok(Self { device })
    }

    pub fn press(&mut self, combo: &[KeyCode]) -> Result<()> {
        let key = |k: KeyCode, v: i32| InputEvent::new(EventType::KEY.0, k.code(), v);
        for k in combo {
            self.device.emit(&[key(*k, 1)])?;
        }
        sleep(Duration::from_millis(8));
        for k in combo.iter().rev() {
            self.device.emit(&[key(*k, 0)])?;
        }
        Ok(())
    }
}

/// Copia e espera a cópia valer. O `wl-copy` volta antes de o processo que
/// serve a seleção assumir; colar nesse intervalo cola o texto ANTERIOR.
pub fn copy(text: &str) -> Result<()> {
    let mut child = Command::new("wl-copy")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .context("wl-copy não encontrado (sudo apt install wl-clipboard)")?;
    child.stdin.take().context("wl-copy sem stdin")?.write_all(text.as_bytes())?;
    child.wait()?;

    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline {
        let got = Command::new("wl-paste").arg("-n").stderr(Stdio::null()).output()?;
        if got.stdout == text.as_bytes() {
            return Ok(());
        }
        sleep(Duration::from_millis(15));
    }
    bail!("a área de transferência não assumiu o texto em 2 s")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn combos_from_the_config() {
        assert_eq!(parse_combo("ctrl+v").unwrap(), [KeyCode::KEY_LEFTCTRL, KeyCode::KEY_V]);
        assert_eq!(
            parse_combo("Ctrl+Shift+V").unwrap(),
            [KeyCode::KEY_LEFTCTRL, KeyCode::KEY_LEFTSHIFT, KeyCode::KEY_V]
        );
        assert!(parse_combo("ctrl+x").is_err());
    }
}
