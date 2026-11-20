//! A bounded parser for controlled, synthetic bootstrap fixtures.
//!
//! This is not an implementation of the HTML tree-construction algorithm. The
//! fixture grammar requires explicitly balanced, properly nested element tags;
//! it does not insert html/head/body nodes or perform omitted-tag/table recovery.
//! Tables, templates, foreign content, select controls, and special text modes
//! other than style/script are rejected rather than assigned invented semantics.
//! Comments and the optional HTML doctype are consumed but are not DOM nodes in
//! this bootstrap data model. Script text is inert and is never executed.
//!
//! Named character references are limited to amp, lt, gt, quot, and apos. Numeric
//! references implement invalid-code-point replacement and the HTML C1 mapping.
//! References in the supported grammar require a semicolon. These restrictions
//! are explicit input errors, not a claim that other HTML is invalid.
//! Input CRLF and lone CR are normalized to LF before tokenization; diagnostics
//! index the normalized UTF-8 byte stream.

use std::collections::BTreeMap;
use std::fmt::Write;

pub const MAX_INPUT_BYTES: usize = 1_048_576;
pub const MAX_NODES: usize = 10_000;
pub const MAX_DEPTH: usize = 128;
pub const MAX_ATTRIBUTES: usize = 128;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Document {
    pub nodes: Vec<Node>,
    pub root: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Node {
    pub parent: Option<usize>,
    pub children: Vec<usize>,
    pub kind: NodeKind,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NodeKind {
    Document,
    Element {
        name: String,
        attributes: BTreeMap<String, String>,
    },
    Text(String),
}

impl Document {
    /// A deterministic JSON representation with stable node IDs and attribute order.
    pub fn canonical_json(&self) -> String {
        let mut out = format!("{{\"root\":{},\"nodes\":[", self.root);
        for (id, node) in self.nodes.iter().enumerate() {
            if id != 0 {
                out.push(',');
            }
            write!(out, "{{\"id\":{id},\"parent\":").expect("writing to a String");
            match node.parent {
                Some(parent) => write!(out, "{parent}").expect("writing to a String"),
                None => out.push_str("null"),
            }
            out.push_str(",\"children\":[");
            for (index, child) in node.children.iter().enumerate() {
                if index != 0 {
                    out.push(',');
                }
                write!(out, "{child}").expect("writing to a String");
            }
            out.push_str("],\"type\":");
            match &node.kind {
                NodeKind::Document => out.push_str("\"document\""),
                NodeKind::Text(text) => {
                    out.push_str("\"text\",\"text\":");
                    out.push_str(&json_string(text));
                }
                NodeKind::Element { name, attributes } => {
                    out.push_str("\"element\",\"name\":");
                    out.push_str(&json_string(name));
                    out.push_str(",\"attributes\":{");
                    for (index, (key, value)) in attributes.iter().enumerate() {
                        if index != 0 {
                            out.push(',');
                        }
                        out.push_str(&json_string(key));
                        out.push(':');
                        out.push_str(&json_string(value));
                    }
                    out.push('}');
                }
            }
            out.push('}');
        }
        out.push_str("]}");
        out
    }
}

/// Quote a string as JSON, retaining valid Unicode and escaping all JSON controls.
pub fn json_string(input: &str) -> String {
    let mut out = String::with_capacity(input.len() + 2);
    out.push('"');
    for ch in input.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{00}'..='\u{1f}' => {
                write!(out, "\\u{:04x}", ch as u32).expect("writing to a String");
            }
            _ => out.push(ch),
        }
    }
    out.push('"');
    out
}

/// Parse the documented bootstrap grammar or return a byte-indexed diagnostic.
pub fn parse(input: &str) -> Result<Document, String> {
    if input.len() > MAX_INPUT_BYTES {
        return Err(format!("input exceeds {MAX_INPUT_BYTES} byte limit"));
    }
    if let Some(index) = input.find('\0') {
        return Err(format!(
            "byte {index}: NUL is outside the bootstrap grammar"
        ));
    }
    let normalized = input.replace("\r\n", "\n").replace('\r', "\n");
    Parser::new(&normalized).run()
}

struct Parser<'a> {
    input: &'a str,
    position: usize,
    document: Document,
    stack: Vec<usize>,
    saw_doctype: bool,
    saw_element: bool,
}

