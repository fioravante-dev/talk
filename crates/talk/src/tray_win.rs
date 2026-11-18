//! O ícone na bandeja do Windows (`tray-icon`), com a mesma cara do `tray.rs`:
//! um ponto que fica vermelho e pulsa gravando, âmbar transcrevendo; clique
//! esquerdo dita, o direito abre o menu.
//!
//! O Windows só entrega as mensagens do ícone na thread que o criou, e essa
//! thread precisa de um laço de mensagens: o do `tao`, numa thread só dela. O
//! serviço fala com ela por um `EventLoopProxy`, que é `Send`.

use std::marker::PhantomData;
use std::sync::mpsc::{self, Sender};
use std::sync::{Mutex, PoisonError};

use tao::event::Event;
use tao::event_loop::{ControlFlow, EventLoopBuilder, EventLoopProxy};
use tao::platform::windows::EventLoopBuilderExtWindows;
use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};

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
    Update,
    Quit,
}

#[derive(Debug)]
pub struct Tray {
    pub look: Look,
    pub tooltip: String,
    /// A versão nova que espera: aparece "Atualizar para ..." no menu.
    pub update: Option<String>,
    pub actions: Sender<Action>,
}

/// O que a thread do ícone precisa para se redesenhar.
#[derive(Debug, Clone)]
struct Paint {
    look: Look,
    tooltip: String,
    update: Option<String>,
}

/// O lado do serviço: guarda o estado e avisa a thread do ícone a cada mudança.
pub struct Handle<T> {
    state: Mutex<Tray>,
    proxy: EventLoopProxy<Paint>,
    _t: PhantomData<T>,
}

impl Handle<Tray> {
    pub fn update(&self, f: impl FnOnce(&mut Tray)) {
        let mut t = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        f(&mut t);
        let _ = self.proxy.send_event(Paint { look: t.look, tooltip: t.tooltip.clone(), update: t.update.clone() });
    }
}

pub trait TrayMethods: Sized {
    fn spawn(self) -> Result<Handle<Self>, String>;
}

/// O ponto, em RGBA. Diferente do Linux, a barra do Windows pode ser clara:
/// um contorno escuro fino mantém o ponto claro de "pronto" visível nas duas.
fn dot(rgb: (u8, u8, u8), alpha: u8) -> Icon {
    let size = 32u32;
    let mut data = Vec::with_capacity((size * size * 4) as usize);
    let c = (size as f32 - 1.0) / 2.0;
    for y in 0..size {
        for x in 0..size {
            let d = ((x as f32 - c).powi(2) + (y as f32 - c).powi(2)).sqrt();
            let ring = (1.0 - ((d - 13.5).abs() - 1.0).clamp(0.0, 1.0)) * 0.55;
            let fill = 1.0 - (d - 8.0).clamp(0.0, 1.0);
            let edge = (1.0 - ((d - 9.0).abs() - 0.5).clamp(0.0, 1.0)) * 0.6;
            let a = ring.max(fill).max(edge);
            // Dentro do ponto, a cor; no contorno, quase preto.
            let k = if fill > 0.5 { 1.0 } else if edge > ring { 0.15 } else { 1.0 };
            let px = |v: u8| (f32::from(v) * k) as u8;
            data.extend_from_slice(&[px(rgb.0), px(rgb.1), px(rgb.2), (a * f32::from(alpha)) as u8]);
        }
    }
    Icon::from_rgba(data, size, size).unwrap_or_else(|_| unreachable_icon())
}

/// 32×32 RGBA sempre é válido; isto só existe para não ter `unwrap`.
fn unreachable_icon() -> Icon {
    Icon::from_rgba(vec![0; 4], 1, 1).unwrap_or_else(|_| std::process::abort())
}

fn icon_for(look: Look) -> Icon {
    match look {
        Look::Idle => dot((0xe9, 0xee, 0xf8), 0xff),
        Look::Recording { bright } => dot((0xff, 0x3b, 0x30), if bright { 0xff } else { 0x90 }),
        Look::Transcribing => dot((0xff, 0xb0, 0x20), 0xff),
    }
}

