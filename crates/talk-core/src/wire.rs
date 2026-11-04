//! O fio entre o serviço e o trabalhador que transcreve.
//!
//! Pela entrada do trabalhador: uma linha JSON (`Job`) e logo depois
//! `samples` × f32 little-endian. Pela saída: uma linha JSON (`Done`). Fechar a
//! entrada é o jeito de mandar o trabalhador embora: ele termina, e só assim a
//! CUDA devolve toda a VRAM.

use std::io::{BufRead, Write};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Job {
    pub language: String,
    pub prompt: String,
    pub beam_size: i32,
    pub samples: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Done {
    pub text: String,
    pub language: String,
    pub stt_seconds: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

pub fn send_job(w: &mut impl Write, job: &Job, audio: &[f32]) -> Result<()> {
    debug_assert_eq!(job.samples, audio.len());
    writeln!(w, "{}", serde_json::to_string(job)?)?;
    let bytes: Vec<u8> = audio.iter().flat_map(|s| s.to_le_bytes()).collect();
    w.write_all(&bytes)?;
    w.flush()?;
    Ok(())
}

/// `None` quando a entrada fechou: hora de terminar.
pub fn read_job(r: &mut impl BufRead) -> Result<Option<(Job, Vec<f32>)>> {
    let mut header = String::new();
    if r.read_line(&mut header)? == 0 {
        return Ok(None);
    }
    let job: Job = serde_json::from_str(header.trim()).context("cabeçalho do trabalho")?;
    if job.samples > 16_000 * 60 * 30 {
        bail!("áudio de {} amostras: mais de 30 min", job.samples);
    }
    let mut bytes = vec![0u8; job.samples * 4];
    r.read_exact(&mut bytes).context("áudio do trabalho")?;
    let audio = bytes.as_chunks::<4>().0.iter().map(|c| f32::from_le_bytes(*c)).collect();
    Ok(Some((job, audio)))
}

pub fn send_done(w: &mut impl Write, done: &Done) -> Result<()> {
    writeln!(w, "{}", serde_json::to_string(done)?)?;
    w.flush()?;
    Ok(())
}

pub fn read_done(r: &mut impl BufRead) -> Result<Done> {
    let mut line = String::new();
    if r.read_line(&mut line)? == 0 {
        bail!("o trabalhador terminou sem responder");
    }
    serde_json::from_str(line.trim()).context("resposta do trabalhador")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn a_job_round_trips_with_its_audio() {
        let job = Job { language: "pt".into(), prompt: "Pop!_OS.".into(), beam_size: 5, samples: 3 };
        let audio = [0.0, -0.5, 0.25];
        let mut buf = vec![];
        send_job(&mut buf, &job, &audio).unwrap();
        send_job(&mut buf, &job, &audio).unwrap();

        let mut r = Cursor::new(buf);
        for _ in 0..2 {
            let (got, samples) = read_job(&mut r).unwrap().unwrap();
            assert_eq!(got, job);
            assert_eq!(samples, audio);
        }
        assert!(read_job(&mut r).unwrap().is_none());
    }

    #[test]
    fn a_worker_that_dies_is_an_error_not_a_hang() {
        assert!(read_done(&mut Cursor::new(vec![])).is_err());
    }
}