impl<'a> Parser<'a> {
    fn new(input: &'a str) -> Self {
        Self {
            input,
            position: 0,
            document: Document {
                root: 0,
                nodes: vec![Node {
                    parent: None,
                    children: Vec::new(),
                    kind: NodeKind::Document,
                }],
            },
            stack: vec![0],
            saw_doctype: false,
            saw_element: false,
        }
    }

    fn run(mut self) -> Result<Document, String> {
        while self.position < self.input.len() {
            let remaining = &self.input[self.position..];
            if remaining.starts_with("<!--") {
                self.comment()?;
            } else if remaining.starts_with("</") {
                self.end_tag()?;
            } else if remaining.starts_with("<!") {
                self.doctype()?;
            } else if remaining.starts_with('<') {
                self.start_tag()?;
            } else {
                self.text()?;
            }
        }
        if self.stack.len() > 1 {
            let name = self.element_name(*self.stack.last().expect("root always exists"));
            return Err(self.error(&format!("unclosed <{name}> element")));
        }
        Ok(self.document)
    }

    fn error(&self, message: &str) -> String {
        format!("byte {}: {message}", self.position)
    }

    fn element_name(&self, id: usize) -> &str {
        match &self.document.nodes[id].kind {
            NodeKind::Element { name, .. } => name,
            _ => "#document",
        }
    }

    fn add_node(&mut self, parent: usize, kind: NodeKind) -> Result<usize, String> {
        if self.document.nodes.len() >= MAX_NODES {
            return Err(self.error(&format!("node count exceeds {MAX_NODES} limit")));
        }
        let id = self.document.nodes.len();
        self.document.nodes.push(Node {
            parent: Some(parent),
            children: Vec::new(),
            kind,
        });
        self.document.nodes[parent].children.push(id);
        Ok(id)
    }

    fn skip_space(&mut self) {
        while self.position < self.input.len()
            && is_html_space(self.input.as_bytes()[self.position])
        {
            self.position += 1;
        }
    }

    fn comment(&mut self) -> Result<(), String> {
        let start = self.position + 4;
        let Some(offset) = self.input[start..].find("-->") else {
            return Err(self.error("unterminated comment"));
        };
        self.position = start + offset + 3;
        Ok(())
    }

    fn doctype(&mut self) -> Result<(), String> {
        let Some(offset) = self.input[self.position..].find('>') else {
            return Err(self.error("unterminated declaration"));
        };
        let declaration = &self.input[self.position..self.position + offset + 1];
        let inner = &declaration[2..declaration.len() - 1];
        let parts: Vec<&str> = inner
            .split(is_html_space_char)
            .filter(|s| !s.is_empty())
            .collect();
        if parts.len() != 2
            || !parts[0].eq_ignore_ascii_case("doctype")
            || !parts[1].eq_ignore_ascii_case("html")
        {
            return Err(self.error("only <!doctype html> is supported in the bootstrap grammar"));
        }
        if self.saw_doctype || self.saw_element || self.stack.len() != 1 {
            return Err(self.error("doctype must occur once, before all elements"));
        }
        self.saw_doctype = true;
        self.position += offset + 1;
        Ok(())
    }

    fn tag_name(&mut self) -> Result<String, String> {
        let start = self.position;
        let bytes = self.input.as_bytes();
        if self.position >= bytes.len() || !bytes[self.position].is_ascii_alphabetic() {
            return Err(self.error("expected an ASCII element name"));
        }
        self.position += 1;
        while self.position < bytes.len()
            && (bytes[self.position].is_ascii_alphanumeric() || bytes[self.position] == b'-')
        {
            self.position += 1;
        }
        Ok(self.input[start..self.position].to_ascii_lowercase())
    }

