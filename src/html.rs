//! A bounded parser for controlled, synthetic bootstrap fixtures.
//!
//! The default fixture grammar requires explicitly balanced, properly nested
//! element tags. Opt-in document and `div` fragment contexts support bounded
//! HTML tree construction for ordinary flow elements, including implicit
//! document structure and optional paragraph end tags.
//! The parser handles raw style text, title RCDATA, and script text without execution.
//! Other special text modes, tables, templates, foreign content, and select
//! controls remain outside the fixture grammar.
//! Comments are consumed without nodes. The optional HTML doctype is a leaf
//! directly beneath the document and counts toward the node limit.
//!
//! Named character references are limited to amp, lt, gt, quot, and apos. Numeric
//! references implement invalid-code-point replacement and the HTML C1 mapping.
//! References in the supported grammar require a semicolon. These restrictions
//! are explicit input errors, not a claim that other HTML is invalid.
//! The original input counts toward the byte limit. Before tokenization, one
//! leading UTF-8 signature is stripped and CRLF and lone CR are normalized to LF.
//! Byte diagnostics index this normalized UTF-8 stream starting at zero.

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::fmt::Write;

mod tree;
use tree::{DocumentPhase, TreeMode};

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
    Doctype(String),
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
                NodeKind::Doctype(name) => {
                    out.push_str("\"doctype\",\"name\":");
                    out.push_str(&json_string(name));
                }
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
    parse_with_mode(input, TreeMode::Bootstrap)
}

/// Parse a controlled document with implicit html/head/body elements and
/// optional paragraph end tags in the supported flow grammar.
pub fn parse_document(input: &str) -> Result<Document, String> {
    parse_with_mode(input, TreeMode::Document(DocumentPhase::BeforeHtml))
}

/// Parse a controlled `div` fragment and return its children under an owned root.
pub fn parse_fragment(input: &str, context: &str) -> Result<Document, String> {
    if !context.eq_ignore_ascii_case("div") {
        return Err("only a div HTML fragment context is supported".to_owned());
    }
    parse_with_mode(input, TreeMode::DivFragment)
}

fn parse_with_mode(input: &str, mode: TreeMode) -> Result<Document, String> {
    if input.len() > MAX_INPUT_BYTES {
        return Err(format!("input exceeds {MAX_INPUT_BYTES} byte limit"));
    }
    let normalized = preprocess(input)?;
    Parser::new(&normalized, mode).run()
}

fn preprocess(input: &str) -> Result<Cow<'_, str>, String> {
    let input = input.strip_prefix('\u{feff}').unwrap_or(input);
    let Some(start) = input.find(['\r', '\0']) else {
        return Ok(Cow::Borrowed(input));
    };
    let (prefix, suffix) = input.split_at(start);
    let mut normalized = String::with_capacity(input.len());
    normalized.push_str(prefix);
    let mut chars = suffix.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '\r' => {
                chars.next_if_eq(&'\n');
                normalized.push('\n');
            }
            '\0' => {
                return Err(format!(
                    "byte {}: NUL is outside the bootstrap grammar",
                    normalized.len()
                ));
            }
            _ => normalized.push(ch),
        }
    }
    Ok(Cow::Owned(normalized))
}

struct Parser<'a> {
    input: &'a str,
    position: usize,
    document: Document,
    stack: Vec<usize>,
    saw_doctype: bool,
    saw_element: bool,
    mode: TreeMode,
}

impl<'a> Parser<'a> {
    fn new(input: &'a str, mode: TreeMode) -> Self {
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
            mode,
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
        self.finish_tree()?;
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
        let valid = inner
            .split_once(is_html_space_char)
            .is_some_and(|(keyword, rest)| {
                keyword.eq_ignore_ascii_case("doctype")
                    && rest
                        .trim_matches(is_html_space_char)
                        .eq_ignore_ascii_case("html")
            });
        if !valid {
            return Err(self.error("only <!doctype html> is supported in the bootstrap grammar"));
        }
        if self.mode == TreeMode::DivFragment {
            self.position += offset + 1;
            return Ok(());
        }
        if self.saw_doctype || self.saw_element || self.stack.len() != 1 {
            return Err(self.error("doctype must occur once, before all elements"));
        }
        self.add_node(self.document.root, NodeKind::Doctype("html".to_owned()))?;
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
        self.place_start_tag(name, attributes)
    }

