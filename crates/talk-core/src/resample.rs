//! Troca a taxa de amostragem do áudio: o Whisper quer 16 kHz, e o microfone no
//! Windows entrega o que o mixer usa (48 ou 44,1 kHz). O PipeWire converte
//! sozinho; o WASAPI, na entrada, não.
//!
//! Sinc janelado, sem biblioteca: no ditado o áudio chega inteiro no fim, então
//! não precisa ser em fluxo, e 30 s a 48 kHz convertem em dezenas de ms.

use std::f64::consts::PI;

/// Amostras de cada lado do filtro, na escala da saída.
const HALF_TAPS: f64 = 16.0;

pub fn resample(input: &[f32], from: u32, to: u32) -> Vec<f32> {
    if from == to || input.is_empty() || from == 0 || to == 0 {
        return input.to_vec();
    }
    let ratio = f64::from(from) / f64::from(to);
    // Descendo a taxa, o corte cai para a nova Nyquist (com folga) e o filtro
    // fica mais largo na entrada, para não dobrar agudos de volta como ruído.
    let cutoff = (1.0 / ratio).min(1.0) * 0.95;
    let half = (HALF_TAPS / cutoff).ceil() as i64;
    let len = (input.len() as f64 / ratio).floor() as usize;
    let last = input.len() as i64 - 1;
    (0..len)
        .map(|i| {
            let t = i as f64 * ratio;
            let center = t.floor() as i64;
            let (mut acc, mut norm) = (0.0f64, 0.0f64);
            for k in (center - half + 1).max(0)..=(center + half).min(last) {
                let x = t - k as f64;
                let sinc = if x.abs() < 1e-9 { 1.0 } else { (PI * cutoff * x).sin() / (PI * cutoff * x) };
                // Janela de Hann sobre a largura do filtro.
                let w = 0.5 + 0.5 * (PI * x / half as f64).cos();
                let h = sinc * w;
                acc += f64::from(input[k as usize]) * h;
                norm += h;
            }
            // Normalizar pela soma mantém o nível certo, inclusive nas bordas.
            if norm.abs() > 1e-12 { (acc / norm) as f32 } else { 0.0 }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(freq: f64, rate: u32, secs: f64) -> Vec<f32> {
        let n = (f64::from(rate) * secs) as usize;
        (0..n).map(|i| (2.0 * PI * freq * i as f64 / f64::from(rate)).sin() as f32).collect()
    }

    fn rms(x: &[f32]) -> f64 {
        let core = &x[x.len() / 10..x.len() * 9 / 10]; // fora das bordas
        (core.iter().map(|v| f64::from(*v).powi(2)).sum::<f64>() / core.len() as f64).sqrt()
    }

    #[test]
    fn same_rate_is_untouched() {
        let x = sine(440.0, 16_000, 0.1);
        assert_eq!(resample(&x, 16_000, 16_000), x);
    }

    #[test]
    fn the_length_follows_the_ratio() {
        assert_eq!(resample(&vec![0.0; 48_000], 48_000, 16_000).len(), 16_000);
        assert_eq!(resample(&vec![0.0; 44_100], 44_100, 16_000).len(), 16_000);
    }

    #[test]
    fn speech_band_survives_and_above_nyquist_is_cut() {
        for from in [48_000, 44_100] {
            let voice = resample(&sine(1_000.0, from, 1.0), from, 16_000);
            assert!((rms(&voice) - 0.707).abs() < 0.02, "1 kHz a {from}: {}", rms(&voice));
            // 12 kHz não cabe em 16 kHz (Nyquist 8 kHz): tem que sumir, não dobrar para 4 kHz.
            let hiss = resample(&sine(12_000.0, from, 1.0), from, 16_000);
            assert!(rms(&hiss) < 0.02, "12 kHz a {from}: {}", rms(&hiss));
        }
    }

    #[test]
    fn dc_level_is_kept() {
        let out = resample(&vec![0.5; 4_800], 48_000, 16_000);
        assert!(out.iter().all(|v| (v - 0.5).abs() < 1e-3));
    }
}
