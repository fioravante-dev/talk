//! O desenho da pílula em Rust puro (`tiny-skia` + `ab_glyph`), para o Windows,
//! onde não há cairo. É uma cópia ponto a ponto do `draw` do `linux.rs`: as
//! mesmas medidas, cores, sombra, lâmpada, texto e medidor.
//!
//! Compila também no Linux, só para os testes e o `--png`: dá para comparar
//! as duas pílulas lado a lado sem abrir o Windows.

use std::f32::consts::PI;

use ab_glyph::{Font, FontRef, ScaleFont};
use tiny_skia::{Color, FillRule, Paint, PathBuilder, Pixmap, Rect, Stroke, Transform};

pub const W: f32 = 260.0;
pub const H: f32 = 44.0;
const PAD: f32 = 8.0;
const W_TRANSCRIBING: f32 = 156.0;

/// A Schibsted Grotesk da janela, em TTF (o WOFF2 da janela convertido pelo
/// fontTools; o `ab_glyph` não lê WOFF2). Licença OFL, em `assets/`.
const FONT: &[u8] = include_bytes!("../assets/schibsted-latin.ttf");

/// O que muda de um quadro para o outro.
#[derive(Debug, Clone, Default)]
pub struct Frame {
    pub recording: bool,
    pub seconds: u64,
    pub level: f32,
    /// 0..1: o brilho da lâmpada gravando (o pulso).
    pub pulse: f32,
}

/// Mesmo mapeamento do medidor da janela: RMS em dB, 12 segmentos.
pub fn lit_segments(level: f32, n: usize) -> usize {
    let db = if level > 0.0 { 20.0 * level.log10() } else { -60.0 };
    (((db + 50.0) / 45.0 * n as f32).round().clamp(0.0, n as f32)) as usize
}

fn color(hex: u32, a: f32) -> Color {
    let c = |s: u32| ((hex >> s) & 0xff) as u8;
    Color::from_rgba8(c(16), c(8), c(0), (a.clamp(0.0, 1.0) * 255.0).round() as u8)
}

fn paint(hex: u32, a: f32) -> Paint<'static> {
    let mut p = Paint::default();
    p.set_color(color(hex, a));
    p.anti_alias = true;
    p
}

/// Retângulo de cantos redondos com arcos de Bézier, como o `rounded` do cairo.
fn rounded(x: f32, y: f32, w: f32, h: f32, r: f32) -> Option<tiny_skia::Path> {
    let k = 0.552_284_8 * r; // o quarto de círculo em cúbica
    let mut pb = PathBuilder::new();
    pb.move_to(x + r, y);
    pb.line_to(x + w - r, y);
    pb.cubic_to(x + w - r + k, y, x + w, y + r - k, x + w, y + r);
    pb.line_to(x + w, y + h - r);
    pb.cubic_to(x + w, y + h - r + k, x + w - r + k, y + h, x + w - r, y + h);
    pb.line_to(x + r, y + h);
    pb.cubic_to(x + r - k, y + h, x, y + h - r + k, x, y + h - r);
    pb.line_to(x, y + r);
    pb.cubic_to(x, y + r - k, x + r - k, y, x + r, y);
    pb.close();
    pb.finish()
}

/// Escreve `text` com a linha de base em (x, y), em pixels já escalados.
fn text(pm: &mut Pixmap, text: &str, x: f32, y: f32, size: f32, hex: u32) {
    let Ok(font) = FontRef::try_from_slice(FONT) else { return };
    // O tamanho do cairo é o quadrado do em; o do ab_glyph, ascendente−descendente.
    let em = font.units_per_em().unwrap_or(1000.0);
    let scaled = font.as_scaled(size * font.height_unscaled() / em);
    let (w, h) = (pm.width() as i32, pm.height() as i32);
    let rgb = [(hex >> 16) & 0xff, (hex >> 8) & 0xff, hex & 0xff].map(|v| v as f32 / 255.0);
    let data = pm.data_mut();
    let mut pen = x;
    let mut prev = None;
    for ch in text.chars() {
        let id = scaled.glyph_id(ch);
        if let Some(p) = prev {
            pen += scaled.kern(p, id);
        }
        let glyph = id.with_scale_and_position(scaled.scale(), ab_glyph::point(pen, y));
        pen += scaled.h_advance(id);
        prev = Some(id);
        let Some(outline) = scaled.outline_glyph(glyph) else { continue };
        let b = outline.px_bounds();
        outline.draw(|gx, gy, cov| {
            let (px, py) = (b.min.x as i32 + gx as i32, b.min.y as i32 + gy as i32);
            if px < 0 || py < 0 || px >= w || py >= h || cov <= 0.0 {
                return;
            }
            // "Over" em RGBA pré-multiplicado, o formato do tiny-skia.
            let i = ((py * w + px) * 4) as usize;
            let a = cov.min(1.0);
            for c in 0..3 {
                let dst = f32::from(data[i + c]) / 255.0;
                data[i + c] = ((rgb[c] * a + dst * (1.0 - a)) * 255.0).round() as u8;
            }
            let dst = f32::from(data[i + 3]) / 255.0;
            data[i + 3] = ((a + dst * (1.0 - a)) * 255.0).round() as u8;
        });
    }
}

