//! Text shaping and grayscale glyph masks for the three pinned M1 font faces.

mod line;
mod raster;

use crate::html::json_string;
use harfrust::{
    ShapeOptions, ShapePlan, Shaper, ShaperData, ShaperInstance, UnicodeBuffer, script,
};
pub use line::WrappedText;
pub use raster::GlyphMask;
use skrifa::{FontRef, MetadataProvider, instance::Size, raw::TableProvider};
use std::fmt::Write;
use std::str::FromStr;

pub const MAX_TEXT_BYTES: usize = 16_384;
/// Maximum scale, corresponding to a 256px em in 26.6 pixel coordinates.
pub const MAX_FONT_SCALE: u32 = 16_384;
const MAX_FONT_BYTES: usize = 16_777_216;
const MAX_GLYPHS: usize = 32_767;
const VARIATIONS: [(&str, f32); 2] = [("wdth", 100.0), ("wght", 400.0)];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PinnedFont {
    Latin,
    Arabic,
    Devanagari,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Ltr,
    Rtl,
}

impl Direction {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ltr => "ltr",
            Self::Rtl => "rtl",
        }
    }
}

impl FromStr for Direction {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "ltr" => Ok(Self::Ltr),
            "rtl" => Ok(Self::Rtl),
            _ => Err("text direction must be ltr or rtl".to_owned()),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShapedRun {
    pub font: PinnedFont,
    pub text: String,
    pub direction: Direction,
    pub language: String,
    pub scale: u32,
    /// Glyph order is visual. Cluster offsets point into the original UTF-8 text.
    pub glyphs: Vec<Glyph>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Glyph {
    pub id: u32,
    pub cluster: u32,
    pub x_offset: i32,
    pub y_offset: i32,
    pub x_advance: i32,
    pub y_advance: i32,
}

/// Vertical metrics in the same units as the scaled glyph positions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FontMetrics {
    pub ascent: i32,
    /// Distance below the baseline, expressed as a positive value.
    pub descent: i32,
    pub line_gap: i32,
}

impl PinnedFont {
    pub fn for_script(script: &str) -> Result<Self, String> {
        match script {
            "Latn" => Ok(Self::Latin),
            "Arab" => Ok(Self::Arabic),
            "Deva" => Ok(Self::Devanagari),
            _ => Err("text script must be Latn, Arab or Deva".to_owned()),
        }
    }

    pub fn for_family(family: &str) -> Result<Self, String> {
        [Self::Latin, Self::Arabic, Self::Devanagari]
            .into_iter()
            .find(|font| font.family().eq_ignore_ascii_case(family))
            .ok_or_else(|| format!("unsupported pinned font family: {family}"))
    }

    pub fn family(self) -> &'static str {
        match self {
            Self::Latin => "Noto Sans",
            Self::Arabic => "Noto Sans Arabic",
            Self::Devanagari => "Noto Sans Devanagari",
        }
    }

