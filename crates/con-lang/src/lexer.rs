//! Lines and tokens of a `.con` script.
//!
//! Must agree with `python/bytefield_tools/con_scan.py` (a test diffs the two over the real data).
//! - CRLF, a lone CR and a lone LF each end a line (community files mix them)
//! - `rem …` comments one line; `beginrem` … `endrem` comment a block
//! - arguments split on blanks (space, tab, VT, FF); `"..."` keeps spaces; bytes are latin-1

/// One command line, before variable substitution.
#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    pub number: usize,
    pub command: String,
    pub args: Vec<String>,
}

pub fn decode(bytes: &[u8]) -> String {
    bytes.iter().map(|&b| b as char).collect()
}

/// Split on CRLF / CR / LF.
pub fn split_lines(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let b = text.as_bytes();
    let (mut start, mut i) = (0, 0);
    while i < b.len() {
        match b[i] {
            b'\n' => {
                out.push(&text[start..i]);
                i += 1;
                start = i;
            }
            b'\r' => {
                out.push(&text[start..i]);
                i += 1;
                if i < b.len() && b[i] == b'\n' {
                    i += 1;
                }
                start = i;
            }
            _ => i += 1,
        }
    }
    if start < b.len() {
        out.push(&text[start..]);
    }
    out
}

/// Blanks between arguments: ASCII only, like the Python scanner.
fn is_blank(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\x0b' | '\x0c')
}

pub fn split_args(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut chars = s.chars().peekable();
    while let Some(&c) = chars.peek() {
        if is_blank(c) {
            chars.next();
        } else if c == '"' {
            chars.next();
            let mut tok = String::new();
            for c in chars.by_ref() {
                if c == '"' {
                    break;
                }
                tok.push(c);
            }
            out.push(tok);
        } else {
            let mut tok = String::new();
            while let Some(&c) = chars.peek() {
                if is_blank(c) {
                    break;
                }
                tok.push(c);
                chars.next();
            }
            out.push(tok);
        }
    }
    out
}

/// `rem` followed by end of line or by something that cannot continue a word.
fn is_rem(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() >= 3
        && b[..3].eq_ignore_ascii_case(b"rem")
        && b.get(3).is_none_or(|c| !(c.is_ascii_alphanumeric() || *c == b'_' || *c == b'.'))
}

/// The command lines of a script, comments and blank lines removed.
pub fn lines(text: &str) -> Vec<Line> {
    let mut out = Vec::new();
    let mut in_block = false;
    for (i, raw) in split_lines(text).into_iter().enumerate() {
        let s = raw.trim_matches(is_blank);
        if s.is_empty() {
            continue;
        }
        let low = s.to_ascii_lowercase();
        if in_block {
            if low.starts_with("endrem") {
                in_block = false;
            }
            continue;
        }
        if low.starts_with("beginrem") {
            in_block = true;
            continue;
        }
        if is_rem(s) {
            continue;
        }
        let mut parts = split_args(s);
        if parts.is_empty() {
            continue;
        }
        let command = parts.remove(0);
        out.push(Line { number: i + 1, command, args: parts });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_endings() {
        assert_eq!(split_lines("a\r\nb\rc\nd"), vec!["a", "b", "c", "d"]);
        assert_eq!(split_lines("a\r\r\n"), vec!["a", ""]);
    }

    #[test]
    fn comments_and_quotes() {
        let l = lines("rem x\nrem*****\nbeginrem\nA.b 1\nendrem\nremove.me 3\nA.c \"x y\" 2\n");
        assert_eq!(l.len(), 2);
        assert_eq!(l[0].command, "remove.me");
        assert_eq!(l[1].args, vec!["x y", "2"]);
        assert_eq!(l[1].number, 7);
    }
}
