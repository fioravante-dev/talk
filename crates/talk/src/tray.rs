//! O ícone na bandeja (StatusNotifierItem, que o painel do COSMIC fala).
//!
//! Desenhado à mão em ARGB: um ponto que fica vermelho e pulsa gravando, âmbar
//! transcrevendo. O visual de verdade vem com o redesign (fase 2).

use std::sync::mpsc::Sender;

use ksni::menu::StandardItem;
use ksni::{Icon, MenuItem};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Look {
    Idle,
    Recording { bright: bool },
    Transcribing,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Toggle,
    Open,
    CopyLast,
    Quit,
}

#[derive(Debug)]
pub struct Tray {
    pub look: Look,
    pub tooltip: String,
    pub actions: Sender<Action>,
}

fn dot(rgb: (u8, u8, u8), alpha: u8) -> Icon {
    let size = 32i32;
    let mut data = Vec::with_capacity((size * size * 4) as usize);
    let c = (size as f32 - 1.0) / 2.0;
    for y in 0..size {
        for x in 0..size {
            let d = ((x as f32 - c).powi(2) + (y as f32 - c).powi(2)).sqrt();
            // Anel fino por fora, ponto cheio por dentro, bordas suavizadas.
            let ring = (1.0 - ((d - 13.5).abs() - 1.0).clamp(0.0, 1.0)) * 0.55;
            let fill = 1.0 - (d - 8.0).clamp(0.0, 1.0);
            let a = (ring.max(fill) * f32::from(alpha)) as u8;
            data.extend_from_slice(&[a, rgb.0, rgb.1, rgb.2]);
        }
    }
    Icon { width: size, height: size, data }
}

impl ksni::Tray for Tray {
    fn id(&self) -> String {
        "talk-rs".into()
    }
    fn title(&self) -> String {
        "talk".into()
    }
    fn icon_pixmap(&self) -> Vec<Icon> {
        vec![match self.look {
            Look::Idle => dot((0xe9, 0xee, 0xf8), 0xd0),
            Look::Recording { bright } => dot((0xff, 0x3b, 0x30), if bright { 0xff } else { 0x90 }),
            Look::Transcribing => dot((0xff, 0xb0, 0x20), 0xff),
        }]
    }
    fn tool_tip(&self) -> ksni::ToolTip {
        ksni::ToolTip { title: "talk".into(), description: self.tooltip.clone(), ..Default::default() }
    }
    /// Clique no ícone: começa ou termina a ditada, como o atalho.
    fn activate(&mut self, _x: i32, _y: i32) {
        let _ = self.actions.send(Action::Toggle);
    }
    fn menu(&self) -> Vec<MenuItem<Self>> {
        let item = |label: &str, action: Action| -> MenuItem<Self> {
            StandardItem {
                label: label.into(),
                activate: Box::new(move |t: &mut Self| {
                    let _ = t.actions.send(action);
                }),
                ..Default::default()
            }
            .into()
        };
        let toggle = if matches!(self.look, Look::Recording { .. }) { "Parar e colar" } else { "Ditar" };
        vec![
            item(toggle, Action::Toggle),
            item("Abrir talk", Action::Open),
            item("Copiar a última ditada", Action::CopyLast),
            MenuItem::Separator,
            item("Sair", Action::Quit),
        ]
    }
}
