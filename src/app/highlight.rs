use crate::text::{self, Color, Token};
use std::ops::Range;

pub(super) struct Highlighting {
    code: String,
    tokens: Vec<Token>,
}

impl Highlighting {
    pub fn new(code: String, language: text::Language) -> Self {
        let tokens = text::highlighting(&code, language);
        Self { code, tokens }
    }

    fn changes(&self, previous: Option<&Self>) -> (Range<usize>, &[Token]) {
        let Some(previous) = previous else {
            return (0..self.code.len(), &self.tokens);
        };
        let old = &previous.code;
        let new = &self.code;
        let mut prefix = old
            .bytes()
            .zip(new.bytes())
            .take_while(|(a, b)| a == b)
            .count();
        if prefix == old.len() && prefix == new.len() {
            // Rich Edit can replace text with identical characters but different formatting.
            return (0..new.len(), &self.tokens);
        }
        while !new.is_char_boundary(prefix) {
            prefix -= 1;
        }
        let mut suffix = old.as_bytes()[prefix..]
            .iter()
            .rev()
            .zip(new.as_bytes()[prefix..].iter().rev())
            .take_while(|(a, b)| a == b)
            .count();
        while !new.is_char_boundary(new.len() - suffix) {
            suffix -= 1;
        }

        let front = previous
            .tokens
            .iter()
            .zip(&self.tokens)
            .take_while(|(before, after)| {
                before.end <= prefix
                    && before.start == after.start
                    && before.end == after.end
                    && before.color == after.color
            })
            .count();
        let old_end = old.len() - suffix;
        let new_end = new.len() - suffix;
        let back = previous.tokens[front..]
            .iter()
            .rev()
            .zip(self.tokens[front..].iter().rev())
            .take_while(|(before, after)| {
                before.start >= old_end
                    && after.start >= new_end
                    && old.len() - before.start == new.len() - after.start
                    && old.len() - before.end == new.len() - after.end
                    && before.color == after.color
            })
            .count();

        let removed = &previous.tokens[front..previous.tokens.len() - back];
        let added = &self.tokens[front..self.tokens.len() - back];
        let start = prefix
            .min(removed.first().map_or(prefix, |token| token.start))
            .min(added.first().map_or(prefix, |token| token.start));
        let removed_end = removed
            .last()
            .map_or(new_end, |token| new_end + token.end.saturating_sub(old_end));
        let end = removed_end.max(added.last().map_or(new_end, |token| token.end));
        (start..end, added)
    }

    pub fn runs(
        &self,
        previous: Option<&Self>,
    ) -> impl Iterator<Item = (Range<i32>, Option<Color>)> {
        let (region, tokens) = self.changes(previous);
        let mut byte = region.start;
        let mut unit = text::wide_offset(&self.code, byte);
        let mut tokens = tokens.iter().peekable();
        std::iter::from_fn(move || {
            if byte >= region.end {
                return None;
            }
            let from = unit;
            let next = tokens.peek();
            let (end, color) = if next.is_some_and(|token| token.start <= byte) {
                let token = tokens.next().unwrap();
                (token.end, Some(token.color))
            } else {
                (next.map_or(region.end, |token| token.start), None)
            };
            unit += self.code[byte..end].encode_utf16().count() as i32;
            byte = end;
            Some((from..unit, color))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use text::Language::Rust;

    #[test]
    fn deleting_a_delimiter_recolors_unchanged_unicode_suffix() {
        let before = Highlighting::new(
            "fn test() { /* \u{d55c}\u{ae00}\u{1f980} */ let x = [8]; }".into(),
            Rust,
        );
        let after = Highlighting::new(
            "fn test() { /* \u{d55c}\u{ae00}\u{1f980}  let x = [8]; }".into(),
            Rust,
        );
        let runs = after.runs(Some(&before)).collect::<Vec<_>>();
        assert!(runs.iter().any(|(range, color)| range.end
            == after.code.encode_utf16().count() as i32
            && *color == Some(Color::Comment)));
        assert!(runs.first().unwrap().0.start > 0);
        let restore = before.runs(Some(&after)).collect::<Vec<_>>();
        assert!(
            restore
                .iter()
                .any(|(_, color)| *color == Some(Color::Bracket(1)))
        );
        assert!(restore.iter().any(|(_, color)| color.is_none()));
    }

    #[test]
    fn plain_deletion_does_not_recolor_surrounding_tokens() {
        let before = Highlighting::new("let mouse = thing(6);".into(), Rust);
        let after = Highlighting::new("let muse = thing(6);".into(), Rust);
        assert_eq!(after.runs(Some(&before)).count(), 0);
    }
}
