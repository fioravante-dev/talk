//! O WAV que chega pela rede (do celular, de outro PC) virando as amostras que
//! o trabalhador quer: mono, f32, 16 kHz.
//!
//! Só PCM de 16 bits ou float de 32, que é o que um gravador entrega cru. Outro
//! formato (mp3, opus) é recusado com uma mensagem clara em vez de virar ruído.

use anyhow::{Result, bail};

use crate::resample::resample;

/// A taxa do Whisper.
pub const RATE: u32 = 16_000;

pub fn decode(bytes: &[u8]) -> Result<Vec<f32>> {
    if bytes.len() < 12 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        bail!("não é um WAV");
    }
    let mut fmt: Option<(u16, u16, u32, u16)> = None;
    let mut at = 12;
    while at + 8 <= bytes.len() {
        let id = &bytes[at..at + 4];
        let size = u32::from_le_bytes([bytes[at + 4], bytes[at + 5], bytes[at + 6], bytes[at + 7]]) as usize;
        let body = at + 8;
        // Gravadores em fluxo escrevem o tamanho do `data` como 0 ou 0xFFFFFFFF:
        // vale até o fim do arquivo.
        let end = body.saturating_add(size).min(bytes.len());
        if id == b"fmt " && end - body >= 16 {
            let b = &bytes[body..];
            let u16_at = |i: usize| u16::from_le_bytes([b[i], b[i + 1]]);
            let rate = u32::from_le_bytes([b[4], b[5], b[6], b[7]]);
            fmt = Some((u16_at(0), u16_at(2), rate, u16_at(14)));
        } else if id == b"data" {
            let Some((format, channels, rate, bits)) = fmt else { bail!("WAV sem o bloco fmt") };
            let data = &bytes[body..end];
            let samples: Vec<f32> = match (format, bits) {
                (1, 16) => data.as_chunks::<2>().0.iter().map(|c| f32::from(i16::from_le_bytes(*c)) / 32768.0).collect(),
                (3, 32) => data.as_chunks::<4>().0.iter().map(|c| f32::from_le_bytes(*c)).collect(),
                // WAVE_FORMAT_EXTENSIBLE: o subformato vem depois; 16 bits é PCM.
                (0xFFFE, 16) => data.as_chunks::<2>().0.iter().map(|c| f32::from(i16::from_le_bytes(*c)) / 32768.0).collect(),
                _ => bail!("WAV de formato {format} com {bits} bits: mande PCM de 16 bits"),
            };
            if channels == 0 || rate == 0 {
                bail!("WAV com cabeçalho inválido");
            }
            let mono: Vec<f32> = if channels == 1 {
                samples
            } else {
                samples
                    .chunks_exact(channels as usize)
                    .map(|f| f.iter().sum::<f32>() / f32::from(channels))
                    .collect()
            };
            return Ok(resample(&mono, rate, RATE));
        }
        // Blocos têm tamanho par: o ímpar ganha um byte de enchimento.
        at = end + (size & 1);
    }
    bail!("WAV sem áudio")
}

/// O contrário, para os testes e para quem quiser mandar um WAV daqui.
pub fn encode(samples: &[f32], rate: u32) -> Vec<u8> {
    let data_len = (samples.len() * 2) as u32;
    let mut out = Vec::with_capacity(44 + data_len as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&1u16.to_le_bytes()); // mono
    out.extend_from_slice(&rate.to_le_bytes());
    out.extend_from_slice(&(rate * 2).to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for s in samples {
        out.extend_from_slice(&((s.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_16k_mono_wav_comes_back_the_same() {
        let x: Vec<f32> = (0..1600).map(|i| ((i as f32) * 0.01).sin() * 0.5).collect();
        let y = decode(&encode(&x, RATE)).unwrap();
        assert_eq!(y.len(), x.len());
        assert!(x.iter().zip(&y).all(|(a, b)| (a - b).abs() < 1e-3));
    }

    #[test]
    fn other_rates_are_resampled_to_16k() {
        assert_eq!(decode(&encode(&vec![0.0; 48_000], 48_000)).unwrap().len(), 16_000);
    }

    #[test]
    fn stereo_is_mixed_down() {
        let mut wav = encode(&[0.5, -0.5, 0.5, -0.5], RATE);
        wav[22] = 2; // canais
        assert_eq!(decode(&wav).unwrap(), vec![0.0, 0.0]);
    }

    #[test]
    fn a_streamed_size_reads_to_the_end() {
        let mut wav = encode(&[0.25; 10], RATE);
        wav[40..44].copy_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(decode(&wav).unwrap().len(), 10);
    }

    #[test]
    fn other_formats_are_refused() {
        assert!(decode(b"ID3\x04 not a wav at all").is_err());
        let mut wav = encode(&[0.0; 4], RATE);
        wav[34] = 24; // 24 bits
        assert!(decode(&wav).is_err());
    }
}
