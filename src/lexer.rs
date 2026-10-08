//! Tokenizer for DBC files.

use crate::model::DbcError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokKind {
    Ident,
    Num,
    Str,
    Punct,
}

#[derive(Debug, Clone)]
pub struct Tok {
    pub kind: TokKind,
    pub text: String,
    pub line: usize,
}

const PUNCT: &str = ":|@+-()[],;";

fn is_space(c: char) -> bool {
    c == ' ' || c == '\t' || c == '\r' || c == '\x0c' || c == '\x0b'
}

/// Split DBC source into tokens. Strings may span lines; `//` starts a comment.
pub fn lex(src: &str) -> Result<Vec<Tok>, DbcError> {
    let chars: Vec<char> = src.chars().collect();
    let n = chars.len();
    let mut toks: Vec<Tok> = Vec::new();
    let mut i: usize = 0;
    let mut line: usize = 1;
    while i < n {
        let c = chars[i];
        if c == '\n' {
            line += 1;
            i += 1;
            continue;
        }
        if is_space(c) {
            i += 1;
            continue;
        }
        if c == '/' && i + 1 < n && chars[i + 1] == '/' {
            while i < n && chars[i] != '\n' {
                i += 1;
            }
            continue;
        }
        if c == '"' {
            let start_line = line;
            i += 1;
            let mut buf = String::new();
            let mut closed = false;
            while i < n {
                let ch = chars[i];
                if ch == '\\' && i + 1 < n && (chars[i + 1] == '"' || chars[i + 1] == '\\') {
                    buf.push(chars[i + 1]);
                    i += 2;
                    continue;
                }
                if ch == '"' {
                    closed = true;
                    i += 1;
                    break;
                }
                if ch == '\n' {
                    line += 1;
                }
                buf.push(ch);
                i += 1;
            }
            if !closed {
                return Err(DbcError::new(start_line, "unterminated string".to_string()));
            }
            toks.push(Tok {
                kind: TokKind::Str,
                text: buf,
                line: start_line,
            });
            continue;
        }
        if c.is_ascii_alphabetic() || c == '_' {
            let mut j = i;
            while j < n && (chars[j].is_ascii_alphanumeric() || chars[j] == '_') {
                j += 1;
            }
            let text: String = chars[i..j].iter().collect();
            toks.push(Tok {
                kind: TokKind::Ident,
                text,
                line,
            });
            i = j;
            continue;
        }
        if c.is_ascii_digit() || (c == '.' && i + 1 < n && chars[i + 1].is_ascii_digit()) {
            let mut j = i;
            while j < n && chars[j].is_ascii_digit() {
                j += 1;
            }
            if j < n && chars[j] == '.' {
                j += 1;
                while j < n && chars[j].is_ascii_digit() {
                    j += 1;
                }
            }
            if j < n && (chars[j] == 'e' || chars[j] == 'E') {
                let mut k = j + 1;
                if k < n && (chars[k] == '+' || chars[k] == '-') {
                    k += 1;
                }
                if k < n && chars[k].is_ascii_digit() {
                    j = k;
                    while j < n && chars[j].is_ascii_digit() {
                        j += 1;
                    }
                }
            }
            let text: String = chars[i..j].iter().collect();
            toks.push(Tok {
                kind: TokKind::Num,
                text,
                line,
            });
            i = j;
            continue;
        }
        if PUNCT.contains(c) {
            toks.push(Tok {
                kind: TokKind::Punct,
                text: c.to_string(),
                line,
            });
            i += 1;
            continue;
        }
        return Err(DbcError::new(
            line,
            format!("unexpected character '{}'", c.escape_default()),
        ));
    }
    Ok(toks)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokenizes_numbers_strings_and_comments() {
        let toks = lex("SG_ X : 0|16@1- (1E-005,-40) [.5|2.] \"a \\\"q\\\" b\" // tail\nBO_").unwrap();
        let texts: Vec<&str> = toks.iter().map(|t| t.text.as_str()).collect();
        assert!(texts.contains(&"1E-005"));
        assert!(texts.contains(&".5"));
        assert!(texts.contains(&"2."));
        assert!(texts.contains(&"a \"q\" b"));
        let last = toks.last().unwrap();
        assert_eq!(last.text, "BO_");
        assert_eq!(last.line, 2);
    }

    #[test]
    fn multiline_strings_advance_the_line_count() {
        let toks = lex("CM_ \"one\ntwo\";\nBO_").unwrap();
        assert_eq!(toks[1].text, "one\ntwo");
        assert_eq!(toks.last().unwrap().line, 3);
    }

    #[test]
    fn reports_unterminated_strings() {
        let e = lex("CM_ \"never closed;\n").unwrap_err();
        assert_eq!(e.line, 1);
        assert!(e.message.contains("unterminated string"));
    }
}