    fn start_tag(&mut self) -> Result<(), String> {
        self.position += 1;
        let name = self.tag_name()?;
        if unsupported_element(&name) {
            return Err(self.error(&format!(
                "<{name}> requires an HTML tree-construction mode outside the bootstrap grammar"
            )));
        }
        let mut attributes = BTreeMap::new();
        let mut attribute_count = 0;
        let mut self_closing = false;
        loop {
            let before_space = self.position;
            self.skip_space();
            if self.position == self.input.len() {
                return Err(self.error("unterminated start tag"));
            }
            let byte = self.input.as_bytes()[self.position];
            if byte == b'>' {
                self.position += 1;
                break;
            }
            if self.input[self.position..].starts_with("/>") {
                self.position += 2;
                self_closing = true;
                break;
            }
            if before_space == self.position {
                return Err(self.error("attributes must be separated by HTML whitespace"));
            }
            attribute_count += 1;
            if attribute_count > MAX_ATTRIBUTES {
                return Err(self.error(&format!("attribute count exceeds {MAX_ATTRIBUTES} limit")));
            }
            let key_start = self.position;
            while self.position < self.input.len() {
                let b = self.input.as_bytes()[self.position];
                if is_html_space(b) || matches!(b, b'=' | b'>' | b'/') {
                    break;
                }
                if !b.is_ascii() || matches!(b, b'\'' | b'"' | b'<' | b'`') {
                    return Err(self.error("unsupported attribute-name character"));
                }
                self.position += 1;
            }
            if self.position == key_start {
                return Err(self.error("expected an attribute name"));
            }
            let key = self.input[key_start..self.position].to_ascii_lowercase();
            let after_key = self.position;
            self.skip_space();
            let value = if self.position < self.input.len()
                && self.input.as_bytes()[self.position] == b'='
            {
                self.position += 1;
                self.skip_space();
                self.attribute_value()?
            } else {
                self.position = after_key;
                String::new()
            };
            attributes.entry(key).or_insert(value);
        }
        if self_closing && !is_void(&name) {
            return Err(self.error("self-closing syntax is supported only for HTML void elements"));
        }
        if self.stack.len() > MAX_DEPTH {
            return Err(self.error(&format!("element depth exceeds {MAX_DEPTH} limit")));
        }
        let parent = *self.stack.last().expect("root always exists");
        let raw = matches!(name.as_str(), "style" | "script");
        let void = is_void(&name);
        let id = self.add_node(
            parent,
            NodeKind::Element {
                name: name.clone(),
                attributes,
            },
        )?;
        self.saw_element = true;
        if raw {
            self.raw_text(id, &name)?;
        } else if !void {
            self.stack.push(id);
        }
        Ok(())
    }

    fn attribute_value(&mut self) -> Result<String, String> {
        if self.position == self.input.len() {
            return Err(self.error("missing attribute value"));
        }
        let first = self.input.as_bytes()[self.position];
        let start;
        let end;
        if matches!(first, b'\'' | b'"') {
            self.position += 1;
            start = self.position;
            let Some(offset) = self.input.as_bytes()[start..]
                .iter()
                .position(|b| *b == first)
            else {
                return Err(self.error("unterminated quoted attribute value"));
            };
            end = start + offset;
            self.position = end + 1;
        } else {
            start = self.position;
            while self.position < self.input.len() {
                let byte = self.input.as_bytes()[self.position];
                if is_html_space(byte) || byte == b'>' {
                    break;
                }
                if matches!(byte, b'\'' | b'"' | b'<' | b'=' | b'`') {
                    return Err(self.error("invalid unquoted attribute-value character"));
                }
                self.position += 1;
            }
            end = self.position;
            if start == end {
                return Err(self.error("missing attribute value"));
            }
        }
        decode_references(&self.input[start..end])
            .map_err(|message| format!("byte {start}: {message}"))
    }

    fn closing_name(&mut self) -> Result<String, String> {
        self.position += 2;
        let name = self.tag_name()?;
        self.skip_space();
        if self.position == self.input.len() || self.input.as_bytes()[self.position] != b'>' {
            return Err(self.error("closing tags must end with > and cannot have attributes"));
        }
        self.position += 1;
        Ok(name)
    }

    fn end_tag(&mut self) -> Result<(), String> {
        let name = self.closing_name()?;
        if self.stack.len() == 1 {
            return Err(self.error(&format!("unexpected closing </{name}> tag")));
        }
        let current = *self.stack.last().expect("root always exists");
        if self.element_name(current) != name {
            return Err(self.error(&format!(
                "misnested closing </{name}>; expected </{}>",
                self.element_name(current)
            )));
        }
        self.stack.pop();
        Ok(())
    }