    pub fn script(self) -> &'static str {
        match self {
            Self::Latin => "Latn",
            Self::Arabic => "Arab",
            Self::Devanagari => "Deva",
        }
    }

    pub fn default_language(self) -> &'static str {
        match self {
            Self::Latin => "en",
            Self::Arabic => "ar",
            Self::Devanagari => "hi",
        }
    }

    fn font(self) -> Result<FontRef<'static>, String> {
        let bytes: &[u8] = match self {
            Self::Latin => {
                include_bytes!("../fixtures/m1/assets/fonts/notosans/NotoSans[wdth,wght].ttf")
            }
            Self::Arabic => include_bytes!(
                "../fixtures/m1/assets/fonts/notosansarabic/NotoSansArabic[wdth,wght].ttf"
            ),
            Self::Devanagari => include_bytes!(
                "../fixtures/m1/assets/fonts/notosansdevanagari/NotoSansDevanagari[wdth,wght].ttf"
            ),
        };
        validated_font(bytes)
    }

    /// Shape one explicitly directed script run in the selected pinned face.
    pub fn shape(
        self,
        input: &str,
        direction: Direction,
        language: &str,
        scale: u32,
    ) -> Result<ShapedRun, String> {
        self.with_shaper(direction, language, scale, |shaper| shaper.shape(input))
    }

    fn with_shaper<T>(
        self,
        direction: Direction,
        language: &str,
        scale: u32,
        run: impl FnOnce(RunShaper<'_>) -> Result<T, String>,
    ) -> Result<T, String> {
        validate_scale(scale)?;
        let language = text_language(language)?;
        let font = self.font()?;
        let glyph_limit = font
            .maxp()
            .map_err(|error| format!("cannot read pinned font glyph table: {error}"))?
            .num_glyphs();
        let data = ShaperData::new(&font);
        let instance = ShaperInstance::from_variations(&font, VARIATIONS);
        let shaper = data.shaper(&font).instance(Some(&instance)).build();
        let hb_direction = match direction {
            Direction::Ltr => harfrust::Direction::LeftToRight,
            Direction::Rtl => harfrust::Direction::RightToLeft,
        };
        let script = match self {
            Self::Latin => script::LATIN,
            Self::Arabic => script::ARABIC,
            Self::Devanagari => script::DEVANAGARI,
        };
        let plan = ShapePlan::new(&shaper, hb_direction, Some(script), Some(&language), &[]);
        run(RunShaper {
            shaper,
            plan,
            font: self,
            direction,
            hb_direction,
            language,
            scale,
            script,
            glyph_limit: u32::from(glyph_limit),
        })
    }

    /// Collapse HTML whitespace and shape lines at a horizontal limit.
    ///
    /// Width is in the same units as the shaping scale. Words that cannot
    /// break at an ASCII space can overflow without dropping any glyphs.
    pub fn wrap(
        self,
        input: &str,
        direction: Direction,
        language: &str,
        scale: u32,
        max_width: i64,
    ) -> Result<WrappedText, String> {
        line::wrap(self, input, direction, language, scale, max_width)
    }

    pub fn metrics(self, scale: u32) -> Result<FontMetrics, String> {
        validate_scale(scale)?;
        let font = self.font()?;
        let location = font.axes().location(VARIATIONS);
        let metrics = font.metrics(Size::new(scale as f32), &location);
        Ok(FontMetrics {
            ascent: metrics.ascent.round() as i32,
            descent: (-metrics.descent).round() as i32,
            line_gap: metrics.leading.round() as i32,
        })
    }

    /// Produce a coverage mask at a fractional screen-pixel position.
    ///
    /// Scale is in 26.6 pixel units. Phase inputs are remainders in 0..64
    /// from the glyph's screen origin, with positive Y pointing downward.
    pub fn rasterize_glyph(
        self,
        id: u32,
        scale: u32,
        x_phase: u8,
        y_phase: u8,
    ) -> Result<GlyphMask, String> {
        validate_scale(scale)?;
        raster::rasterize(&self.font()?, id, scale, x_phase, y_phase)
    }
}

// A wrap can probe many candidate line lengths. Keep the immutable face,
// variation and lookup plan for that run rather than rebuilding them per probe.
struct RunShaper<'a> {
    shaper: Shaper<'a>,
    plan: ShapePlan,
    font: PinnedFont,
    direction: Direction,
    hb_direction: harfrust::Direction,
    language: harfrust::Language,
    scale: u32,
    script: harfrust::Script,
    glyph_limit: u32,
}

