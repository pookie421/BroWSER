use super::VARIATIONS;
use ab_glyph_rasterizer::{Point, Rasterizer, point};
use skrifa::{
    FontRef, GlyphId, MetadataProvider,
    instance::Size,
    outline::{DrawSettings, OutlinePen},
    raw::TableProvider,
};

const MAX_OUTLINE_SEGMENTS: usize = 16_384;
const MAX_GLYPH_DIMENSION: u32 = 2_048;
const MAX_GLYPH_PIXELS: usize = 1_048_576;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GlyphMask {
    /// Pixel offset from the integer part of the screen glyph origin.
    pub left: i32,
    /// Pixel offset from the integer part of the screen glyph baseline.
    pub top: i32,
    pub width: u32,
    pub height: u32,
    /// Row-major unsigned 8-bit coverage.
    pub coverage: Vec<u8>,
}

#[derive(Clone, Copy)]
enum Segment {
    Line([Point; 2]),
    Quad([Point; 3]),
    Cubic([Point; 4]),
}

impl Segment {
    fn points(&self) -> &[Point] {
        match self {
            Self::Line(points) => points,
            Self::Quad(points) => points,
            Self::Cubic(points) => points,
        }
    }
}

#[derive(Default)]
struct Outline {
    segments: Vec<Segment>,
    contour: Option<(Point, Point)>,
    invalid: bool,
}

impl Outline {
    fn segment(&mut self, end: Point, make: impl FnOnce(Point) -> Segment) {
        let Some((start, current)) = self.contour else {
            self.invalid = true;
            return;
        };
        let segment = make(current);
        if self.segments.len() == MAX_OUTLINE_SEGMENTS
            || segment
                .points()
                .iter()
                .any(|point| !point.x.is_finite() || !point.y.is_finite())
        {
            self.invalid = true;
            return;
        }
        self.segments.push(segment);
        self.contour = Some((start, end));
    }
}

impl OutlinePen for Outline {
    fn move_to(&mut self, x: f32, y: f32) {
        self.close();
        self.contour = Some((point(x, -y), point(x, -y)));
    }

    fn line_to(&mut self, x: f32, y: f32) {
        let end = point(x, -y);
        self.segment(end, |start| Segment::Line([start, end]));
    }

    fn quad_to(&mut self, cx: f32, cy: f32, x: f32, y: f32) {
        let end = point(x, -y);
        self.segment(end, |start| Segment::Quad([start, point(cx, -cy), end]));
    }

    fn curve_to(&mut self, cx0: f32, cy0: f32, cx1: f32, cy1: f32, x: f32, y: f32) {
        let end = point(x, -y);
        self.segment(end, |start| {
            Segment::Cubic([start, point(cx0, -cy0), point(cx1, -cy1), end])
        });
    }

    fn close(&mut self) {
        if let Some((start, end)) = self.contour {
            if start != end {
                self.segment(start, |end| Segment::Line([end, start]));
            }
            self.contour = None;
        }
    }
}

pub(super) fn rasterize(
    font: &FontRef<'_>,
    id: u32,
    scale: u32,
    x_phase: u8,
    y_phase: u8,
) -> Result<GlyphMask, String> {
    if x_phase >= 64 || y_phase >= 64 {
        return Err("glyph subpixel phase must be in 0..64".to_owned());
    }
    let glyph_count = font
        .maxp()
        .map_err(|error| format!("cannot read pinned font glyph table: {error}"))?
        .num_glyphs();
    if id == 0 || id >= u32::from(glyph_count) {
        return Err(format!("invalid pinned glyph ID: {id}"));
    }
    let outlines = font.outline_glyphs();
    let glyph = outlines
        .get(GlyphId::new(id))
        .ok_or_else(|| format!("pinned glyph {id} has no supported outline"))?;
    let location = font.axes().location(VARIATIONS);
    let mut outline = Outline::default();
    glyph
        .draw(
            DrawSettings::unhinted(Size::new(scale as f32 / 64.0), &location),
            &mut outline,
        )
        .map_err(|error| format!("could not draw pinned glyph {id}: {error}"))?;
    outline.close();
    if outline.invalid {
        return Err(format!(
            "invalid or oversized outline for pinned glyph {id}"
        ));
    }
    if outline.segments.is_empty() {
        return Ok(GlyphMask {
            left: 0,
            top: 0,
            width: 0,
            height: 0,
            coverage: Vec::new(),
        });
    }

    let x_phase = f32::from(x_phase) / 64.0;
    let y_phase = f32::from(y_phase) / 64.0;
    let [mut min_x, mut min_y, mut max_x, mut max_y] = [
        f32::INFINITY,
        f32::INFINITY,
        f32::NEG_INFINITY,
        f32::NEG_INFINITY,
    ];
    for point in outline.segments.iter().flat_map(Segment::points) {
        min_x = min_x.min(point.x + x_phase);
        max_x = max_x.max(point.x + x_phase);
        min_y = min_y.min(point.y + y_phase);
        max_y = max_y.max(point.y + y_phase);
    }
    // The endpoints and control points enclose each Bézier segment.
    let left = min_x.floor();
    let top = min_y.floor();
    let width = max_x.ceil() - left;
    let height = max_y.ceil() - top;
    if width > MAX_GLYPH_DIMENSION as f32
        || height > MAX_GLYPH_DIMENSION as f32
        || left.abs() > MAX_GLYPH_DIMENSION as f32
        || top.abs() > MAX_GLYPH_DIMENSION as f32
    {
        return Err("pinned glyph bounds exceed the raster limit".to_owned());
    }
    let (width, height) = (width as u32, height as u32);
    let pixels = width as usize * height as usize;
    if pixels > MAX_GLYPH_PIXELS {
        return Err(format!("glyph raster exceeds {MAX_GLYPH_PIXELS} pixels"));
    }
    let mut coverage = vec![0; pixels];
    if pixels != 0 {
        let mut raster = Rasterizer::new(width as usize, height as usize);
        let translate = |p: Point| point(p.x + x_phase - left, p.y + y_phase - top);
        for segment in outline.segments {
            match segment {
                Segment::Line([a, b]) => raster.draw_line(translate(a), translate(b)),
                Segment::Quad([a, b, c]) => {
                    raster.draw_quad(translate(a), translate(b), translate(c))
                }
                Segment::Cubic([a, b, c, d]) => {
                    raster.draw_cubic(translate(a), translate(b), translate(c), translate(d))
                }
            }
        }
        raster.for_each_pixel(|index, alpha| {
            coverage[index] = (alpha.clamp(0.0, 1.0) * 255.0).round() as u8;
        });
    }
    Ok(GlyphMask {
        left: left as i32,
        top: top as i32,
        width,
        height,
        coverage,
    })
}