    fn push_element(
        &mut self,
        name: String,
        attributes: BTreeMap<String, String>,
    ) -> Result<(), String> {
        if self.stack.len() > MAX_DEPTH {
            return Err(self.error(&format!("element depth exceeds {MAX_DEPTH} limit")));
        }
        let parent = *self.stack.last().expect("root always exists");
        let text_element = matches!(name.as_str(), "style" | "script" | "title");
        let void = is_void(&name);
        let id = self.add_node(
            parent,
            NodeKind::Element {
                name: name.clone(),
                attributes,
            },
        )?;
        self.saw_element = true;
        if text_element {
            self.text_element(id, &name)?;
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
        decode_references(&self.input[start..end]).map_err(|error| error.at(start))
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
        self.place_end_tag(&name)
    }

    fn pop_element(&mut self, name: &str) -> Result<(), String> {
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

    fn text_element(&mut self, parent: usize, name: &str) -> Result<(), String> {
        let start = self.position;
        let needle = format!("</{name}");
        let bytes = self.input.as_bytes();
        let mut cursor = start;
        let end = loop {
            if cursor + needle.len() > bytes.len() {
                return Err(self.error(&format!("unclosed <{name}> text element")));
            }
            if bytes[cursor..cursor + needle.len()].eq_ignore_ascii_case(needle.as_bytes()) {
                let next = cursor + needle.len();
                if next < bytes.len()
                    && (matches!(bytes[next], b'>' | b'/') || is_html_space(bytes[next]))
                {
                    break cursor;
                }
            }
            cursor += 1;
        };
        let source = &self.input[start..end];
        if name == "script" && source.contains("<!--") {
            return Err(
                self.error("script escaped-comment states are outside the bootstrap grammar")
            );
        }
        self.position = end;
        let close = self.closing_name()?;
        if close != name {
            return Err(self.error("text element closing tag mismatch"));
        }
        if !source.is_empty() {
            let text = if name == "title" {
                decode_references(source).map_err(|error| error.at(start))?
            } else {
                source.to_owned()
            };
            self.add_node(parent, NodeKind::Text(text))?;
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
            .map_err(|error| error.at(start))?;
        self.place_text(text)
    }

    fn append_text(&mut self, text: Cow<'_, str>) -> Result<(), String> {
        let parent = *self.stack.last().expect("root always exists");
        if let Some(&last) = self.document.nodes[parent].children.last()
            && let NodeKind::Text(previous) = &mut self.document.nodes[last].kind
        {
            previous.push_str(&text);
            return Ok(());
        }
        self.add_node(parent, NodeKind::Text(text.into_owned()))?;
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
            | "xmp"
            | "iframe"
            | "noembed"
            | "noframes"
            | "noscript"
            | "plaintext"
    )
}

struct ReferenceError {
    offset: usize,
    message: String,
}

impl ReferenceError {
    fn at(self, source_start: usize) -> String {
        format!("byte {}: {}", source_start + self.offset, self.message)
    }
}

fn decode_references(input: &str) -> Result<String, ReferenceError> {
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
        let error = |message| ReferenceError {
            offset: start,
            message,
        };
        if rest.as_bytes().get(token_length) != Some(&b';') {
            return Err(error(format!(
                "character reference &{token} requires a semicolon in the bootstrap grammar"
            )));
        }
        let decoded = match token {
            "amp" => '&',
            "lt" => '<',
            "gt" => '>',
            "quot" => '"',
            "apos" => '\'',
            _ if token.starts_with('#') => numeric_reference(token).map_err(error)?,
            _ => {
                return Err(error(format!(
                    "unsupported named character reference &{token};"
                )));
            }
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

    fn element_children(document: &Document, id: usize) -> Vec<(usize, &str)> {
        document.nodes[id]
            .children
            .iter()
            .filter_map(|&child| match &document.nodes[child].kind {
                NodeKind::Element { name, .. } => Some((child, name.as_str())),
                _ => None,
            })
            .collect()
    }

    fn head_and_body(document: &Document) -> (usize, usize) {
        let [(html, "html")] = element_children(document, document.root)[..] else {
            panic!("expected one html element");
        };
        let [(head, "head"), (body, "body")] = element_children(document, html)[..] else {
            panic!("expected head and body beneath html");
        };
        assert_eq!(document.nodes[head].parent, Some(html));
        assert_eq!(document.nodes[body].parent, Some(html));
        (head, body)
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
    fn strips_only_one_leading_signature_and_preserves_dom() {
        let source = "<!doctype html><div style='width:2px;height:2px;background:red'></div>";
        let plain = parse(source).unwrap();
        let with_signature = parse(&format!("\u{feff}{source}")).unwrap();
        assert_eq!(plain.canonical_json(), with_signature.canonical_json());

        let literal_signatures = parse("\u{feff}\u{feff}<p>a\u{feff}b</p>").unwrap();
        assert_eq!(text_at(&literal_signatures, 1), "\u{feff}");
        assert_eq!(text_at(&literal_signatures, 3), "a\u{feff}b");

        let at_limit = format!("\u{feff}{}", " ".repeat(MAX_INPUT_BYTES - 3));
        assert!(parse(&at_limit).is_ok());
        assert_eq!(
            parse(&(at_limit + " ")).unwrap_err(),
            "input exceeds 1048576 byte limit"
        );
    }

    #[test]
    fn diagnostics_count_normalized_utf8_bytes_to_the_bad_reference_or_nul() {
        for (input, expected) in [
            (
                "\u{feff}<p>é\r\n &copy;</p>",
                "byte 7: unsupported named character reference &copy;",
            ),
            (
                "<p data-note='é\r\n &amp nope'></p>",
                "byte 18: character reference &amp requires a semicolon in the bootstrap grammar",
            ),
            (
                "<title>é\r\n &#x;</title>",
                "byte 11: invalid numeric character reference &#x;",
            ),
            (
                "<p>é &amp; &copy;</p>",
                "byte 12: unsupported named character reference &copy;",
            ),
            (
                "\u{feff}<p>é\r\n\0</p>",
                "byte 6: NUL is outside the bootstrap grammar",
            ),
        ] {
            assert_eq!(parse(input).unwrap_err(), expected);
        }
    }

    #[test]
    fn rejects_unimplemented_reference_grammar() {
        for input in ["&copy;", "&amp", "&#;", "&#x;", "&#12z;"] {
            assert!(parse(input).is_err(), "{input}");
        }
        assert_eq!(text_at(&parse("A & B").unwrap(), 1), "A & B");
    }

    #[test]
    fn creates_doctype_and_consumes_comments_without_nodes() {
        let d = parse("<!DoCtYpE\r\n\t HTML\x0c ><!--before--><p>a<!--between-->b</p>").unwrap();
        assert_eq!(d.nodes.len(), 4);
        assert_eq!(d.nodes[d.root].children, [1, 2]);
        assert_eq!(
            d.nodes[1],
            Node {
                kind: NodeKind::Doctype("html".to_owned()),
                parent: Some(d.root),
                children: vec![],
            }
        );
        assert_eq!(text_at(&d, 3), "ab");
        assert_eq!(
            parse("<!DOCTYPE HTML>").unwrap().canonical_json(),
            "{\"root\":0,\"nodes\":[{\"id\":0,\"parent\":null,\"children\":[1],\"type\":\"document\"},{\"id\":1,\"parent\":0,\"children\":[],\"type\":\"doctype\",\"name\":\"html\"}]}"
        );
        assert!(parse("<!doctype html><!doctype html>").is_err());
        assert!(parse("<p></p><!doctype html>").is_err());
        for invalid in ["<! doctype html>", "<!doctypehtml>", "<!doctype svg>"] {
            assert_eq!(
                parse(invalid).unwrap_err(),
                "byte 0: only <!doctype html> is supported in the bootstrap grammar"
            );
        }
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
        assert!(parse("<style>x</style/></style>").is_err());
    }

    #[test]
    fn title_decodes_references_without_parsing_markup() {
        let d = parse(
            "<head><title>é &amp; <b> &#x41; &lt;/title&gt; </titlex></TITLE ></head><body></body>",
        )
        .unwrap();
        assert_eq!(d.nodes[0].children, [1, 4]);
        assert_eq!(d.nodes[1].children, [2]);
        assert_eq!(d.nodes[2].children, [3]);
        assert_eq!(d.nodes[3].parent, Some(2));
        assert_eq!(text_at(&d, 3), "é & <b> A </title> </titlex>");
    }

    #[test]
    fn title_in_frozen_document_preserves_head_and_body_subtrees() {
        let d = parse_document(include_str!("../fixtures/m1/full/document.html")).unwrap();
        let [doctype, html, ..] = d.nodes[d.root].children[..] else {
            panic!("expected a doctype and document element");
        };
        assert_eq!(d.nodes[doctype].kind, NodeKind::Doctype("html".to_owned()));
        let [head, body] = d.nodes[html].children[..] else {
            panic!("expected separate head and body nodes");
        };
        let [title] = d.nodes[head].children[..] else {
            panic!("expected a title node in the head");
        };
        let [(paragraph, "p")] = element_children(&d, body)[..] else {
            panic!("expected a paragraph node in the body");
        };
        for (id, expected) in [
            (html, "html"),
            (head, "head"),
            (title, "title"),
            (body, "body"),
        ] {
            assert!(
                matches!(&d.nodes[id].kind, NodeKind::Element { name, .. } if name == expected)
            );
        }
        assert_eq!(text_at(&d, d.nodes[title].children[0]), "Fixture");
        let NodeKind::Element { name, attributes } = &d.nodes[paragraph].kind else {
            panic!("expected a paragraph element");
        };
        assert_eq!(name, "p");
        assert_eq!(attributes["id"], "greeting");
        assert_eq!(text_at(&d, d.nodes[paragraph].children[0]), "Hello");
    }

    #[test]
    fn div_fragment_uses_the_fragment_children_as_roots() {
        let source = include_str!("../fixtures/m1/full/fragment.html");
        let d = parse_fragment(source, "div").unwrap();
        let [(paragraph, "p"), (last, "div")] = element_children(&d, d.root)[..] else {
            panic!("expected the paragraph and div directly beneath the fragment root");
        };
        assert_eq!(text_at(&d, d.nodes[paragraph].children[0]), "A & B");
        let NodeKind::Element { attributes, .. } = &d.nodes[last].kind else {
            panic!("element");
        };
        assert_eq!(
            attributes,
            &BTreeMap::from([("data-x".to_owned(), "1".to_owned())])
        );
        assert_eq!(parse_fragment(source, "DIV").unwrap(), d);
        assert_eq!(
            parse_fragment(source, "table").unwrap_err(),
            "only a div HTML fragment context is supported"
        );
        assert!(
            parse_fragment("<html></html>", "div")
                .unwrap_err()
                .contains("controlled HTML tree grammar")
        );
    }

    #[test]
    fn document_implies_paragraph_end_tags_and_document_structure() {
        let source = include_str!("../fixtures/m1/full/malformed-nesting.html");
        assert!(parse(source).is_err());
        let d = parse_document(source).unwrap();
        let (head, body) = head_and_body(&d);
        assert!(d.nodes[head].children.is_empty());
        let [(first, "p"), (second, "p")] = element_children(&d, body)[..] else {
            panic!("expected adjacent paragraphs beneath body");
        };
        assert_eq!(text_at(&d, d.nodes[first].children[0]), "one");
        assert_eq!(text_at(&d, d.nodes[second].children[0]), "two");

        let d =
            parse_fragment("<p><span>initial</span><section><p>nested</section>", "div").unwrap();
        let [(first, "p"), (section, "section")] = element_children(&d, d.root)[..] else {
            panic!("expected the paragraph to close before the section");
        };
        assert_eq!(element_children(&d, first)[0].1, "span");
        let [(nested, "p")] = element_children(&d, section)[..] else {
            panic!("expected a paragraph in the section");
        };
        assert_eq!(text_at(&d, d.nodes[nested].children[0]), "nested");
        assert!(
            parse_document("<div><span></div></span>")
                .unwrap_err()
                .contains("misnested")
        );
    }

    #[test]
    fn document_keeps_references_duplicate_attributes_and_raw_text_in_their_parent() {
        let d = parse_document(include_str!(
            "../fixtures/m1/full/character-references.html"
        ))
        .unwrap();
        let (_, body) = head_and_body(&d);
        let [(p, "p")] = element_children(&d, body)[..] else {
            panic!("expected a paragraph");
        };
        assert_eq!(text_at(&d, d.nodes[p].children[0]), "&<>\"'AA");

        let d = parse_document(include_str!("../fixtures/m1/full/attributes.html")).unwrap();
        let (_, body) = head_and_body(&d);
        let [(div, "div")] = element_children(&d, body)[..] else {
            panic!("expected a div");
        };
        let NodeKind::Element { attributes, .. } = &d.nodes[div].kind else {
            panic!("element");
        };
        assert_eq!(
            attributes,
            &BTreeMap::from([
                ("data-x".to_owned(), "1".to_owned()),
                ("id".to_owned(), "first".to_owned())
            ])
        );

        let d = parse_document(include_str!("../fixtures/m1/full/rawtext.html")).unwrap();
        let (head, body) = head_and_body(&d);
        let [(style, "style")] = element_children(&d, head)[..] else {
            panic!("expected a stylesheet in head");
        };
        assert!(element_children(&d, style).is_empty());
        assert_eq!(
            text_at(&d, d.nodes[style].children[0]),
            "div::before { content: \"<b>&amp;\"; }"
        );
        assert_eq!(element_children(&d, body)[0].1, "div");
    }

    #[test]
    fn document_modes_preserve_whitespace_and_metadata_placement() {
        let d = parse_document(" \r\n<!doctype html><html lang=en> \n<head> \n<title>x</title></head>\n<style>x</style><body>Text</body></html>\r\n").unwrap();
        let (head, body) = head_and_body(&d);
        let html = d.nodes[head].parent.unwrap();
        let [(title, "title"), (style, "style")] = element_children(&d, head)[..] else {
            panic!("expected title and style within head");
        };
        assert_eq!(d.nodes[d.root].children, [1, html]);
        assert_eq!(text_at(&d, d.nodes[head].children[0]), " \n");
        assert_eq!(text_at(&d, d.nodes[title].children[0]), "x");
        assert_eq!(text_at(&d, d.nodes[style].children[0]), "x");
        assert_eq!(text_at(&d, d.nodes[html].children[1]), "\n");
        assert_eq!(text_at(&d, d.nodes[body].children[0]), "Text\n");

        let empty = parse_document("\r\n").unwrap();
        let (head, body) = head_and_body(&empty);
        assert!(empty.nodes[head].children.is_empty());
        assert!(empty.nodes[body].children.is_empty());
    }

    #[test]
    fn document_structure_counts_toward_depth_and_node_limits() {
        for nested in [MAX_DEPTH - 2, MAX_DEPTH - 1] {
            let source = format!("{}{}", "<div>".repeat(nested), "</div>".repeat(nested));
            if nested + 2 == MAX_DEPTH {
                assert!(parse_document(&source).is_ok());
            } else {
                assert!(
                    parse_document(&source)
                        .unwrap_err()
                        .contains("element depth exceeds 128 limit")
                );
            }
        }
        for count in [MAX_NODES - 1, MAX_NODES, MAX_NODES + 1] {
            let source = format!("<!doctype html>{}", "<div></div>".repeat(count - 5));
            if count <= MAX_NODES {
                assert_eq!(parse_document(&source).unwrap().nodes.len(), count);
            } else {
                assert!(
                    parse_document(&source)
                        .unwrap_err()
                        .contains("node count exceeds 10000 limit")
                );
            }
        }
    }

    #[test]
    fn title_keeps_strict_closing_tag_and_reference_diagnostics() {
        for (input, expected) in [
            ("<title>unfinished", "unclosed <title>"),
            (
                "<title>x</title bogus></title>",
                "closing tags must end with >",
            ),
            ("<title>x</title/></title>", "closing tags must end with >"),
            (
                "<title>&copy;</title>",
                "unsupported named character reference",
            ),
            ("<title>&amp</title>", "requires a semicolon"),
        ] {
            assert!(parse(input).unwrap_err().contains(expected), "{input}");
        }
        let d = parse("<title></title>").unwrap();
        assert!(d.nodes[1].children.is_empty());
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
    fn node_limit_includes_document_and_doctype() {
        let nodes = "<br>".repeat(MAX_NODES - 1);
        assert_eq!(parse(&nodes).unwrap().nodes.len(), MAX_NODES);
        assert!(parse(&(nodes + "<br>")).unwrap_err().contains("node count"));

        for count in [MAX_NODES - 1, MAX_NODES, MAX_NODES + 1] {
            let input = format!(
                "<!doctype html><html><head></head><body>{}</body></html>",
                "<div></div>".repeat(count - 5)
            );
            if count <= MAX_NODES {
                assert_eq!(parse(&input).unwrap().nodes.len(), count);
            } else {
                assert!(
                    parse(&input)
                        .unwrap_err()
                        .contains("node count exceeds 10000 limit")
                );
            }
        }
    }

    #[test]
    fn start_tag_attribute_token_limit_counts_duplicates() {
        let attrs = (0..MAX_ATTRIBUTES)
            .map(|n| format!(" a{n}=x"))
            .collect::<String>();
        assert!(parse(&format!("<p{attrs}></p>")).is_ok());
        assert!(
            parse(&format!("<p{attrs} extra=x></p>"))
                .unwrap_err()
                .contains("attribute count")
        );

        let repeated = format!(" id=first{}", " ID=remaining".repeat(MAX_ATTRIBUTES - 1));
        let d = parse(&format!("<p{repeated}></p>")).unwrap();
        let NodeKind::Element { attributes, .. } = &d.nodes[1].kind else {
            panic!("element");
        };
        assert_eq!(attributes.len(), 1);
        assert_eq!(attributes["id"], "first");
        assert!(
            parse(&format!("<p{repeated} id=excess></p>"))
                .unwrap_err()
                .contains("attribute count exceeds 128 limit")
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
