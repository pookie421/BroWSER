//! Single-script text lines and bounded glyph coverage for controlled documents.

use super::{Rect, TextStyle};
use crate::html::json_string;
use crate::text::{Direction, PinnedFont, ShapedRun};
use std::fmt::Write;

const SUBPIXELS: i64 = 64;
const MAX_INLINE_BYTES: usize = 65_536;
const MAX_INLINE_SHAPING_BYTES: usize = 2_097_152;
const MAX_INLINE_GLYPHS: usize = 16_384;
const MAX_GLYPH_COVERAGE_BYTES: usize = 16_777_216;

#[derive(Debug, Default)]
pub(super) struct InlineBudget {
    text_bytes: usize,
    shaping_bytes: usize,
    glyphs: usize,
}

#[derive(Debug, Default)]
pub(super) struct RasterBudget {
    glyphs: usize,
    coverage_bytes: usize,
}

#[derive(Debug)]
pub(super) struct InlineLayout {
    pub content_height: i64,
    collapsed_text: String,
    line_height: i64,
    lines: Vec<Line>,
}

#[derive(Debug)]
struct Line {
    x: i64,
    baseline: i64,
    run: ShapedRun,
}

#[derive(Debug)]
pub(super) struct PositionedGlyph {
    pub bounds: Rect,
    pub coverage: Vec<u8>,
}

fn charge(total: &mut usize, amount: usize, limit: usize, label: &str) -> Result<(), String> {
    if amount > limit - *total {
        return Err(format!("inline {label} exceeds the {limit} limit"));
    }
    *total += amount;
    Ok(())
}

pub(super) fn layout(
    text: &str,
    settings: TextStyle,
    language: Option<&str>,
    origin: (i64, i64),
    width: i64,
    budget: &mut InlineBudget,
) -> Result<InlineLayout, String> {
    charge(
        &mut budget.text_bytes,
        text.len(),
        MAX_INLINE_BYTES,
        "text bytes",
    )?;
    check_single_script(text, settings.font, settings.direction)?;
    let scale = settings.font_px * SUBPIXELS as u32;
    let wrapped = settings.font.wrap(
        text,
        settings.direction,
        language.unwrap_or_else(|| settings.font.default_language()),
        scale,
        width * SUBPIXELS,
    )?;
    charge(
        &mut budget.shaping_bytes,
        wrapped.shaped_bytes,
        MAX_INLINE_SHAPING_BYTES,
        "shaping work bytes",
    )?;
    charge(
        &mut budget.glyphs,
        wrapped.lines.iter().map(|run| run.glyphs.len()).sum(),
        MAX_INLINE_GLYPHS,
        "glyph count",
    )?;

    let metrics = settings.font.metrics(scale)?;
    let ascent = i64::from(metrics.ascent);
    let descent = i64::from(metrics.descent);
    let default_height =
        (ascent + descent + i64::from(metrics.line_gap).max(0) + SUBPIXELS - 1) / SUBPIXELS;
    let line_height = settings.line_px.map_or(default_height, i64::from);
    let baseline_in_line = ascent + (line_height * SUBPIXELS - ascent - descent).div_euclid(2);
    let (left, top) = origin;
    let lines: Vec<_> = wrapped
        .lines
        .into_iter()
        .enumerate()
        .map(|(index, run)| {
            let start = match settings.direction {
                Direction::Ltr => left * SUBPIXELS,
                Direction::Rtl => (left + width) * SUBPIXELS - run.advance(),
            };
            Line {
                x: start,
                baseline: (top + index as i64 * line_height) * SUBPIXELS + baseline_in_line,
                run,
            }
        })
        .collect();
    Ok(InlineLayout {
        collapsed_text: wrapped.collapsed_text,
        content_height: line_height * lines.len() as i64,
        line_height,
        lines,
    })
}