impl RunShaper<'_> {
    fn shape(&self, input: &str) -> Result<ShapedRun, String> {
        if input.len() > MAX_TEXT_BYTES {
            return Err(format!(
                "text exceeds {MAX_TEXT_BYTES} bytes per shaped run"
            ));
        }
        if input.chars().any(char::is_control) {
            return Err("text shaping requires a run without control characters".to_owned());
        }

        let mut buffer = UnicodeBuffer::new();
        let count = input.chars().count();
        if !buffer.reserve(count) {
            return Err("could not allocate shaping buffer".to_owned());
        }
        buffer.push_str(input);
        buffer.set_direction(self.hb_direction);
        buffer.set_script(self.script);
        buffer.set_language(self.language.clone());
        let shaped = self.shaper.shape(
            buffer,
            ShapeOptions::new()
                .scale(Some(self.scale as i32))
                .plan(Some(&self.plan)),
        );
        if shaped.len() > MAX_GLYPHS {
            return Err(format!("shaped run exceeds {MAX_GLYPHS} glyphs"));
        }
        let glyphs = shaped
            .glyph_infos()
            .iter()
            .zip(shaped.glyph_positions())
            .map(|(glyph, position)| {
                if glyph.glyph_id == 0 {
                    return Err(format!(
                        "{} has no glyph for text at byte {}",
                        self.font.family(),
                        glyph.cluster
                    ));
                }
                if glyph.glyph_id >= self.glyph_limit
                    || glyph.cluster as usize >= input.len()
                    || !input.is_char_boundary(glyph.cluster as usize)
                {
                    return Err("shaper returned an invalid glyph or UTF-8 cluster".to_owned());
                }
                Ok(Glyph {
                    id: glyph.glyph_id,
                    x_offset: position.x_offset,
                    y_offset: position.y_offset,
                    x_advance: position.x_advance,
                    y_advance: position.y_advance,
                    cluster: glyph.cluster,
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        if glyphs.windows(2).any(|pair| match self.direction {
            Direction::Ltr => pair[0].cluster > pair[1].cluster,
            Direction::Rtl => pair[0].cluster < pair[1].cluster,
        }) {
            return Err("shaper returned clusters out of visual order".to_owned());
        }
        Ok(ShapedRun {
            font: self.font,
            text: input.to_owned(),
            direction: self.direction,
            language: self.language.as_str().to_owned(),
            scale: self.scale,
            glyphs,
        })
    }
}

impl ShapedRun {
    pub fn advance(&self) -> i64 {
        self.glyphs
            .iter()
            .map(|glyph| i64::from(glyph.x_advance))
            .sum()
    }

    pub fn canonical_json(&self) -> String {
        let mut output = format!(
            "{{\"text\":{},\"direction\":\"{}\",\"script\":\"{}\",\"language\":{},\"font_scale\":{},\"variation\":{{\"wdth\":100,\"wght\":400}},\"cluster_units\":\"UTF-8 byte offsets\",\"glyphs\":[",
            json_string(&self.text),
            self.direction.as_str(),
            self.font.script(),
            json_string(&self.language),
            self.scale,
        );
        for (index, glyph) in self.glyphs.iter().enumerate() {
            if index != 0 {
                output.push(',');
            }
            write!(
                output,
                "{{\"g\":{},\"cl\":{},\"dx\":{},\"dy\":{},\"ax\":{},\"ay\":{}}}",
                glyph.id,
                glyph.cluster,
                glyph.x_offset,
                glyph.y_offset,
                glyph.x_advance,
                glyph.y_advance,
            )
            .expect("writing to a String");
        }
        output.push_str("]}");
        output
    }
}

fn validate_scale(scale: u32) -> Result<(), String> {
    if !(1..=MAX_FONT_SCALE).contains(&scale) {
        return Err(format!("text scale must be in 1..={MAX_FONT_SCALE}"));
    }
    Ok(())
}

fn validated_font(bytes: &[u8]) -> Result<FontRef<'_>, String> {
    if bytes.len() > MAX_FONT_BYTES {
        return Err(format!("font data exceeds {MAX_FONT_BYTES} bytes"));
    }
    let font = FontRef::new(bytes).map_err(|error| format!("invalid font data: {error}"))?;
    if !font
        .head()
        .is_ok_and(|head| (16..=16_384).contains(&head.units_per_em()))
        || !font.maxp().is_ok_and(|maxp| maxp.num_glyphs() > 0)
        || font.cmap().is_err()
        || font.hmtx().is_err()
    {
        return Err("invalid font data: required font metrics or tables are missing".to_owned());
    }
    Ok(font)
}

fn text_language(tag: &str) -> Result<harfrust::Language, String> {
    let mut subtags = tag.split('-');
    let primary = subtags.next().unwrap_or_default();
    if tag.len() > 63
        || !(1..=8).contains(&primary.len())
        || !primary.bytes().all(|byte| byte.is_ascii_alphabetic())
        || !subtags.all(|subtag| {
            (1..=8).contains(&subtag.len())
                && subtag.bytes().all(|byte| byte.is_ascii_alphanumeric())
        })
    {
        return Err("text language must be a hyphen-separated ASCII language tag".to_owned());
    }
    tag.parse().map_err(|_| "invalid text language".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_and_oversized_font_bytes_are_diagnostic() {
        let pinned = include_bytes!("../fixtures/m1/assets/fonts/notosans/NotoSans[wdth,wght].ttf");
        for bytes in [&[][..], b"not a font", &pinned[..48]] {
            assert!(
                validated_font(bytes)
                    .err()
                    .unwrap()
                    .contains("invalid font")
            );
        }
        let oversized = vec![0; MAX_FONT_BYTES + 1];
        assert!(validated_font(&oversized).err().unwrap().contains("bytes"));
    }
}