/// O quadro inteiro, em `scale` (1 = 96 dpi). Fundo transparente.
pub fn draw(frame: &Frame, scale: f32) -> Option<Pixmap> {
    let mut pm = Pixmap::new((W * scale).round() as u32, (H * scale).round() as u32)?;
    let t = Transform::from_scale(scale, scale);

    let full = W - 2.0 * PAD;
    let w = if frame.recording { full } else { W_TRANSCRIBING };
    let (x, y, h) = (PAD + (full - w) / 2.0, PAD, H - 2.0 * PAD);

    // sombra curta e macia, com deslocamento: o aparelho pousado sobre a tela
    for (i, a) in [(3.0, 0.05), (2.0, 0.07), (1.0, 0.09)] {
        if let Some(p) = rounded(x - i + 1.0, y - i + 2.5, w + 2.0 * i - 2.0, h + 2.0 * i - 2.0, h / 2.0 + i) {
            pm.fill_path(&p, &paint(0x2a2a28, a), FillRule::Winding, t, None);
        }
    }
    if let Some(body) = rounded(x, y, w, h, h / 2.0) {
        pm.fill_path(&body, &paint(0xeeefed, 1.0), FillRule::Winding, t, None);
        let stroke = Stroke { width: 1.0, ..Stroke::default() };
        pm.stroke_path(&body, &paint(0xc3c5c2, 1.0), &stroke, t, None);
    }

    let (lamp, alpha) = if frame.recording { (0xd8442e, frame.pulse) } else { (0xc98a1b, 1.0) };
    if let Some(dot) = PathBuilder::from_circle(x + 18.0, y + h / 2.0, 5.0) {
        pm.fill_path(&dot, &paint(lamp, alpha), FillRule::Winding, t, None);
    }

    let label = if frame.recording {
        format!("gravando {}:{:02}", frame.seconds / 60, frame.seconds % 60)
    } else {
        "transcrevendo".to_owned()
    };
    text(&mut pm, &label, (x + 32.0) * scale, (y + h / 2.0 + 4.5) * scale, 12.5 * scale, 0x2a2a28);

    if frame.recording {
        let n = 12;
        let lit = lit_segments(frame.level, n);
        for i in 0..n {
            let sx = x + w - 18.0 - (n - i) as f32 * 7.0;
            let c = if i >= lit { 0xc3c5c2 } else if i >= 10 { 0xd8442e } else { 0x3f8f4a };
            if let Some(r) = Rect::from_xywh(sx, y + h / 2.0 - 4.0, 5.0, 8.0) {
                pm.fill_rect(r, &paint(c, 1.0), t, None);
            }
        }
    }
    Some(pm)
}

/// O brilho da lâmpada `secs` depois do início: o mesmo pulso do cairo.
/// (No Linux só os testes usam: lá quem pulsa é o cairo.)
#[cfg_attr(not(windows), allow(dead_code))]
pub fn pulse(secs: f32) -> f32 {
    0.55 + 0.45 * (secs * PI).cos().abs()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn px(pm: &Pixmap, x: f32, y: f32) -> [u8; 4] {
        let p = pm.pixel(x as u32, y as u32).map(|c| [c.red(), c.green(), c.blue(), c.alpha()]);
        p.unwrap_or_default()
    }

    #[test]
    fn the_meter_matches_the_window() {
        assert_eq!(lit_segments(0.0, 12), 0);
        assert_eq!(lit_segments(1.0, 12), 12);
        assert!(lit_segments(0.05, 12) > lit_segments(0.01, 12));
    }

    #[test]
    fn recording_draws_the_red_lamp_on_the_light_pill() {
        let pm = draw(&Frame { recording: true, seconds: 7, level: 0.06, pulse: 1.0 }, 1.0).unwrap();
        assert_eq!((pm.width(), pm.height()), (260, 44));
        assert_eq!(px(&pm, 0.0, 0.0)[3], 0, "o canto fica transparente");
        assert_eq!(px(&pm, 26.0, 22.0), [0xd8, 0x44, 0x2e, 0xff], "a lâmpada");
        assert_eq!(px(&pm, 130.0, 12.0), [0xee, 0xef, 0xed, 0xff], "o corpo");
    }

    #[test]
    fn transcribing_shrinks_and_turns_amber() {
        let pm = draw(&Frame::default(), 1.0).unwrap();
        assert_eq!(px(&pm, 12.0, 22.0)[3], 0, "fora da pílula encolhida");
        assert_eq!(px(&pm, 52.0 + 18.0, 22.0), [0xc9, 0x8a, 0x1b, 0xff], "a lâmpada âmbar");
    }

    #[test]
    fn it_scales_for_high_dpi() {
        let pm = draw(&Frame { recording: true, pulse: 1.0, ..Frame::default() }, 1.5).unwrap();
        assert_eq!((pm.width(), pm.height()), (390, 66));
    }

    #[test]
    fn the_lamp_pulses_between_55_and_100_percent() {
        assert!((pulse(0.0) - 1.0).abs() < 1e-6);
        assert!((pulse(0.5) - 0.55).abs() < 1e-6);
    }

    #[test]
    fn the_label_is_drawn() {
        let pm = draw(&Frame { recording: true, pulse: 1.0, ..Frame::default() }, 1.0).unwrap();
        // Algum pixel escuro na faixa do texto (x 40..100, y 14..30).
        let dark = (40..100).flat_map(|x| (14..30).map(move |y| (x, y))).any(|(x, y)| {
            let [r, ..] = px(&pm, x as f32, y as f32);
            r < 0x80
        });
        assert!(dark, "o texto 'gravando' tem que aparecer");
    }
}
