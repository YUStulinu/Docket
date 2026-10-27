//! Text folding: the form in which OCR text is searched and compared.
//!
//! Folded text is lowercase, without diacritics (ă â î ș ț and the cedilla forms ş ţ that old
//! documents and some OCR output use), with runs of whitespace collapsed to one space. Each folded
//! byte remembers where it came from in the original, so a match in the folded text maps back to
//! the exact span the user sees.

use unicode_normalization::UnicodeNormalization;

pub struct Folded {
    pub text: String,
    /// For each byte of `text`, the byte offset in the original string of the character it came from.
    map: Vec<usize>,
    original_len: usize,
}

impl Folded {
    pub fn new(original: &str) -> Folded {
        let mut text = String::with_capacity(original.len());
        let mut map = Vec::with_capacity(original.len());
        let mut last_space = true; // drops leading whitespace
        for (pos, ch) in original.char_indices() {
            if ch.is_whitespace() {
                if !last_space {
                    text.push(' ');
                    map.push(pos);
                    last_space = true;
                }
                continue;
            }
            last_space = false;
            for f in fold_char(ch) {
                let before = text.len();
                text.push(f);
                for _ in before..text.len() {
                    map.push(pos);
                }
            }
        }
        if text.ends_with(' ') {
            text.pop();
            map.pop();
        }
        Folded {
            text,
            map,
            original_len: original.len(),
        }
    }

    /// The original byte range of the folded range `start..end`.
    pub fn original_range(&self, start: usize, end: usize) -> (usize, usize) {
        let s = self.map.get(start).copied().unwrap_or(self.original_len);
        let e = if end >= self.map.len() {
            self.original_len
        } else {
            // The end is the start of the character after the last one matched.
            self.map[end]
        };
        (s, e.max(s))
    }
}

/// Folds one character: lowercase, base letter without diacritics.
pub fn fold_char(ch: char) -> impl Iterator<Item = char> {
    let lower: Vec<char> = ch.to_lowercase().collect();
    let mut out: Vec<char> = Vec::with_capacity(lower.len());
    for c in lower {
        match c {
            // Romanian letters, comma and cedilla forms.
            'ă' | 'â' => out.push('a'),
            'î' => out.push('i'),
            'ș' | 'ş' => out.push('s'),
            'ț' | 'ţ' => out.push('t'),
            // Typographic punctuation OCR produces.
            '\u{2018}' | '\u{2019}' | '\u{201a}' | '`' => out.push('\''),
            '\u{201c}' | '\u{201d}' | '\u{201e}' => out.push('"'),
            '\u{2013}' | '\u{2014}' => out.push('-'),
            c if c.is_ascii() => out.push(c),
            c => {
                // Anything else: its decomposition without combining marks.
                for d in c.nfd() {
                    if !is_combining(d) {
                        out.push(d);
                    }
                }
            }
        }
    }
    out.into_iter()
}

fn is_combining(c: char) -> bool {
    matches!(c as u32, 0x0300..=0x036f | 0x1ab0..=0x1aff | 0x1dc0..=0x1dff | 0x20d0..=0x20ff | 0xfe20..=0xfe2f)
}

/// Folds a whole string (without the offset map).
pub fn fold(s: &str) -> String {
    Folded::new(s).text
}

/// Words of folded text: runs of letters and digits.
pub fn words(folded: &str) -> impl Iterator<Item = (usize, &str)> {
    let mut out = Vec::new();
    let mut start = None;
    for (i, c) in folded.char_indices() {
        if c.is_alphanumeric() {
            if start.is_none() {
                start = Some(i);
            }
        } else if let Some(s) = start.take() {
            out.push((s, &folded[s..i]));
        }
    }
    if let Some(s) = start {
        out.push((s, &folded[s..]));
    }
    out.into_iter()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folds_romanian_and_whitespace() {
        assert_eq!(
            fold("  Garanție   până la\n14 MARTIE 2027 "),
            "garantie pana la 14 martie 2027"
        );
        assert_eq!(fold("Poliţă şi ÎNMATRICULARE"), "polita si inmatriculare");
        assert_eq!(fold("Müller – „test”"), "muller - \"test\"");
    }

    #[test]
    fn maps_back_to_the_original() {
        let original = "Valabilă până la: 14.03.2027";
        let f = Folded::new(original);
        let at = f.text.find("pana la").unwrap();
        let (s, e) = f.original_range(at, at + "pana la".len());
        assert_eq!(&original[s..e], "până la");
        let at = f.text.find("2027").unwrap();
        let (s, e) = f.original_range(at, at + 4);
        assert_eq!(&original[s..e], "2027");
    }

    #[test]
    fn splits_words() {
        let w: Vec<&str> = words("rca nr. 123/2026, valabil").map(|(_, w)| w).collect();
        assert_eq!(w, ["rca", "nr", "123", "2026", "valabil"]);
    }
}
