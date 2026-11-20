//! Deterministic block-box renderer for controlled bootstrap fixtures.
//!
//! This is an explicitly bounded fixture grammar, not a claim of general CSS or
//! HTML rendering support. It paints empty block boxes, solid borders, backgrounds
//! and overflow clips. Visible text and replaced/inline content are rejected until
//! text shaping and the corresponding formatting algorithms are implemented.
//! Margins do not collapse in this bootstrap grammar.

use crate::html::{Document, NodeKind, json_string};
use std::collections::BTreeMap;

const MAX_VIEWPORT: u32 = 2048;
const MAX_NODES: usize = 10_000;
const MAX_DEPTH: usize = 128;
const MAX_RULES: usize = 512;
const MAX_DECLARATIONS: usize = 4096;
const MAX_CSS_BYTES: usize = 262_144;
const MAX_LENGTH: i64 = 16_384;

pub struct RenderOutput {
    pub ppm: Vec<u8>,
    pub layout_json: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Color(u8, u8, u8);

impl Color {
    fn hex(self) -> String {
        format!("#{:02x}{:02x}{:02x}", self.0, self.1, self.2)
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct Edges {
    top: i64,
    right: i64,
    bottom: i64,
    left: i64,
}

#[derive(Clone, Debug)]
struct Style {
    hidden: bool,
    explicit_block: bool,
    width: Option<i64>,
    height: Option<i64>,
    margin: Edges,
    padding: Edges,
    border: Edges,
    background: Option<Color>,
    border_color: Option<Color>,
    solid_border: bool,
    clip_children: bool,
}

impl Style {
    fn for_tag(tag: &str) -> Self {
        Self {
            hidden: matches!(tag, "head" | "style" | "script" | "title" | "meta" | "link"),
            explicit_block: false,
            width: None,
            height: None,
            margin: if tag == "body" {
                Edges {
                    top: 8,
                    right: 8,
                    bottom: 8,
                    left: 8,
                }
            } else {
                Edges::default()
            },
            padding: Edges::default(),
            border: Edges::default(),
            background: None,
            border_color: Some(Color(0, 0, 0)),
            solid_border: false,
            clip_children: false,
        }
    }
}

#[derive(Clone, Copy)]
enum Relation {
    Descendant,
    Child,
}

#[derive(Clone)]
struct Compound {
    tag: Option<String>,
    ids: Vec<String>,
    classes: Vec<String>,
}

#[derive(Clone)]
struct Selector {
    compounds: Vec<Compound>,
    relations: Vec<Relation>,
    specificity: (u32, u32, u32),
}

#[derive(Clone)]
struct Declaration {
    name: String,
    value: String,
    important: bool,
    order: usize,
}

struct Rule {
    selectors: Vec<Selector>,
    declarations: Vec<Declaration>,
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct Rank {
    important: bool,
    inline: bool,
    specificity: (u32, u32, u32),
    order: usize,
}

#[derive(Clone, Copy, Debug)]
struct Rect {
    x: i64,
    y: i64,
    width: i64,
    height: i64,
}

impl Rect {
    fn intersect(self, other: Self) -> Self {
        let x = self.x.max(other.x);
        let y = self.y.max(other.y);
        let right = (self.x + self.width).min(other.x + other.width);
        let bottom = (self.y + self.height).min(other.y + other.height);
        Self {
            x,
            y,
            width: (right - x).max(0),
            height: (bottom - y).max(0),
        }
    }
}

struct LayoutBox {
    node: usize,
    tag: String,
    border_box: Rect,
    content_box: Rect,
    clip: Rect,
    style: Style,
}

/// Render the declared block-box fixture subset into binary P6 PPM and geometry.
/// No networking, script execution, font lookup, or image decoding occurs here.
pub fn render(doc: &Document, width: u32, height: u32) -> Result<RenderOutput, String> {
    if width == 0 || height == 0 || width > MAX_VIEWPORT || height > MAX_VIEWPORT {
        return Err(format!(
            "viewport dimensions must be between 1 and {MAX_VIEWPORT}"
        ));
    }
    validate_document(doc)?;
    let rules = collect_rules(doc)?;
    let mut boxes = Vec::new();
    let viewport = Rect {
        x: 0,
        y: 0,
        width: i64::from(width),
        height: i64::from(height),
    };
    let mut y = 0;
    for child in &doc.nodes[doc.root].children {
        y += layout_node(
            doc,
            *child,
            (0, y),
            i64::from(width),
            viewport,
            &rules,
            &mut boxes,
        )?;
    }
    let byte_count = (width as usize)
        .checked_mul(height as usize)
        .and_then(|count| count.checked_mul(3))
        .ok_or_else(|| "surface size overflow".to_owned())?;
    let mut pixels = vec![255u8; byte_count];
    for item in &boxes {
        paint_box(&mut pixels, width, item);
    }
    let mut ppm = format!("P6\n{width} {height}\n255\n").into_bytes();
    ppm.extend_from_slice(&pixels);
    let layout_json = layout_json(width, height, &boxes);
    Ok(RenderOutput { ppm, layout_json })
}

fn validate_document(doc: &Document) -> Result<(), String> {
    if doc.nodes.is_empty() || doc.nodes.len() > MAX_NODES || doc.root >= doc.nodes.len() {
        return Err("invalid or oversized document node store".to_owned());
    }
    if !matches!(doc.nodes[doc.root].kind, NodeKind::Document)
        || doc.nodes[doc.root].parent.is_some()
    {
        return Err("document root must be an unparented document node".to_owned());
    }
    let mut seen = vec![false; doc.nodes.len()];
    let mut stack = vec![(doc.root, 0usize)];
    while let Some((node, depth)) = stack.pop() {
        if node >= doc.nodes.len() || depth > MAX_DEPTH {
            return Err("document contains an invalid child or exceeds depth limit".to_owned());
        }
        if seen[node] {
            return Err("document contains a cycle or shared child".to_owned());
        }
        seen[node] = true;
        if matches!(doc.nodes[node].kind, NodeKind::Text(_)) && !doc.nodes[node].children.is_empty()
        {
            return Err("text nodes cannot have children".to_owned());
        }
        if node != doc.root && matches!(doc.nodes[node].kind, NodeKind::Document) {
            return Err("nested document nodes cannot be rendered".to_owned());
        }
        for child in &doc.nodes[node].children {
            if *child >= doc.nodes.len() || doc.nodes[*child].parent != Some(node) {
                return Err("document parent/child relationship is inconsistent".to_owned());
            }
            let child_depth =
                depth + usize::from(matches!(doc.nodes[*child].kind, NodeKind::Element { .. }));
            stack.push((*child, child_depth));
        }
    }
    if seen.iter().any(|visited| !visited) {
        return Err("document contains unreachable nodes".to_owned());
    }
    Ok(())
}

fn collect_rules(doc: &Document) -> Result<Vec<Rule>, String> {
    let mut css = String::new();
    let mut source_bytes = 0usize;
    for node in &doc.nodes {
        if let NodeKind::Element { name, attributes } = &node.kind {
            if let Some(inline) = attributes.get("style") {
                add_css_source_bytes(&mut source_bytes, inline.len())?;
            }
            if name == "link"
                && attributes.contains_key("href")
                && attributes.get("rel").is_some_and(|value| {
                    value
                        .split_ascii_whitespace()
                        .any(|relation| relation.eq_ignore_ascii_case("stylesheet"))
                })
            {
                return Err(
                    "unsupported external stylesheet: bootstrap styles must be embedded".to_owned(),
                );
            }
            if name == "style" {
                for child in &node.children {
                    match &doc.nodes[*child].kind {
                        NodeKind::Text(text) => {
                            add_css_source_bytes(&mut source_bytes, text.len())?;
                            css.push_str(text);
                        }
                        _ => return Err("style content must be raw CSS text".to_owned()),
                    }
                }
                css.push('\n');
            }
        }
    }
    let css = strip_comments(&css)?;
    let mut rest = css.as_str();
    let mut rules = Vec::new();
    let mut order = 0;
    while !rest.trim().is_empty() {
        rest = rest.trim_start();
        if rest.starts_with('@') {
            return Err(
                "unsupported CSS at-rule: imports and external stylesheet loading are unavailable"
                    .to_owned(),
            );
        }
        let open = rest
            .find('{')
            .ok_or_else(|| "stylesheet rule has no opening brace".to_owned())?;
        let selector_text = rest[..open].trim();
        let after_open = &rest[open + 1..];
        let close = after_open
            .find('}')
            .ok_or_else(|| "stylesheet rule has no closing brace".to_owned())?;
        if after_open[..close].contains('{') {
            return Err(
                "nested rules and at-rules are outside the bootstrap CSS grammar".to_owned(),
            );
        }
        let selectors: Result<Vec<_>, _> = selector_text.split(',').map(parse_selector).collect();
        let declarations = parse_declarations(&after_open[..close], &mut order)?;
        rules.push(Rule {
            selectors: selectors?,
            declarations,
        });
        if rules.len() > MAX_RULES {
            return Err(format!("stylesheet exceeds {MAX_RULES} rules"));
        }
        rest = &after_open[close + 1..];
    }
    Ok(rules)
}

fn add_css_source_bytes(total: &mut usize, bytes: usize) -> Result<(), String> {
    *total = total
        .checked_add(bytes)
        .ok_or_else(|| "total CSS source size overflow".to_owned())?;
    if *total > MAX_CSS_BYTES {
        return Err(
            "total embedded and inline CSS source exceeds 256 KiB bootstrap limit".to_owned(),
        );
    }
    Ok(())
}

fn strip_comments(css: &str) -> Result<String, String> {
    let mut out = String::with_capacity(css.len());
    let mut rest = css;
    while let Some(start) = rest.find("/*") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        let end = after
            .find("*/")
            .ok_or_else(|| "unterminated CSS comment".to_owned())?;
        out.push(' ');
        rest = &after[end + 2..];
    }
    out.push_str(rest);
    Ok(out)
}

fn parse_selector(text: &str) -> Result<Selector, String> {
    let mut tokens = Vec::new();
    let spaced = text.replace('>', " > ");
    tokens.extend(spaced.split_whitespace());
    if tokens.is_empty() {
        return Err("empty CSS selector".to_owned());
    }
    let mut compounds = Vec::new();
    let mut relations = Vec::new();
    let mut pending_child = false;
    let mut specificity = (0, 0, 0);
    for token in tokens {
        if token == ">" {
            if compounds.is_empty() || pending_child {
                return Err("invalid child combinator".to_owned());
            }
            pending_child = true;
            continue;
        }
        if !compounds.is_empty() {
            relations.push(if pending_child {
                Relation::Child
            } else {
                Relation::Descendant
            });
        }
        pending_child = false;
        let compound = parse_compound(token)?;
        specificity.0 += compound.ids.len() as u32;
        specificity.1 += compound.classes.len() as u32;
        specificity.2 += u32::from(compound.tag.is_some());
        compounds.push(compound);
        if compounds.len() > 16 {
            return Err("selector exceeds 16 compound selectors".to_owned());
        }
    }
    if pending_child {
        return Err("selector ends with a child combinator".to_owned());
    }
    Ok(Selector {
        compounds,
        relations,
        specificity,
    })
}

fn parse_compound(text: &str) -> Result<Compound, String> {
    if !text.is_ascii() {
        return Err("non-ASCII/escaped selectors are outside the bootstrap CSS grammar".to_owned());
    }
    let bytes = text.as_bytes();
    let mut index = 0;
    let mut result = Compound {
        tag: None,
        ids: Vec::new(),
        classes: Vec::new(),
    };
    if bytes.first() == Some(&b'*') {
        index = 1;
    } else if bytes.first().is_some_and(|b| b.is_ascii_alphabetic()) {
        let start = index;
        while index < bytes.len() && identifier_byte(bytes[index]) {
            index += 1;
        }
        result.tag = Some(text[start..index].to_ascii_lowercase());
    }
    while index < bytes.len() {
        let marker = bytes[index];
        if marker != b'.' && marker != b'#' {
            return Err(format!("unsupported bootstrap selector: {text}"));
        }
        index += 1;
        let start = index;
        while index < bytes.len() && identifier_byte(bytes[index]) {
            index += 1;
        }
        if start == index {
            return Err("selector class or ID is empty".to_owned());
        }
        let identifier_bytes = &bytes[start..index];
        let valid_start = identifier_bytes[0].is_ascii_alphabetic()
            || identifier_bytes[0] == b'_'
            || (identifier_bytes[0] == b'-'
                && identifier_bytes.get(1).is_some_and(|byte| {
                    byte.is_ascii_alphabetic() || *byte == b'_' || *byte == b'-'
                }));
        if !valid_start {
            return Err("selector identifiers require an ASCII identifier start".to_owned());
        }
        let identifier = text[start..index].to_owned();
        if marker == b'.' {
            result.classes.push(identifier);
        } else {
            result.ids.push(identifier);
        }
    }
    if result.tag.is_none() && result.ids.is_empty() && result.classes.is_empty() && text != "*" {
        return Err(format!("unsupported bootstrap selector: {text}"));
    }
    Ok(result)
}

fn identifier_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_'
}

fn parse_declarations(text: &str, order: &mut usize) -> Result<Vec<Declaration>, String> {
    let text = strip_comments(text)?;
    let mut out = Vec::new();
    for declaration in text.split(';') {
        let declaration = declaration.trim();
        if declaration.is_empty() {
            continue;
        }
        let Some((name, value)) = declaration.split_once(':') else {
            // A malformed declaration does not invalidate its neighboring declarations.
            continue;
        };
        let name = name.trim().to_ascii_lowercase();
        let mut value = value.trim().to_ascii_lowercase();
        let important = value
            .rsplit_once('!')
            .is_some_and(|(_, suffix)| suffix.trim() == "important");
        if important {
            value.truncate(
                value
                    .rfind('!')
                    .expect("important suffix has an exclamation mark"),
            );
            value = value.trim_end().to_owned();
        }
        for (expanded_name, expanded_value) in expand_declaration(&name, &value)? {
            *order += 1;
            if *order > MAX_DECLARATIONS {
                return Err(format!(
                    "stylesheet exceeds {MAX_DECLARATIONS} effective declarations"
                ));
            }
            out.push(Declaration {
                name: expanded_name,
                value: expanded_value,
                important,
                order: *order,
            });
        }
    }
    Ok(out)
}

fn expand_declaration(name: &str, value: &str) -> Result<Vec<(String, String)>, String> {
    let mut out = Vec::new();
    match name {
        "background-image"
        | "border-image"
        | "border-image-source"
        | "list-style-image"
        | "mask-image" => {
            if value != "none" {
                return Err(format!(
                    "unsupported resource or image value for {name}: bootstrap paints solid colors only"
                ));
            }
        }
        "margin" | "padding" | "border-width" => {
            let values: Vec<_> = value.split_whitespace().collect();
            if values.is_empty() || values.len() > 4 {
                return Ok(out);
            }
            let expanded = match values.len() {
                1 => [values[0], values[0], values[0], values[0]],
                2 => [values[0], values[1], values[0], values[1]],
                3 => [values[0], values[1], values[2], values[1]],
                _ => [values[0], values[1], values[2], values[3]],
            };
            for (side, v) in ["top", "right", "bottom", "left"].iter().zip(expanded) {
                let property = if name == "border-width" {
                    format!("border-{side}-width")
                } else {
                    format!("{name}-{side}")
                };
                // Validate the entire shorthand before allowing it to enter cascade.
                if parse_length(v, name == "margin")?.is_none() {
                    return Ok(Vec::new());
                }
                out.push((property, v.to_owned()));
            }
        }
        "border" => {
            if value == "none" || value == "0" {
                let mut result = expand_declaration("border-width", "0")?;
                result.push(("border-style".to_owned(), "none".to_owned()));
                return Ok(result);
            }
            let values: Vec<_> = value.split_whitespace().collect();
            if values.len() != 3 || values[1] != "solid" {
                return Err("bootstrap border requires '<px> solid <color>' or 'none'".to_owned());
            }
            if parse_length(values[0], false)?.is_none()
                || (parse_color(values[2]).is_none() && values[2] != "transparent")
            {
                return Ok(Vec::new());
            }
            out.extend(expand_declaration("border-width", values[0])?);
            out.push(("border-style".to_owned(), "solid".to_owned()));
            out.push(("border-color".to_owned(), values[2].to_owned()));
        }
        "background" => {
            if parse_color(value).is_some() || value == "transparent" {
                out.push(("background-color".to_owned(), value.to_owned()));
            } else {
                return Err(
                    "only solid-color backgrounds are supported by the bootstrap renderer"
                        .to_owned(),
                );
            }
        }
        "display" => {
            if !matches!(value, "block" | "none") {
                return Err(format!("unsupported bootstrap display value: {value}"));
            }
            out.push((name.to_owned(), value.to_owned()));
        }
        "overflow" => {
            if !matches!(value, "visible" | "hidden") {
                return Err(format!("unsupported bootstrap overflow value: {value}"));
            }
            out.push((name.to_owned(), value.to_owned()));
        }
        "box-sizing" => {
            if value != "content-box" {
                return Err("bootstrap renderer supports content-box sizing only".to_owned());
            }
        }
        "border-style" => {
            if !matches!(value, "solid" | "none") {
                return Err("bootstrap border style supports solid and none only".to_owned());
            }
            out.push((name.to_owned(), value.to_owned()));
        }
        "width" | "height" => {
            if value == "auto" || parse_length(value, false)?.is_some() {
                out.push((name.to_owned(), value.to_owned()));
            }
        }
        "background-color" | "border-color" => {
            if value == "transparent" || parse_color(value).is_some() {
                out.push((name.to_owned(), value.to_owned()));
            }
        }
        "margin-top"
        | "margin-right"
        | "margin-bottom"
        | "margin-left"
        | "padding-top"
        | "padding-right"
        | "padding-bottom"
        | "padding-left"
        | "border-top-width"
        | "border-right-width"
        | "border-bottom-width"
        | "border-left-width"
            if parse_length(value, name.starts_with("margin-"))?.is_some() =>
        {
            out.push((name.to_owned(), value.to_owned()));
        }
        // Unsupported property names are ignored rather than inventing behavior.
        _ => {}
    }
    Ok(out)
}

fn parse_length(value: &str, negative: bool) -> Result<Option<i64>, String> {
    let number = if value == "0" {
        "0"
    } else if let Some(number) = value.strip_suffix("px") {
        number
    } else {
        if value.chars().any(|c| c.is_ascii_digit()) {
            return Err(format!(
                "bootstrap lengths require integer px values: {value}"
            ));
        }
        return Ok(None);
    };
    let Ok(number) = number.parse::<i64>() else {
        return Err(format!("bootstrap length is not an integer: {value}"));
    };
    if number.abs_diff(0) > MAX_LENGTH as u64 {
        return Err(format!("CSS length exceeds {MAX_LENGTH}px limit"));
    }
    if !negative && number < 0 {
        return Ok(None);
    }
    Ok(Some(number))
}

fn parse_color(value: &str) -> Option<Color> {
    match value {
        "white" => Some(Color(255, 255, 255)),
        "black" => Some(Color(0, 0, 0)),
        "red" => Some(Color(255, 0, 0)),
        "green" => Some(Color(0, 128, 0)),
        "blue" => Some(Color(0, 0, 255)),
        "yellow" => Some(Color(255, 255, 0)),
        "gray" | "grey" => Some(Color(128, 128, 128)),
        _ => {
            let digits = value.strip_prefix('#')?;
            if digits.len() == 3 && digits.is_ascii() {
                let r = u8::from_str_radix(&digits[0..1], 16).ok()?;
                let g = u8::from_str_radix(&digits[1..2], 16).ok()?;
                let b = u8::from_str_radix(&digits[2..3], 16).ok()?;
                Some(Color(r * 17, g * 17, b * 17))
            } else if digits.len() == 6 && digits.is_ascii() {
                Some(Color(
                    u8::from_str_radix(&digits[0..2], 16).ok()?,
                    u8::from_str_radix(&digits[2..4], 16).ok()?,
                    u8::from_str_radix(&digits[4..6], 16).ok()?,
                ))
            } else {
                None
            }
        }
    }
}

fn matches_compound(doc: &Document, node: usize, compound: &Compound) -> bool {
    let NodeKind::Element { name, attributes } = &doc.nodes[node].kind else {
        return false;
    };
    if compound.tag.as_ref().is_some_and(|tag| tag != name) {
        return false;
    }
    if compound
        .ids
        .iter()
        .any(|id| attributes.get("id") != Some(id))
    {
        return false;
    }
    let classes = attributes.get("class").map(String::as_str).unwrap_or("");
    compound
        .classes
        .iter()
        .all(|class| classes.split_ascii_whitespace().any(|value| value == class))
}

fn matches_selector(doc: &Document, node: usize, selector: &Selector) -> bool {
    let last = selector.compounds.len() - 1;
    if !matches_compound(doc, node, &selector.compounds[last]) {
        return false;
    }
    let mut chain = Vec::new();
    let mut current = Some(node);
    while let Some(ancestor) = current {
        chain.push(ancestor);
        current = doc.nodes[ancestor].parent;
    }
    // Dynamic matching bounds work by depth × selector length. Recursive ancestor
    // backtracking can take exponential time on adversarial repeated selectors.
    let mut matches = vec![false; chain.len()];
    matches[0] = true;
    for index in (0..last).rev() {
        let mut next = vec![false; chain.len()];
        let mut descendant_candidate = false;
        for ancestor in 1..chain.len() {
            descendant_candidate |= matches[ancestor - 1];
            let allowed = match selector.relations[index] {
                Relation::Child => matches[ancestor - 1],
                Relation::Descendant => descendant_candidate,
            };
            next[ancestor] =
                allowed && matches_compound(doc, chain[ancestor], &selector.compounds[index]);
        }
        matches = next;
    }
    matches.iter().any(|matched| *matched)
}

fn computed_style(doc: &Document, node: usize, rules: &[Rule]) -> Result<Style, String> {
    let NodeKind::Element { name, attributes } = &doc.nodes[node].kind else {
        return Err("style requested for a non-element node".to_owned());
    };
    let mut style = Style::for_tag(name);
    let mut winners: BTreeMap<&str, (Rank, &str)> = BTreeMap::new();
    for rule in rules {
        let specificity = rule
            .selectors
            .iter()
            .filter(|selector| matches_selector(doc, node, selector))
            .map(|selector| selector.specificity)
            .max();
        if let Some(specificity) = specificity {
            for declaration in &rule.declarations {
                let rank = Rank {
                    important: declaration.important,
                    inline: false,
                    specificity,
                    order: declaration.order,
                };
                if winners
                    .get(declaration.name.as_str())
                    .is_none_or(|(old_rank, _)| rank >= *old_rank)
                {
                    winners.insert(&declaration.name, (rank, &declaration.value));
                }
            }
        }
    }
    let mut inline_order = 0;
    let inline = attributes.get("style").map(String::as_str).unwrap_or("");
    if inline.len() > MAX_CSS_BYTES {
        return Err("inline style exceeds 256 KiB limit".to_owned());
    }
    let inline_declarations = parse_declarations(inline, &mut inline_order)?;
    for declaration in &inline_declarations {
        let rank = Rank {
            important: declaration.important,
            inline: true,
            specificity: (0, 0, 0),
            order: declaration.order,
        };
        if winners
            .get(declaration.name.as_str())
            .is_none_or(|(old_rank, _)| rank >= *old_rank)
        {
            winners.insert(&declaration.name, (rank, &declaration.value));
        }
    }
    for (property, (_, value)) in winners {
        match property {
            "display" => {
                style.hidden = value == "none";
                style.explicit_block = value == "block";
            }
            "width" => {
                style.width = if value == "auto" {
                    None
                } else {
                    parse_length(value, false)?
                }
            }
            "height" => {
                style.height = if value == "auto" {
                    None
                } else {
                    parse_length(value, false)?
                }
            }
            "background-color" => style.background = parse_color(value),
            "border-color" => style.border_color = parse_color(value),
            "border-style" => style.solid_border = value == "solid",
            "overflow" => style.clip_children = value == "hidden",
            _ => {
                let length = parse_length(value, property.starts_with("margin-"))?.unwrap_or(0);
                match property {
                    "margin-top" => style.margin.top = length,
                    "margin-right" => style.margin.right = length,
                    "margin-bottom" => style.margin.bottom = length,
                    "margin-left" => style.margin.left = length,
                    "padding-top" => style.padding.top = length,
                    "padding-right" => style.padding.right = length,
                    "padding-bottom" => style.padding.bottom = length,
                    "padding-left" => style.padding.left = length,
                    "border-top-width" => style.border.top = length,
                    "border-right-width" => style.border.right = length,
                    "border-bottom-width" => style.border.bottom = length,
                    "border-left-width" => style.border.left = length,
                    _ => return Err(format!("unrecognized expanded property: {property}")),
                }
            }
        }
    }
    if !style.solid_border {
        style.border = Edges::default();
    }
    Ok(style)
}

fn layout_node(
    doc: &Document,
    node: usize,
    origin: (i64, i64),
    available_width: i64,
    clip: Rect,
    rules: &[Rule],
    boxes: &mut Vec<LayoutBox>,
) -> Result<i64, String> {
    let (x, y) = origin;
    let (name, _) = match &doc.nodes[node].kind {
        NodeKind::Text(text) => {
            if text.chars().any(|c| !c.is_ascii_whitespace()) {
                return Err("unsupported visible text: shaping and inline layout are outside the bootstrap renderer".to_owned());
            }
            return Ok(0);
        }
        NodeKind::Document => return Err("nested document nodes cannot be rendered".to_owned()),
        NodeKind::Element { name, attributes } => (name, attributes),
    };
    let style = computed_style(doc, node, rules)?;
    if style.hidden {
        return Ok(0);
    }
    if matches!(
        name.as_str(),
        "img" | "svg" | "canvas" | "video" | "audio" | "iframe" | "input" | "object" | "embed"
    ) {
        return Err(format!(
            "visible replaced element <{name}> is outside the bootstrap renderer"
        ));
    }
    if !style.explicit_block
        && !matches!(
            name.as_str(),
            "html"
                | "body"
                | "div"
                | "section"
                | "article"
                | "main"
                | "header"
                | "footer"
                | "nav"
                | "aside"
        )
    {
        return Err(format!(
            "<{name}> requires explicit display:block in the bootstrap fixture grammar"
        ));
    }
    let horizontal_edges =
        style.padding.left + style.padding.right + style.border.left + style.border.right;
    let content_width = style.width.unwrap_or(
        (available_width - style.margin.left - style.margin.right - horizontal_edges).max(0),
    );
    let border_x = x + style.margin.left;
    let border_y = y + style.margin.top;
    let content_x = border_x + style.border.left + style.padding.left;
    let content_y = border_y + style.border.top + style.padding.top;
    let box_index = boxes.len();
    boxes.push(LayoutBox {
        node,
        tag: name.clone(),
        border_box: Rect {
            x: border_x,
            y: border_y,
            width: content_width + horizontal_edges,
            height: 0,
        },
        content_box: Rect {
            x: content_x,
            y: content_y,
            width: content_width,
            height: 0,
        },
        clip,
        style: style.clone(),
    });
    // Fixed-height overflow clips are known before child layout. Auto-height boxes
    // contain their normal-flow children vertically, so no extra vertical clip is needed.
    let child_clip = if style.clip_children {
        let clip_height = style
            .height
            .map(|height| height + style.padding.top + style.padding.bottom)
            .unwrap_or(i64::MAX / 4);
        clip.intersect(Rect {
            x: border_x + style.border.left,
            y: border_y + style.border.top,
            width: content_width + style.padding.left + style.padding.right,
            height: clip_height,
        })
    } else {
        clip
    };
    let mut cursor = content_y;
    for child in &doc.nodes[node].children {
        cursor += layout_node(
            doc,
            *child,
            (content_x, cursor),
            content_width,
            child_clip,
            rules,
            boxes,
        )?;
    }
    let content_height = style.height.unwrap_or((cursor - content_y).max(0));
    let border_height = content_height
        + style.padding.top
        + style.padding.bottom
        + style.border.top
        + style.border.bottom;
    boxes[box_index].border_box.height = border_height;
    boxes[box_index].content_box.height = content_height;
    if style.clip_children {
        let final_clip = clip.intersect(Rect {
            x: border_x + style.border.left,
            y: border_y + style.border.top,
            width: content_width + style.padding.left + style.padding.right,
            height: content_height + style.padding.top + style.padding.bottom,
        });
        for child in &mut boxes[box_index + 1..] {
            child.clip = child.clip.intersect(final_clip);
        }
    }
    Ok(style.margin.top + border_height + style.margin.bottom)
}

fn fill(pixels: &mut [u8], width: u32, rect: Rect, clip: Rect, color: Color) {
    let bounds = rect.intersect(clip);
    for y in bounds.y..bounds.y + bounds.height {
        for x in bounds.x..bounds.x + bounds.width {
            let offset = ((y as usize) * width as usize + x as usize) * 3;
            pixels[offset..offset + 3].copy_from_slice(&[color.0, color.1, color.2]);
        }
    }
}

fn paint_box(pixels: &mut [u8], width: u32, item: &LayoutBox) {
    let r = item.border_box;
    let border = item.style.border;
    if let Some(background) = item.style.background {
        fill(pixels, width, r, item.clip, background);
    }
    let edges = [
        Rect {
            x: r.x,
            y: r.y,
            width: r.width,
            height: border.top,
        },
        Rect {
            x: r.x,
            y: r.y + r.height - border.bottom,
            width: r.width,
            height: border.bottom,
        },
        Rect {
            x: r.x,
            y: r.y,
            width: border.left,
            height: r.height,
        },
        Rect {
            x: r.x + r.width - border.right,
            y: r.y,
            width: border.right,
            height: r.height,
        },
    ];
    if let Some(border_color) = item.style.border_color {
        for edge in edges {
            fill(pixels, width, edge, item.clip, border_color);
        }
    }
}

fn rect_json(rect: Rect) -> String {
    format!(
        "{{\"x\":{},\"y\":{},\"width\":{},\"height\":{}}}",
        rect.x, rect.y, rect.width, rect.height
    )
}

fn layout_json(width: u32, height: u32, boxes: &[LayoutBox]) -> String {
    let entries: Vec<_> = boxes.iter().map(|item| {
        let background = item.style.background.map(|color| json_string(&color.hex())).unwrap_or_else(|| "null".to_owned());
        format!("{{\"node\":{},\"tag\":{},\"border_box\":{},\"content_box\":{},\"clip\":{},\"background\":{}}}",
            item.node, json_string(&item.tag), rect_json(item.border_box), rect_json(item.content_box), rect_json(item.clip), background)
    }).collect();
    format!(
        "{{\"viewport\":{{\"width\":{width},\"height\":{height}}},\"boxes\":[{}]}}\n",
        entries.join(",")
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::html::parse;

    fn output(html: &str, width: u32, height: u32) -> RenderOutput {
        render(&parse(html).unwrap(), width, height).unwrap()
    }

    fn pixel(output: &RenderOutput, width: u32, height: u32, x: usize, y: usize) -> &[u8] {
        let header = format!("P6\n{width} {height}\n255\n");
        let offset = header.len() + (y * width as usize + x) * 3;
        &output.ppm[offset..offset + 3]
    }

    #[test]
    fn paints_fixed_box_and_preserves_white_canvas() {
        let out = output(
            "<style>body{margin:0}div{width:4px;height:3px;background:red}</style><body><div></div></body>",
            8,
            6,
        );
        assert_eq!(pixel(&out, 8, 6, 3, 2), [255, 0, 0]);
        assert_eq!(pixel(&out, 8, 6, 4, 2), [255, 255, 255]);
        assert!(
            out.layout_json
                .contains("\"content_box\":{\"x\":0,\"y\":0,\"width\":4,\"height\":3}")
        );
    }

    #[test]
    fn nested_edges_place_child_and_paint_borders() {
        let out = output(
            "<style>body{margin:0}#parent{margin:3px;padding:4px;border:2px solid black;width:30px;height:20px;background:red}#child{width:8px;height:6px;background:blue}</style><body><div id=parent><div id=child></div></div></body>",
            64,
            48,
        );
        assert_eq!(pixel(&out, 64, 48, 3, 3), [0, 0, 0]);
        assert_eq!(pixel(&out, 64, 48, 5, 5), [255, 0, 0]);
        assert_eq!(pixel(&out, 64, 48, 9, 9), [0, 0, 255]);
        assert!(
            out.layout_json
                .contains("\"content_box\":{\"x\":9,\"y\":9,\"width\":8,\"height\":6}")
        );
    }

    #[test]
    fn cascade_obeys_specificity_source_order_and_importance() {
        let out = output(
            "<style>body{margin:0}div{width:4px;height:4px;background:red}.a{background:blue}#x{background:green}.a{background:yellow!important}</style><body><div id=x class=a style='background:black'></div></body>",
            8,
            8,
        );
        assert_eq!(pixel(&out, 8, 8, 0, 0), [255, 255, 0]);
        let out = output(
            "<style>body{margin:0}#x{width:4px;height:4px;background:green!important}</style><body><div id=x style='background:blue!important'></div></body>",
            8,
            8,
        );
        assert_eq!(pixel(&out, 8, 8, 0, 0), [0, 0, 255]);
    }

    #[test]
    fn descendant_and_child_selectors_have_distinct_meaning() {
        let out = output(
            "<style>body{margin:0}div{height:4px}body div{background:red}body>div{background:blue}</style><body><div><div></div></div></body>",
            8,
            8,
        );
        assert_eq!(pixel(&out, 8, 8, 0, 0), [255, 0, 0]);
    }

    #[test]
    fn display_none_does_not_consume_flow_or_reject_hidden_text() {
        let out = output(
            "<style>body{margin:0}.hidden{display:none}#shown{width:2px;height:2px;background:blue}</style><body><div class=hidden>inert text</div><div id=shown></div></body>",
            4,
            4,
        );
        assert_eq!(pixel(&out, 4, 4, 0, 0), [0, 0, 255]);
        assert_eq!(out.layout_json.matches("\"tag\":\"div\"").count(), 1);
    }

    #[test]
    fn overflow_hidden_clips_child_to_padding_edge() {
        let out = output(
            "<style>body{margin:0}#p{width:3px;height:3px;overflow:hidden}#c{width:7px;height:7px;background:red}</style><body><div id=p><div id=c></div></div></body>",
            8,
            8,
        );
        assert_eq!(pixel(&out, 8, 8, 2, 2), [255, 0, 0]);
        assert_eq!(pixel(&out, 8, 8, 3, 2), [255, 255, 255]);
        assert_eq!(pixel(&out, 8, 8, 2, 3), [255, 255, 255]);
    }

    #[test]
    fn negative_margin_and_oversized_box_clip_to_viewport() {
        let out = output(
            "<body style='margin:0'><div style='margin-left:-2px;width:8px;height:8px;background:#123'></div></body>",
            4,
            4,
        );
        assert_eq!(pixel(&out, 4, 4, 0, 0), [17, 34, 51]);
        assert_eq!(pixel(&out, 4, 4, 3, 3), [17, 34, 51]);
    }

    #[test]
    fn transparent_border_preserves_geometry_and_reveals_background() {
        let out = output(
            "<body style='margin:0'><div style='width:2px;height:2px;border:2px solid transparent;background:red'></div></body>",
            8,
            8,
        );
        assert_eq!(pixel(&out, 8, 8, 0, 0), [255, 0, 0]);
        assert_eq!(pixel(&out, 8, 8, 5, 5), [255, 0, 0]);
        assert_eq!(pixel(&out, 8, 8, 6, 5), [255, 255, 255]);
        assert!(
            out.layout_json
                .contains("\"border_box\":{\"x\":0,\"y\":0,\"width\":6,\"height\":6}")
        );
    }

    #[test]
    fn border_width_does_not_imply_solid_border_style() {
        let out = output(
            "<body style='margin:0'><div style='width:2px;height:2px;border-width:2px;background:red'></div></body>",
            8,
            8,
        );
        assert_eq!(pixel(&out, 8, 8, 0, 0), [255, 0, 0]);
        assert_eq!(pixel(&out, 8, 8, 2, 0), [255, 255, 255]);
        let out = output(
            "<body style='margin:0'><div style='width:2px;height:2px;border-width:2px;border-style:solid;background:red'></div></body>",
            8,
            8,
        );
        assert_eq!(pixel(&out, 8, 8, 0, 0), [0, 0, 0]);
        assert_eq!(pixel(&out, 8, 8, 2, 2), [255, 0, 0]);
    }

    #[test]
    fn auto_height_overflow_clip_uses_final_padding_box() {
        let out = output(
            "<style>body{margin:0}#p{width:4px;overflow:hidden}#c{height:8px;margin-bottom:-6px;background:red}</style><body><div id=p><div id=c></div></div></body>",
            8,
            8,
        );
        assert_eq!(pixel(&out, 8, 8, 0, 1), [255, 0, 0]);
        assert_eq!(pixel(&out, 8, 8, 0, 2), [255, 255, 255]);
    }

    #[test]
    fn important_keyword_accepts_whitespace_after_exclamation() {
        let out = output(
            "<style>div{background:red ! important;width:2px;height:2px}</style><body style='margin:0'><div style='background:blue'></div></body>",
            4,
            4,
        );
        assert_eq!(pixel(&out, 4, 4, 0, 0), [255, 0, 0]);
    }

    #[test]
    fn repetitive_nonmatching_ancestor_selector_has_bounded_work() {
        let mut input = String::from(
            "<style>missing div div div div div div div div div div div div div div div{background:red}</style><body style='margin:0'>",
        );
        input.push_str(&"<div>".repeat(100));
        input.push_str(&"</div>".repeat(100));
        input.push_str("</body>");
        assert!(render(&parse(&input).unwrap(), 8, 8).is_ok());
    }

    #[test]
    fn rejects_visible_text_and_replaced_content() {
        assert!(
            render(&parse("<div>hello</div>").unwrap(), 8, 8)
                .err()
                .unwrap()
                .contains("visible text")
        );
        assert!(
            render(&parse("<img src=x>").unwrap(), 8, 8)
                .err()
                .unwrap()
                .contains("replaced element")
        );
    }

    #[test]
    fn rejects_external_stylesheets_even_inside_hidden_head() {
        for input in [
            "<html><head><link rel=stylesheet href=https://example.invalid/style.css></head><body><div></div></body></html>",
            "<html><head><link rel='alternate STYLESHEET' href='local.css'></head><body></body></html>",
            "<link REL=StyleSheet HREF=''><div></div>",
        ] {
            let error = render(&parse(input).unwrap(), 8, 8).err().unwrap();
            assert!(error.contains("unsupported external stylesheet"), "{error}");
        }
    }

    #[test]
    fn rejects_imports_and_unimplemented_image_resource_values() {
        for input in [
            "<style>@import url('theme.css');</style><div></div>",
            "<style>/* comment */ @import 'theme.css'; div{height:2px}</style><div></div>",
            "<style>div{background-image:url(https://example.invalid/image.png)}</style><div></div>",
            "<div style='background-image:url(local.png)'></div>",
            "<div style='border-image-source:url(local.png)'></div>",
        ] {
            let error = render(&parse(input).unwrap(), 8, 8).err().unwrap();
            assert!(error.contains("unsupported"), "{error}");
        }
    }

    #[test]
    fn non_stylesheet_links_and_explicit_no_image_do_not_invent_resources() {
        let out = output(
            "<head><link rel=canonical href='https://example.invalid/'></head><div style='background-image:none;height:2px;background:red'></div>",
            4,
            4,
        );
        assert_eq!(pixel(&out, 4, 4, 0, 0), [255, 0, 0]);
    }

    #[test]
    fn rejects_invalid_viewports_and_out_of_scope_css() {
        let doc = parse("<div></div>").unwrap();
        assert!(render(&doc, 0, 1).is_err());
        assert!(render(&doc, 2049, 1).is_err());
        assert!(render(&parse("<div style='width:50%'></div>").unwrap(), 8, 8).is_err());
        assert!(
            render(
                &parse("<style>div:hover{width:2px}</style><div></div>").unwrap(),
                8,
                8
            )
            .is_err()
        );
        assert!(
            render(
                &parse("<style>div{width:2px</style><div></div>").unwrap(),
                8,
                8
            )
            .is_err()
        );
    }

    #[test]
    fn css_unknown_and_malformed_declarations_do_not_erase_valid_values() {
        let out = output(
            "<body style='margin:0'><div style='width:2px;bad;made-up:7;width:cats;height:2px;background:#f00'></div></body>",
            4,
            4,
        );
        assert_eq!(pixel(&out, 4, 4, 1, 1), [255, 0, 0]);
        assert_eq!(pixel(&out, 4, 4, 2, 1), [255, 255, 255]);
    }

    #[test]
    fn malformed_document_store_returns_error_instead_of_panicking() {
        let mut doc = parse("<div></div>").unwrap();
        doc.nodes[doc.root].children.push(usize::MAX);
        assert!(render(&doc, 4, 4).is_err());
    }

    #[test]
    fn element_depth_limit_does_not_count_raw_text_as_an_element() {
        let input = format!(
            "{}<style>div{{height:1px}}</style>{}",
            "<div>".repeat(MAX_DEPTH - 1),
            "</div>".repeat(MAX_DEPTH - 1)
        );
        assert!(render(&parse(&input).unwrap(), 8, 8).is_ok());

        let input = format!(
            "{}{}",
            "<div>".repeat(MAX_DEPTH),
            "</div>".repeat(MAX_DEPTH)
        );
        let mut document = parse(&input).unwrap();
        assert!(render(&document, 8, 8).is_ok());
        let parent = document.nodes.len() - 1;
        let child = document.nodes.len();
        document.nodes.push(crate::html::Node {
            parent: Some(parent),
            children: Vec::new(),
            kind: NodeKind::Element {
                name: "div".to_owned(),
                attributes: BTreeMap::new(),
            },
        });
        document.nodes[parent].children.push(child);
        assert!(
            render(&document, 8, 8)
                .err()
                .unwrap()
                .contains("depth limit")
        );
    }

    #[test]
    fn css_source_limit_counts_all_inline_and_embedded_bytes() {
        let one_inline = format!(
            "ignored:{}",
            "a".repeat(MAX_CSS_BYTES / 2 - "ignored:".len())
        );
        let at_limit = format!("<div style='{one_inline}'></div><div style='{one_inline}'></div>");
        assert!(render(&parse(&at_limit).unwrap(), 8, 8).is_ok());
        let over_limit =
            format!("<div style='{one_inline}'></div><div style='{one_inline} '></div>");
        assert!(
            render(&parse(&over_limit).unwrap(), 8, 8)
                .err()
                .unwrap()
                .contains("total embedded and inline CSS")
        );

        let inline = "width:1px";
        let embedded = format!("/*{}*/", "a".repeat(MAX_CSS_BYTES - inline.len() - 4));
        let at_limit = format!("<style>{embedded}</style><div style='{inline}'></div>");
        assert!(render(&parse(&at_limit).unwrap(), 8, 8).is_ok());
        let over_limit = format!("<style>{embedded}</style><div style='{inline} '></div>");
        assert!(render(&parse(&over_limit).unwrap(), 8, 8).is_err());
    }

    #[test]
    fn repeated_render_is_byte_identical() {
        let doc =
            parse("<body style='margin:0'><div style='height:7px;background:#abc'></div></body>")
                .unwrap();
        let first = render(&doc, 16, 16).unwrap();
        for _ in 0..100 {
            let next = render(&doc, 16, 16).unwrap();
            assert_eq!(first.ppm, next.ppm);
            assert_eq!(first.layout_json, next.layout_json);
        }
    }
}
