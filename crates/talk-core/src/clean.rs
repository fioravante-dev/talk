//! As duas lições do talk em Python que custaram caro, portadas com os testes.

/// O tamanho do cabeçalho AU que o `pw-record` escreve antes das amostras,
/// mesmo pedindo `--format f32`. Lido como áudio, ele injeta amostras falsas;
/// uma é 0xFFFFFFFF, NaN em f32, e um único NaN faz o Whisper transcrever lixo
/// (ele normaliza o espectrograma pelo máximo). `None` = ainda faltam bytes.
pub fn au_header_len(head: &[u8]) -> Option<usize> {
    if head.len() < 8 {
        return None;
    }
    // O magic ".snd" sai invertido quando o campo vem em little-endian.
    let magic = &head[..4];
    if magic != b".snd" && magic != b"dns." {
        return Some(0); // PCM cru: nada a pular
    }
    let off = if magic == b".snd" {
        u32::from_be_bytes([head[4], head[5], head[6], head[7]])
    } else {
        u32::from_le_bytes([head[4], head[5], head[6], head[7]])
    };
    Some(off as usize)
}

/// A alucinação em laço do Whisper, que aparece quando ele recebe silêncio ou
/// ruído: "))))))", "Obrigado. Obrigado. Obrigado.". Texto humano não se
/// repete tanto; um laço, sim.
pub fn is_degenerate(text: &str) -> bool {
    let t = text.trim();
    if t.chars().count() < 16 {
        return false;
    }
    let mut distinct: Vec<char> = t.chars().filter(|c| *c != ' ').collect();
    distinct.sort_unstable();
    distinct.dedup();
    if distinct.len() <= 3 {
        return true;
    }
    let lower = t.to_lowercase();
    let words: Vec<&str> = lower
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .collect();
    let mut unique = words.clone();
    unique.sort_unstable();
    unique.dedup();
    words.len() >= 6 && (unique.len() as f64) / (words.len() as f64) < 0.4
}

/// As frases que o Whisper inventa quando recebe silêncio ou ruído: legendas e
/// encerramentos de vídeo, o que ele mais viu no treino. Uma ditada curta que
/// COMEÇA assim é descartada; mencionada no meio de uma frase de verdade, passa.
pub fn is_known_hallucination(text: &str) -> bool {
    const FRASES: &[&str] = &[
        "inscreva-se no canal",
        "ative o sininho",
        "legendas pela comunidade amara.org",
        "obrigado por assistir",
        "obrigada por assistir",
        "thanks for watching",
        "subtitles by the amara.org community",
    ];
    let t = text.trim().trim_start_matches(|c: char| !c.is_alphanumeric()).to_lowercase();
    t.chars().count() < 120 && FRASES.iter().any(|f| t.starts_with(f))
}

/// Só os trechos com fala, colados: o que o Silero achou, em centissegundos,
/// com uma folga de `pad` amostras de cada lado. Vazio = não houve fala, e o
/// Whisper nem é chamado (é no vazio que ele inventa legendas).
pub fn keep_speech(audio: &[f32], spans_cs: &[(f32, f32)], pad: usize) -> Vec<f32> {
    let mut out = Vec::with_capacity(audio.len());
    let mut last_end = 0usize;
    for &(start, end) in spans_cs {
        let a = ((start * 160.0) as usize).saturating_sub(pad).max(last_end);
        let b = (((end * 160.0) as usize) + pad).min(audio.len());
        if a < b {
            out.extend_from_slice(&audio[a..b]);
            last_end = b;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skips_the_au_header_in_either_byte_order() {
        let mut be = b".snd".to_vec();
        be.extend(32u32.to_be_bytes());
        assert_eq!(au_header_len(&be), Some(32));
        let mut le = b"dns.".to_vec();
        le.extend(24u32.to_le_bytes());
        assert_eq!(au_header_len(&le), Some(24));
        assert_eq!(au_header_len(&[0; 8]), Some(0));
        assert_eq!(au_header_len(b".sn"), None);
    }

    #[test]
    fn drops_the_youtube_outro_but_not_a_sentence_that_mentions_it() {
        assert!(is_known_hallucination(
            "Inscreva-se no canal e ative o sininho para receber notificações de novos vídeos."
        ));
        assert!(is_known_hallucination("Obrigado por assistir!"));
        assert!(!is_known_hallucination(
            "No fim do vídeo ele fala obrigado por assistir, mas o conteúdo sobre Rust e whisper.cpp foi muito bom mesmo."
        ));
    }

    #[test]
    fn keeps_only_the_speech_with_padding_and_no_overlap() {
        let audio: Vec<f32> = (0..16_000).map(|i| i as f32).collect();
        // 0,10–0,20 s e 0,21–0,30 s: as folgas se cruzam e não duplicam amostras.
        let got = keep_speech(&audio, &[(10.0, 20.0), (21.0, 30.0)], 160);
        assert_eq!(got.first(), Some(&1440.0));
        assert_eq!(got.last(), Some(&4959.0));
        assert_eq!(got.len(), 4960 - 1440);
        assert!(keep_speech(&audio, &[], 160).is_empty());
    }

    #[test]
    fn catches_the_loops_and_keeps_speech() {
        assert!(is_degenerate(")))))))))))))))))))))"));
        assert!(is_degenerate("Obrigado. Obrigado. Obrigado. Obrigado. Obrigado. Obrigado."));
        assert!(!is_degenerate("Vou fazer uns testes rápidos aqui, tá?"));
        assert!(!is_degenerate("ok ok ok"));
    }
}
