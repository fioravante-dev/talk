//! O texto que vira fala: o que o Claude escreveu, limpo para ser ouvido.
//!
//! O resumo falado já vem escrito para o ouvido, mas nada garante: um bloco de
//! código, uma tabela ou uma URL lidos em voz alta são ruído. Aqui o markdown
//! vira prosa, e a prosa vira frases curtas, para o primeiro áudio sair logo
//! (o modelo sintetiza a primeira frase enquanto as outras esperam).

use std::io::{BufRead, Write};
use std::sync::LazyLock;

use anyhow::{Context, Result, bail};
use regex::Regex;
use serde::{Deserialize, Serialize};

static LINK: LazyLock<Regex> = LazyLock::new(|| re(r"\[([^\]]*)\]\([^)]*\)"));
static URL: LazyLock<Regex> = LazyLock::new(|| re(r"https?://\S+"));
static CODE: LazyLock<Regex> = LazyLock::new(|| re(r"`([^`]*)`"));
static STRONG: LazyLock<Regex> = LazyLock::new(|| re(r"\*\*|__|~~"));
static EMPHASIS: LazyLock<Regex> = LazyLock::new(|| re(r"(^|\s)[*_]|[*_](\s|$)"));
static MARKER: LazyLock<Regex> = LazyLock::new(|| re(r"^\s*(#{1,6}\s+|>\s*|[-*+]\s+|\d+[.)]\s+)"));
static SPACES: LazyLock<Regex> = LazyLock::new(|| re(r"\s+"));

fn re(pattern: &str) -> Regex {
    #[allow(clippy::unwrap_used)] // padrões fixos, cobertos pelos testes
    Regex::new(pattern).unwrap()
}

/// O markdown em prosa para ler em voz alta. Some com blocos de código,
/// tabelas, URLs e marcações; caminhos viram só o nome do arquivo.
pub fn to_speech(markdown: &str) -> String {
    let mut lines = vec![];
    let mut in_fence = false;
    for line in markdown.lines() {
        let t = line.trim();
        if t.starts_with("```") || t.starts_with("~~~") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence || t.starts_with('|') || t.is_empty() || t.chars().all(|c| "-=*_ ".contains(c)) {
            continue;
        }
        let t = MARKER.replace(t, "");
        let t = LINK.replace_all(&t, "$1");
        let t = URL.replace_all(&t, "");
        let t = CODE.replace_all(&t, |c: &regex::Captures| spoken_code(&c[1]));
        let t = STRONG.replace_all(&t, "");
        let t = EMPHASIS.replace_all(&t, "$1$2");
        let t = t.trim();
        if t.is_empty() {
            continue;
        }
        // Cada linha é uma ideia: sem pontuação no fim, a voz emendaria a próxima.
        let ends = t.ends_with(|c: char| ".!?…:;,".contains(c));
        lines.push(if ends { t.to_owned() } else { format!("{t}.") });
    }
    SPACES.replace_all(&lines.join(" "), " ").trim().to_owned()
}

/// Código no meio da frase: um caminho vira o nome do arquivo (sem a linha), o
/// resto fica como está.
fn spoken_code(code: &str) -> String {
    let code = code.trim();
    if code.contains('/') && !code.contains(' ') {
        let name = code.trim_end_matches('/').rsplit('/').next().unwrap_or(code);
        return name.split(':').next().unwrap_or(name).to_owned();
    }
    code.to_owned()
}

/// A prosa em frases de até `max` caracteres, cortando primeiro no ponto,
/// depois na vírgula, por último no espaço. Nenhuma palavra se perde.
pub fn sentences(text: &str, max: usize) -> Vec<String> {
    let mut out = vec![];
    let mut current = String::new();
    for word in text.split_whitespace() {
        if !current.is_empty() && current.chars().count() + 1 + word.chars().count() > max {
            // Estourou: corta na última vírgula, se houver uma razoável.
            match current.rfind(", ") {
                Some(i) if i > max / 3 => {
                    let rest = current.split_off(i + 1);
                    out.push(std::mem::take(&mut current));
                    current = rest.trim().to_owned();
                }
                _ => out.push(std::mem::take(&mut current)),
            }
        }
        if !current.is_empty() {
            current.push(' ');
        }
        current.push_str(word);
        if word.ends_with(|c: char| ".!?…".contains(c)) && !is_abbreviation(word) {
            out.push(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        out.push(current);
    }
    out
}

/// "Sr." e "ex." não terminam frase; "pronto." termina.
fn is_abbreviation(word: &str) -> bool {
    const ABBR: &[&str] = &["sr.", "sra.", "dr.", "dra.", "ex.", "etc.", "e.g.", "i.e.", "vs.", "mr.", "mrs."];
    ABBR.contains(&word.to_lowercase().as_str())
}

/// `pt` ou `en`, pelo jeito do texto: acentos e palavras que só um dos dois
/// usa. Empate (ou nenhuma pista) fica com o português, a língua da casa.
pub fn detect_language(text: &str) -> &'static str {
    const PT: &[&str] = &[
        "não", "que", "de", "do", "da", "o", "os", "as", "é", "um", "uma", "para", "com", "você", "está",
        "isso", "se", "mas", "no", "na", "eu", "ele", "quer", "agora", "depois", "pronto", "também",
    ];
    const EN: &[&str] = &[
        "the", "and", "is", "to", "of", "you", "it", "that", "this", "are", "was", "for", "with", "i",
        "now", "should", "would", "done", "can", "my", "your", "be", "have", "not",
    ];
    let lower = text.to_lowercase();
    let accents = lower.chars().filter(|c| "ãõçáéíóúâêôà".contains(*c)).count();
    let (mut pt, mut en) = (accents, 0);
    for w in lower.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()) {
        pt += usize::from(PT.contains(&w));
        en += usize::from(EN.contains(&w));
    }
    if en > pt { "en" } else { "pt" }
}