    fn raw_text(&mut self, parent: usize, name: &str) -> Result<(), String> {
        let start = self.position;
        let needle = format!("</{name}");
        let bytes = self.input.as_bytes();
        let mut cursor = start;
        let end = loop {
            if cursor + needle.len() > bytes.len() {
                return Err(self.error(&format!("unclosed <{name}> raw-text element")));
            }
            if bytes[cursor..cursor + needle.len()].eq_ignore_ascii_case(needle.as_bytes()) {
                let next = cursor + needle.len();
                if next < bytes.len() && (bytes[next] == b'>' || is_html_space(bytes[next])) {
                    break cursor;
                }
            }
            cursor += 1;
        };
        if name == "script" && self.input[start..end].contains("<!--") {
            return Err(
                self.error("script escaped-comment states are outside the bootstrap grammar")
            );
        }
        if start != end {
            self.add_node(parent, NodeKind::Text(self.input[start..end].to_string()))?;
        }
        self.position = end;
        let close = self.closing_name()?;
        if close != name {
            return Err(self.error("raw-text closing tag mismatch"));
        }
        Ok(())
    }

    fn text(&mut self) -> Result<(), String> {
        let start = self.position;
        let length = self.input[start..]
            .find('<')
            .unwrap_or(self.input.len() - start);
        self.position += length;
        let text = decode_references(&self.input[start..self.position])
            .map_err(|message| format!("byte {start}: {message}"))?;
        let parent = *self.stack.last().expect("root always exists");
        if let Some(&last) = self.document.nodes[parent].children.last()
            && let NodeKind::Text(previous) = &mut self.document.nodes[last].kind
        {
            previous.push_str(&text);
            return Ok(());
        }
        self.add_node(parent, NodeKind::Text(text))?;
        Ok(())
    }
}

fn is_html_space(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\n' | b'\r' | 0x0c)
}

fn is_html_space_char(ch: char) -> bool {
    matches!(ch, ' ' | '\t' | '\n' | '\r' | '\u{0c}')
}

fn is_void(name: &str) -> bool {
    matches!(
        name,
        "area"
            | "base"
            | "br"
            | "col"
            | "embed"
            | "hr"
            | "img"
            | "input"
            | "link"
            | "meta"
            | "param"
            | "source"
            | "track"
            | "wbr"
    )
}

fn unsupported_element(name: &str) -> bool {
    matches!(
        name,
        "table"
            | "thead"
            | "tbody"
            | "tfoot"
            | "tr"
            | "td"
            | "th"
            | "caption"
            | "colgroup"
            | "template"
            | "svg"
            | "math"
            | "select"
            | "option"
            | "optgroup"
            | "textarea"
            | "title"
            | "xmp"
            | "iframe"
            | "noembed"
            | "noframes"
            | "noscript"
            | "plaintext"
    )
}

fn decode_references(input: &str) -> Result<String, String> {
    let mut out = String::with_capacity(input.len());
    let mut position = 0;
    while let Some(relative) = input[position..].find('&') {
        let start = position + relative;
        out.push_str(&input[position..start]);
        let rest = &input[start + 1..];
        let first = rest.as_bytes().first().copied();
        if !first.is_some_and(|b| b.is_ascii_alphanumeric() || b == b'#') {
            out.push('&');
            position = start + 1;
            continue;
        }
        let token_length = rest
            .bytes()
            .take_while(|b| b.is_ascii_alphanumeric() || *b == b'#')
            .count();
        let token = &rest[..token_length];
        if rest.as_bytes().get(token_length) != Some(&b';') {
            return Err(format!(
                "character reference &{token} requires a semicolon in the bootstrap grammar"
            ));
        }
        let decoded = match token {
            "amp" => '&',
            "lt" => '<',
            "gt" => '>',
            "quot" => '"',
            "apos" => '\'',
            _ if token.starts_with('#') => numeric_reference(token)?,
            _ => return Err(format!("unsupported named character reference &{token};")),
        };
        out.push(decoded);
        position = start + token_length + 2;
    }
    out.push_str(&input[position..]);
    Ok(out)
}

