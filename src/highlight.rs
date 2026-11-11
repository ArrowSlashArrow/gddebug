//! Syntax highlighting for listing lines (raw, details and tasm modes). [`tokens`] splits a line
//! into classified spans. The GUI colours them; the CLI colours them with ANSI escapes when it
//! prints to a terminal.

use std::ops::Range;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tok {
    /// Anything unclassified: words, spaces, punctuation.
    Plain,
    /// Instructions and trigger names: `SPAWN`, `IFE`, `ITEM_EDIT`.
    Keyword,
    /// `g12`, `group_12`, `_init`, `LEVEL`.
    Group,
    /// Counters, timers, collision blocks and built-in items: `c3`, `T1`, `POINTS`, `b4`, `COLL_P1`.
    Item,
    /// Numbers (with an `s`, `%` or `°` unit) and `true` / `false`.
    Number,
    /// `= + - * / < > ! ? | ~ ->`.
    Operator,
    /// TASM flag names: the `lmod:` in `lmod:2`.
    Flag,
    /// `abs(`, `floor(`, `round(`, `ceil(`.
    Function,
    /// A trailing `; ...` comment.
    Comment,
    /// Listing bookkeeping: `#123`, `x=`, `[spawn]`, a leading `;`.
    Meta,
    /// A routine label at the start of a line: `group_4:`.
    Label,
    /// A hex colour `#00409a`; drawn in that colour.
    Colour,
}

const FUNCTIONS: [&str; 4] = ["abs", "floor", "round", "ceil"];
const ITEM_WORDS: [&str; 11] = [
    "points",
    "POINTS",
    "MainTime",
    "MAINTIME",
    "attempts",
    "ATTEMPTS",
    "COLL_P1",
    "COLL_P2",
    "COLL_P_ANY",
    "P1",
    "P2",
];

/// `prefix` followed by one or more digits (and nothing else).
fn numbered(w: &str, prefix: &str) -> bool {
    w.strip_prefix(prefix)
        .is_some_and(|d| !d.is_empty() && d.bytes().all(|b| b.is_ascii_digit()))
}

fn word_kind(w: &str) -> Tok {
    if numbered(w, "g") || numbered(w, "group_") || w == "_init" || w == "LEVEL" {
        Tok::Group
    } else if ["c", "C", "t", "T", "b", "coll"]
        .iter()
        .any(|p| numbered(w, p))
        || ITEM_WORDS.contains(&w)
    {
        Tok::Item
    } else if w == "true" || w == "false" {
        Tok::Number
    } else if w.len() >= 2
        && w.bytes().any(|b| b.is_ascii_uppercase())
        && w.bytes()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')
    {
        Tok::Keyword
    } else {
        Tok::Plain
    }
}