fn check_single_script(text: &str, font: PinnedFont, direction: Direction) -> Result<(), String> {
    // Restrict each face to a supported homogeneous input subset while the
    // renderer has one script and one visual direction for each shaped line.
    let natural_direction = match font {
        PinnedFont::Latin | PinnedFont::Devanagari => Direction::Ltr,
        PinnedFont::Arabic => Direction::Rtl,
    };
    let supported = text.chars().all(|character| {
        if matches!(character, ' ' | '\t' | '\n' | '\r' | '\x0c' | '\u{00a0}') {
            return true;
        }
        match font {
            PinnedFont::Latin => {
                character.is_ascii_graphic()
                    || matches!(character, '\u{00c0}'..='\u{024f}' | '\u{0300}'..='\u{036f}')
            }
            PinnedFont::Arabic => {
                matches!(character, '\u{0620}'..='\u{063f}' | '\u{0640}'..='\u{065f}' | '\u{0670}'..='\u{06d3}')
            }
            PinnedFont::Devanagari => {
                matches!(character, '\u{0900}'..='\u{097f}')
                    || (character.is_ascii_graphic() && !character.is_ascii_alphabetic())
            }
        }
    });
    if direction != natural_direction || !supported {
        return Err(format!(
            "inline layout requires a homogeneous {} {} run",
            font.script(),
            natural_direction.as_str()
        ));
    }
    Ok(())
}

impl InlineLayout {
    pub fn shift_vertical(&mut self, delta: i64) {
        for line in &mut self.lines {
            line.baseline += delta * SUBPIXELS;
        }
    }

    pub fn canonical_json(&self) -> String {
        let mut output = format!(
            "{{\"collapsed_text\":{},\"line_height_px\":{},\"content_height_px\":{},\"lines\":[",
            json_string(&self.collapsed_text),
            self.line_height,
            self.content_height,
        );
        for (index, line) in self.lines.iter().enumerate() {
            if index != 0 {
                output.push(',');
            }
            write!(
                output,
                "{{\"x_64\":{},\"baseline_64\":{},\"run\":{}}}",
                line.x,
                line.baseline,
                line.run.canonical_json(),
            )
            .expect("writing to a String");
        }
        output.push_str("]}");
        output
    }

