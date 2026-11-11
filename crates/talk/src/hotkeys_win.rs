//! Os atalhos no Windows. Lá não há atalho do sistema que rode um comando
//! (no Linux o COSMIC chama `talk-rs toggle`), então o próprio serviço
//! registra as teclas, uma por idioma, a partir do `[hotkeys]` do config.
//!
//! O `RegisterHotKey` só entrega a tecla na thread que a registrou: tudo aqui
//! vive numa thread só, presa no laço de mensagens.

use std::sync::Arc;

use anyhow::{Context, Result, anyhow};
use talk_core::config::Config;
use windows_hotkeys::keys::{ModKey, VKey};
use windows_hotkeys::singlethreaded::HotkeyManager;
use windows_hotkeys::HotkeyManagerImpl;

use crate::service::Service;

/// "ctrl+alt+d" → as modificadoras e a tecla. `NoRepeat`: segurar a tecla não
/// liga e desliga a gravação em rajada.
fn parse(combo: &str) -> Result<(VKey, Vec<ModKey>)> {
    let parts: Vec<&str> = combo.split('+').map(str::trim).collect();
    let (key, mods) = parts.split_last().ok_or_else(|| anyhow!("atalho vazio"))?;
    let mut mods = mods
        .iter()
        .map(|m| ModKey::from_keyname(m).map_err(|e| anyhow!("{combo:?}: {e}")))
        .collect::<Result<Vec<_>>>()?;
    mods.push(ModKey::NoRepeat);
    let key = VKey::from_keyname(key).map_err(|e| anyhow!("{combo:?}: {e}"))?;
    Ok((key, mods))
}

pub fn spawn(service: Arc<Service>, cfg: &Config) {
    let wanted = cfg.hotkeys.clone();
    std::thread::spawn(move || {
        let mut hkm = HotkeyManager::<()>::new();
        for (combo, language) in wanted {
            let me = service.clone();
            let registered = parse(&combo).and_then(|(key, mods)| {
                hkm.register(key, &mods, move || {
                    eprintln!("[talk] atalho apertado ({language})");
                    if let Err(e) = me.toggle(Some(language.clone())) {
                        eprintln!("[talk] {e:#}");
                    }
                })
                .map_err(|e| anyhow!("{e}"))
                .context("outro programa já usa esta tecla, ou o Windows a reserva")
            });
            match registered {
                Ok(_) => eprintln!("[talk] atalho {combo}"),
                Err(e) => eprintln!("[talk] sem o atalho {combo}: {e:#}"),
            }
        }
        hkm.event_loop();
    });
}
