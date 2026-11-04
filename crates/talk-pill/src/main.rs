//! `talk-pill`: a pílula que aparece enquanto o talk grava.
//!
//! Uma superfície layer-shell no rodapé da tela: fica por cima de tudo, não
//! aceita teclado (o foco continua no app onde você está ditando) e some quando
//! a ditada termina. O serviço a abre no início da gravação; ela ouve os
//! eventos do serviço pelo socket e sai sozinha quando a fase volta a pronto.
//!
//! GTK nativo e cairo, sem motor web: ~30 MB, e só durante a gravação.

use std::cell::RefCell;
use std::f64::consts::PI;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::rc::Rc;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use gtk::cairo::{Context, FontSlant, FontWeight};
use gtk::prelude::*;
use gtk_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};
use talk_core::paths;

const W: i32 = 260;
/// Transcrevendo não há medidor: a pílula encolhe para o texto.
const W_TRANSCRIBING: f64 = 156.0;

/// A mesma Schibsted Grotesk da janela. O cairo acha fontes pelo fontconfig,
/// então ela é posta uma vez em ~/.local/share/fonts, antes de o GTK subir.
const FONT: &[u8] = include_bytes!("../../talk-ui/assets/schibsted-latin.woff2");
const FACE: &str = "Schibsted Grotesk";

fn install_font() {
    let Some(home) = std::env::var_os("HOME") else { return };
    let dir = std::path::Path::new(&home).join(".local/share/fonts/talk");
    let file = dir.join("schibsted-grotesk.woff2");
    if !file.exists() && std::fs::create_dir_all(&dir).is_ok() {
        let _ = std::fs::write(file, FONT);
    }
}
const H: i32 = 44;
const PAD: f64 = 8.0; // a sombra precisa de espaço em volta

#[derive(Default)]
struct State {
    phase: String,
    seconds: u64,
    level: f32,
    started: Option<Instant>,
}

fn rgb(cr: &Context, hex: u32, a: f64) {
    let c = |s: u32| f64::from((hex >> s) & 0xff) / 255.0;
    cr.set_source_rgba(c(16), c(8), c(0), a);
}

fn rounded(cr: &Context, x: f64, y: f64, w: f64, h: f64, r: f64) {
    cr.new_sub_path();
    cr.arc(x + w - r, y + r, r, -PI / 2.0, 0.0);
    cr.arc(x + w - r, y + h - r, r, 0.0, PI / 2.0);
    cr.arc(x + r, y + h - r, r, PI / 2.0, PI);
    cr.arc(x + r, y + r, r, PI, 1.5 * PI);
    cr.close_path();
}

/// Mesmo mapeamento do medidor da janela: RMS em dB, 12 segmentos.
fn lit_segments(level: f32, n: usize) -> usize {
    let db = if level > 0.0 { 20.0 * level.log10() } else { -60.0 };
    (((db + 50.0) / 45.0 * n as f32).round().clamp(0.0, n as f32)) as usize
}

fn draw(cr: &Context, st: &State) {
    cr.set_operator(gtk::cairo::Operator::Source);
    cr.set_source_rgba(0.0, 0.0, 0.0, 0.0);
    let _ = cr.paint();
    cr.set_operator(gtk::cairo::Operator::Over);

    let recording = st.phase == "recording";
    let full = f64::from(W) - 2.0 * PAD;
    let w = if recording { full } else { W_TRANSCRIBING };
    let (x, y, h) = (PAD + (full - w) / 2.0, PAD, f64::from(H) - 2.0 * PAD);
    // sombra curta e macia, com deslocamento: o aparelho pousado sobre a tela
    for (i, a) in [(3.0, 0.05), (2.0, 0.07), (1.0, 0.09)] {
        rounded(cr, x - i + 1.0, y - i + 2.5, w + 2.0 * i - 2.0, h + 2.0 * i - 2.0, h / 2.0 + i);
        rgb(cr, 0x2a2a28, a);
        let _ = cr.fill();
    }
    rounded(cr, x, y, w, h, h / 2.0);
    rgb(cr, 0xeeefed, 1.0);
    let _ = cr.fill_preserve();
    cr.set_line_width(1.0);
    rgb(cr, 0xc3c5c2, 1.0);
    let _ = cr.stroke();

    let pulse = st.started.map_or(1.0, |t| 0.55 + 0.45 * (t.elapsed().as_secs_f64() * PI).cos().abs());
    let (lamp, alpha) = if recording { (0xd8442e, pulse) } else { (0xc98a1b, 1.0) };
    cr.arc(x + 18.0, y + h / 2.0, 5.0, 0.0, 2.0 * PI);
    rgb(cr, lamp, alpha);
    let _ = cr.fill();

    cr.select_font_face(FACE, FontSlant::Normal, FontWeight::Normal);
    cr.set_font_size(12.5);
    rgb(cr, 0x2a2a28, 1.0);
    let label = if recording {
        format!("gravando {}:{:02}", st.seconds / 60, st.seconds % 60)
    } else {
        "transcrevendo".to_owned()
    };
    cr.move_to(x + 32.0, y + h / 2.0 + 4.5);
    let _ = cr.show_text(&label);

    if recording {
        let n = 12;
        let lit = lit_segments(st.level, n);
        for i in 0..n {
            let sx = x + w - 18.0 - (n - i) as f64 * 7.0;
            cr.rectangle(sx, y + h / 2.0 - 4.0, 5.0, 8.0);
            let color = if i >= lit { 0xc3c5c2 } else if i >= 10 { 0xd8442e } else { 0x3f8f4a };
            rgb(cr, color, 1.0);
            let _ = cr.fill();
        }
    }
}

