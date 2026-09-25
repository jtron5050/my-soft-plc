//! ST-subset lexer (case-insensitive keywords).

use crate::error::{CompileError, ErrorCode, Span};

/// Lexical token.
#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    /// Kind.
    pub kind: TokenKind,
    /// Span in source bytes.
    pub span: Span,
    /// Raw lexeme (for idents / literals).
    pub text: String,
}

/// Token kinds.
#[derive(Debug, Clone, PartialEq)]
pub enum TokenKind {
    /// End of file.
    Eof,
    /// Identifier (non-keyword).
    Ident,
    /// Integer literal text.
    IntLit,
    /// Real literal text.
    RealLit,
    /// TIME literal (ms value parsed into text as decimal ms).
    TimeLit,
    /// Keyword (uppercase canonical).
    Keyword(&'static str),
    /// `:=`
    Assign,
    /// `=>`
    Arrow,
    /// `=`
    Eq,
    /// `<>`
    Ne,
    /// `<`
    Lt,
    /// `<=`
    Le,
    /// `>`
    Gt,
    /// `>=`
    Ge,
    /// `+`
    Plus,
    /// `-`
    Minus,
    /// `*`
    Star,
    /// `/`
    Slash,
    /// `(`
    LParen,
    /// `)`
    RParen,
    /// `[`
    LBracket,
    /// `]`
    RBracket,
    /// `{`
    LBrace,
    /// `}`
    RBrace,
    /// `,`
    Comma,
    /// `;`
    Semi,
    /// `:`
    Colon,
    /// `.`
    Dot,
    /// `..`
    DotDot,
    /// Direct address `%I0` etc. (plane + index in text `"I:0"`).
    DirectAddr,
}

/// Keywords recognized (canonical uppercase).
const KEYWORDS: &[&str] = &[
    "PROGRAM",
    "END_PROGRAM",
    "FUNCTION_BLOCK",
    "END_FUNCTION_BLOCK",
    "VAR",
    "VAR_INPUT",
    "VAR_OUTPUT",
    "VAR_RETAIN",
    "VAR_GLOBAL",
    "VAR_IN_OUT",
    "VAR_EXTERNAL",
    "END_VAR",
    "CONSTANT",
    "AT",
    "IF",
    "THEN",
    "ELSIF",
    "ELSE",
    "END_IF",
    "CASE",
    "OF",
    "END_CASE",
    "WHILE",
    "DO",
    "END_WHILE",
    "FOR",
    "TO",
    "BY",
    "END_FOR",
    "REPEAT",
    "UNTIL",
    "END_REPEAT",
    "EXIT",
    "CONTINUE",
    "RETURN",
    "AND",
    "OR",
    "XOR",
    "NOT",
    "MOD",
    "TRUE",
    "FALSE",
    "BOOL",
    "INT",
    "DINT",
    "REAL",
    "TIME",
    "LINT",
    "LREAL",
    "STRING",
    "WSTRING",
    "ARRAY",
    "CONFIGURATION",
    "RESOURCE",
    "METHOD",
    "CLASS",
    "INTERFACE",
    "REF_TO",
    "ADR",
    "EN",
    "ENO",
];

fn keyword_lookup(s: &str) -> Option<&'static str> {
    let upper = s.to_ascii_uppercase();
    KEYWORDS
        .iter()
        .find(|k| k.eq_ignore_ascii_case(&upper))
        .copied()
}

