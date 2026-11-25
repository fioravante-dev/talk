//! O histórico do que o talk falou: o texto para sempre, o áudio por um tempo.
//!
//! `spoken.jsonl` guarda uma linha por fala (o texto já limpo, a conversa do
//! T3, a voz). O áudio fica em `clips/<id>.wav` e é apagado depois de
//! `speech_audio_keep` ou quando a pasta passa de `speech_audio_max_mb`: uma
//! fala sem áudio ainda toca, gerada de novo a partir do texto.

use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::paths;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Spoken {
    /// Também o nome do áudio: `clips/<id>.wav`.
    pub id: String,
    pub at: String,
    /// O título da conversa do T3; vazio quando veio de fora do T3.
    #[serde(default)]
    pub thread: String,
    pub language: String,
    pub voice: String,
    /// O texto como foi falado (já sem markdown).
    pub text: String,
    pub seconds: f64,
}

pub fn dir() -> PathBuf {
    paths::cache_dir().join("speech")
}

pub fn file() -> PathBuf {
    dir().join("spoken.jsonl")
}

pub fn clips_dir() -> PathBuf {
    dir().join("clips")
}

pub fn clip_path(id: &str) -> PathBuf {
    clips_dir().join(format!("{id}.wav"))
}

/// Um id que ordena pelo tempo e serve de nome de arquivo.
pub fn new_id() -> String {
    chrono::Local::now().format("%Y%m%d-%H%M%S-%3f").to_string()
}

pub fn append(path: &Path, s: &Spoken) -> Result<()> {
    if let Some(d) = path.parent() {
        std::fs::create_dir_all(d)?;
    }
    let mut f = OpenOptions::new().create(true).append(true).open(path)?;
    writeln!(f, "{}", serde_json::to_string(s)?)?;
    Ok(())
}

/// As mais recentes primeiro; uma linha estragada é pulada.
pub fn read(path: &Path, limit: usize) -> Vec<Spoken> {
    let Ok(text) = std::fs::read_to_string(path) else { return vec![] };
    text.lines().rev().filter_map(|l| serde_json::from_str(l).ok()).take(limit).collect()
}

pub fn find(path: &Path, id: &str) -> Option<Spoken> {
    let text = std::fs::read_to_string(path).ok()?;
    text.lines().rev().filter_map(|l| serde_json::from_str::<Spoken>(l).ok()).find(|s| s.id == id)
}

/// A linha do menu: "14:32 · conversa — começo do texto".
pub fn label(s: &Spoken, width: usize) -> String {
    let time = s.at.split_once('T').map_or("", |(_, t)| t.get(..5).unwrap_or(t));
    let head = if s.thread.is_empty() { String::new() } else { format!("{} — ", s.thread) };
    let full = format!("{time} · {head}{}", s.text);
    if full.chars().count() <= width {
        return full;
    }
    let cut: String = full.chars().take(width.saturating_sub(1)).collect();
    format!("{}…", cut.trim_end())
}

/// WAV mono de 16 bits: metade do f32 e o `pw-play` toca direto.
pub fn write_wav(path: &Path, rate: u32, samples: &[f32]) -> Result<()> {
    if let Some(d) = path.parent() {
        std::fs::create_dir_all(d)?;
    }
    let data = (samples.len() * 2) as u32;
    let mut out = Vec::with_capacity(44 + data as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes()); // tamanho do bloco fmt
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&1u16.to_le_bytes()); // mono
    out.extend_from_slice(&rate.to_le_bytes());
    out.extend_from_slice(&(rate * 2).to_le_bytes()); // bytes por segundo
    out.extend_from_slice(&2u16.to_le_bytes()); // bytes por quadro
    out.extend_from_slice(&16u16.to_le_bytes()); // bits
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data.to_le_bytes());
    for s in samples {
        out.extend_from_slice(&((s.clamp(-1.0, 1.0) * 32767.0).round() as i16).to_le_bytes());
    }
    // Escreve ao lado e renomeia: quem toca nunca lê um arquivo pela metade.
    let tmp = path.with_extension("part");
    std::fs::write(&tmp, out)?;
    std::fs::rename(tmp, path)?;
    Ok(())
}

/// Lê de volta um WAV do `write_wav` (PCM de 16 bits, mono): (taxa, amostras).
pub fn read_wav(path: &Path) -> Result<(u32, Vec<f32>)> {
    let b = std::fs::read(path)?;
    if b.len() < 44 || &b[..4] != b"RIFF" || &b[8..16] != b"WAVEfmt " {
        anyhow::bail!("não é um WAV do talk");
    }
    let rate = u32::from_le_bytes([b[24], b[25], b[26], b[27]]);
    let (channels, bits) = (u16::from_le_bytes([b[22], b[23]]), u16::from_le_bytes([b[34], b[35]]));
    if channels != 1 || bits != 16 || &b[36..40] != b"data" {
        anyhow::bail!("WAV fora do formato do talk ({channels} canais, {bits} bits)");
    }
    let samples = b[44..].as_chunks::<2>().0.iter().map(|c| f32::from(i16::from_le_bytes(*c)) / 32767.0).collect();
    Ok((rate, samples))
}

