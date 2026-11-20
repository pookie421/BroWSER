//! Deterministic renderer for controlled HTML/CSS fixtures.
//!
//! This is an explicitly bounded fixture grammar, not a claim of general CSS or
//! HTML rendering support. It paints empty block boxes, solid borders, backgrounds
//! and overflow clips. The M1 profile additionally shapes and paints direct
//! text in standalone block boxes and collapses adjoining in-flow margins.

use crate::html::{Document, NodeKind, json_string};
use crate::png_image::{PngImage, PngResources};
use crate::text::{Direction, PinnedFont};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write;

mod css;
mod inline;
mod margins;
use css::Token;
use css::colors::{ColorValue, color_value};
use css::values::{edge_length, font_size_px, line_height_px, parse_declarations, parse_length};

const MAX_VIEWPORT: u32 = 2048;
const MAX_NODES: usize = 10_000;
const MAX_DEPTH: usize = 128;
const MAX_RULES: usize = 512;
const MAX_DECLARATIONS: usize = 4096;
const MAX_CSS_BYTES: usize = 262_144;
const MAX_LENGTH: i64 = 16_384;
const MEDIUM_BORDER_WIDTH: i64 = 3;
const MAX_SELECTOR_COMPOUNDS: usize = u16::BITS as usize;
const MAX_SIBLING_SELECTOR_CHECKS: usize = MAX_NODES * MAX_RULES;
const MAX_SELECTOR_CHECKS: usize = MAX_NODES * MAX_RULES;
const MAX_CASCADE_CANDIDATES: usize = MAX_NODES * MAX_RULES;
const MAX_PAINT_PIXELS: usize = 8 * MAX_VIEWPORT as usize * MAX_VIEWPORT as usize;

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

fn color_json(color: Option<Color>) -> String {
    json_string(&color.map_or_else(|| "transparent".to_owned(), Color::hex))
}

#[derive(Clone, Copy, Debug, Default)]
struct Edges {
    top: i64,
    right: i64,
    bottom: i64,
    left: i64,
}