impl TrayMethods for Tray {
    fn spawn(self) -> Result<Handle<Self>, String> {
        let (ready_tx, ready_rx) = mpsc::channel::<Result<EventLoopProxy<Paint>, String>>();
        let actions = self.actions.clone();
        let first = Paint { look: self.look, tooltip: self.tooltip.clone(), update: self.update.clone() };
        std::thread::spawn(move || {
            let event_loop = EventLoopBuilder::<Paint>::with_user_event().with_any_thread(true).build();
            let toggle = MenuItem::with_id("toggle", "Ditar", true, None);
            // Entra no menu (antes do "Sair") só quando há versão nova.
            let update = MenuItem::with_id("update", "Atualizar", true, None);
            let mut update_shown = false;
            let menu = Menu::new();
            let built = menu
                .append_items(&[
                    &toggle,
                    &MenuItem::with_id("open", "Abrir talk", true, None),
                    &MenuItem::with_id("copy", "Copiar a última ditada", true, None),
                    &PredefinedMenuItem::separator(),
                    &MenuItem::with_id("quit", "Sair", true, None),
                ])
                .map_err(|e| e.to_string())
                .and_then(|()| {
                    TrayIconBuilder::new()
                        .with_icon(icon_for(first.look))
                        .with_tooltip(format!("talk: {}", first.tooltip))
                        .with_menu(Box::new(menu.clone()))
                        .with_menu_on_left_click(false)
                        .build()
                        .map_err(|e| e.to_string())
                });
            let tray = match built {
                Ok(t) => t,
                Err(e) => {
                    let _ = ready_tx.send(Err(e));
                    return;
                }
            };

            let menu_actions = actions.clone();
            MenuEvent::set_event_handler(Some(move |e: MenuEvent| {
                let action = match e.id.0.as_str() {
                    "toggle" => Action::Toggle,
                    "open" => Action::Open,
                    "copy" => Action::CopyLast,
                    "update" => Action::Update,
                    "quit" => Action::Quit,
                    _ => return,
                };
                let _ = menu_actions.send(action);
            }));
            TrayIconEvent::set_event_handler(Some(move |e: TrayIconEvent| {
                if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = e {
                    let _ = actions.send(Action::Toggle);
                }
            }));

            let _ = ready_tx.send(Ok(event_loop.create_proxy()));
            // Os quatro desenhos ficam prontos: o pulso troca de ícone a cada 500 ms.
            let icons = [
                (Look::Idle, icon_for(Look::Idle)),
                (Look::Recording { bright: true }, icon_for(Look::Recording { bright: true })),
                (Look::Recording { bright: false }, icon_for(Look::Recording { bright: false })),
                (Look::Transcribing, icon_for(Look::Transcribing)),
            ];
            event_loop.run(move |event, _, flow| {
                *flow = ControlFlow::Wait;
                if let Event::UserEvent(p) = event {
                    if let Some((_, icon)) = icons.iter().find(|(l, _)| *l == p.look) {
                        let _ = tray.set_icon(Some(icon.clone()));
                    }
                    let _ = tray.set_tooltip(Some(format!("talk: {}", p.tooltip)));
                    toggle.set_text(if matches!(p.look, Look::Recording { .. }) { "Parar e colar" } else { "Ditar" });
                    match &p.update {
                        Some(v) => {
                            update.set_text(format!("Atualizar para {v}"));
                            // 4 = depois de ditar, abrir, copiar e o separador.
                            if !update_shown && menu.insert(&update, 4).is_ok() {
                                update_shown = true;
                            }
                        }
                        None if update_shown && menu.remove(&update).is_ok() => update_shown = false,
                        None => {}
                    }
                }
            });
        });
        let proxy = ready_rx.recv().map_err(|_| "a thread da bandeja morreu".to_owned())??;
        Ok(Handle { state: Mutex::new(self), proxy, _t: PhantomData })
    }
}