fn numeric_reference(token: &str) -> Result<char, String> {
    let digits = &token[1..];
    let (digits, radix) = match digits.as_bytes().first() {
        Some(b'x' | b'X') => (&digits[1..], 16),
        _ => (digits, 10),
    };
    if digits.is_empty() || !digits.bytes().all(|b| (b as char).is_digit(radix)) {
        return Err(format!("invalid numeric character reference &{token};"));
    }
    let value = u32::from_str_radix(digits, radix).unwrap_or(0x11_0000);
    let value = match value {
        0x80 => 0x20ac,
        0x82 => 0x201a,
        0x83 => 0x0192,
        0x84 => 0x201e,
        0x85 => 0x2026,
        0x86 => 0x2020,
        0x87 => 0x2021,
        0x88 => 0x02c6,
        0x89 => 0x2030,
        0x8a => 0x0160,
        0x8b => 0x2039,
        0x8c => 0x0152,
        0x8e => 0x017d,
        0x91 => 0x2018,
        0x92 => 0x2019,
        0x93 => 0x201c,
        0x94 => 0x201d,
        0x95 => 0x2022,
        0x96 => 0x2013,
        0x97 => 0x2014,
        0x98 => 0x02dc,
        0x99 => 0x2122,
        0x9a => 0x0161,
        0x9b => 0x203a,
        0x9c => 0x0153,
        0x9e => 0x017e,
        0x9f => 0x0178,
        _ => value,
    };
    Ok(if value == 0 {
        '\u{fffd}'
    } else {
        char::from_u32(value).unwrap_or('\u{fffd}')
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text_at(document: &Document, id: usize) -> &str {
        match &document.nodes[id].kind {
            NodeKind::Text(text) => text,
            _ => panic!("expected text node"),
        }
    }

    #[test]
    fn builds_identity_parent_and_children_with_unicode() {
        let d = parse("<div id='x'><span>नमस्ते العربية</span><br>done</div>").unwrap();
        assert_eq!(d.root, 0);
        assert_eq!(d.nodes[0].children, [1]);
        assert_eq!(d.nodes[1].children, [2, 4, 5]);
        assert_eq!(d.nodes[3].parent, Some(2));
        assert_eq!(text_at(&d, 3), "नमस्ते العربية");
        assert_eq!(text_at(&d, 5), "done");
    }

    #[test]
    fn casefolds_names_and_preserves_first_duplicate_attribute() {
        let d = parse("<DIV ID='first' id=second data-v=ok disabled></div>").unwrap();
        let NodeKind::Element { name, attributes } = &d.nodes[1].kind else {
            panic!("element");
        };
        assert_eq!(name, "div");
        assert_eq!(attributes["id"], "first");
        assert_eq!(attributes["data-v"], "ok");
        assert_eq!(attributes["disabled"], "");
    }

    #[test]
    fn decodes_supported_references_in_text_and_attributes() {
        let d =
            parse("<p title='&quot;x&quot;'>&amp;&lt;&gt;&apos;&#65;&#x1f642;&#128;</p>").unwrap();
        assert_eq!(text_at(&d, 2), "&<>'A🙂€");
        let NodeKind::Element { attributes, .. } = &d.nodes[1].kind else {
            panic!("element");
        };
        assert_eq!(attributes["title"], "\"x\"");
    }

    #[test]
    fn invalid_numeric_codepoints_are_replaced() {
        let d = parse("&#0;&#xD800;&#1114112;&#999999999999999999999999999;").unwrap();
        assert_eq!(text_at(&d, 1), "����");
    }

    #[test]
    fn preprocesses_crlf_and_lone_cr_before_text_and_attributes() {
        let d = parse("<p title='a\r\nb\rc'>x\r\ny\rz</p>").unwrap();
        assert_eq!(text_at(&d, 2), "x\ny\nz");
        let NodeKind::Element { attributes, .. } = &d.nodes[1].kind else {
            panic!("element");
        };
        assert_eq!(attributes["title"], "a\nb\nc");
    }

    #[test]
    fn rejects_unimplemented_reference_grammar() {
        for input in ["&copy;", "&amp", "&#;", "&#x;", "&#12z;"] {
            assert!(parse(input).is_err(), "{input}");
        }
        assert_eq!(text_at(&parse("A & B").unwrap(), 1), "A & B");
    }

    #[test]
    fn consumes_doctype_and_comments_without_inventing_nodes() {
        let d = parse("<!DOCTYPE html><!--before--><p>a<!--between-->b</p>").unwrap();
        assert_eq!(d.nodes.len(), 3);
        assert_eq!(text_at(&d, 2), "ab");
        assert!(parse("<!doctype html><!doctype html>").is_err());
        assert!(parse("<p></p><!doctype html>").is_err());
        assert!(parse("<!doctype svg>").is_err());
    }

    #[test]
    fn raw_style_and_script_are_inert_and_do_not_decode_references() {
        let d = parse(
            "<style>.x { color: red; } &amp;</STYLE><script>if (a < b) x = '&amp;';</script>",
        )
        .unwrap();
        assert_eq!(text_at(&d, 2), ".x { color: red; } &amp;");
        assert_eq!(text_at(&d, 4), "if (a < b) x = '&amp;';");
        assert!(parse("<script><!--<script></script>").is_err());
    }

    #[test]
    fn raw_end_requires_a_tag_name_boundary() {
        let d = parse("<style>a</stylex>b</style>").unwrap();
        assert_eq!(text_at(&d, 2), "a</stylex>b");
        assert!(parse("<style>unterminated").is_err());
        assert!(parse("<style>x</style bad>").is_err());
    }

    #[test]
    fn rejects_truncation_misnest_and_illegal_self_close() {
        for input in [
            "<",
            "<p",
            "<p x='",
            "<!--",
            "<!doctype html",
            "<p>",
            "</p>",
            "<p><b></p></b>",
            "<div/>",
            "<p x=>",
            "<p x='a'y='b'></p>",
        ] {
            assert!(parse(input).is_err(), "{input}");
        }
    }

    #[test]
    fn rejects_unimplemented_tree_construction_modes() {
        for input in [
            "<table></table>",
            "<template></template>",
            "<svg></svg>",
            "<math></math>",
            "<select></select>",
            "<textarea>x</textarea>",
        ] {
            assert!(
                parse(input)
                    .unwrap_err()
                    .contains("outside the bootstrap grammar")
            );
        }
    }

    #[test]
    fn void_tags_do_not_change_parent_and_unquoted_slash_is_value() {
        let d = parse("<div><img src=x/><br/><input disabled></div>").unwrap();
        assert_eq!(d.nodes[1].children, [2, 3, 4]);
        let NodeKind::Element { attributes, .. } = &d.nodes[2].kind else {
            panic!("element");
        };
        assert_eq!(attributes["src"], "x/");
        assert!(parse("<br></br>").is_err());
    }

    #[test]
    fn input_limit_and_nul_are_diagnostic() {
        assert!(parse(&"a".repeat(MAX_INPUT_BYTES)).is_ok());
        assert!(
            parse(&"a".repeat(MAX_INPUT_BYTES + 1))
                .unwrap_err()
                .contains("byte limit")
        );
        assert!(parse("a\0b").unwrap_err().contains("NUL"));
    }

    #[test]
    fn depth_boundary_is_exact() {
        let at = format!(
            "{}{}",
            "<div>".repeat(MAX_DEPTH),
            "</div>".repeat(MAX_DEPTH)
        );
        assert!(parse(&at).is_ok());
        let above = format!(
            "{}{}",
            "<div>".repeat(MAX_DEPTH + 1),
            "</div>".repeat(MAX_DEPTH + 1)
        );
        assert!(parse(&above).unwrap_err().contains("depth"));
    }

    #[test]
    fn node_and_attribute_limits_are_enforced() {
        let nodes = "<br>".repeat(MAX_NODES - 1);
        assert_eq!(parse(&nodes).unwrap().nodes.len(), MAX_NODES);
        assert!(parse(&(nodes + "<br>")).unwrap_err().contains("node count"));
        let attrs = (0..MAX_ATTRIBUTES)
            .map(|n| format!(" a{n}=x"))
            .collect::<String>();
        assert!(parse(&format!("<p{attrs}></p>")).is_ok());
        assert!(
            parse(&format!("<p{attrs} extra=x></p>"))
                .unwrap_err()
                .contains("attribute count")
        );
    }

    #[test]
    fn canonical_json_is_stable_and_escapes_controls() {
        let d = parse("<p z='&quot;' a='\\'>é\n</p>").unwrap();
        assert_eq!(
            d.canonical_json(),
            "{\"root\":0,\"nodes\":[{\"id\":0,\"parent\":null,\"children\":[1],\"type\":\"document\"},{\"id\":1,\"parent\":0,\"children\":[2],\"type\":\"element\",\"name\":\"p\",\"attributes\":{\"a\":\"\\\\\",\"z\":\"\\\"\"}},{\"id\":2,\"parent\":1,\"children\":[],\"type\":\"text\",\"text\":\"é\\n\"}]}"
        );
        assert_eq!(
            json_string("\u{01}\t\r\n\u{08}\u{0c}"),
            "\"\\u0001\\t\\r\\n\\b\\f\""
        );
    }
}
