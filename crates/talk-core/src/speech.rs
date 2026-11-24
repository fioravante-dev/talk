//! O texto que vira fala: o que o Claude escreveu, limpo para ser ouvido.
//!
//! O resumo falado já vem escrito para o ouvido, mas nada garante: um bloco de
//! código, uma tabela ou uma URL lidos em voz alta são ruído. Aqui o markdown
//! vira prosa, e a prosa vira frases curtas, para o primeiro áudio sair logo
//! (o modelo sintetiza a primeira frase enquanto as outras esperam).

use std::sync::LazyLock;

use regex::Regex;

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
}