/// Uma frase para o trabalhador da fala, numa linha JSON.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Say {
    pub text: String,
    /// `pt` ou `en`.
    pub language: String,
    pub speaker: String,
}

/// O que volta, quantas vezes for: uma linha JSON e logo depois `samples` ×
/// f32 little-endian, mono. A frase termina com `done` (e `samples` = 0).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Chunk {
    pub samples: usize,
    pub rate: u32,
    #[serde(default)]
    pub done: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

pub fn send_say(w: &mut impl Write, say: &Say) -> Result<()> {
    writeln!(w, "{}", serde_json::to_string(say)?)?;
    w.flush()?;
    Ok(())
}

/// O próximo pedaço de áudio. Erro quando a saída fecha: o trabalhador caiu.
pub fn read_chunk(r: &mut impl BufRead) -> Result<(Chunk, Vec<f32>)> {
    let mut header = String::new();
    if r.read_line(&mut header)? == 0 {
        bail!("o trabalhador da fala fechou a saída");
    }
    let chunk: Chunk = serde_json::from_str(header.trim()).with_context(|| format!("cabeçalho da fala: {header:?}"))?;
    if chunk.samples > 48_000 * 60 {
        bail!("pedaço de {} amostras: mais de um minuto", chunk.samples);
    }
    let mut bytes = vec![0u8; chunk.samples * 4];
    r.read_exact(&mut bytes).context("áudio da fala")?;
    let audio = bytes.as_chunks::<4>().0.iter().map(|c| f32::from_le_bytes(*c)).collect();
    Ok((chunk, audio))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drops_code_tables_and_urls_and_keeps_the_prose() {
        let md = "## Pronto\n\nMudei o `crates/talk/src/worker.rs:42` e **três** testes passam.\n\n\
                  ```rust\nfn main() {}\n```\n\n| a | b |\n|---|---|\n| 1 | 2 |\n\n\
                  - Veja [o PR](https://github.com/x/y/pull/1)\n- Detalhes em https://example.com";
        assert_eq!(
            to_speech(md),
            "Pronto. Mudei o worker.rs e três testes passam. Veja o PR. Detalhes em."
        );
    }

    #[test]
    fn keeps_short_inline_code_and_existing_punctuation() {
        assert_eq!(to_speech("Rode `cargo test`:\n\n1. depois o clippy"), "Rode cargo test: depois o clippy.");
    }

    #[test]
    fn splits_on_sentence_ends_but_not_abbreviations() {
        assert_eq!(
            sentences("O Sr. Pedro chegou. Tudo certo? Sim!", 200),
            ["O Sr. Pedro chegou.", "Tudo certo?", "Sim!"]
        );
    }

    #[test]
    fn long_sentences_break_at_a_comma_and_lose_no_words() {
        let text = "O serviço sobe o trabalhador, carrega o modelo na placa de vídeo e começa a falar logo";
        let got = sentences(text, 40);
        assert!(got.iter().all(|s| s.chars().count() <= 40), "{got:?}");
        assert_eq!(got[0], "O serviço sobe o trabalhador,");
        assert_eq!(got.join(" "), text);
    }

    #[test]
    fn tells_portuguese_from_english() {
        assert_eq!(detect_language("Pronto, corrigi os dois testes. Quer que eu olhe o terceiro?"), "pt");
        assert_eq!(detect_language("Done. Two of the three tests pass now; should I dig into it?"), "en");
        assert_eq!(detect_language("worker.rs"), "pt");
    }

    #[test]
    fn reads_the_chunks_the_python_worker_writes() {
        let mut wire = b"{\"samples\":2,\"rate\":24000}\n".to_vec();
        wire.extend(0.5f32.to_le_bytes());
        wire.extend((-1.0f32).to_le_bytes());
        wire.extend(b"{\"samples\":0,\"rate\":24000,\"done\":true}\n");
        let mut r = std::io::Cursor::new(wire);
        let (c, a) = read_chunk(&mut r).unwrap();
        assert_eq!((c.samples, c.rate, c.done, a), (2, 24000, false, vec![0.5, -1.0]));
        let (c, a) = read_chunk(&mut r).unwrap();
        assert!(c.done && a.is_empty() && c.error.is_none());
        assert!(read_chunk(&mut r).is_err());
    }
}
