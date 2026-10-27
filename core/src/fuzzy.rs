//! Approximate substring search: where in a page does a quote occur, allowing for OCR errors?
//!
//! Sellers' algorithm (edit distance where the match may start anywhere in the text), on folded
//! text. Quotes are short and pages a few thousand characters, so the O(n·m) table is cheap.

use crate::text::Folded;

/// The best match of `needle` in `haystack` (both already folded), as (start, end, edits) in
/// bytes of `haystack`, if it needs at most `max_ratio` edits per character of the needle.
pub fn find(haystack: &str, needle: &str, max_ratio: f64) -> Option<(usize, usize, usize)> {
    let n: Vec<char> = needle.chars().collect();
    if n.is_empty() {
        return None;
    }
    let h: Vec<(usize, char)> = haystack.char_indices().collect();
    let m = n.len();
    let max_edits = ((m as f64) * max_ratio).floor() as usize;
    // prev[i] = edits to match n[..i] ending at the previous haystack position; start[i] its start.
    let mut prev: Vec<usize> = (0..=m).collect();
    let mut prev_start: Vec<usize> = vec![0; m + 1];
    let mut best: Option<(usize, usize, usize)> = None;
    for (j, &(_, hc)) in h.iter().enumerate() {
        let mut cur = vec![0usize; m + 1];
        let mut cur_start = vec![j + 1; m + 1];
        cur[0] = 0; // a match can start anywhere
        for i in 1..=m {
            let sub = prev[i - 1] + usize::from(n[i - 1] != hc);
            let del = prev[i] + 1; // haystack char not in the needle
            let ins = cur[i - 1] + 1; // needle char missing from the haystack
            let (v, s) = if sub <= del && sub <= ins {
                (sub, if i == 1 { j } else { prev_start[i - 1] })
            } else if del <= ins {
                (del, prev_start[i])
            } else {
                (ins, cur_start[i - 1])
            };
            cur[i] = v;
            cur_start[i] = s;
        }
        if cur[m] <= max_edits && best.is_none_or(|(_, _, e)| cur[m] < e) {
            let start_byte = h[cur_start[m].min(h.len() - 1)].0;
            let end_byte = h.get(j + 1).map(|x| x.0).unwrap_or(haystack.len());
            best = Some((start_byte, end_byte, cur[m]));
        }
        prev = cur;
        prev_start = cur_start;
    }
    best
}

/// Finds `quote` in `page` (raw text). Returns the span in the raw page text and the share of
/// the quote that matched (1.0 = exact after folding).
pub fn locate(page: &str, quote: &str, max_ratio: f64) -> Option<(usize, usize, f64)> {
    let fp = Folded::new(page);
    let q = crate::text::fold(quote);
    let qlen = q.chars().count().max(1);
    let (s, e, edits) = find(&fp.text, &q, max_ratio)?;
    let (os, oe) = fp.original_range(s, e);
    Some((os, oe, 1.0 - edits as f64 / qlen as f64))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_and_noisy() {
        let page = "POLIȚA DE ASIGURARE RCA\nValabilă de la 15.03.2026 până la 14.03.2027\nAsigurat: ION POPESCU";
        let (s, e, score) =
            locate(page, "valabila de la 15.03.2026 pana la 14.03.2027", 0.1).unwrap();
        assert_eq!(&page[s..e], "Valabilă de la 15.03.2026 până la 14.03.2027");
        assert_eq!(score, 1.0);
        // OCR read "Va1abila" and dropped a letter.
        let (s, e, score) =
            locate(page, "Va1abila de la 15.03.2026 pna la 14.03.2027", 0.1).unwrap();
        assert!(page[s..e].starts_with("Valabil"), "{}", &page[s..e]);
        assert!(score > 0.9 && score < 1.0);
    }

    #[test]
    fn absent_quotes_are_not_found() {
        let page = "Certificat de garanție pentru mașina de spălat";
        assert!(locate(page, "valabil până la 14.03.2027", 0.15).is_none());
        assert!(locate(page, "", 0.15).is_none());
    }

    #[test]
    fn best_of_several() {
        let (s, e, edits) = find("abc 14.03.2026 xyz 14.03.2027", "14.03.2027", 0.2).unwrap();
        assert_eq!((s, e, edits), (19, 29, 0));
    }
}