    pub fn rasterize(
        &self,
        clip: Rect,
        budget: &mut RasterBudget,
    ) -> Result<Vec<PositionedGlyph>, String> {
        if clip.width <= 0 || clip.height <= 0 {
            return Ok(Vec::new());
        }
        let mut positioned = Vec::new();
        for line in &self.lines {
            let mut pen_x = line.x;
            let mut pen_y = line.baseline;
            for glyph in &line.run.glyphs {
                charge(
                    &mut budget.glyphs,
                    1,
                    MAX_INLINE_GLYPHS,
                    "raster glyph count",
                )?;
                let x = pen_x + i64::from(glyph.x_offset);
                let y = pen_y - i64::from(glyph.y_offset);
                let mut mask = line.run.font.rasterize_glyph(
                    glyph.id,
                    line.run.scale,
                    x.rem_euclid(SUBPIXELS) as u8,
                    y.rem_euclid(SUBPIXELS) as u8,
                )?;
                charge(
                    &mut budget.coverage_bytes,
                    mask.coverage.len(),
                    MAX_GLYPH_COVERAGE_BYTES,
                    "glyph coverage bytes",
                )?;
                let bounds = Rect {
                    x: x.div_euclid(SUBPIXELS) + i64::from(mask.left),
                    y: y.div_euclid(SUBPIXELS) + i64::from(mask.top),
                    width: i64::from(mask.width),
                    height: i64::from(mask.height),
                };
                let visible = bounds.intersect(clip);
                if visible.width > 0 && visible.height > 0 {
                    let mask_stride = mask.width as usize;
                    let width = visible.width as usize;
                    let height = visible.height as usize;
                    let left = (visible.x - bounds.x) as usize;
                    let top = (visible.y - bounds.y) as usize;
                    for row in 0..height {
                        let start = (top + row) * mask_stride + left;
                        mask.coverage.copy_within(start..start + width, row * width);
                    }
                    mask.coverage.truncate(width * height);
                    positioned.push(PositionedGlyph {
                        bounds: visible,
                        coverage: mask.coverage,
                    });
                }
                pen_x += i64::from(glyph.x_advance);
                pen_y -= i64::from(glyph.y_advance);
            }
        }
        Ok(positioned)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lays_out_collapsed_wrapped_text_with_explicit_line_height() {
        let settings = TextStyle {
            line_px: Some(20),
            ..TextStyle::INITIAL
        };
        let block = layout(
            "one    two\nthree",
            settings,
            Some("en"),
            (6, 3),
            80,
            &mut InlineBudget::default(),
        )
        .unwrap();
        assert_eq!(block.collapsed_text, "one two three");
        assert_eq!(block.line_height, 20);
        assert_eq!(block.content_height, 40);
        assert_eq!(block.lines.len(), 2);
        assert_eq!(block.lines[0].run.text, "one two");
        assert_eq!(block.lines[1].run.text, "three");
        assert_eq!(block.lines[0].x, 6 * SUBPIXELS);
        assert_eq!(
            block.lines[1].baseline - block.lines[0].baseline,
            20 * SUBPIXELS
        );
        assert!(block.canonical_json().starts_with(
            "{\"collapsed_text\":\"one two three\",\"line_height_px\":20,\"content_height_px\":40,"
        ));
    }

    #[test]
    fn aligns_rtl_runs_to_the_right_and_rejects_mixed_runs() {
        let settings = TextStyle {
            font: PinnedFont::Arabic,
            direction: Direction::Rtl,
            ..TextStyle::INITIAL
        };
        let block = layout(
            "سلام",
            settings,
            Some("ar"),
            (4, 0),
            80,
            &mut InlineBudget::default(),
        )
        .unwrap();
        let line = &block.lines[0];
        assert_eq!(line.run.direction, Direction::Rtl);
        assert_eq!(line.x + line.run.advance(), 84 * SUBPIXELS);
        assert!(
            line.run
                .glyphs
                .windows(2)
                .all(|glyph| glyph[0].cluster >= glyph[1].cluster)
        );
        assert!(
            layout(
                "سلام 123",
                settings,
                Some("ar"),
                (4, 0),
                80,
                &mut InlineBudget::default()
            )
            .unwrap_err()
            .contains("homogeneous Arab rtl run")
        );
        let settings = TextStyle {
            font: PinnedFont::Devanagari,
            ..TextStyle::INITIAL
        };
        assert!(
            layout(
                "नमस्ते office",
                settings,
                Some("hi"),
                (0, 0),
                80,
                &mut InlineBudget::default()
            )
            .unwrap_err()
            .contains("homogeneous Deva ltr run")
        );
    }

    #[test]
    fn clips_glyph_coverage_and_bounds_work_across_inline_blocks() {
        let mut inline_budget = InlineBudget::default();
        let block = layout(
            "A",
            TextStyle::INITIAL,
            None,
            (0, 0),
            80,
            &mut inline_budget,
        )
        .unwrap();
        let viewport = Rect {
            x: 0,
            y: 0,
            width: 80,
            height: 80,
        };
        let full = block
            .rasterize(viewport, &mut RasterBudget::default())
            .unwrap();
        assert_eq!(full.len(), 1);
        assert!(full[0].coverage.iter().any(|&alpha| alpha > 0));
        let bounds = full[0].bounds;
        let clip = Rect {
            x: bounds.x + 1,
            y: bounds.y + 1,
            width: bounds.width - 2,
            height: bounds.height - 2,
        };
        let shifted = block.rasterize(clip, &mut RasterBudget::default()).unwrap();
        assert_eq!(shifted.len(), 1);
        let stride = bounds.width as usize;
        let expected: Vec<_> = full[0]
            .coverage
            .chunks_exact(stride)
            .skip(1)
            .take(clip.height as usize)
            .flat_map(|row| &row[1..stride - 1])
            .copied()
            .collect();
        assert_eq!(shifted[0].bounds.width, clip.width);
        assert_eq!(shifted[0].bounds.y, clip.y);
        assert_eq!(shifted[0].coverage, expected);

        let mut almost_full = InlineBudget {
            text_bytes: MAX_INLINE_BYTES - 1,
            ..InlineBudget::default()
        };
        assert!(layout("A", TextStyle::INITIAL, None, (0, 0), 80, &mut almost_full).is_ok());
        assert!(
            layout("A", TextStyle::INITIAL, None, (0, 0), 80, &mut almost_full)
                .unwrap_err()
                .contains("text bytes")
        );
        let mut raster_budget = RasterBudget {
            coverage_bytes: MAX_GLYPH_COVERAGE_BYTES,
            ..RasterBudget::default()
        };
        assert!(
            block
                .rasterize(viewport, &mut raster_budget)
                .unwrap_err()
                .contains("coverage bytes")
        );
    }
}
