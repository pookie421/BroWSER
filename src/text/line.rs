use super::{Direction, MAX_TEXT_BYTES, PinnedFont, ShapedRun};

// Charge every byte given to the shaper while probing line boundaries. This
// bounds repeated shaping of growing candidates on very wide lines.
const MAX_WRAP_WORK_BYTES: usize = 512 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WrappedText {
    pub collapsed_text: String,
    pub lines: Vec<ShapedRun>,
    /// Total bytes shaped while determining line boundaries.
    pub shaped_bytes: usize,
}

pub(super) fn wrap(
    font: PinnedFont,
    input: &str,
    direction: Direction,
    language: &str,
    scale: u32,
    max_width: i64,
) -> Result<WrappedText, String> {
    if input.len() > MAX_TEXT_BYTES {
        return Err(format!(
            "text exceeds {MAX_TEXT_BYTES} bytes per wrapped run"
        ));
    }
    if max_width < 0 {
        return Err("line width cannot be negative".to_owned());
    }
    let mut collapsed_text = String::with_capacity(input.len());
    let mut word_separator = false;
    for ch in input.chars() {
        if matches!(ch, ' ' | '\t' | '\r' | '\n' | '\x0c') {
            word_separator = !collapsed_text.is_empty();
            continue;
        }
        if word_separator {
            collapsed_text.push(' ');
            word_separator = false;
        }
        collapsed_text.push(ch);
    }
    let (lines, shaped_bytes) = font.with_shaper(direction, language, scale, |shaper| {
        let mut lines = Vec::new();
        let mut start = 0;
        let mut pending = None::<(usize, ShapedRun)>;
        let mut work = 0;
        let mut shape_candidate = |text: &str| {
            work += text.len();
            if work > MAX_WRAP_WORK_BYTES {
                return Err(format!(
                    "line wrapping exceeds {MAX_WRAP_WORK_BYTES} shaped bytes"
                ));
            }
            shaper.shape(text)
        };
        let boundaries = collapsed_text
            .match_indices(' ')
            .map(|(index, _)| index)
            .chain((!collapsed_text.is_empty()).then_some(collapsed_text.len()));
        for end in boundaries {
            let mut candidate = shape_candidate(&collapsed_text[start..end])?;
            if candidate.advance() > max_width
                && let Some((previous_end, previous)) = pending.take()
            {
                lines.push(previous);
                start = previous_end + 1;
                candidate = shape_candidate(&collapsed_text[start..end])?;
            }
            pending = Some((end, candidate));
        }
        if let Some((_, last)) = pending {
            lines.push(last);
        }
        Ok((lines, work))
    })?;

    Ok(WrappedText {
        collapsed_text,
        lines,
        shaped_bytes,
    })
}
