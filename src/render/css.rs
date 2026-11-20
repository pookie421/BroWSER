//! Token boundaries for the bounded bootstrap CSS grammar. Comments disappear
//! between tokens, while actual whitespace and nested component values remain.

pub(super) mod colors;
pub(super) mod values;

const MAX_COMPONENT_DEPTH: usize = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Token<'a> {
    Space,
    Name(&'a str),
    Number(&'a str),
    Hash(&'a str),
    String(&'a str),
    Function(&'a str),
    Block(u8, &'a str),
    Delimiter(u8),
}

pub(super) fn tokens(css: &str) -> Result<Vec<Token<'_>>, String> {
    let mut input = Input { css, index: 0 };
    let mut tokens = Vec::new();
    while let Some(token) = input.next(0)? {
        tokens.push(token);
    }
    Ok(tokens)
}

struct Input<'a> {
    css: &'a str,
    index: usize,
}

impl<'a> Input<'a> {
    fn rest(&self) -> &'a str {
        &self.css[self.index..]
    }

    fn next(&mut self, depth: usize) -> Result<Option<Token<'a>>, String> {
        while let Some(comment) = self.rest().strip_prefix("/*") {
            let end = comment
                .find("*/")
                .ok_or_else(|| "unterminated CSS comment".to_owned())?;
            self.index += end + 4;
        }
        let start = self.index;
        let rest = self.rest();
        let Some(&first) = rest.as_bytes().first() else {
            return Ok(None);
        };
        let token = if whitespace(first) {
            self.index += rest.bytes().take_while(|byte| whitespace(*byte)).count();
            Token::Space
        } else if matches!(first, b'\'' | b'"') {
            self.string(first)?;
            Token::String(&self.css[start..self.index])
        } else if matches!(first, b'{' | b'[' | b'(') {
            Token::Block(first, self.block(first, depth)?)
        } else if let Some(end) = number_end(rest) {
            self.index += end;
            if self.rest().starts_with('%') {
                self.index += 1;
            } else if starts_name(self.rest()) {
                self.name();
            }
            Token::Number(&self.css[start..self.index])
        } else if starts_name(rest) {
            self.name();
            if self.rest().starts_with('(') {
                self.block(b'(', depth)?;
                Token::Function(&self.css[start..self.index])
            } else {
                Token::Name(&self.css[start..self.index])
            }
        } else if first == b'#' && rest.as_bytes().get(1).is_some_and(|next| name_byte(*next)) {
            self.index += 1;
            self.name();
            Token::Hash(&self.css[start..self.index])
        } else {
            self.index += 1;
            Token::Delimiter(first)
        };
        Ok(Some(token))
    }

    fn block(&mut self, open: u8, depth: usize) -> Result<&'a str, String> {
        if depth == MAX_COMPONENT_DEPTH {
            return Err(format!(
                "CSS component nesting exceeds {MAX_COMPONENT_DEPTH}"
            ));
        }
        let close = match open {
            b'{' => b'}',
            b'[' => b']',
            _ => b')',
        };
        self.index += 1;
        let start = self.index;
        while let Some(token) = self.next(depth + 1)? {
            if token == Token::Delimiter(close) {
                return Ok(&self.css[start..self.index - 1]);
            }
        }
        if open == b'{' {
            Err("stylesheet rule has no closing brace".to_owned())
        } else {
            Err("unterminated CSS component value".to_owned())
        }
    }

    fn string(&mut self, quote: u8) -> Result<(), String> {
        self.index += 1;
        while let Some(character) = self.rest().chars().next() {
            self.index += character.len_utf8();
            if character == char::from(quote) {
                return Ok(());
            }
            if character == '\\' {
                if let Some(escaped) = self.rest().chars().next() {
                    self.index += escaped.len_utf8();
                }
            } else if matches!(character, '\n' | '\r' | '\x0c') {
                return Err("unescaped newline in CSS string".to_owned());
            }
        }
        Err("unterminated CSS string".to_owned())
    }

    fn name(&mut self) {
        while let Some(&byte) = self.rest().as_bytes().first() {
            if byte == b'\\' {
                self.index += 1;
                let digits = self
                    .rest()
                    .bytes()
                    .take(6)
                    .take_while(u8::is_ascii_hexdigit)
                    .count();
                if digits == 0 {
                    if let Some(escaped) = self.rest().chars().next() {
                        self.index += escaped.len_utf8();
                    }
                } else {
                    self.index += digits;
                    if self
                        .rest()
                        .as_bytes()
                        .first()
                        .is_some_and(|byte| whitespace(*byte))
                    {
                        self.index += 1;
                    }
                }
            } else if name_byte(byte) {
                self.index += self.rest().chars().next().unwrap().len_utf8();
            } else {
                break;
            }
        }
    }
}

fn whitespace(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\n' | b'\r' | b'\x0c')
}

fn name_start(byte: u8) -> bool {
    byte.is_ascii_alphabetic() || byte >= 0x80 || matches!(byte, b'_' | b'\\')
}

fn name_byte(byte: u8) -> bool {
    name_start(byte) || byte.is_ascii_digit() || byte == b'-'
}

fn starts_name(text: &str) -> bool {
    match text.as_bytes() {
        [b'-', second, ..] => name_start(*second) || *second == b'-',
        [first, ..] => name_start(*first),
        [] => false,
    }
}

// Consume the number prefix of a CSS numeric token, excluding a unit or '%'.
pub(super) fn number_end(text: &str) -> Option<usize> {
    let bytes = text.as_bytes();
    let start = usize::from(matches!(bytes.first(), Some(b'+' | b'-')));
    let mut end = start
        + bytes[start..]
            .iter()
            .take_while(|b| b.is_ascii_digit())
            .count();
    if bytes.get(end) == Some(&b'.') && bytes.get(end + 1).is_some_and(u8::is_ascii_digit) {
        end += 1;
        end += bytes[end..]
            .iter()
            .take_while(|b| b.is_ascii_digit())
            .count();
    }
    if end == start {
        return None;
    }
    if matches!(bytes.get(end), Some(b'e' | b'E')) {
        let exponent = end + 1 + usize::from(matches!(bytes.get(end + 1), Some(b'+' | b'-')));
        let digits = bytes[exponent..]
            .iter()
            .take_while(|b| b.is_ascii_digit())
            .count();
        if digits > 0 {
            end = exponent + digits;
        }
    }
    Some(end)
}