fn is_ident(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// Splits `line` into classified byte ranges that cover it completely.
pub fn tokens(line: &str) -> Vec<(Range<usize>, Tok)> {
    let chars: Vec<(usize, char)> = line.char_indices().collect();
    let at = |k: usize| chars.get(k).map(|c| c.1);
    let pos = |k: usize| chars.get(k).map_or(line.len(), |c| c.0);
    let mut out: Vec<(Range<usize>, Tok)> = Vec::new();
    let mut push = |a: usize, b: usize, t: Tok| {
        if a < b {
            match out.last_mut() {
                Some((r, last)) if *last == t && r.end == a => r.end = b,
                _ => out.push((a..b, t)),
            }
        }
    };
    let indented = line.starts_with(char::is_whitespace);
    let mut k = 0;
    while k < chars.len() {
        let c = chars[k].1;
        let start = k;
        let prev = if k > 0 { at(k - 1) } else { None };
        let kind = if c == ';' {
            if line[..pos(k)].trim().is_empty() {
                // a whole-line comment row: its content is still highlighted
                k += 1;
                Tok::Meta
            } else {
                push(pos(k), line.len(), Tok::Comment);
                break;
            }
        } else if c.is_whitespace() {
            while at(k).is_some_and(char::is_whitespace) {
                k += 1;
            }
            Tok::Plain
        } else if c == '#' {
            let six = |f: fn(&char) -> bool| (1..=6).all(|d| at(k + d).as_ref().is_some_and(f));
            // "#123456" is an object index, "#00409a" a colour
            let hex = six(char::is_ascii_hexdigit)
                && !six(char::is_ascii_digit)
                && !at(k + 7).is_some_and(is_ident);
            let digits = at(k + 1).is_some_and(|d| d.is_ascii_digit());
            k += 1;
            if hex {
                k += 6;
                Tok::Colour
            } else if digits {
                while at(k).is_some_and(|d| d.is_ascii_digit()) {
                    k += 1;
                }
                Tok::Meta
            } else {
                Tok::Plain
            }
        } else if c == '[' && chars[k..].iter().any(|x| x.1 == ']') {
            while at(k) != Some(']') {
                k += 1;
            }
            k += 1;
            Tok::Meta
        } else if c.is_ascii_digit()
            || ((c == '-' || c == '.')
                && at(k + 1).is_some_and(|d| d.is_ascii_digit())
                && !prev.is_some_and(is_ident))
        {
            k += 1;
            while at(k).is_some_and(|d| d.is_ascii_digit() || d == '.') {
                k += 1;
            }
            // units: 0.5s, 95%, 90°
            if matches!(at(k), Some('%') | Some('°'))
                || (at(k) == Some('s') && !at(k + 1).is_some_and(is_ident))
            {
                k += 1;
            }
            if at(k).is_some_and(is_ident) {
                // part of a word after all (e.g. "2x")
                while at(k).is_some_and(is_ident) {
                    k += 1;
                }
                Tok::Plain
            } else {
                Tok::Number
            }
        } else if c.is_ascii_alphabetic() || c == '_' {
            while at(k).is_some_and(is_ident) {
                k += 1;
            }
            let w = &line[pos(start)..pos(k)];
            let next = at(k);
            if next == Some(':')
                && start == 0
                && !indented
                && line[pos(k + 1)..].trim_start().is_empty_or_comment()
            {
                k += 1;
                Tok::Label
            } else if next == Some(':')
                && (at(k + 1).is_some_and(|n| !n.is_whitespace()) || w == "remap")
            {
                k += 1;
                Tok::Flag
            } else if next == Some('(') && FUNCTIONS.contains(&w) {
                Tok::Function
            } else if (w == "x" || w == "y") && next == Some('=') {
                k += 1;
                Tok::Meta
            } else {
                word_kind(w)
            }
        } else if "=+-*/<>!?|~&±".contains(c) {
            while at(k).is_some_and(|o| "=+-*/<>!?|~&±".contains(o)) {
                k += 1;
            }
            Tok::Operator
        } else {
            k += 1;
            Tok::Plain
        };
        push(pos(start), pos(k), kind);
    }
    out
}

trait EmptyOrComment {
    fn is_empty_or_comment(&self) -> bool;
}

impl EmptyOrComment for str {
    fn is_empty_or_comment(&self) -> bool {
        self.is_empty() || self.starts_with(';')
    }
}

/// Parses `#rrggbb`.
pub fn hex_rgb(s: &str) -> Option<(u8, u8, u8)> {
    let h = s.strip_prefix('#')?;
    let v = u32::from_str_radix(h.get(..6)?, 16).ok()?;
    Some(((v >> 16) as u8, (v >> 8) as u8, v as u8))
}

/// Text colour of a token, shared by the GUI and the CLI. `None` keeps the line's own colour.
pub fn rgb(t: Tok) -> Option<(u8, u8, u8)> {
    Some(match t {
        Tok::Plain | Tok::Keyword | Tok::Colour => return None,
        Tok::Group => (120, 200, 255),
        Tok::Item => (245, 190, 100),
        Tok::Number => (175, 225, 130),
        Tok::Operator => (235, 120, 165),
        Tok::Flag => (170, 150, 245),
        Tok::Function => (110, 215, 210),
        Tok::Comment | Tok::Meta => (125, 125, 125),
        Tok::Label => (235, 205, 95),
    })
}

/// The whole text with ANSI 24-bit colours, for printing to a terminal.
pub fn ansi(text: &str) -> String {
    let mut out = String::with_capacity(text.len() * 2);
    for (n, line) in text.split('\n').enumerate() {
        if n > 0 {
            out.push('\n');
        }
        for (r, t) in tokens(line) {
            let s = &line[r];
            let col = match t {
                Tok::Colour => hex_rgb(s),
                _ => rgb(t),
            };
            match (col, t) {
                (Some((r, g, b)), _) => out += &format!("\x1b[38;2;{r};{g};{b}m{s}\x1b[0m"),
                (None, Tok::Keyword) => out += &format!("\x1b[1m{s}\x1b[0m"),
                _ => out += s,
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(line: &str) -> Vec<(&str, Tok)> {
        tokens(line)
            .into_iter()
            .filter(|(_, t)| *t != Tok::Plain)
            .map(|(r, t)| (&line[r], t))
            .collect()
    }

    #[test]
    fn tasm_lines() {
        assert_eq!(
            kinds("  IFE group_3, g4, C1, 2 | lmod:2 rmode:floor   ; #25 x=105.0"),
            vec![
                ("IFE", Tok::Keyword),
                ("group_3", Tok::Group),
                ("g4", Tok::Group),
                ("C1", Tok::Item),
                ("2", Tok::Number),
                ("|", Tok::Operator),
                ("lmod:", Tok::Flag),
                ("2", Tok::Number),
                ("rmode:", Tok::Flag),
                ("; #25 x=105.0", Tok::Comment),
            ]
        );
        assert_eq!(
            kinds("group_4:   ; 3 object(s)"),
            vec![("group_4:", Tok::Label), ("; 3 object(s)", Tok::Comment)]
        );
        assert_eq!(
            kinds("  ~TOGGLEON g5"),
            vec![
                ("~", Tok::Operator),
                ("TOGGLEON", Tok::Keyword),
                ("g5", Tok::Group)
            ]
        );
    }

    #[test]
    fn details_lines() {
        assert_eq!(
            kinds("MainTime = abs(floor(t1 / 2.4))"),
            vec![
                ("MainTime", Tok::Item),
                ("=", Tok::Operator),
                ("abs", Tok::Function),
                ("floor", Tok::Function),
                ("t1", Tok::Item),
                ("/", Tok::Operator),
                ("2.4", Tok::Number),
            ]
        );
        assert_eq!(
            kinds("ch 1000 = #00409a in 0.5s"),
            vec![
                ("1000", Tok::Number),
                ("=", Tok::Operator),
                ("#00409a", Tok::Colour),
                ("0.5s", Tok::Number)
            ]
        );
        assert_eq!(
            kinds("#9076   x=  6750.0 [x-pos ] ADVANCED_RANDOM pick: g2 95%"),
            vec![
                ("#9076", Tok::Meta),
                ("x=", Tok::Meta),
                ("6750.0", Tok::Number),
                ("[x-pos ]", Tok::Meta),
                ("ADVANCED_RANDOM", Tok::Keyword),
                ("g2", Tok::Group),
                ("95%", Tok::Number),
            ]
        );
    }

    #[test]
    fn tokens_cover_the_line() {
        for line in [
            "  ; <- g1 via #5 ITEM_COMPARE (if true)",
            "±0.5 ° é ünïcode → ok",
            "",
            "#12345678 #fff",
        ] {
            let t = tokens(line);
            let mut end = 0;
            for (r, _) in &t {
                assert_eq!(r.start, end, "{line:?}: {t:?}");
                end = r.end;
            }
            assert_eq!(end, line.len(), "{line:?}");
        }
    }
}