/// Lex `source` into tokens (comments stripped).
pub fn lex(source: &str) -> Result<Vec<Token>, CompileError> {
    let bytes = source.as_bytes();
    let mut i = 0usize;
    let mut out = Vec::new();

    while i < bytes.len() {
        // Whitespace
        if bytes[i].is_ascii_whitespace() {
            i += 1;
            continue;
        }
        // (* comment *)
        if bytes[i] == b'(' && i + 1 < bytes.len() && bytes[i + 1] == b'*' {
            i += 2;
            let mut closed = false;
            while i + 1 < bytes.len() {
                if bytes[i] == b'*' && bytes[i + 1] == b')' {
                    i += 2;
                    closed = true;
                    break;
                }
                i += 1;
            }
            if !closed {
                return Err(CompileError::new(ErrorCode::ELex, "unclosed (* comment *)")
                    .with_span(Span::at(i)));
            }
            continue;
        }
        // // line comment (non-IEC convenience)
        if bytes[i] == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'/' {
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            continue;
        }

        let start = i;
        // Direct address %I0 / %Q12 / %M0 / %R0
        if bytes[i] == b'%' {
            i += 1;
            if i >= bytes.len() {
                return Err(
                    CompileError::new(ErrorCode::ELex, "lone '%'").with_span(Span::at(start))
                );
            }
            let plane = (bytes[i] as char).to_ascii_uppercase();
            if !matches!(plane, 'I' | 'Q' | 'M' | 'R') {
                return Err(CompileError::new(
                    ErrorCode::ELex,
                    format!("unknown direct address plane %{plane}"),
                )
                .with_span(Span::new(start, i + 1)));
            }
            i += 1;
            let num_start = i;
            while i < bytes.len() && bytes[i].is_ascii_digit() {
                i += 1;
            }
            if num_start == i {
                return Err(
                    CompileError::new(ErrorCode::ELex, "direct address missing index")
                        .with_span(Span::new(start, i)),
                );
            }
            let idx = &source[num_start..i];
            out.push(Token {
                kind: TokenKind::DirectAddr,
                span: Span::new(start, i),
                text: format!("{plane}:{idx}"),
            });
            continue;
        }

        // TIME literal T#…
        if (bytes[i] == b'T' || bytes[i] == b't') && i + 1 < bytes.len() && bytes[i + 1] == b'#' {
            i += 2;
            let lit_start = i;
            while i < bytes.len()
                && (bytes[i].is_ascii_alphanumeric()
                    || bytes[i] == b'_'
                    || bytes[i] == b'.'
                    || bytes[i] == b'-')
            {
                i += 1;
            }
            let body = &source[lit_start..i];
            let ms = parse_time_literal(body).map_err(|m| {
                CompileError::new(ErrorCode::ELex, m).with_span(Span::new(start, i))
            })?;
            out.push(Token {
                kind: TokenKind::TimeLit,
                span: Span::new(start, i),
                text: ms.to_string(),
            });
            continue;
        }

        // Number
        if bytes[i].is_ascii_digit() {
            let mut is_real = false;
            while i < bytes.len() && bytes[i].is_ascii_digit() {
                i += 1;
            }
            if i < bytes.len() && bytes[i] == b'.' {
                // Distinguish `1..10` from `1.5`
                if i + 1 < bytes.len() && bytes[i + 1] == b'.' {
                    // integer then DotDot handled later — finish int here
                } else if i + 1 < bytes.len() && bytes[i + 1].is_ascii_digit() {
                    is_real = true;
                    i += 1;
                    while i < bytes.len() && bytes[i].is_ascii_digit() {
                        i += 1;
                    }
                }
            }
            // exponent
            if is_real && i < bytes.len() && (bytes[i] == b'e' || bytes[i] == b'E') {
                i += 1;
                if i < bytes.len() && (bytes[i] == b'+' || bytes[i] == b'-') {
                    i += 1;
                }
                while i < bytes.len() && bytes[i].is_ascii_digit() {
                    i += 1;
                }
            }
            out.push(Token {
                kind: if is_real {
                    TokenKind::RealLit
                } else {
                    TokenKind::IntLit
                },
                span: Span::new(start, i),
                text: source[start..i].to_string(),
            });
            continue;
        }

        // Identifier / keyword
        if bytes[i].is_ascii_alphabetic() || bytes[i] == b'_' {
            i += 1;
            while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                i += 1;
            }
            let text = source[start..i].to_string();
            let kind = if let Some(kw) = keyword_lookup(&text) {
                TokenKind::Keyword(kw)
            } else {
                TokenKind::Ident
            };
            out.push(Token {
                kind,
                span: Span::new(start, i),
                text,
            });
            continue;
        }

        // Multi-char operators
        let two = if i + 1 < bytes.len() {
            &source[i..i + 2]
        } else {
            ""
        };
        let (kind, len) = match two {
            ":=" => (TokenKind::Assign, 2),
            "=>" => (TokenKind::Arrow, 2),
            "<>" => (TokenKind::Ne, 2),
            "<=" => (TokenKind::Le, 2),
            ">=" => (TokenKind::Ge, 2),
            ".." => (TokenKind::DotDot, 2),
            _ => match bytes[i] as char {
                '=' => (TokenKind::Eq, 1),
                '<' => (TokenKind::Lt, 1),
                '>' => (TokenKind::Gt, 1),
                '+' => (TokenKind::Plus, 1),
                '-' => (TokenKind::Minus, 1),
                '*' => (TokenKind::Star, 1),
                '/' => (TokenKind::Slash, 1),
                '(' => (TokenKind::LParen, 1),
                ')' => (TokenKind::RParen, 1),
                '[' => (TokenKind::LBracket, 1),
                ']' => (TokenKind::RBracket, 1),
                '{' => (TokenKind::LBrace, 1),
                '}' => (TokenKind::RBrace, 1),
                ',' => (TokenKind::Comma, 1),
                ';' => (TokenKind::Semi, 1),
                ':' => (TokenKind::Colon, 1),
                '.' => (TokenKind::Dot, 1),
                other => {
                    return Err(CompileError::new(
                        ErrorCode::ELex,
                        format!("unexpected character {other:?}"),
                    )
                    .with_span(Span::at(i)));
                }
            },
        };
        i += len;
        out.push(Token {
            kind,
            span: Span::new(start, i),
            text: source[start..i].to_string(),
        });
    }

    out.push(Token {
        kind: TokenKind::Eof,
        span: Span::at(bytes.len()),
        text: String::new(),
    });
    Ok(out)
}