fn listen(tx: mpsc::Sender<serde_json::Value>) {
    std::thread::spawn(move || {
        let Ok(stream) = UnixStream::connect(paths::socket()) else {
            let _ = tx.send(serde_json::json!({"event": "phase", "phase": "off"}));
            return;
        };
        let mut out = &stream;
        let _ = writeln!(out, r#"{{"cmd":"subscribe"}}"#);
        for line in BufReader::new(&stream).lines() {
            let Ok(line) = line else { break };
            if let Ok(v) = serde_json::from_str(&line)
                && tx.send(v).is_err()
            {
                return;
            }
        }
        let _ = tx.send(serde_json::json!({"event": "phase", "phase": "off"}));
    });
}

/// `talk-pill --png DIR`: desenha os dois estados em PNG, para conferir o
/// visual sem abrir nada na tela.
fn png(dir: &str) {
    install_font();
    for (name, st) in [
        ("pilula-gravando", State { phase: "recording".into(), seconds: 7, level: 0.06, started: None }),
        ("pilula-transcrevendo", State { phase: "transcribing".into(), ..State::default() }),
    ] {
        let Ok(surface) = gtk::cairo::ImageSurface::create(gtk::cairo::Format::ARgb32, W, H) else { return };
        if let Ok(cr) = Context::new(&surface) {
            draw(&cr, &st);
        }
        if let Ok(mut f) = std::fs::File::create(format!("{dir}/{name}.png")) {
            let _ = surface.write_to_png(&mut f);
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some("--png") {
        return png(args.get(2).map_or(".", String::as_str));
    }
    install_font();
    if gtk::init().is_err() || !gtk_layer_shell::is_supported() {
        eprintln!("[talk-pill] sem GTK ou sem layer-shell neste compositor");
        return;
    }
    let window = gtk::Window::new(gtk::WindowType::Toplevel);
    window.init_layer_shell();
    window.set_namespace("talk-pill");
    window.set_layer(Layer::Overlay);
    window.set_keyboard_mode(KeyboardMode::None);
    window.set_anchor(Edge::Bottom, true);
    window.set_layer_shell_margin(Edge::Bottom, 48);
    window.set_exclusive_zone(-1);
    window.set_app_paintable(true);
    if let Some(visual) = WidgetExt::screen(&window).and_then(|s| s.rgba_visual()) {
        window.set_visual(Some(&visual));
    }
    window.set_size_request(W, H);
    // clique atravessa: a pílula não é alvo de nada
    window.connect_realize(|w| {
        if let Some(gdk) = w.window() {
            gdk.input_shape_combine_region(&gtk::cairo::Region::create(), 0, 0);
        }
    });

    let state = Rc::new(RefCell::new(State {
        phase: "recording".into(),
        started: Some(Instant::now()),
        ..State::default()
    }));
    let area = gtk::DrawingArea::new();
    let s = state.clone();
    area.connect_draw(move |_, cr| {
        draw(cr, &s.borrow());
        gtk::glib::Propagation::Stop
    });
    window.add(&area);
    window.show_all();

    let (tx, rx) = mpsc::channel();
    listen(tx);
    gtk::glib::timeout_add_local(Duration::from_millis(33), move || {
        let mut st = state.borrow_mut();
        while let Ok(v) = rx.try_recv() {
            match v["event"].as_str() {
                Some("level") => {
                    st.level = v["level"].as_f64().unwrap_or(0.0) as f32;
                    st.seconds = v["seconds"].as_u64().unwrap_or(st.seconds);
                }
                Some("phase") => {
                    let phase = v["phase"].as_str().unwrap_or("idle").to_owned();
                    if phase == "idle" || phase == "off" {
                        gtk::main_quit();
                        return gtk::glib::ControlFlow::Break;
                    }
                    st.phase = phase;
                }
                _ => {}
            }
        }
        area.queue_draw();
        gtk::glib::ControlFlow::Continue
    });
    gtk::main();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_meter_matches_the_window() {
        assert_eq!(lit_segments(0.0, 12), 0);
        assert_eq!(lit_segments(1.0, 12), 12);
        assert!(lit_segments(0.05, 12) > lit_segments(0.01, 12));
    }
}