impl Edges {
    fn values(self) -> [i64; 4] {
        [self.top, self.right, self.bottom, self.left]
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Display {
    Inline,
    Block,
    None,
}

impl Display {
    fn css(self) -> &'static str {
        match self {
            Self::Inline => "inline",
            Self::Block => "block",
            Self::None => "none",
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct TextStyle {
    font: PinnedFont,
    font_px: u32,
    line_px: Option<u32>,
    direction: Direction,
}

impl TextStyle {
    const INITIAL: Self = Self {
        font: PinnedFont::Latin,
        font_px: 16,
        line_px: None,
        direction: Direction::Ltr,
    };
}

#[derive(Clone, Debug)]
struct Style {
    display: Display,
    width: Option<i64>,
    height: Option<i64>,
    margin: Edges,
    padding: Edges,
    border: Edges,
    background: ColorValue,
    foreground: Option<Color>,
    border_color: ColorValue,
    solid_border: bool,
    clip_children: bool,
    text: TextStyle,
}

impl Style {
    fn initial() -> Self {
        Self {
            display: Display::Inline,
            width: None,
            height: None,
            margin: Edges::default(),
            padding: Edges::default(),
            border: Edges {
                top: MEDIUM_BORDER_WIDTH,
                right: MEDIUM_BORDER_WIDTH,
                bottom: MEDIUM_BORDER_WIDTH,
                left: MEDIUM_BORDER_WIDTH,
            },
            background: ColorValue::Transparent,
            foreground: Some(Color(0, 0, 0)),
            border_color: ColorValue::Current,
            solid_border: false,
            clip_children: false,
            text: TextStyle::INITIAL,
        }
    }

    fn for_tag(tag: &str, hidden: bool) -> Self {
        let mut style = Self::initial();
        // These are the controlled HTML subset's user-agent declarations.
        style.display =
            if hidden || matches!(tag, "head" | "style" | "script" | "title" | "meta" | "link") {
                Display::None
            } else if matches!(
                tag,
                "html"
                    | "body"
                    | "div"
                    | "p"
                    | "section"
                    | "article"
                    | "main"
                    | "header"
                    | "footer"
                    | "nav"
                    | "aside"
            ) {
                Display::Block
            } else {
                Display::Inline
            };
        if tag == "body" {
            style.margin = Edges {
                top: 8,
                right: 8,
                bottom: 8,
                left: 8,
            };
        }
        style
    }

    fn background_color(&self) -> Option<Color> {
        self.background.resolve(self.foreground)
    }

    fn border_color(&self) -> Option<Color> {
        self.border_color.resolve(self.foreground)
    }
}

#[derive(Clone, Copy)]
enum Relation {
    Descendant,
    Child,
    AdjacentSibling,
    GeneralSibling,
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
    specificity: Specificity,
    has_sibling: bool,
}

#[derive(Clone)]
struct Declaration {
    name: String,
    value: String,
    important: bool,
    order: usize,
}

struct Rule {
    origin: StylesheetOrigin,
    selectors: Vec<Selector>,
    declarations: Vec<Declaration>,
}

type Specificity = (u32, u32, u32);

#[derive(Clone, Copy)]
enum StylesheetOrigin {
    User,
    Author,
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum CascadeLevel {
    UserNormal,
    AuthorNormal,
    AuthorImportant,
    UserImportant,
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct Rank {
    cascade_level: CascadeLevel,
    inline: bool,
    specificity: Specificity,
    order: usize,
}

struct ElementLinks<'a> {
    node: usize,
    tag: &'a str,
    id: Option<&'a str>,
    classes: BTreeSet<&'a str>,
    parent: Option<usize>,
    previous_sibling: Option<usize>,
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
    image: Option<String>,
    inline: Option<(inline::InlineLayout, Rect)>,
}

/// Render the declared block-box fixture subset into binary P6 PPM and geometry.
pub fn render(doc: &Document, width: u32, height: u32) -> Result<RenderOutput, String> {
    render_with_resources(doc, width, height, &PngResources::default())
}

/// Render with an explicit map of decoded PNGs. The renderer cannot resolve
/// filesystem paths or network addresses.
pub fn render_with_resources(
    doc: &Document,
    width: u32,
    height: u32,
    images: &PngResources,
) -> Result<RenderOutput, String> {
    render_fixture(doc, width, height, images, false)
}

/// Render a controlled document using the pinned-font text profile.
pub fn render_m1(
    doc: &Document,
    width: u32,
    height: u32,
    images: &PngResources,
) -> Result<RenderOutput, String> {
    render_fixture(doc, width, height, images, true)
}

fn render_fixture(
    doc: &Document,
    width: u32,
    height: u32,
    images: &PngResources,
    m1_profile: bool,
) -> Result<RenderOutput, String> {
    if width == 0 || height == 0 || width > MAX_VIEWPORT || height > MAX_VIEWPORT {
        return Err(format!(
            "viewport dimensions must be between 1 and {MAX_VIEWPORT}"
        ));
    }
    let elements = validate_document(doc)?;
    let rules = collect_rules(doc, &elements, "")?;
    let winners = cascade(doc, &elements, &rules)?;
    let mut resources = LayoutResources {
        winners: &winners,
        images,
        inline_budget: m1_profile.then(inline::InlineBudget::default),
    };
    let mut boxes = Vec::new();
    let viewport = Rect {
        x: 0,
        y: 0,
        width: i64::from(width),
        height: i64::from(height),
    };
    let initial = Style::initial();
    let mut y = 0;
    for child in &doc.nodes[doc.root].children {
        y += layout_node(
            doc,
            *child,
            (0, y),
            i64::from(width),
            (viewport, &initial),
            &mut resources,
            &mut boxes,
        )?;
    }
    if m1_profile {
        margins::reflow(doc, &mut boxes, viewport);
    }
    let byte_count = (width as usize)
        .checked_mul(height as usize)
        .and_then(|count| count.checked_mul(3))
        .ok_or_else(|| "surface size overflow".to_owned())?;
    let mut glyphs = BTreeMap::new();
    let mut raster_budget = inline::RasterBudget::default();
    for item in &boxes {
        if let Some((layout, clip)) = &item.inline
            && item.style.foreground.is_some()
        {
            glyphs.insert(item.node, layout.rasterize(*clip, &mut raster_budget)?);
        }
    }
    let fills = paint_fills(doc, &boxes, viewport, images, &glyphs)?;
    let mut ppm = format!("P6\n{width} {height}\n255\n").into_bytes();
    let header_len = ppm.len();
    ppm.resize(header_len + byte_count, 255);
    paint(&mut ppm[header_len..], width, &fills);
    let layout_json = layout_json(width, height, &boxes, m1_profile);
    Ok(RenderOutput { ppm, layout_json })
}

/// Serialize the fixture subset's computed properties in document order.
///
/// `node` is the DOM node index. `id` is the optional HTML ID attribute.
/// Lengths are integer CSS pixels, with null representing auto width or height.
/// The optional user-origin declarations share the normal stylesheet bounds.
pub fn inspect_styles(doc: &Document, user_css: &str) -> Result<String, String> {
    let elements = validate_document(doc)?;
    let rules = collect_rules(doc, &elements, user_css)?;
    let winners = cascade(doc, &elements, &rules)?;
    let initial = Style::initial();
    let mut computed = vec![None; doc.nodes.len()];
    let mut out = String::from("{\"elements\":[");
    for (index, element) in elements.iter().enumerate() {
        let parent = element.parent.and_then(|parent| computed[parent].as_ref());
        let style = computed_style(
            doc,
            element.node,
            parent.unwrap_or(&initial),
            &winners[element.node],
        )?;
        if index > 0 {
            out.push(',');
        }
        write_style_json(&mut out, element, &style);
        computed[element.node] = Some(style);
    }
    out.push_str("]}");
    Ok(out)
}

fn write_style_json(out: &mut String, element: &ElementLinks<'_>, style: &Style) {
    let id = element.id.map_or_else(|| "null".to_owned(), json_string);
    let length = |value: Option<i64>| value.map_or_else(|| "null".to_owned(), |n| n.to_string());
    write!(
        out,
        concat!(
            "{{\"node\":{},\"tag\":{},\"id\":{},\"properties\":",
            "{{\"color\":{},\"background-color\":{},\"width_px\":{},\"height_px\":{},",
            "\"display\":\"{}\",\"overflow\":\"{}\",\"border-color\":{},\"border-style\":\"{}\",",
            "\"margin_px\":{:?},\"padding_px\":{:?},\"border_width_px\":{:?},",
            "\"font-family\":{},\"font_size_px\":{},\"line_height_px\":{},\"direction\":\"{}\",",
            "\"white-space\":\"normal\"}}}}",
        ),
        element.node,
        json_string(element.tag),
        id,
        color_json(style.foreground),
        color_json(style.background_color()),
        length(style.width),
        length(style.height),
        style.display.css(),
        if style.clip_children {
            "hidden"
        } else {
            "visible"
        },
        color_json(style.border_color()),
        if style.solid_border { "solid" } else { "none" },
        style.margin.values(),
        style.padding.values(),
        style.border.values(),
        json_string(style.text.font.family()),
        style.text.font_px,
        length(style.text.line_px.map(i64::from)),
        style.text.direction.as_str(),
    )
    .expect("writing to a String");
}

fn validate_document(doc: &Document) -> Result<Vec<ElementLinks<'_>>, String> {
    if doc.nodes.is_empty() || doc.nodes.len() > MAX_NODES || doc.root >= doc.nodes.len() {
        return Err("invalid or oversized document node store".to_owned());
    }
    if !matches!(doc.nodes[doc.root].kind, NodeKind::Document)
        || doc.nodes[doc.root].parent.is_some()
    {
        return Err("document root must be an unparented document node".to_owned());
    }
    let mut seen = vec![false; doc.nodes.len()];
    let mut elements = Vec::new();
    let mut stack = vec![(doc.root, 0usize, None)];
    while let Some((node, depth, previous_sibling)) = stack.pop() {
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
        if matches!(doc.nodes[node].kind, NodeKind::Doctype(_))
            && (doc.nodes[node].parent != Some(doc.root) || !doc.nodes[node].children.is_empty())
        {
            return Err("doctype must be a document child without descendants".to_owned());
        }
        if let NodeKind::Element { name, attributes } = &doc.nodes[node].kind {
            elements.push(ElementLinks {
                node,
                tag: name,
                id: attributes.get("id").map(String::as_str),
                classes: attributes.get("class").map_or_else(BTreeSet::new, |value| {
                    value.split_ascii_whitespace().collect()
                }),
                parent: doc.nodes[node].parent,
                previous_sibling,
            });
        }
        let first_child = stack.len();
        let mut previous_element = None;
        for &child in &doc.nodes[node].children {
            if child >= doc.nodes.len() || doc.nodes[child].parent != Some(node) {
                return Err("document parent/child relationship is inconsistent".to_owned());
            }
            let is_element = matches!(doc.nodes[child].kind, NodeKind::Element { .. });
            stack.push((child, depth + usize::from(is_element), previous_element));
            if is_element {
                previous_element = Some(child);
            }
        }
        stack[first_child..].reverse();
    }
    doc.validate_detached_nodes(&mut seen)?;
    Ok(elements)
}

fn collect_rules(
    doc: &Document,
    elements: &[ElementLinks<'_>],
    user_css: &str,
) -> Result<Vec<Rule>, String> {
    let mut source_bytes = 0;
    let mut rules = Vec::new();
    let mut rule_count = 0;
    let mut order = 0;
    add_css_source_bytes(&mut source_bytes, user_css.len())?;
    collect_sheet(
        user_css,
        StylesheetOrigin::User,
        &mut rules,
        &mut rule_count,
        &mut order,
    )?;
    for element in elements {
        let node = &doc.nodes[element.node];
        let NodeKind::Element { name, attributes } = &node.kind else {
            unreachable!("validated element links only reference elements");
        };
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
            let mut css = String::new();
            for child in &node.children {
                match &doc.nodes[*child].kind {
                    NodeKind::Text(text) => {
                        add_css_source_bytes(&mut source_bytes, text.len())?;
                        css.push_str(text);
                    }
                    _ => return Err("style content must be raw CSS text".to_owned()),
                }
            }
            if is_css_type(attributes.get("type")) && matches_screen(attributes.get("media"))? {
                collect_sheet(
                    &css,
                    StylesheetOrigin::Author,
                    &mut rules,
                    &mut rule_count,
                    &mut order,
                )?;
            }
        }
    }
    Ok(rules)
}

fn is_css_type(value: Option<&String>) -> bool {
    value.is_none_or(|value| {
        let value = value.trim_ascii();
        value.is_empty() || value.eq_ignore_ascii_case("text/css")
    })
}

fn matches_screen(value: Option<&String>) -> Result<bool, String> {
    let Some(value) = value
        .map(|value| value.trim_ascii())
        .filter(|value| !value.is_empty())
    else {
        return Ok(true);
    };
    let mut matches = false;
    for medium in value.split(',') {
        match medium.trim_ascii().to_ascii_lowercase().as_str() {
            "all" | "screen" => matches = true,
            "print" | "speech" => {}
            _ => {
                return Err(format!(
                    "unsupported bootstrap stylesheet media query: {medium}"
                ));
            }
        }
    }
    Ok(matches)
}

fn collect_sheet(
    css: &str,
    origin: StylesheetOrigin,
    rules: &mut Vec<Rule>,
    rule_count: &mut usize,
    order: &mut usize,
) -> Result<(), String> {
    let tokens = css::tokens(css)?;
    let mut rest = tokens.as_slice();
    while let Some(index) = rest.iter().position(|token| *token != Token::Space) {
        rest = &rest[index..];
        if rest[0] == Token::Delimiter(b'@') {
            return Err(
                "unsupported CSS at-rule: imports and external stylesheet loading are unavailable"
                    .to_owned(),
            );
        }
        let open = rest
            .iter()
            .position(|token| matches!(token, Token::Block(b'{', _)))
            .ok_or_else(|| "stylesheet rule has no opening brace".to_owned())?;
        let Token::Block(b'{', body) = rest[open] else {
            unreachable!("located a declaration block");
        };
        *rule_count += 1;
        if *rule_count > MAX_RULES {
            return Err(format!("stylesheet exceeds {MAX_RULES} rules"));
        }
        let selectors = rest[..open]
            .split(|token| *token == Token::Delimiter(b','))
            .map(parse_selector)
            .collect::<Result<_, _>>()?;
        let declarations = parse_declarations(body, order)?;
        if !declarations.is_empty() {
            rules.push(Rule {
                origin,
                selectors,
                declarations,
            });
        }
        rest = &rest[open + 1..];
    }
    Ok(())
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

fn parse_selector(tokens: &[Token<'_>]) -> Result<Selector, String> {
    let mut text = String::new();
    let mut previous_is_identifier = false;
    for token in tokens {
        match token {
            Token::Space => text.push(' '),
            Token::Name(_) if previous_is_identifier => {
                return Err("adjacent selector identifiers require real whitespace".to_owned());
            }
            Token::Name(value) | Token::Hash(value) => text.push_str(value),
            Token::Delimiter(byte @ (b'.' | b'*' | b'>' | b'+' | b'~')) => {
                text.push(char::from(*byte));
            }
            _ => return Err("unsupported bootstrap selector syntax".to_owned()),
        }
        previous_is_identifier = matches!(token, Token::Name(_) | Token::Hash(_));
    }
    if text.trim().is_empty() {
        return Err("empty CSS selector".to_owned());
    }
    let spaced = text
        .replace('>', " > ")
        .replace('+', " + ")
        .replace('~', " ~ ");
    let mut compounds = Vec::new();
    let mut relations = Vec::new();
    let mut pending = None;
    let mut specificity = (0, 0, 0);
    let mut has_sibling = false;
    for token in spaced.split_whitespace() {
        let relation = match token {
            ">" => Some(Relation::Child),
            "+" => Some(Relation::AdjacentSibling),
            "~" => Some(Relation::GeneralSibling),
            _ => None,
        };
        if let Some(relation) = relation {
            if compounds.is_empty() || pending.is_some() {
                return Err("invalid selector combinator".to_owned());
            }
            has_sibling |= matches!(
                relation,
                Relation::AdjacentSibling | Relation::GeneralSibling
            );
            pending = Some(relation);
            continue;
        }
        if !compounds.is_empty() {
            relations.push(pending.take().unwrap_or(Relation::Descendant));
        }
        let compound = parse_compound(token)?;
        specificity.0 += compound.ids.len() as u32;
        specificity.1 += compound.classes.len() as u32;
        specificity.2 += u32::from(compound.tag.is_some());
        compounds.push(compound);
        if compounds.len() > MAX_SELECTOR_COMPOUNDS {
            return Err(format!(
                "selector exceeds {MAX_SELECTOR_COMPOUNDS} compound selectors"
            ));
        }
    }
    if pending.is_some() {
        return Err("selector ends with a combinator".to_owned());
    }
    Ok(Selector {
        compounds,
        relations,
        specificity,
        has_sibling,
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

fn matches_compound(element: &ElementLinks<'_>, compound: &Compound) -> bool {
    compound.tag.as_deref().is_none_or(|tag| tag == element.tag)
        && compound.ids.iter().all(|id| element.id == Some(id))
        && compound
            .classes
            .iter()
            .all(|class| element.classes.contains(class.as_str()))
}

#[derive(Clone, Copy, Default)]
struct SelectorMatches {
    prefixes: u16,
    ancestors: u16,
    earlier_siblings: u16,
}

type Winners<'a> = BTreeMap<&'a str, (Rank, &'a str)>;

fn cascade<'a>(
    doc: &Document,
    elements: &[ElementLinks<'_>],
    rules: &'a [Rule],
) -> Result<Vec<Winners<'a>>, String> {
    let mut sibling_checks = 0;
    let mut matching_checks = 0;
    // Charge the upper bound before matching any selector. Probe costs account
    // for large compounds, even when a particular predicate short-circuits.
    for selector in rules.iter().flat_map(|rule| &rule.selectors) {
        if selector.has_sibling {
            sibling_checks += elements.len() * selector.compounds.len();
            if sibling_checks > MAX_SIBLING_SELECTOR_CHECKS {
                return Err(format!(
                    "sibling selectors exceed {MAX_SIBLING_SELECTOR_CHECKS} matching checks"
                ));
            }
        }
        let probes: usize = selector
            .compounds
            .iter()
            .map(|compound| {
                (usize::from(compound.tag.is_some()) + compound.ids.len() + compound.classes.len())
                    .max(1)
            })
            .sum();
        matching_checks += elements.len() * probes;
    }
    if matching_checks > MAX_SELECTOR_CHECKS {
        return Err(format!(
            "selectors exceed {MAX_SELECTOR_CHECKS} matching checks"
        ));
    }
    let mut winners = vec![Winners::new(); doc.nodes.len()];
    let mut states = vec![SelectorMatches::default(); doc.nodes.len()];
    let mut specificity = vec![None; doc.nodes.len()];
    let mut candidates = 0;
    for rule in rules {
        specificity.fill(None);
        for selector in &rule.selectors {
            match_selector(elements, selector, &mut states, &mut specificity);
        }
        for element in elements {
            if let Some(specificity) = specificity[element.node] {
                candidates += rule.declarations.len();
                if candidates > MAX_CASCADE_CANDIDATES {
                    return Err(format!(
                        "cascade exceeds {MAX_CASCADE_CANDIDATES} declaration candidates"
                    ));
                }
                for declaration in &rule.declarations {
                    offer(
                        &mut winners[element.node],
                        declaration,
                        rule.origin,
                        specificity,
                        false,
                    );
                }
            }
        }
    }
    Ok(winners)
}

fn match_selector(
    elements: &[ElementLinks<'_>],
    selector: &Selector,
    states: &mut [SelectorMatches],
    specificity: &mut [Option<Specificity>],
) {
    let complete = 1 << (selector.compounds.len() - 1);
    // Each bit is a matched selector prefix. Parents and preceding element
    // siblings precede their dependents in tree order. Every selector needs one
    // pass without rescanning earlier siblings or backtracking through ancestors.
    for element in elements {
        let parent = element.parent.map(|id| states[id]).unwrap_or_default();
        let sibling = element
            .previous_sibling
            .map(|id| states[id])
            .unwrap_or_default();
        let ancestors = parent.prefixes | parent.ancestors;
        let earlier_siblings = sibling.prefixes | sibling.earlier_siblings;
        let mut prefixes = u16::from(matches_compound(element, &selector.compounds[0]));
        for (index, relation) in selector.relations.iter().enumerate() {
            let candidates = match relation {
                Relation::Child => parent.prefixes,
                Relation::Descendant => ancestors,
                Relation::AdjacentSibling => sibling.prefixes,
                Relation::GeneralSibling => earlier_siblings,
            };
            if candidates & (1 << index) != 0
                && matches_compound(element, &selector.compounds[index + 1])
            {
                prefixes |= 1 << (index + 1);
            }
        }
        states[element.node] = SelectorMatches {
            prefixes,
            ancestors,
            earlier_siblings,
        };
        if prefixes & complete != 0 {
            specificity[element.node] = specificity[element.node].max(Some(selector.specificity));
        }
    }
}

fn offer<'a>(
    winners: &mut Winners<'a>,
    declaration: &'a Declaration,
    origin: StylesheetOrigin,
    specificity: Specificity,
    inline: bool,
) {
    let rank = Rank {
        cascade_level: match (origin, declaration.important) {
            (StylesheetOrigin::User, false) => CascadeLevel::UserNormal,
            (StylesheetOrigin::Author, false) => CascadeLevel::AuthorNormal,
            (StylesheetOrigin::Author, true) => CascadeLevel::AuthorImportant,
            (StylesheetOrigin::User, true) => CascadeLevel::UserImportant,
        },
        inline,
        specificity,
        order: declaration.order,
    };
    let winner = winners
        .entry(&declaration.name)
        .or_insert((rank, &declaration.value));
    if rank >= winner.0 {
        *winner = (rank, &declaration.value);
    }
}

fn computed_style(
    doc: &Document,
    node: usize,
    parent: &Style,
    stylesheet_winners: &Winners<'_>,
) -> Result<Style, String> {
    let NodeKind::Element { name, attributes } = &doc.nodes[node].kind else {
        return Err("style requested for a non-element node".to_owned());
    };
    let mut style = Style::for_tag(name, attributes.contains_key("hidden"));
    let initial = Style::initial();
    style.foreground = parent.foreground;
    style.text = parent.text;
    let mut winners = stylesheet_winners.clone();
    let inline = attributes.get("style").map(String::as_str).unwrap_or("");
    let inline_declarations = parse_declarations(inline, &mut 0)?;
    for declaration in &inline_declarations {
        offer(
            &mut winners,
            declaration,
            StylesheetOrigin::Author,
            (0, 0, 0),
            true,
        );
    }
    if name == "img" {
        if !winners.contains_key("width") {
            style.width = image_attribute_size(attributes, "width")?;
        }
        if !winners.contains_key("height") {
            style.height = image_attribute_size(attributes, "height")?;
        }
    }
    if !winners.contains_key("display")
        && attributes
            .get("hidden")
            .is_some_and(|value| value.eq_ignore_ascii_case("until-found"))
    {
        return Err("hidden until-found is outside the bootstrap renderer".to_owned());
    }
    if let Some(dir) = attributes.get("dir") {
        if dir.eq_ignore_ascii_case("rtl") {
            style.text.direction = Direction::Rtl;
        } else if dir.eq_ignore_ascii_case("ltr") {
            style.text.direction = Direction::Ltr;
        } else if dir.eq_ignore_ascii_case("auto") && !winners.contains_key("direction") {
            return Err(
                "automatic HTML text direction is outside the controlled renderer".to_owned(),
            );
        }
    }
    for (property, (_, value)) in winners {
        let default_from = match value {
            "inherit" => Some(parent),
            "unset"
                if matches!(
                    property,
                    "color"
                        | "font-family"
                        | "font-size"
                        | "line-height"
                        | "white-space"
                        | "direction"
                ) =>
            {
                Some(parent)
            }
            "initial" | "unset" => Some(&initial),
            _ => None,
        };
        match property {
            "display" => {
                style.display = default_from.map_or_else(
                    || {
                        if value == "none" {
                            Display::None
                        } else {
                            Display::Block
                        }
                    },
                    |source| source.display,
                );
            }
            "width" => {
                style.width = if let Some(source) = default_from {
                    source.width
                } else {
                    parse_length(value, false)?
                };
            }
            "height" => {
                style.height = if let Some(source) = default_from {
                    source.height
                } else {
                    parse_length(value, false)?
                };
            }
            "color" => {
                style.foreground = if let Some(source) = default_from {
                    source.foreground
                } else {
                    absolute_or_current_color(value)?.resolve(parent.foreground)
                };
            }
            "background-color" => {
                style.background = if let Some(source) = default_from {
                    source.background
                } else {
                    absolute_or_current_color(value)?
                };
            }
            "border-color" => {
                style.border_color = if let Some(source) = default_from {
                    source.border_color
                } else {
                    absolute_or_current_color(value)?
                };
            }
            "border-style" => {
                style.solid_border =
                    default_from.map_or(value == "solid", |source| source.solid_border);
            }
            "overflow" => {
                style.clip_children =
                    default_from.map_or(value == "hidden", |source| source.clip_children);
            }
            "font-family" => {
                style.text.font = if let Some(source) = default_from {
                    source.text.font
                } else {
                    PinnedFont::for_family(value)?
                };
            }
            "font-size" => {
                style.text.font_px = if let Some(source) = default_from {
                    source.text.font_px
                } else {
                    font_size_px(value)?
                        .ok_or_else(|| format!("invalid expanded font-size: {value}"))?
                };
            }
            "line-height" => {
                style.text.line_px = if let Some(source) = default_from {
                    source.text.line_px
                } else {
                    line_height_px(value)?
                };
            }
            "direction" => {
                style.text.direction = if let Some(source) = default_from {
                    source.text.direction
                } else {
                    value.parse()?
                };
            }
            "white-space" => {} // The supported value and every default compute to normal.
            _ => {
                apply_edge(&mut style, property, value, default_from)?;
            }
        }
    }
    if !style.solid_border {
        style.border = Edges::default();
    }
    Ok(style)
}

fn absolute_or_current_color(value: &str) -> Result<ColorValue, String> {
    color_value(value)?.ok_or_else(|| format!("invalid expanded color: {value}"))
}

fn apply_edge(
    style: &mut Style,
    property: &str,
    value: &str,
    source: Option<&Style>,
) -> Result<(), String> {
    let (edges, inherited, side) = if let Some(side) = property.strip_prefix("margin-") {
        (&mut style.margin, source.map(|s| s.margin), side)
    } else if let Some(side) = property.strip_prefix("padding-") {
        (&mut style.padding, source.map(|s| s.padding), side)
    } else if let Some(side) = property
        .strip_prefix("border-")
        .and_then(|rest| rest.strip_suffix("-width"))
    {
        (&mut style.border, source.map(|s| s.border), side)
    } else {
        return Err(format!("unrecognized expanded edge property: {property}"));
    };
    let (slot, previous) = match side {
        "top" => (&mut edges.top, inherited.map(|e| e.top)),
        "right" => (&mut edges.right, inherited.map(|e| e.right)),
        "bottom" => (&mut edges.bottom, inherited.map(|e| e.bottom)),
        "left" => (&mut edges.left, inherited.map(|e| e.left)),
        _ => return Err(format!("unrecognized expanded edge side: {side}")),
    };
    *slot = if let Some(length) = previous {
        length
    } else {
        edge_length(property, value)?
            .ok_or_else(|| format!("invalid expanded edge length: {property}: {value}"))?
    };
    Ok(())
}

struct LayoutResources<'a> {
    winners: &'a [Winners<'a>],
    images: &'a PngResources,
    inline_budget: Option<inline::InlineBudget>,
}

fn layout_node(
    doc: &Document,
    node: usize,
    origin: (i64, i64),
    available_width: i64,
    context: (Rect, &Style),
    resources: &mut LayoutResources<'_>,
    boxes: &mut Vec<LayoutBox>,
) -> Result<i64, String> {
    let (x, y) = origin;
    let (clip, parent) = context;
    let (name, attributes) = match &doc.nodes[node].kind {
        NodeKind::Text(text) => {
            if text.chars().any(|c| !c.is_ascii_whitespace()) {
                return Err("unsupported visible text: shaping and inline layout are outside the bootstrap renderer".to_owned());
            }
            return Ok(0);
        }
        NodeKind::Doctype(_) => return Ok(0),
        NodeKind::Document => return Err("nested document nodes cannot be rendered".to_owned()),
        NodeKind::Element { name, attributes } => (name, attributes),
    };
    let style = computed_style(doc, node, parent, &resources.winners[node])?;
    if style.display == Display::None {
        return Ok(0);
    }
    if matches!(
        name.as_str(),
        "svg" | "canvas" | "video" | "audio" | "iframe" | "input" | "object" | "embed"
    ) {
        return Err(format!(
            "visible replaced element <{name}> is outside the bootstrap renderer"
        ));
    }
    let mut image = None;
    let mut image_size = None;
    if name == "img" {
        if attributes.contains_key("srcset") || attributes.contains_key("sizes") {
            return Err("responsive image sources are outside the fixture renderer".to_owned());
        }
        let label = attributes
            .get("src")
            .ok_or("replaced element <img> requires a PNG resource label")?;
        let source = resources.images.get(label).ok_or_else(|| {
            format!("replaced element <img> requires a declared PNG resource: {label:?}")
        })?;
        if style.display != Display::Block
            && let Some(parent_node) = doc.nodes[node].parent
        {
            for &sibling in &doc.nodes[parent_node].children {
                if sibling == node {
                    continue;
                }
                let visible = match &doc.nodes[sibling].kind {
                    NodeKind::Doctype(_) => false,
                    NodeKind::Text(text) => !text.chars().all(|ch| ch.is_ascii_whitespace()),
                    NodeKind::Element { .. } => {
                        computed_style(doc, sibling, parent, &resources.winners[sibling])?.display
                            != Display::None
                    }
                    NodeKind::Document => true,
                };
                if visible {
                    return Err(
                        "an inline fixture image must be the only visible content in its parent"
                            .to_owned(),
                    );
                }
            }
        }
        if !doc.nodes[node].children.is_empty() {
            return Err("a fixture image cannot have children".to_owned());
        }
        let (width, height) = source.size();
        let (width, height) = (i64::from(width), i64::from(height));
        image_size = Some(match (style.width, style.height) {
            (None, None) => (width, height),
            (Some(w), None) => (w, (w * height + width / 2) / width),
            (None, Some(h)) => ((h * width + height / 2) / height, h),
            (Some(w), Some(h)) => (w, h),
        });
        image = Some(label.clone());
    } else if style.display != Display::Block {
        return Err(format!(
            "<{name}> requires explicit display:block in the bootstrap fixture grammar"
        ));
    }
    let horizontal_edges =
        style.padding.left + style.padding.right + style.border.left + style.border.right;
    let content_width = image_size
        .map(|(width, _)| width)
        .or(style.width)
        .unwrap_or_else(|| {
            (available_width - style.margin.left - style.margin.right - horizontal_edges).max(0)
        });
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
        image,
        inline: None,
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
    let children = &doc.nodes[node].children;
    let mut inline = None;
    if let Some(budget) = &mut resources.inline_budget
        && children.iter().any(|&child| {
            matches!(&doc.nodes[child].kind, NodeKind::Text(text) if !text.chars().all(|ch| ch.is_ascii_whitespace()))
        })
    {
        if children.iter().any(|&child| !matches!(doc.nodes[child].kind, NodeKind::Text(_))) {
            return Err("mixed text and element children are outside the pinned inline fixture grammar".to_owned());
        }
        let text: String = children.iter().filter_map(|&child| match &doc.nodes[child].kind {
            NodeKind::Text(text) => Some(text.as_str()),
            _ => None,
        }).collect();
        inline = Some(inline::layout(
            &text,
            style.text,
            text_language(doc, node),
            (content_x, content_y),
            content_width,
            budget,
        )?);
    }
    let mut cursor = content_y;
    if let Some(layout) = &inline {
        cursor += layout.content_height;
    } else {
        for child in children {
            cursor += layout_node(
                doc,
                *child,
                (content_x, cursor),
                content_width,
                (child_clip, &style),
                resources,
                boxes,
            )?;
        }
    }
    let content_height = image_size
        .map(|(_, height)| height)
        .or(style.height)
        .unwrap_or_else(|| (cursor - content_y).max(0));
    let border_height = content_height
        + style.padding.top
        + style.padding.bottom
        + style.border.top
        + style.border.bottom;
    boxes[box_index].border_box.height = border_height;
    boxes[box_index].content_box.height = content_height;
    let final_clip = if style.clip_children {
        let padding_clip = clip.intersect(Rect {
            x: border_x + style.border.left,
            y: border_y + style.border.top,
            width: content_width + style.padding.left + style.padding.right,
            height: content_height + style.padding.top + style.padding.bottom,
        });
        for child in &mut boxes[box_index + 1..] {
            child.clip = child.clip.intersect(padding_clip);
            if let Some((_, inline_clip)) = &mut child.inline {
                *inline_clip = inline_clip.intersect(padding_clip);
            }
        }
        padding_clip
    } else {
        clip
    };
    boxes[box_index].inline = inline.map(|layout| (layout, final_clip));
    Ok(style.margin.top + border_height + style.margin.bottom)
}

fn text_language(doc: &Document, node: usize) -> Option<&str> {
    std::iter::successors(Some(node), |node| doc.nodes[*node].parent)
        .find_map(|node| match &doc.nodes[node].kind {
            NodeKind::Element { attributes, .. } => attributes.get("lang").map(String::as_str),
            _ => None,
        })
        .filter(|language| !language.is_empty())
}

fn image_attribute_size(
    attributes: &BTreeMap<String, String>,
    axis: &str,
) -> Result<Option<i64>, String> {
    attributes
        .get(axis)
        .map(|value| {
            value
                .trim_ascii()
                .parse::<i64>()
                .ok()
                .filter(|value| (0..=MAX_LENGTH).contains(value))
                .ok_or_else(|| {
                    format!("fixture image {axis} requires an integer in 0..={MAX_LENGTH}")
                })
        })
        .transpose()
}

enum Paint<'a> {
    Solid(Color),
    Image(&'a PngImage, Rect),
    Glyph(Color, &'a inline::PositionedGlyph),
}

struct Fill<'a> {
    bounds: Rect,
    paint: Paint<'a>,
}

fn canvas_background(doc: &Document, boxes: &[LayoutBox]) -> Option<(usize, Color)> {
    let html = doc.nodes[doc.root].children.iter().find(
        |&&node| matches!(&doc.nodes[node].kind, NodeKind::Element { name, .. } if name == "html"),
    );
    let body_parent = if let Some(&html) = html {
        // A display:none document element does not generate a canvas background.
        let root = boxes.iter().find(|item| item.node == html)?;
        if let Some(color) = root.style.background_color() {
            return Some((html, color));
        }
        html
    } else {
        // Explicit bootstrap document snippets may have a top-level body.
        doc.root
    };
    let &body = doc.nodes[body_parent].children.iter().find(
        |&&node| matches!(&doc.nodes[node].kind, NodeKind::Element { name, .. } if name == "body"),
    )?;
    let body = boxes.iter().find(|item| item.node == body)?;
    Some((body.node, body.style.background_color()?))
}

fn paint_fills<'a>(
    doc: &Document,
    boxes: &[LayoutBox],
    viewport: Rect,
    images: &'a PngResources,
    glyphs: &'a BTreeMap<usize, Vec<inline::PositionedGlyph>>,
) -> Result<Vec<Fill<'a>>, String> {
    let mut pixels = 0;
    let mut fills = Vec::new();
    let mut add = |rect: Rect, clip: Rect, paint| -> Result<(), String> {
        let bounds = rect.intersect(clip);
        if bounds.width > 0 && bounds.height > 0 {
            pixels += bounds.width as usize * bounds.height as usize;
            if pixels > MAX_PAINT_PIXELS {
                return Err(format!(
                    "paint exceeds {MAX_PAINT_PIXELS} clipped pixel writes"
                ));
            }
            fills.push(Fill { bounds, paint });
        }
        Ok(())
    };
    let canvas = canvas_background(doc, boxes);
    if let Some((_, color)) = canvas {
        add(viewport, viewport, Paint::Solid(color))?;
    }
    for item in boxes {
        let r = item.border_box;
        let border = item.style.border;
        if canvas.is_none_or(|(source, _)| source != item.node)
            && let Some(background) = item.style.background_color()
        {
            add(r, item.clip, Paint::Solid(background))?;
        }
        if item.style.solid_border
            && let Some(border_color) = item.style.border_color()
        {
            // Top and bottom bands include corners; side bands only cover the
            // intervening height, so the corners are painted exactly once.
            let side_height = r.height - border.top - border.bottom;
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
                    y: r.y + border.top,
                    width: border.left,
                    height: side_height,
                },
                Rect {
                    x: r.x + r.width - border.right,
                    y: r.y + border.top,
                    width: border.right,
                    height: side_height,
                },
            ];
            for edge in edges {
                add(edge, item.clip, Paint::Solid(border_color))?;
            }
        }
        if let Some(label) = &item.image {
            let image = images
                .get(label)
                .ok_or("layout image resource disappeared")?;
            add(
                item.content_box,
                item.clip,
                Paint::Image(image, item.content_box),
            )?;
        }
        if let Some(colors) = item.style.foreground
            && let Some(glyphs) = glyphs.get(&item.node)
        {
            for glyph in glyphs {
                add(glyph.bounds, item.clip, Paint::Glyph(colors, glyph))?;
            }
        }
    }
    Ok(fills)
}

fn paint(pixels: &mut [u8], width: u32, fills: &[Fill<'_>]) {
    let stride = width as usize * 3;
    for &Fill { bounds, ref paint } in fills {
        let start = bounds.y as usize * stride + bounds.x as usize * 3;
        let row_bytes = bounds.width as usize * 3;
        match *paint {
            Paint::Solid(color) => {
                let first_row = start..start + row_bytes;
                pixels[first_row.clone()]
                    .as_chunks_mut::<3>()
                    .0
                    .fill([color.0, color.1, color.2]);
                for row in 1..bounds.height as usize {
                    pixels.copy_within(first_row.clone(), start + row * stride);
                }
            }
            Paint::Image(image, destination) => {
                let (width, height) = image.size();
                for row in 0..bounds.height {
                    let y =
                        (bounds.y + row - destination.y) * i64::from(height) / destination.height;
                    let offset = start + row as usize * stride;
                    let row = pixels[offset..offset + row_bytes].as_chunks_mut::<3>().0;
                    for (column, pixel) in row.iter_mut().enumerate() {
                        let x = (bounds.x + column as i64 - destination.x) * i64::from(width)
                            / destination.width;
                        let [red, green, blue, alpha] = image.rgba(x as u32, y as u32);
                        blend(pixel, [red, green, blue], alpha);
                    }
                }
            }
            Paint::Glyph(color, glyph) => {
                let source_stride = glyph.bounds.width as usize;
                let first = (bounds.y - glyph.bounds.y) as usize * source_stride
                    + (bounds.x - glyph.bounds.x) as usize;
                for row in 0..bounds.height as usize {
                    let offset = start + row * stride;
                    let source = first + row * source_stride;
                    let coverage = &glyph.coverage[source..source + bounds.width as usize];
                    for (pixel, &alpha) in pixels[offset..offset + row_bytes]
                        .as_chunks_mut::<3>()
                        .0
                        .iter_mut()
                        .zip(coverage)
                    {
                        blend(pixel, [color.0, color.1, color.2], alpha);
                    }
                }
            }
        }
    }
}

fn blend(destination: &mut [u8; 3], color: [u8; 3], opacity: u8) {
    let alpha = u16::from(opacity);
    for (background, foreground) in destination.iter_mut().zip(color) {
        *background =
            ((u16::from(foreground) * alpha + u16::from(*background) * (255 - alpha) + 127) / 255)
                as u8;
    }
}

fn rect_json(rect: Rect) -> String {
    format!(
        "{{\"x\":{},\"y\":{},\"width\":{},\"height\":{}}}",
        rect.x, rect.y, rect.width, rect.height
    )
}

fn layout_json(width: u32, height: u32, boxes: &[LayoutBox], m1_profile: bool) -> String {
    let entries: Vec<_> = boxes.iter().map(|item| {
        let background = item.style.background_color().map(|color| json_string(&color.hex())).unwrap_or_else(|| "null".to_owned());
        format!("{{\"node\":{},\"tag\":{},\"border_box\":{},\"content_box\":{},\"clip\":{},\"background\":{}}}",
            item.node, json_string(&item.tag), rect_json(item.border_box), rect_json(item.content_box), rect_json(item.clip), background)
    }).collect();
    let text = if m1_profile {
        let lines = boxes
            .iter()
            .filter_map(|item| {
                item.inline.as_ref().map(|(inline, _)| {
                    format!(
                        "{{\"node\":{},\"inline\":{}}}",
                        item.node,
                        inline.canonical_json()
                    )
                })
            })
            .collect::<Vec<_>>();
        format!(",\"text\":[{}]", lines.join(","))
    } else {
        String::new()
    };
    format!(
        "{{\"viewport\":{{\"width\":{width},\"height\":{height}}},\"boxes\":[{}]{text}}}\n",
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

    fn properties_for<'a>(json: &'a str, id: &str) -> &'a str {
        let prefix = format!("\"id\":\"{id}\",\"properties\":{{");
        json.split_once(&prefix)
            .unwrap()
            .1
            .split_once('}')
            .unwrap()
            .0
    }

    #[test]
    fn computes_frozen_selector_and_recovery_properties_without_layout() {
        let doc = parse(include_str!("../fixtures/m1/full/selectors-v2.html")).unwrap();
        let output = inspect_styles(&doc, "").unwrap();
        for (id, property) in [
            ("a", "\"background-color\":\"#ff0000\""),
            ("b", "\"background-color\":\"#ff0000\""),
            ("c", "\"background-color\":\"#00ff00\""),
            ("s", "\"color\":\"#0000ff\""),
            ("p", "\"color\":\"#008000\""),
        ] {
            assert!(properties_for(&output, id).contains(property));
        }

        let doc = parse(include_str!("../fixtures/m1/full/css-recovery.html")).unwrap();
        let output = inspect_styles(&doc, "").unwrap();
        let props = properties_for(&output, "box");
        assert!(props.contains("\"width_px\":10,\"height_px\":12"));
        assert!(props.contains("\"color\":\"#000000\",\"background-color\":\"#ff0000\""));
        assert!(!output.contains("unknown-prop"));
    }

    #[test]
    fn computes_user_and_author_cascade_levels_and_inline_specificity() {
        let doc = parse(include_str!("../fixtures/m1/full/cascade-origins.html")).unwrap();
        let user_css = "#target{color:#008000!important}";
        let output = inspect_styles(&doc, user_css).unwrap();
        assert!(
            properties_for(&output, "target")
                .starts_with("\"color\":\"#008000\",\"background-color\":\"#00ff00\"")
        );
        let output = inspect_styles(&doc, "#target{color:#008000}").unwrap();
        assert!(properties_for(&output, "target").starts_with("\"color\":\"#ff0000\""));

        let doc = parse(
            "<style>div{width:10px;color:red!important}</style>\
             <div id=target style='height:7px;color:blue!important'></div>",
        )
        .unwrap();
        let output = inspect_styles(
            &doc,
            "#target { width:4px;height:2px; color:orange!important; }\
             #target { color:green!important }",
        )
        .unwrap();
        let props = properties_for(&output, "target");
        assert!(props.contains("\"width_px\":10,\"height_px\":7"));
        assert!(props.starts_with("\"color\":\"#008000\""));
        let output = inspect_styles(&doc, "").unwrap();
        assert!(properties_for(&output, "target").starts_with("\"color\":\"#0000ff\""));
    }

    #[test]
    fn inherits_computed_values_and_applies_initial_and_unset_defaults() {
        let doc = parse(
            "<body id=body style='color:red;background:currentcolor;width:9px;height:11px;\
                 margin:initial;padding:3px 2px;border:4px solid currentcolor;overflow:hidden'>\
             <div id=inherited style='color:blue;background:inherit;width:inherit;min-width:nope;\
                 padding:inherit;border:inherit;overflow:inherit'></div>\
             <div id=defaults style='color:unset;width:unset;background:unset;border:unset;\
                 padding:initial;display:initial'></div>\
             <div hidden id=overridden></div>\
             </body>",
        )
        .unwrap();
        let output = inspect_styles(&doc, "#overridden{display:block}").unwrap();
        let parent = properties_for(&output, "body");
        assert!(parent.contains("\"margin_px\":[0, 0, 0, 0]"));
        let inherited = properties_for(&output, "inherited");
        assert!(inherited.starts_with(
            "\"color\":\"#0000ff\",\"background-color\":\"#0000ff\",\"width_px\":9,\"height_px\":null"
        ));
        assert!(inherited.contains(
            "\"overflow\":\"hidden\",\"border-color\":\"#0000ff\",\"border-style\":\"solid\""
        ));
        assert!(inherited.contains("\"padding_px\":[3, 2, 3, 2],\"border_width_px\":[4, 4, 4, 4]"));
        let defaults = properties_for(&output, "defaults");
        assert!(defaults.starts_with("\"color\":\"#ff0000\",\"background-color\":\"transparent\",\"width_px\":null,\"height_px\":null,\"display\":\"inline\""));
        assert!(defaults.contains("\"border-style\":\"none\""));
        assert!(defaults.contains("\"padding_px\":[0, 0, 0, 0],\"border_width_px\":[0, 0, 0, 0]"));
        let overridden = properties_for(&output, "overridden");
        assert!(overridden.contains("\"display\":\"block\""));

        let out = render(
            &parse(
                "<div style='color:red;width:1px'><div style='color:initial;width:inherit;\
             height:1px;background:currentcolor'></div></div>",
            )
            .unwrap(),
            2,
            2,
        )
        .unwrap();
        assert_eq!(pixel(&out, 2, 2, 0, 0), [0, 0, 0]);
        assert_eq!(pixel(&out, 2, 2, 1, 0), [255, 255, 255]);
    }

    #[test]
    fn checks_user_sheet_syntax_and_the_aggregate_css_budget() {
        let doc = parse("<div style='width:1px'></div>").unwrap();
        let inline_bytes = "width:1px".len();
        let css = format!("/*{}*/", "a".repeat(MAX_CSS_BYTES - inline_bytes - 4));
        assert!(inspect_styles(&doc, &css).is_ok());
        assert!(
            inspect_styles(&doc, &(css + " "))
                .unwrap_err()
                .contains("256 KiB")
        );
        assert!(
            inspect_styles(&doc, "@import 'local.css';")
                .unwrap_err()
                .contains("at-rule")
        );
        assert!(
            inspect_styles(&doc, "div{color:revert}")
                .unwrap_err()
                .contains("revert")
        );
    }

    #[test]
    fn computes_bounded_inherited_typography_and_html_direction() {
        let doc = parse(
            "<div dir=rtl style='font-family:\"Noto Sans Arabic\";font-size:24px;line-height:28px'>\
             <p id=inherit style='font-size:unset; white-space:normal'></p>\
             <p id=reset dir=rtl style='font-family:initial;font-size:initial;line-height:normal;direction:ltr'></p>\
             </div>",
        )
        .unwrap();
        let output = inspect_styles(&doc, "").unwrap();
        let inherited = properties_for(&output, "inherit");
        assert!(inherited.contains(
            "\"font-family\":\"Noto Sans Arabic\",\"font_size_px\":24,\"line_height_px\":28,\"direction\":\"rtl\",\"white-space\":\"normal\""
        ));
        let reset = properties_for(&output, "reset");
        assert!(reset.contains(
            "\"font-family\":\"Noto Sans\",\"font_size_px\":16,\"line_height_px\":null,\"direction\":\"ltr\""
        ));
        let doc =
            parse("<div style='font-family:Noto Sans Devanagari;font-size:256px' id=max></div>")
                .unwrap();
        let output = inspect_styles(&doc, "").unwrap();
        assert!(
            properties_for(&output, "max")
                .contains("\"font-family\":\"Noto Sans Devanagari\",\"font_size_px\":256")
        );
        for (css, diagnostic) in [
            ("font-family:Arial", "unsupported pinned font family"),
            ("font-family:'Noto Sans',serif", "one pinned Noto family"),
            ("font-size:257px", "in 1..=256 CSS pixels"),
            ("font-size:150%", "integer px"),
            ("line-height:1.2", "integer CSS pixels or normal"),
            ("white-space:pre", "unsupported bootstrap value"),
            ("white-space:preserve nowrap", "unsupported bootstrap value"),
        ] {
            let doc = parse(&format!("<div style=\"{css}\"></div>")).unwrap();
            assert!(
                inspect_styles(&doc, "").unwrap_err().contains(diagnostic),
                "{css}"
            );
        }
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
    fn sibling_selectors_follow_element_order_within_a_parent() {
        let out = output(
            "<style>body{margin:0}div{height:1px}.marker{display:none}
             .marker ~ div{background:red}.marker+div{background:blue}</style>
             <body><section><div class=marker></div> \n<!-- gap -->
             <div></div><div></div></section><div></div><div class=marker></div></body>",
            4,
            4,
        );
        assert_eq!(pixel(&out, 4, 4, 0, 0), [0, 0, 255]);
        assert_eq!(pixel(&out, 4, 4, 0, 1), [255, 0, 0]);
        assert_eq!(pixel(&out, 4, 4, 0, 2), [255, 255, 255]);
    }

    #[test]
    fn mixed_combinators_preserve_earlier_matches_and_cascade_specificity() {
        let out = output(
            "<style>body{margin:0}.target{height:1px;background:red}
             .anchor + div~section>article .target{background:green}
             section .target{background:blue}</style>
             <body><div class=anchor></div><div></div><div></div>
             <section><article><main><div class=target></div></main></article></section>
             <section><main><article><div class=target></div></article></main></section></body>",
            4,
            4,
        );
        assert_eq!(pixel(&out, 4, 4, 0, 0), [0, 128, 0]);
        assert_eq!(pixel(&out, 4, 4, 0, 1), [0, 0, 255]);

        let out = output(
            "<style>body{margin:0}main>div.item{height:1px;background:red}
             div+div{background:green}div.item+div.item{background:blue}</style>
             <body><main><div class=item></div><div class=item></div></main></body>",
            4,
            4,
        );
        assert_eq!(pixel(&out, 4, 4, 0, 0), [255, 0, 0]);
        assert_eq!(pixel(&out, 4, 4, 0, 1), [0, 0, 255]);

        let out = output(
            "<style>body{margin:0}main>div.item{height:1px;background:red}
             div+div{background:green}</style>
             <body><main><div class=item></div><div class=item></div></main></body>",
            4,
            4,
        );
        assert_eq!(pixel(&out, 4, 4, 0, 1), [255, 0, 0]);
    }

    #[test]
    fn sibling_matching_uses_child_order_after_reordering() {
        let mut doc = parse(
            "<style>div{height:1px}.a+.b{background:red}</style>
             <main><div class=a></div><div class=b></div></main>",
        )
        .unwrap();
        let main = doc
            .nodes
            .iter_mut()
            .find(|node| matches!(&node.kind, NodeKind::Element { name, .. } if name == "main"));
        main.unwrap().children.reverse();
        let out = render(&doc, 4, 4).unwrap();
        assert_eq!(pixel(&out, 4, 4, 0, 0), [255, 255, 255]);
        assert_eq!(pixel(&out, 4, 4, 0, 1), [255, 255, 255]);
    }

    #[test]
    fn selector_lists_keep_the_highest_specificity_that_matches() {
        let out = output(
            "<style>div{height:1px}div, #marker+div{background:blue}
             .target{background:red}</style>
             <main><section id=marker></section><div class=target></div>
             <div class=target></div></main>",
            4,
            4,
        );
        assert_eq!(pixel(&out, 4, 4, 0, 0), [0, 0, 255]);
        assert_eq!(pixel(&out, 4, 4, 0, 1), [255, 0, 0]);
    }

    #[test]
    fn rejects_malformed_sibling_combinators() {
        for selector in ["+div", "div~", "div+~div", "div >+ div"] {
            let input = format!("<style>{selector}{{height:1px}}</style><div></div>");
            let err = render(&parse(&input).unwrap(), 4, 4).err().unwrap();
            assert!(err.contains("combinator"), "{selector}: {err}");
        }
    }

    #[test]
    fn long_sibling_chain_at_node_limit_and_selector_boundary() {
        let selector = format!("div{}", "~div".repeat(MAX_SELECTOR_COMPOUNDS - 1));
        let mut input =
            format!("<style>div{{height:1px}}{selector}{{background:red}}</style><main>");
        input.push_str(&"<div></div>".repeat(MAX_NODES - 4));
        input.push_str("</main>");
        let height = MAX_SELECTOR_COMPOUNDS as u32 + 1;
        let out = output(&input, 4, height);
        assert_eq!(
            pixel(&out, 4, height, 0, MAX_SELECTOR_COMPOUNDS - 2),
            [255, 255, 255]
        );
        assert_eq!(
            pixel(&out, 4, height, 0, MAX_SELECTOR_COMPOUNDS - 1),
            [255, 0, 0]
        );

        let over = format!("<style>{selector}~div{{height:1px}}</style><div></div>");
        let err = render(&parse(&over).unwrap(), 4, 4).err().unwrap();
        assert!(
            err.contains("selector exceeds 16 compound selectors"),
            "{err}"
        );
    }

    #[test]
    fn sibling_selector_budget_counts_every_selector_in_a_rule() {
        let element_count = MAX_NODES / 2;
        let at_limit = vec![".missing~div"; MAX_RULES].join(",");
        let nodes = "<div></div>".repeat(element_count - 2); // Plus style and main.
        let input = format!("<style>{at_limit}{{height:1px}}</style><main>{nodes}</main>");
        assert!(render(&parse(&input).unwrap(), 4, 4).is_ok());

        let input =
            format!("<style>{at_limit},.missing~div{{height:1px}}</style><main>{nodes}</main>");
        let err = render(&parse(&input).unwrap(), 4, 4).err().unwrap();
        assert!(
            err.contains(&format!(
                "sibling selectors exceed {MAX_SIBLING_SELECTOR_CHECKS} matching checks"
            )),
            "{err}"
        );
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
    fn declared_png_scales_clips_and_composites_straight_alpha() {
        let mut encoded = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut encoded, 1, 1);
            encoder.set_color(png::ColorType::Rgba);
            let mut output = encoder.write_header().unwrap();
            output.write_image_data(&[255, 0, 0, 128]).unwrap();
            output.finish().unwrap();
        }
        let mut resources = PngResources::default();
        resources.insert("red.png", &encoded).unwrap();
        let input = "<div style='width:2px;height:1px;overflow:hidden;background:blue'><div hidden></div><img src='red.png' width='3'><span style='display:none'></span></div>";
        let out = render_with_resources(&parse(input).unwrap(), 4, 3, &resources).unwrap();
        assert_eq!(pixel(&out, 4, 3, 0, 0), [128, 0, 127]);
        assert_eq!(pixel(&out, 4, 3, 1, 0), [128, 0, 127]);
        assert_eq!(pixel(&out, 4, 3, 2, 0), [255, 255, 255]);
        assert_eq!(pixel(&out, 4, 3, 0, 1), [255, 255, 255]);
        assert!(
            out.layout_json
                .contains("\"content_box\":{\"x\":0,\"y\":0,\"width\":3,\"height\":3}")
        );
        let input = "<div><img src='red.png' width='3' style='width:auto'></div>";
        let auto = render_with_resources(&parse(input).unwrap(), 4, 3, &resources).unwrap();
        assert_eq!(pixel(&auto, 4, 3, 1, 0), [255, 255, 255]);
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
    fn supported_box_control_and_recognized_unsupported_properties() {
        let box_style = "width:2px;height:2px;background:red";
        let control = output(&format!("<div style='{box_style}'></div>"), 4, 4);
        assert_eq!(pixel(&control, 4, 4, 1, 1), [255, 0, 0]);
        assert_eq!(pixel(&control, 4, 4, 2, 1), [255, 255, 255]);
        for (declaration, property) in [
            ("opacity:0", "opacity"),
            ("position:relative;left:2px", "position"),
            ("min-width:3px", "min-width"),
            ("visibility:hidden", "visibility"),
            ("border-radius:1px", "border-radius"),
        ] {
            let doc = parse(&format!("<div style='{box_style};{declaration}'></div>")).unwrap();
            let error = render(&doc, 4, 4).err().unwrap();
            assert_eq!(
                error,
                format!("unsupported bootstrap CSS property: {property}")
            );
        }
    }

    #[test]
    fn comments_preserve_selector_boundaries_and_quoted_components() {
        let out = output(
            r#"<style>
                div { height:1px; }
                div.card/**/.active { unused:"} ; /* "; background:red; }
                div.card /**/.active { unused:fn([";"] {"}"}); background:blue; }
                </style>
                <div class="card active"></div>
                <div class="card" style="height:auto"><div class="active" style="margin-top:1px"></div></div>
                <div style='unused:"; } /* escaped \" quote";background:green'></div>"#,
            2,
            5,
        );
        assert_eq!(pixel(&out, 2, 5, 0, 0), [255, 0, 0]);
        assert_eq!(pixel(&out, 2, 5, 0, 1), [255, 255, 255]);
        assert_eq!(pixel(&out, 2, 5, 0, 2), [0, 0, 255]);
        assert_eq!(pixel(&out, 2, 5, 0, 3), [0, 128, 0]);
        let doc = parse("<style>di/**/v{height:1px}</style><div></div>").unwrap();
        assert_eq!(
            render(&doc, 2, 2).err().unwrap(),
            "adjacent selector identifiers require real whitespace"
        );
    }

    #[test]
    fn invalid_values_recover_without_accepting_valid_unsupported_lengths() {
        let out = output(
            "<style>div{display:block;width:2px;width:noise2;width:3px;\
             width:4/**/px;display:nonsense;background:red;background-color:#+f00ff;\
             position:nonsense;opacity:mystery;height:2px}</style><div></div>",
            4,
            4,
        );
        assert_eq!(pixel(&out, 4, 4, 2, 1), [255, 0, 0]);
        assert_eq!(pixel(&out, 4, 4, 3, 1), [255, 255, 255]);
        for (value, expected) in [
            ("50%", "bootstrap lengths require integer px values: 50%"),
            ("1.5px", "bootstrap length is not an integer: 1.5px"),
            ("16385px", "CSS length exceeds 16384px limit"),
        ] {
            let doc = parse(&format!("<div style='width:2px;width:{value}'></div>")).unwrap();
            assert_eq!(render(&doc, 4, 4).err().unwrap(), expected);
        }
        let doc =
            parse("<div style='background:red url(local.png) center / cover'></div>").unwrap();
        assert!(
            render(&doc, 4, 4)
                .err()
                .unwrap()
                .contains("unsupported complex background")
        );
    }

    #[test]
    fn recognizes_fixed_named_colors_and_rejects_system_colors() {
        let painted = output("<div style='height:1px;background:HotPink'></div>", 2, 2);
        assert_eq!(pixel(&painted, 2, 2, 0, 0), [255, 105, 180]);
        let doc = parse("<div style='height:1px;background:CanvasText'></div>").unwrap();
        assert_eq!(
            render(&doc, 2, 2).err().unwrap(),
            "unsupported bootstrap color value: canvastext"
        );
    }

    #[test]
    fn stylesheet_type_and_media_control_applicability() {
        for (attributes, expected) in [
            ("type='text/plain'", [0, 0, 255]),
            ("media=print", [0, 0, 255]),
            ("media='screen'", [255, 0, 0]),
            ("media='print, SCREEN'", [255, 0, 0]),
            ("type='TEXT/CSS'", [255, 0, 0]),
        ] {
            let input = format!(
                "<style>div{{height:1px;background:blue}}</style><style {attributes}>div{{background:red}}</style><div></div>"
            );
            let out = output(&input, 2, 2);
            assert_eq!(pixel(&out, 2, 2, 0, 0), expected, "{attributes}");
        }
        let doc =
            parse("<style media='screen and (min-width:1px)'>div{height:1px}</style><div></div>")
                .unwrap();
        assert!(
            render(&doc, 2, 2)
                .err()
                .unwrap()
                .contains("unsupported bootstrap stylesheet media query")
        );
    }

    #[test]
    fn border_shorthand_resets_all_longhands_and_resolves_current_color() {
        let out = output(
            "<div style='width:2px;height:2px;border-style:solid;background:red'></div>",
            10,
            10,
        );
        assert!(
            out.layout_json
                .contains("\"border_box\":{\"x\":0,\"y\":0,\"width\":8,\"height\":8}")
        );
        assert_eq!(pixel(&out, 10, 10, 2, 3), [0, 0, 0]);
        assert_eq!(pixel(&out, 10, 10, 3, 3), [255, 0, 0]);
        assert_eq!(pixel(&out, 10, 10, 8, 3), [255, 255, 255]);

        let out = output(
            "<main style='color:blue'><div style='width:2px;height:2px;border:5px solid red;border:none;border-style:solid;border-right-width:1px;background:yellow'></div></main>",
            8,
            8,
        );
        assert!(
            out.layout_json
                .contains("\"border_box\":{\"x\":0,\"y\":0,\"width\":6,\"height\":8}")
        );
        assert_eq!(pixel(&out, 8, 8, 2, 3), [0, 0, 255]);
        assert_eq!(pixel(&out, 8, 8, 3, 3), [255, 255, 0]);
        assert_eq!(pixel(&out, 8, 8, 5, 3), [0, 0, 255]);
        assert_eq!(pixel(&out, 8, 8, 6, 3), [255, 255, 255]);
    }

    #[test]
    fn stylesheets_follow_tree_order_and_have_independent_boundaries() {
        let blue = "<style>div{height:1px;background:blue}</style>";
        let red = "<style>div{height:1px;background:red}</style>";
        let mut doc = parse(&format!("<main>{blue}{red}<div></div></main>")).unwrap();
        let main = doc.nodes[doc.root].children[0];
        let div = doc.nodes[main].children[2];
        let before = render(&doc, 2, 2).unwrap();
        assert_eq!(pixel(&before, 2, 2, 0, 0), [255, 0, 0]);
        doc.nodes[main].children.swap(0, 1);
        let after = render(&doc, 2, 2).unwrap();
        let fresh = output(&format!("<main>{red}{blue}<div></div></main>"), 2, 2);
        assert_eq!(doc.nodes[main].children[2], div);
        assert_eq!(
            before
                .layout_json
                .matches(&format!("\"node\":{div},"))
                .count(),
            1
        );
        assert_eq!(after.layout_json, fresh.layout_json);
        assert_eq!(after.ppm, fresh.ppm);
        assert_eq!(pixel(&after, 2, 2, 0, 0), [0, 0, 255]);
        let doc = parse("<style>div{height:1px</style><style>;background:red}</style><div></div>")
            .unwrap();
        assert_eq!(
            render(&doc, 2, 2).err().unwrap(),
            "stylesheet rule has no closing brace"
        );
    }

    #[test]
    fn html_hidden_state_changes_flow_and_author_display_can_override_it() {
        let out = output(
            "<div hidden style='height:2px;background:red'>inert</div><div hidden style='display:block;height:1px;background:blue'></div><div style='height:1px;background:green'></div>",
            3,
            3,
        );
        assert_eq!(pixel(&out, 3, 3, 0, 0), [0, 0, 255]);
        assert_eq!(pixel(&out, 3, 3, 0, 1), [0, 128, 0]);
        assert_eq!(pixel(&out, 3, 3, 0, 2), [255, 255, 255]);
        assert_eq!(out.layout_json.matches("\"tag\":\"div\"").count(), 2);
        let doc = parse("<div hidden=until-found></div>").unwrap();
        assert_eq!(
            render(&doc, 2, 2).err().unwrap(),
            "hidden until-found is outside the bootstrap renderer"
        );
    }

    #[test]
    fn html_root_and_body_propagate_solid_canvas_backgrounds() {
        let body = "<body style='margin:1px;width:1px;height:1px;background:red'></body>";
        let out = output(&format!("<html>{body}</html>"), 3, 3);
        assert_eq!(pixel(&out, 3, 3, 1, 1), [255, 0, 0]);
        assert_eq!(pixel(&out, 3, 3, 2, 2), [255, 0, 0]);
        let out = output(
            &format!("<html style='width:2px;height:2px;background:blue'>{body}</html>"),
            3,
            3,
        );
        assert_eq!(pixel(&out, 3, 3, 1, 1), [255, 0, 0]);
        assert_eq!(pixel(&out, 3, 3, 2, 2), [0, 0, 255]);
    }

    #[test]
    fn specificity_and_adjacent_controls_can_both_win_where_required() {
        let out = output(
            "<style>div{height:1px}main>div.item{background:red}div+div{background:green}</style><main><div class=item></div><div class=item></div><div></div></main>",
            2,
            4,
        );
        assert_eq!(pixel(&out, 2, 4, 0, 1), [255, 0, 0]);
        assert_eq!(pixel(&out, 2, 4, 0, 2), [0, 128, 0]);
    }

    #[test]
    fn all_effective_selectors_count_toward_match_budget() {
        let count = 5_000; // Style, main and the 4,998 div elements.
        let divs = "<div></div>".repeat(count - 2);
        let maximum = MAX_SELECTOR_CHECKS / count;
        for selectors in [maximum - 1, maximum, maximum + 1] {
            let list = vec![".missing"; selectors].join(",");
            let input = format!("<style>{list}{{height:1px}}</style><main>{divs}</main>");
            let result = render(&parse(&input).unwrap(), 2, 2);
            if selectors <= maximum {
                assert!(result.is_ok());
            } else {
                assert_eq!(
                    result.err().unwrap(),
                    format!("selectors exceed {MAX_SELECTOR_CHECKS} matching checks")
                );
                let input =
                    format!("<style>{list}{{unknown-property:1}}</style><main>{divs}</main>");
                assert!(render(&parse(&input).unwrap(), 2, 2).is_ok());
            }
        }
    }

    #[test]
    fn matched_declarations_have_a_separate_cascade_budget() {
        let count = 5_000;
        let divs = "<div></div>".repeat(count - 2);
        let maximum = MAX_CASCADE_CANDIDATES / count;
        for declarations in [maximum, maximum + 1] {
            let css = "height:1px;".repeat(declarations);
            let input = format!("<style>*{{{css}}}</style><main>{divs}</main>");
            let result = render(&parse(&input).unwrap(), 2, 2);
            if declarations == maximum {
                assert!(result.is_ok());
            } else {
                assert_eq!(
                    result.err().unwrap(),
                    format!("cascade exceeds {MAX_CASCADE_CANDIDATES} declaration candidates")
                );
            }
        }
    }

    #[test]
    fn clipped_fill_budget_checks_below_at_and_above_before_paint() {
        let css = "<style>div{width:2048px;height:2048px;background:#123}div+div{margin-top:-2048px}</style>";
        let seven = "<div></div>".repeat(7);
        let below = output(
            &format!("{css}{seven}<div style='width:2047px;background:red'></div>"),
            2048,
            2048,
        );
        assert_eq!(pixel(&below, 2048, 2048, 2046, 2047), [255, 0, 0]);
        assert_eq!(pixel(&below, 2048, 2048, 2047, 2047), [17, 34, 51]);
        let at_input = format!("{css}{seven}<div style='background:red'></div>");
        let at = output(&at_input, 2048, 2048);
        assert_eq!(pixel(&at, 2048, 2048, 2047, 2047), [255, 0, 0]);
        let above = parse(&format!(
            "{at_input}<div style='width:1px;height:1px'></div>"
        ))
        .unwrap();
        assert_eq!(
            render(&above, 2048, 2048).err().unwrap(),
            format!("paint exceeds {MAX_PAINT_PIXELS} clipped pixel writes")
        );
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
