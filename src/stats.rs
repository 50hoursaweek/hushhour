//! Word, character and reading-time statistics, iA Writer style.

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Stats {
    pub words: usize,
    pub characters: usize,
    pub sentences: usize,
    pub paragraphs: usize,
    pub reading_seconds: u64,
}

/// Average silent reading speed used by iA Writer for its estimate.
const WORDS_PER_MINUTE: f64 = 200.0;

pub fn compute(text: &str) -> Stats {
    let words = text
        .split_whitespace()
        .filter(|w| w.chars().any(|c| c.is_alphanumeric()))
        .count();
    let characters = text.chars().filter(|c| *c != '\n' && *c != '\r').count();
    let sentences = text
        .split(['.', '!', '?'])
        .filter(|s| s.chars().any(|c| c.is_alphanumeric()))
        .count();
    let paragraphs = text
        .split("\n\n")
        .filter(|p| p.chars().any(|c| !c.is_whitespace()))
        .count();
    let reading_seconds = (words as f64 / WORDS_PER_MINUTE * 60.0).round() as u64;
    Stats { words, characters, sentences, paragraphs, reading_seconds }
}

impl Stats {
    pub fn reading_time(&self) -> String {
        let s = self.reading_seconds;
        if s < 60 {
            format!("{s} sec")
        } else if s < 3600 {
            format!("{} min", s.div_ceil(60))
        } else {
            format!("{} h {} min", s / 3600, (s % 3600).div_ceil(60))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_words_and_sentences() {
        let s = compute("Hello world. This is a test!\n\nSecond paragraph?");
        assert_eq!(s.words, 8);
        assert_eq!(s.sentences, 3);
        assert_eq!(s.paragraphs, 2);
    }
}
