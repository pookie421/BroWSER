//! Declaration recovery, bootstrap value classification and shorthand expansion.

use super::colors::{color_value, unsupported_color};
use super::{Token, number_end, tokens};
use crate::render::{Declaration, MAX_DECLARATIONS, MAX_LENGTH, MEDIUM_BORDER_WIDTH};

pub(in crate::render) fn parse_declarations(
    text: &str,
    order: &mut usize,
) -> Result<Vec<Declaration>, String> {
    let tokens = tokens(text)?;
    let mut out = Vec::new();
    for declaration in tokens.split(|token| *token == Token::Delimiter(b';')) {
        let declaration: Vec<_> = declaration
            .iter()
            .copied()
            .filter(|token| *token != Token::Space)
            .collect();
        let [Token::Name(name), Token::Delimiter(b':'), values @ ..] = declaration.as_slice()
        else {
            if declaration
                .iter()
                .any(|token| matches!(token, Token::Block(b'{', _)))
            {
                return Err("nested rules are outside the bootstrap CSS grammar".to_owned());
            }
            // Malformed declarations do not invalidate their neighbors.
            continue;
        };
        if name.contains('\\') {
            return Err("escaped property names are outside the bootstrap CSS grammar".to_owned());
        }
        let name = name.to_ascii_lowercase();
        if !known_property(&name) {
            continue;
        }
        let (important, values) = match values {
            [values @ .., Token::Delimiter(b'!'), Token::Name(important)]
                if important.eq_ignore_ascii_case("important") =>
            {
                (true, values)
            }
            _ => (false, values),
        };
        let parts: Vec<_> = values
            .iter()
            .map(|token| match token {
                Token::Name(value)
                | Token::Number(value)
                | Token::Hash(value)
                | Token::String(value)
                | Token::Function(value) => value.to_ascii_lowercase(),
                Token::Block(delimiter, _) | Token::Delimiter(delimiter) => {
                    char::from(*delimiter).to_string()
                }
                Token::Space => unreachable!("filtered declaration whitespace"),
            })
            .collect();
        if parts.is_empty() {
            continue;
        }
        let values: Vec<_> = parts.iter().map(String::as_str).collect();
        for (expanded_name, expanded_value) in expand_declaration(&name, &values)? {
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

fn expand_declaration(name: &str, values: &[&str]) -> Result<Vec<(String, String)>, String> {
    if matches!(
        values,
        ["initial" | "inherit" | "unset" | "revert" | "revert-layer"]
    ) || values
        .iter()
        .any(|part| part.starts_with("var(") || part.contains('\\'))
    {
        return Err(format!(
            "unsupported bootstrap value for {name}: {}",
            values.join(" ")
        ));
    }
    let mut out = Vec::new();
    if matches!(name, "margin" | "padding" | "border-width") {
        if values.len() <= 4 {
            return expand_edge_lengths(name, values);
        }
        return Ok(out);
    }
    if name == "border" {
        return expand_border(values);
    }
    if ["border-color", "border-style", "overflow"].contains(&name) && values.len() > 1 {
        let (limit, valid): (usize, fn(&str) -> bool) = match name {
            "border-color" => (4, |value| {
                color_value(value).is_ok_and(|color| color.is_some()) || unsupported_color(value)
            }),
            "border-style" => (4, border_style),
            _ => (2, overflow_value),
        };
        if values.len() <= limit && values.iter().copied().all(valid) {
            return Err(format!(
                "unsupported bootstrap value for {name}: {}",
                values.join(" ")
            ));
        }
        return Ok(out);
    }
    let [value] = values else {
        if name == "display" && multi_keyword_display(values) {
            return Err(format!(
                "unsupported bootstrap value for {name}: {}",
                values.join(" ")
            ));
        }
        if image_property(name) && values.iter().any(|value| image_value(value)) {
            return Err(format!(
                "unsupported resource or image value for {name}: bootstrap paints solid colors only"
            ));
        }
        if name == "background" && values.iter().any(|value| background_component(value)) {
            return Err(
                "unsupported complex background value: bootstrap paints solid colors only"
                    .to_owned(),
            );
        }
        if unsupported_property(name) && valid_unsupported_property(name, values) {
            return Err(format!("unsupported bootstrap CSS property: {name}"));
        }
        return Ok(out);
    };
    let mut emit = |property: &str, value: &str| out.push((property.to_owned(), value.to_owned()));
    let unsupported = || Err(format!("unsupported bootstrap value for {name}: {value}"));
    match name {
        "background-image"
        | "border-image"
        | "border-image-source"
        | "list-style-image"
        | "mask-image" => {
            if *value != "none" && (image_value(value) || name == "border-image") {
                return Err(format!(
                    "unsupported resource or image value for {name}: bootstrap paints solid colors only"
                ));
            }
        }
        "background" | "background-color" | "border-color" | "color" => {
            if color_value(value)?.is_some() {
                emit(
                    if name == "background" {
                        "background-color"
                    } else {
                        name
                    },
                    value,
                );
            } else if name == "background" && *value == "none" {
                emit("background-color", "transparent");
            } else if name == "background" && background_component(value) {
                return unsupported();
            }
        }
        "display" => match *value {
            "block" | "none" => emit(name, value),
            "inline" | "inline-block" | "flow-root" | "contents" | "list-item" | "table"
            | "inline-table" | "table-row-group" | "table-header-group" | "table-footer-group"
            | "table-row" | "table-cell" | "table-column-group" | "table-column"
            | "table-caption" | "flex" | "inline-flex" | "grid" | "inline-grid" | "ruby"
            | "ruby-base" | "ruby-text" => return unsupported(),
            _ => {}
        },
        "overflow" => {
            if matches!(*value, "visible" | "hidden") {
                emit(name, value);
            } else if overflow_value(value) {
                return unsupported();
            }
        }
        "box-sizing" => {
            if *value == "border-box" {
                return unsupported();
            }
        }
        "border-style" => {
            if matches!(*value, "solid" | "none") {
                emit(name, value);
            } else if border_style(value) {
                return unsupported();
            }
        }
        "width" | "height" => match *value {
            "auto" => emit(name, value),
            "min-content" | "max-content" | "fit-content" | "stretch" => return unsupported(),
            _ => {
                if parse_length(value, false)?.is_some() {
                    emit(name, value);
                }
            }
        },
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
        | "border-left-width" => {
            if edge_length(name, value)?.is_some() {
                emit(name, value);
            }
        }
        _ if valid_unsupported_property(name, values) => {
            return Err(format!("unsupported bootstrap CSS property: {name}"));
        }
        _ => {}
    }
    Ok(out)
}

fn known_property(name: &str) -> bool {
    matches!(
        name,
        "background-image"
            | "border-image"
            | "border-image-source"
            | "list-style-image"
            | "mask-image"
            | "background"
            | "background-color"
            | "border-color"
            | "color"
            | "display"
            | "overflow"
            | "box-sizing"
            | "width"
            | "height"
            | "margin"
            | "padding"
            | "border-width"
            | "border"
            | "border-style"
            | "margin-top"
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
    ) || unsupported_property(name)
}

fn unsupported_property(name: &str) -> bool {
    matches!(
        name,
        "opacity"
            | "all"
            | "zoom"
            | "visibility"
            | "position"
            | "left"
            | "right"
            | "top"
            | "bottom"
            | "inset"
            | "inset-block"
            | "inset-inline"
            | "inset-block-start"
            | "inset-block-end"
            | "inset-inline-start"
            | "inset-inline-end"
            | "min-width"
            | "max-width"
            | "min-height"
            | "max-height"
            | "aspect-ratio"
            | "block-size"
            | "inline-size"
            | "min-block-size"
            | "max-block-size"
            | "min-inline-size"
            | "max-inline-size"
            | "float"
            | "clear"
            | "z-index"
            | "transform"
            | "transform-origin"
            | "translate"
            | "rotate"
            | "scale"
            | "filter"
            | "backdrop-filter"
            | "clip"
            | "clip-path"
            | "mask"
            | "mix-blend-mode"
            | "isolation"
            | "box-shadow"
            | "text-shadow"
            | "overflow-x"
            | "overflow-y"
            | "resize"
            | "scrollbar-width"
            | "scrollbar-color"
            | "border-radius"
            | "border-top-left-radius"
            | "border-top-right-radius"
            | "border-bottom-left-radius"
            | "border-bottom-right-radius"
            | "border-top"
            | "border-right"
            | "border-bottom"
            | "border-left"
            | "border-top-color"
            | "border-right-color"
            | "border-bottom-color"
            | "border-left-color"
            | "border-top-style"
            | "border-right-style"
            | "border-bottom-style"
            | "border-left-style"
            | "border-image-slice"
            | "border-image-width"
            | "border-image-outset"
            | "border-image-repeat"
            | "outline"
            | "outline-color"
            | "outline-offset"
            | "outline-style"
            | "outline-width"
            | "background-clip"
            | "background-origin"
            | "background-position"
            | "background-position-x"
            | "background-position-y"
            | "background-size"
            | "background-repeat"
            | "background-attachment"
            | "background-blend-mode"
            | "margin-inline"
            | "margin-inline-start"
            | "margin-inline-end"
            | "margin-block"
            | "margin-block-start"
            | "margin-block-end"
            | "padding-inline"
            | "padding-inline-start"
            | "padding-inline-end"
            | "padding-block"
            | "padding-block-start"
            | "padding-block-end"
            | "writing-mode"
            | "direction"
            | "unicode-bidi"
            | "white-space"
            | "text-align"
            | "vertical-align"
            | "line-height"
            | "font"
            | "font-size"
            | "font-family"
            | "font-style"
            | "font-weight"
            | "font-stretch"
            | "letter-spacing"
            | "word-spacing"
            | "text-overflow"
            | "text-indent"
            | "text-transform"
            | "text-decoration"
            | "text-decoration-line"
            | "word-break"
            | "overflow-wrap"
            | "content"
            | "list-style"
            | "list-style-type"
            | "list-style-position"
            | "animation"
            | "animation-name"
            | "transition"
            | "transition-property"
            | "container"
            | "container-name"
            | "container-type"
            | "contain"
            | "content-visibility"
            | "columns"
            | "column-width"
            | "column-count"
            | "column-gap"
            | "column-span"
            | "column-rule"
            | "order"
            | "flex"
            | "flex-grow"
            | "flex-shrink"
            | "flex-basis"
            | "flex-flow"
            | "flex-direction"
            | "flex-wrap"
            | "align-content"
            | "align-items"
            | "align-self"
            | "justify-content"
            | "justify-items"
            | "justify-self"
            | "place-content"
            | "place-items"
            | "place-self"
            | "gap"
            | "row-gap"
            | "grid"
            | "grid-template"
            | "grid-template-rows"
            | "grid-template-columns"
            | "grid-template-areas"
            | "grid-auto-rows"
            | "grid-auto-columns"
            | "grid-auto-flow"
            | "grid-row"
            | "grid-column"
            | "grid-area"
    )
}

fn valid_unsupported_property(name: &str, values: &[&str]) -> bool {
    let math = |value: &str| {
        ["calc(", "min(", "max(", "clamp("]
            .iter()
            .any(|prefix| value.starts_with(prefix))
    };
    match name {
        "all" => false, // Valid CSS-wide keywords are handled before this check.
        "opacity" => {
            matches!(values, [value] if math(value) || number_end(value).is_some_and(|end| matches!(&value[end..], "" | "%")))
        }
        "position" => matches!(
            values,
            ["static" | "relative" | "absolute" | "fixed" | "sticky"]
        ),
        "visibility" => matches!(values, ["visible" | "hidden" | "collapse"]),
        "left" | "right" | "top" | "bottom" => {
            matches!(values, [value] if *value == "auto" || parse_length(value, true).map_or(true, |length| length.is_some()))
        }
        "min-width" | "max-width" | "min-height" | "max-height" => matches!(values, [value]
            if matches!(*value, "auto" | "none" | "min-content" | "max-content" | "fit-content" | "stretch")
                || parse_length(value, false).map_or(true, |length| length.is_some())),
        _ => true,
    }
}

fn multi_keyword_display(values: &[&str]) -> bool {
    let outside = |value| matches!(value, "block" | "inline" | "run-in");
    let inside = |value| {
        matches!(
            value,
            "flow" | "flow-root" | "table" | "flex" | "grid" | "ruby"
        )
    };
    if let [first, second] = values
        && (outside(*first) && inside(*second) || inside(*first) && outside(*second))
    {
        return true;
    }
    let outside_count = values.iter().filter(|value| outside(**value)).count();
    let flow_count = values
        .iter()
        .filter(|value| matches!(**value, "flow" | "flow-root"))
        .count();
    values.contains(&"list-item")
        && outside_count <= 1
        && flow_count <= 1
        && values.len() == outside_count + flow_count + 1
}

fn image_property(name: &str) -> bool {
    matches!(
        name,
        "background-image"
            | "border-image"
            | "border-image-source"
            | "list-style-image"
            | "mask-image"
    )
}

fn expand_edge_lengths(name: &str, values: &[&str]) -> Result<Vec<(String, String)>, String> {
    let expanded = match values {
        [all] => [all, all, all, all],
        [vertical, horizontal] => [vertical, horizontal, vertical, horizontal],
        [top, horizontal, bottom] => [top, horizontal, bottom, horizontal],
        [top, right, bottom, left] => [top, right, bottom, left],
        _ => return Ok(Vec::new()),
    };
    let mut out = Vec::with_capacity(4);
    let mut unsupported = None;
    for (side, value) in ["top", "right", "bottom", "left"].into_iter().zip(expanded) {
        let property = if name == "border-width" {
            format!("border-{side}-width")
        } else {
            format!("{name}-{side}")
        };
        match edge_length(&property, value) {
            Ok(Some(_)) => out.push((property, (*value).to_owned())),
            Ok(None) => return Ok(Vec::new()),
            Err(error) => unsupported = Some(error),
        }
    }
    if let Some(error) = unsupported {
        return Err(error);
    }
    Ok(out)
}

fn expand_border(values: &[&str]) -> Result<Vec<(String, String)>, String> {
    let mut width = None;
    let mut style = None;
    let mut color = None;
    let mut unsupported = None;
    for &value in values {
        if border_style(value) {
            if style.replace(value).is_some() {
                return Ok(Vec::new());
            }
        } else {
            match edge_length("border-width", value) {
                Ok(Some(_)) => {
                    if width.replace(value).is_some() {
                        return Ok(Vec::new());
                    }
                    continue;
                }
                Ok(None) => {}
                Err(error) => {
                    if width.replace(value).is_some() {
                        return Ok(Vec::new());
                    }
                    unsupported = Some(error);
                    continue;
                }
            }
            match color_value(value) {
                Ok(Some(_)) => {}
                Ok(None) => return Ok(Vec::new()),
                Err(error) => unsupported = Some(error),
            }
            if color.replace(value).is_some() {
                return Ok(Vec::new());
            }
        }
    }
    let style = style.unwrap_or("none");
    if !matches!(style, "solid" | "none") {
        return Err(format!("unsupported bootstrap border style: {style}"));
    }
    if let Some(error) = unsupported {
        return Err(error);
    }
    let mut out = expand_edge_lengths("border-width", &[width.unwrap_or("medium")])?;
    out.push(("border-style".to_owned(), style.to_owned()));
    out.push((
        "border-color".to_owned(),
        color.unwrap_or("currentcolor").to_owned(),
    ));
    Ok(out)
}

fn border_style(value: &str) -> bool {
    matches!(
        value,
        "none"
            | "hidden"
            | "solid"
            | "dotted"
            | "dashed"
            | "double"
            | "groove"
            | "ridge"
            | "inset"
            | "outset"
    )
}

fn overflow_value(value: &str) -> bool {
    matches!(
        value,
        "visible" | "hidden" | "clip" | "scroll" | "auto" | "overlay"
    )
}

fn image_value(value: &str) -> bool {
    [
        "url(",
        "src(",
        "image(",
        "image-set(",
        "cross-fade(",
        "element(",
        "paint(",
        "linear-gradient(",
        "radial-gradient(",
        "conic-gradient(",
        "repeating-linear-gradient(",
        "repeating-radial-gradient(",
        "repeating-conic-gradient(",
    ]
    .iter()
    .any(|prefix| value.starts_with(prefix))
}

fn background_component(value: &str) -> bool {
    image_value(value)
        || matches!(
            value,
            "none"
                | "scroll"
                | "fixed"
                | "local"
                | "left"
                | "right"
                | "top"
                | "bottom"
                | "center"
                | "repeat"
                | "no-repeat"
                | "repeat-x"
                | "repeat-y"
                | "round"
                | "space"
                | "padding-box"
                | "border-box"
                | "content-box"
                | "text"
                | "contain"
                | "cover"
        )
        || number_end(value).is_some_and(|end| unit(&value[end..]))
}

pub(in crate::render) fn edge_length(property: &str, value: &str) -> Result<Option<i64>, String> {
    if property.starts_with("border-") {
        match value {
            "thin" => return Ok(Some(1)),
            "medium" => return Ok(Some(MEDIUM_BORDER_WIDTH)),
            "thick" => return Ok(Some(5)),
            _ => {}
        }
    }
    if property.starts_with("margin-") && value == "auto" {
        return Err(format!("unsupported bootstrap value for {property}: auto"));
    }
    // Percentages are invalid for border widths but valid for margins and padding.
    if property.starts_with("border-") && value.ends_with('%') {
        return Ok(None);
    }
    parse_length(value, property.starts_with("margin-"))
}

fn unit(suffix: &str) -> bool {
    matches!(
        suffix,
        "%" | "px"
            | "em"
            | "rem"
            | "ex"
            | "rex"
            | "ch"
            | "rch"
            | "cap"
            | "rcap"
            | "ic"
            | "ric"
            | "lh"
            | "rlh"
            | "cm"
            | "mm"
            | "q"
            | "in"
            | "pt"
            | "pc"
            | "vw"
            | "vh"
            | "vi"
            | "vb"
            | "vmin"
            | "vmax"
            | "svw"
            | "svh"
            | "svi"
            | "svb"
            | "svmin"
            | "svmax"
            | "lvw"
            | "lvh"
            | "lvi"
            | "lvb"
            | "lvmin"
            | "lvmax"
            | "dvw"
            | "dvh"
            | "dvi"
            | "dvb"
            | "dvmin"
            | "dvmax"
            | "cqw"
            | "cqh"
            | "cqi"
            | "cqb"
            | "cqmin"
            | "cqmax"
    )
}

pub(in crate::render) fn parse_length(value: &str, negative: bool) -> Result<Option<i64>, String> {
    let Some(end) = number_end(value) else {
        if ["calc(", "min(", "max(", "clamp(", "fit-content("]
            .iter()
            .any(|prefix| value.starts_with(prefix))
        {
            return Err(format!(
                "bootstrap lengths require integer px values: {value}"
            ));
        }
        return Ok(None);
    };
    let (number, suffix) = value.split_at(end);
    let Ok(approximate) = number.parse::<f64>() else {
        return Ok(None);
    };
    if !negative && approximate < 0.0 || !unit(suffix) && !(suffix.is_empty() && approximate == 0.0)
    {
        return Ok(None);
    }
    if suffix == "px" && approximate.abs() > MAX_LENGTH as f64 {
        return Err(format!("CSS length exceeds {MAX_LENGTH}px limit"));
    }
    if !matches!(suffix, "" | "px") {
        return Err(format!(
            "bootstrap lengths require integer px values: {value}"
        ));
    }
    let length = number
        .parse::<i64>()
        .map_err(|_| format!("bootstrap length is not an integer: {value}"))?;
    Ok(Some(length))
}