/// Apaga os áudios mais velhos que `keep` e, dos que sobram, os mais antigos
/// até a pasta caber em `max_bytes`. `None` em `keep` = nunca por idade.
/// Volta quantos apagou.
pub fn prune_clips(dir: &Path, keep: Option<Duration>, max_bytes: u64) -> Result<usize> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Ok(0) };
    let mut clips: Vec<(PathBuf, SystemTime, u64)> = entries
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|x| x == "wav"))
        .filter_map(|e| {
            let m = e.metadata().ok()?;
            Some((e.path(), m.modified().ok()?, m.len()))
        })
        .collect();
    clips.sort_by_key(|c| std::cmp::Reverse(c.1)); // mais novo primeiro
    let now = SystemTime::now();
    let (mut total, mut removed) = (0u64, 0);
    for (path, modified, len) in clips {
        let old = keep.is_some_and(|k| now.duration_since(modified).unwrap_or_default() > k);
        if old || total + len > max_bytes {
            std::fs::remove_file(&path)?;
            removed += 1;
        } else {
            total += len;
        }
    }
    Ok(removed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spoken(id: &str, thread: &str, text: &str) -> Spoken {
        Spoken {
            id: id.into(),
            at: "2026-11-24T14:32:10".into(),
            thread: thread.into(),
            language: "pt".into(),
            voice: "vivian".into(),
            text: text.into(),
            seconds: 3.0,
        }
    }

    #[test]
    fn keeps_the_text_and_reads_it_back_newest_first() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("spoken.jsonl");
        append(&f, &spoken("a", "", "primeira")).unwrap();
        append(&f, &spoken("b", "Conversa", "segunda")).unwrap();
        let got = read(&f, 10);
        assert_eq!(got.iter().map(|s| s.id.as_str()).collect::<Vec<_>>(), ["b", "a"]);
        assert_eq!(find(&f, "a").unwrap().text, "primeira");
        assert!(find(&f, "z").is_none());
    }

    #[test]
    fn menu_label_has_time_conversation_and_is_cut() {
        assert_eq!(label(&spoken("a", "", "Pronto."), 40), "14:32 · Pronto.");
        let l = label(&spoken("a", "Fala por foco", "Corrigi os dois testes do login e o terceiro"), 30);
        assert!(l.starts_with("14:32 · Fala por foco — "), "{l}");
        assert_eq!(l.chars().count(), 30);
        assert!(l.ends_with('…'));
    }

    #[test]
    fn writes_a_wav_header_pw_play_understands() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("x.wav");
        write_wav(&p, 24_000, &[0.0, 1.0, -1.0]).unwrap();
        let b = std::fs::read(&p).unwrap();
        assert_eq!(&b[..4], b"RIFF");
        assert_eq!(&b[8..16], b"WAVEfmt ");
        assert_eq!(u32::from_le_bytes([b[24], b[25], b[26], b[27]]), 24_000);
        assert_eq!(b.len(), 44 + 6);
        assert_eq!(i16::from_le_bytes([b[46], b[47]]), 32767);
    }

    #[test]
    fn reads_back_what_it_wrote() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("x.wav");
        write_wav(&p, 24_000, &[0.0, 0.5, -1.0]).unwrap();
        let (rate, s) = read_wav(&p).unwrap();
        assert_eq!(rate, 24_000);
        assert_eq!(s.len(), 3);
        assert!((s[1] - 0.5).abs() < 1e-3 && (s[2] + 1.0).abs() < 1e-3);
        std::fs::write(&p, b"nada").unwrap();
        assert!(read_wav(&p).is_err());
    }

    #[test]
    fn prunes_by_size_keeping_the_newest() {
        let dir = tempfile::tempdir().unwrap();
        for (i, name) in ["velho", "meio", "novo"].iter().enumerate() {
            let p = dir.path().join(format!("{name}.wav"));
            std::fs::write(&p, vec![0u8; 100]).unwrap();
            let t = SystemTime::now() - Duration::from_secs(100 - i as u64 * 10);
            std::fs::File::options().write(true).open(&p).unwrap().set_modified(t).unwrap();
        }
        std::fs::write(dir.path().join("nota.txt"), "fica").unwrap();
        assert_eq!(prune_clips(dir.path(), None, 250).unwrap(), 1);
        assert!(!dir.path().join("velho.wav").exists());
        assert!(dir.path().join("novo.wav").exists() && dir.path().join("nota.txt").exists());
        // Por idade: tudo tem mais de 50 s.
        assert_eq!(prune_clips(dir.path(), Some(Duration::from_secs(50)), u64::MAX).unwrap(), 2);
    }
}
