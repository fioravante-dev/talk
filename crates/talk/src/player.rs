//! O alto-falante da fala no Linux: o `pw-play` do PipeWire, lendo f32 cru
//! pela entrada. O `player_win.rs` tem a mesma cara, pelo WASAPI.

use std::io::Write;
use std::process::{Child, ChildStdin, Command, Stdio};

use anyhow::{Context, Result, anyhow};

/// O som tocando: dá para cortar (`kill`) e saber se acabou.
pub struct Player(Child);

/// Por onde as amostras entram. Largar (drop) é o fim do áudio: o que já
/// entrou termina de tocar.
pub struct Sink(ChildStdin);

/// Abre o alto-falante para áudio mono na taxa `rate`.
pub fn open(rate: u32) -> Result<(Player, Sink)> {
    let mut child = Command::new("pw-play")
        .args(["--raw", "--format", "f32", "--channels", "1", "--rate", &rate.to_string()])
        .args(["--latency", "50ms", "--media-role", "Communication", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .context("não deu para abrir o pw-play (vem com o PipeWire)")?;
    let stdin = child.stdin.take().ok_or_else(|| anyhow!("pw-play sem entrada"))?;
    Ok((Player(child), Sink(stdin)))
}

impl Sink {
    /// Falha quando o som foi cortado: o resto pode ser descartado.
    pub fn write(&mut self, samples: &[f32]) -> Result<()> {
        let bytes: Vec<u8> = samples.iter().flat_map(|s| s.to_le_bytes()).collect();
        self.0.write_all(&bytes)?;
        Ok(())
    }
}

impl Player {
    pub fn kill(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }

    pub fn finished(&mut self) -> bool {
        !matches!(self.0.try_wait(), Ok(None))
    }
}