fn parse_time_literal(body: &str) -> Result<i32, String> {
    // Supports T#1s, T#500ms, T#1m, T#1h, T#1.5s, combinations like T#1s500ms
    let b = body.to_ascii_lowercase();
    if b.is_empty() {
        return Err("empty TIME literal".into());
    }
    let mut total_ms: i64 = 0;
    let mut num = String::new();
    let mut chars = b.chars().peekable();
    while let Some(c) = chars.next() {
        if c.is_ascii_digit() || c == '.' {
            num.push(c);
            continue;
        }
        if num.is_empty() {
            return Err(format!("TIME literal expected number before {c}"));
        }
        let value: f64 = num.parse().map_err(|_| format!("bad TIME number {num}"))?;
        num.clear();
        let unit = match c {
            'h' => 3_600_000.0,
            'm' => {
                if chars.peek() == Some(&'s') {
                    chars.next();
                    1.0
                } else {
                    60_000.0
                }
            }
            's' => 1_000.0,
            other => return Err(format!("unknown TIME unit {other}")),
        };
        total_ms += (value * unit).round() as i64;
    }
    if !num.is_empty() {
        return Err("TIME literal missing unit".into());
    }
    i32::try_from(total_ms).map_err(|_| "TIME literal out of i32 ms range".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lex_program_keywords_and_assign() {
        let toks = lex("PROGRAM Main\n x := TRUE;\nEND_PROGRAM").unwrap();
        assert!(matches!(toks[0].kind, TokenKind::Keyword("PROGRAM")));
        assert!(toks.iter().any(|t| t.kind == TokenKind::Assign));
        assert!(toks
            .iter()
            .any(|t| matches!(t.kind, TokenKind::Keyword("TRUE"))));
    }

    #[test]
    fn lex_time_and_direct() {
        let toks = lex("PT := T#1s; AT %I0").unwrap();
        let time = toks.iter().find(|t| t.kind == TokenKind::TimeLit).unwrap();
        assert_eq!(time.text, "1000");
        let addr = toks
            .iter()
            .find(|t| t.kind == TokenKind::DirectAddr)
            .unwrap();
        assert_eq!(addr.text, "I:0");
    }
}
