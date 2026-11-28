//! Backgrounds as data: a painting described as layers of simple shapes,
//! drawn by the game's own renderer. The game's seven built-in places are
//! painted by code (`art`); a story can add places of its own as painting
//! files in a `paintings` folder beside its story file
//! (`paintings/<name>.json`), made by hand or generated in the editor.
//!
//! The canvas is the game's frame, 1152 x 648, x to the right and y down.
//! Layers are drawn in order, first at the back.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::gfx::{rgb, with_alpha, Canvas, H, W};

/// The most layers, and the most corners of one polygon, a painting may have.
pub const MAX_LAYERS: usize = 600;
pub const MAX_POINTS: usize = 96;
/// The most dabs of the brush in all a painting's dabs layers.
pub const MAX_DABS: u32 = 20_000;
/// The longest a brush stroke may be, in pixels.
pub const MAX_STROKE: f32 = 4000.0;
/// The most washes and hatchings together.
pub const MAX_AREA_LAYERS: usize = 150;

fn one() -> f32 {
    1.0
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Pt {
    pub x: f32,
    pub y: f32,
}

/// A point of a brush stroke: where, and how hard the brush presses there (0 to 1).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Press {
    pub x: f32,
    pub y: f32,
    #[serde(default = "half")]
    pub p: f32,
}

fn half() -> f32 {
    0.5
}

fn third() -> f32 {
    0.3
}

/// A seed from any number a model may write (negative, huge, with a fraction): only its variety matters.
fn any_seed<'de, D: serde::Deserializer<'de>>(d: D) -> Result<u32, D::Error> {
    let v = serde_json::Value::deserialize(d)?;
    Ok(match v {
        serde_json::Value::Number(n) => n.as_u64().map(|u| u as u32).or_else(|| n.as_i64().map(|i| i as u32)).unwrap_or_else(|| n.as_f64().map_or(0, |f| if f.is_finite() { (f.abs() as u64) as u32 } else { 0 })),
        serde_json::Value::String(t) => t.bytes().fold(0u32, |h, b| h.wrapping_mul(31).wrapping_add(b as u32)),
        _ => 0,
    })
}

fn round_brush() -> String {
    "round".into()
}

/// One layer of a painting.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "shape", rename_all = "lowercase")]
pub enum Layer {
    /// A band from `y0` to `y1`, shading from `top` to `bottom` colour, across the whole width.
    Gradient { top: String, bottom: String, y0: f32, y1: f32 },
    Rect { x: f32, y: f32, w: f32, h: f32, color: String, #[serde(default = "one")] alpha: f32 },
    /// A rectangle with soft edges `feather` pixels wide (haze, light through a window).
    Soft { x: f32, y: f32, w: f32, h: f32, feather: f32, color: String, #[serde(default = "one")] alpha: f32 },
    Ellipse { x: f32, y: f32, rx: f32, ry: f32, color: String, #[serde(default = "one")] alpha: f32 },
    Poly { points: Vec<Pt>, color: String, #[serde(default = "one")] alpha: f32 },
    /// A line with round ends, `width` pixels thick.
    Line { x0: f32, y0: f32, x1: f32, y1: f32, width: f32, color: String, #[serde(default = "one")] alpha: f32 },
    /// Light: a soft round glow that brightens what is under it.
    Glow { x: f32, y: f32, r: f32, color: String, strength: f32 },
    /// Scattered points of light between `y0` and `y1` (stars, dust, distant windows).
    Stars { count: u32, y0: f32, y1: f32, color: String, #[serde(deserialize_with = "any_seed")] seed: u32 },
    /// A brush stroke along a path, with pressure (see `brush`): `brush` is one of
    /// `brush::BRUSHES`, `load` how much paint it carries (0 a clean brush that
    /// blends, 0.1 a scrub, 0.3 ordinary, 0.9 thick impasto), `alpha` how much the paint covers.
    Stroke {
        points: Vec<Press>,
        width: f32,
        color: String,
        #[serde(default = "round_brush")]
        brush: String,
        #[serde(default = "third")]
        load: f32,
        #[serde(default = "one")]
        alpha: f32,
        #[serde(default, deserialize_with = "any_seed")]
        seed: u32,
    },
    /// A watercolour wash over a polygon: transparent, pooling at its edges; `wet` 0 crisp to 1 blooming.
    Wash {
        points: Vec<Pt>,
        color: String,
        #[serde(default = "half")]
        alpha: f32,
        #[serde(default = "third")]
        wet: f32,
        #[serde(default, deserialize_with = "any_seed")]
        seed: u32,
    },
    /// Pen hatching over a polygon: lines `spacing` apart at `angle` degrees (and across them if `cross`).
    Hatch {
        points: Vec<Pt>,
        angle: f32,
        spacing: f32,
        width: f32,
        color: String,
        #[serde(default = "one")]
        alpha: f32,
        #[serde(default)]
        cross: bool,
        #[serde(default, deserialize_with = "any_seed")]
        seed: u32,
    },
    /// Many short touches of a brush scattered over a box, each a little different in colour (`jitter` 0 to 1).
    Dabs { x: f32, y: f32, w: f32, h: f32, count: u32, size: f32, angle: f32, color: String, jitter: f32, #[serde(default, deserialize_with = "any_seed")] seed: u32 },
}

/// A way of painting: how the marks are made, and the surface they end on.
pub struct Style {
    pub id: &'static str,
    pub name: &'static str,
    /// For the page: a line on what it looks like.
    pub blurb: &'static str,
    /// For a language model painting in it: how to work.
    pub guide: &'static str,
    /// For an image model (ComfyUI): the words for it.
    pub words: &'static str,
}

pub const STYLES: [Style; 8] = [
    Style {
        id: "matte",
        name: "Matte painting",
        blurb: "Clean layered shapes, soft haze and glowing light: the game's own look.",
        guide: "Work like a matte painter: flat and gradient shapes, soft haze for distance, glows for light. Mostly gradient, rect, soft, ellipse, poly, line, glow and stars.",
        words: "digital matte painting, painterly game background, soft atmospheric light",
    },
    Style {
        id: "oil",
        name: "Oil painting (bristle brush)",
        blurb: "Built from simulated brush strokes with real bristles, paint load and impasto, after aibrush.",
        guide: "Paint it in oils, stroke by stroke, the way a painter works at the easel (after the aibrush method). Plan before painting: the composition, the value hierarchy (where the lightest light and darkest dark go), the direction of the strokes, where edges are sharp and where they are lost. Then: 1) a toned ground: one gradient or rect over the whole canvas in a middle value; 2) block in the big masses with wide brushes (flat, width 100 to 140, load 0.24 to 0.3, pressure 0.6 to 0.8), long strokes that follow the planes; 3) model the forms with medium strokes (flat or round, width 20 to 60, load 0.3 to 0.5) laid in the direction of each surface, lights over darks; 4) soften some edges with a clean brush (load 0) dragged across them; 5) details with round and liner brushes (width 2 to 12); 6) the last accents and highlights with thick paint (load 0.7 to 0.9, alpha 1) so they stand up from the canvas. Every stroke has a reason: a form, a light, an edge or an accent; avoid uniform strokes laid like tiles, vary brush, width and load between the main subject and the background, and leave the background looser. Use stroke for almost everything: 120 to 300 strokes, each with 3 to 12 points and a pressure on each point; a few wash or glow layers are allowed for glazes of light.",
        words: "oil painting, visible bristle brushstrokes, impasto, painted on linen canvas, rich layered color, painterly",
    },
    Style {
        id: "impressionist",
        name: "Impressionist",
        blurb: "Broken colour in short dabs of thick paint; light and atmosphere over detail.",
        guide: "Paint it as an Impressionist would, en plein air: light and atmosphere over detail, no black, shadows coloured (blue, violet) and lights warm. Block in the large areas with a few broad strokes (flat, load 0.3), then build the surfaces with dabs layers: many short touches whose colour jitters (jitter 0.1 to 0.3), sized 6 to 20, angled with the surface (horizontal on water, vertical on foliage), side by side in complementary colours that mix in the eye. A few strokes for edges and accents of thick paint. Mostly dabs and stroke.",
        words: "impressionist oil painting, broken color, short visible dabs of paint, plein air light, Monet style",
    },
    Style {
        id: "watercolor",
        name: "Watercolour",
        blurb: "Transparent washes on paper that pool at their edges, with white paper for the lights.",
        guide: "Paint it in watercolour: begin with the white paper (a pale gradient or rect, near white) and never paint the lightest lights: leave the paper. Work light to dark in transparent wash layers (alpha 0.3 to 0.7); each wash darkens what is under it, so overlap them to build depth. Big wet washes first (wet 0.6 to 1) for sky and distance, then crisper washes (wet 0 to 0.3) for the nearer shapes, then the darkest darks last in small areas. A few fine strokes (liner or round, load 0.2, alpha 0.8) for drawing. Mostly wash: 30 to 90 of them.",
        words: "watercolor painting, transparent washes, wet-in-wet blooms, paper texture, white of the paper for highlights",
    },
    Style {
        id: "ink",
        name: "Pen and ink",
        blurb: "Hatching and line on paper, with a little wash: an illustrator's drawing.",
        guide: "Draw it in pen and ink, as a book illustrator: warm white paper (a gradient), outlines with line layers (width 1 to 3, dark sepia or black), tone built with hatch layers: wider spacing (8 to 14) for half-tones, closer (3 to 6) and cross-hatched for shadows, the angle following each surface. A few pale washes (alpha 0.2 to 0.4) of one or two colours are allowed. No glows; keep plenty of white.",
        words: "pen and ink illustration, cross-hatching, fine linework, sepia ink on paper, light watercolor wash",
    },
    Style {
        id: "pixel",
        name: "Pixel art",
        blurb: "Big square pixels and a small dithered palette, like an old adventure game.",
        guide: "Make it a pixel-art background for a classic adventure game: it will be turned into big pixels (4 by 4) with a small dithered palette, so use strong simple shapes, clear silhouettes and a limited palette of 12 to 20 colours; bands of gradient for the sky; no tiny details (nothing smaller than 8 pixels). Mostly rect, poly, gradient and ellipse.",
        words: "pixel art, 16-bit adventure game background, limited palette, dithering, crisp pixels",
    },
    Style {
        id: "poster",
        name: "Gouache poster",
        blurb: "Flat opaque colour in a few bold steps, like a mid-century travel poster.",
        guide: "Design it as a mid-century travel poster in gouache: flat opaque shapes in a bold limited palette (6 to 10 colours), simplified geometric forms, strong silhouettes, a clear focal point, no outlines and no soft haze. Mostly poly, rect and ellipse; a few stroke layers (flat, load 0.4) for texture.",
        words: "vintage travel poster, flat gouache, bold limited palette, simplified shapes, mid-century illustration",
    },
    Style {
        id: "woodblock",
        name: "Woodblock print",
        blurb: "Flat inks, bold dark outlines and paper grain, after Japanese prints.",
        guide: "Make it a Japanese woodblock print (ukiyo-e): flat areas of colour as each block would print them, graded skies (gradient), bold dark outlines (line layers, width 2 to 4, dark indigo or black) around every form, stylised waves, clouds and trees, a limited palette of indigo, vermilion, ochre, green and grey. No glows, no soft haze. Mostly poly, line, gradient and ellipse.",
        words: "ukiyo-e woodblock print, flat colors, bold outlines, washi paper texture, Hiroshige style",
    },
];

/// A style by its id; the matte style for anything else.
pub fn style(id: &str) -> &'static Style {
    STYLES.iter().find(|s| s.id == id).unwrap_or(&STYLES[0])
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PaintingData {
    #[serde(default)]
    pub title: String,
    /// What it shows, in words: the prompt for ComfyUI to paint it from.
    #[serde(default)]
    pub prompt: String,
    #[serde(default)]
    pub layers: Vec<Layer>,
    /// Darken the corners a little, as the game's own paintings do (0 to 1).
    #[serde(default)]
    pub vignette: f32,
    /// How it is painted: one of `STYLES` (empty is the matte style).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub style: String,
}

/// "#rrggbb" as the canvas's colour; mid grey if it is not one.
pub fn colour(hex: &str) -> u32 {
    let h = hex.trim().trim_start_matches('#');
    match (h.len(), u32::from_str_radix(h, 16)) {
        (6, Ok(n)) => rgb((n >> 16) as u8, (n >> 8) as u8, n as u8),
        _ => rgb(128, 128, 128),
    }
}

fn clean_hex(c: &mut String) {
    let h = c.trim().trim_start_matches('#').to_lowercase();
    *c = if h.len() == 6 && h.chars().all(|c| c.is_ascii_hexdigit()) { format!("#{}", h) } else { "#808080".into() };
}

fn fin(v: &mut f32, lo: f32, hi: f32) {
    *v = if v.is_finite() { v.clamp(lo, hi) } else { lo.max(0.0).min(hi) };
}

impl PaintingData {
    /// Bring a painting from anywhere within what the renderer will draw.
    pub fn clean(&mut self) {
        self.title = self.title.chars().filter(|c| !c.is_control() && *c != '|').take(80).collect::<String>().trim().to_string();
        self.prompt = self.prompt.chars().filter(|c| !c.is_control()).take(600).collect::<String>().trim().to_string();
        fin(&mut self.vignette, 0.0, 1.0);
        if !STYLES.iter().any(|st| st.id == self.style) || self.style == "matte" {
            self.style = String::new();
        }
        self.layers.truncate(MAX_LAYERS);
        // touches of the brush, all the dabs layers together: no more than this many
        let dabs: u64 = self.layers.iter().map(|l| if let Layer::Dabs { count, .. } = l { (*count).min(4000) as u64 } else { 0 }).sum();
        if dabs > MAX_DABS as u64 {
            let k = MAX_DABS as f64 / dabs as f64;
            for l in &mut self.layers {
                if let Layer::Dabs { count, .. } = l {
                    *count = ((*count).min(4000) as f64 * k).floor() as u32;
                }
            }
        }
        // washes and hatching are worked out pixel by pixel over their area: a limit on how many
        let mut area_layers = 0;
        self.layers.retain(|l| {
            if matches!(l, Layer::Wash { .. } | Layer::Hatch { .. }) {
                area_layers += 1;
                area_layers <= MAX_AREA_LAYERS
            } else {
                true
            }
        });
        // (a little way beyond the frame is allowed: shapes may run off its edges)
        let (xs, ys) = (-(W as f32), 2.0 * W as f32);
        let (y_lo, y_hi) = (-(H as f32), 2.0 * H as f32);
        for l in &mut self.layers {
            match l {
                Layer::Gradient { top, bottom, y0, y1 } => {
                    clean_hex(top);
                    clean_hex(bottom);
                    fin(y0, 0.0, H as f32);
                    fin(y1, 0.0, H as f32);
                    if *y1 < *y0 {
                        std::mem::swap(y0, y1);
                    }
                }
                Layer::Rect { x, y, w, h, color, alpha } | Layer::Soft { x, y, w, h, color, alpha, .. } => {
                    fin(x, xs, ys);
                    fin(y, y_lo, y_hi);
                    fin(w, 0.0, 3.0 * W as f32);
                    fin(h, 0.0, 3.0 * H as f32);
                    clean_hex(color);
                    fin(alpha, 0.0, 1.0);
                }
                Layer::Ellipse { x, y, rx, ry, color, alpha } => {
                    fin(x, xs, ys);
                    fin(y, y_lo, y_hi);
                    fin(rx, 0.0, 2.0 * W as f32);
                    fin(ry, 0.0, 2.0 * W as f32);
                    clean_hex(color);
                    fin(alpha, 0.0, 1.0);
                }
                Layer::Poly { points, color, alpha } => {
                    points.truncate(MAX_POINTS);
                    for p in points.iter_mut() {
                        fin(&mut p.x, xs, ys);
                        fin(&mut p.y, y_lo, y_hi);
                    }
                    clean_hex(color);
                    fin(alpha, 0.0, 1.0);
                }
                Layer::Line { x0, y0, x1, y1, width, color, alpha } => {
                    for v in [x0, x1] {
                        fin(v, xs, ys);
                    }
                    for v in [y0, y1] {
                        fin(v, y_lo, y_hi);
                    }
                    fin(width, 0.5, 200.0);
                    clean_hex(color);
                    fin(alpha, 0.0, 1.0);
                }
                Layer::Glow { x, y, r, color, strength } => {
                    fin(x, xs, ys);
                    fin(y, y_lo, y_hi);
                    fin(r, 1.0, 2.0 * W as f32);
                    clean_hex(color);
                    fin(strength, 0.0, 3.0);
                }
                Layer::Stars { count, y0, y1, color, .. } => {
                    *count = (*count).min(3000);
                    fin(y0, 0.0, H as f32);
                    fin(y1, 0.0, H as f32);
                    if *y1 < *y0 {
                        std::mem::swap(y0, y1);
                    }
                    clean_hex(color);
                }
                Layer::Stroke { points, width, color, brush, load, alpha, .. } => {
                    points.truncate(MAX_POINTS);
                    // a stroke is an arm's sweep, not a scribble across the whole canvas many times over
                    let mut length = 0.0;
                    let keep = points
                        .windows(2)
                        .take_while(|w| {
                            length += (w[1].x - w[0].x).hypot(w[1].y - w[0].y);
                            length <= MAX_STROKE
                        })
                        .count()
                        + 1;
                    points.truncate(keep.max(1));
                    for p in points.iter_mut() {
                        fin(&mut p.x, xs, ys);
                        fin(&mut p.y, y_lo, y_hi);
                        fin(&mut p.p, 0.0, 1.0);
                    }
                    fin(width, 0.5, 300.0);
                    clean_hex(color);
                    if !crate::brush::BRUSHES.contains(&brush.as_str()) {
                        *brush = "round".into();
                    }
                    fin(load, 0.0, 1.0);
                    fin(alpha, 0.0, 1.0);
                }
                Layer::Wash { points, color, alpha, wet, .. } => {
                    points.truncate(MAX_POINTS);
                    for p in points.iter_mut() {
                        fin(&mut p.x, xs, ys);
                        fin(&mut p.y, y_lo, y_hi);
                    }
                    clean_hex(color);
                    fin(alpha, 0.0, 1.0);
                    fin(wet, 0.0, 1.0);
                }
                Layer::Hatch { points, angle, spacing, width, color, alpha, .. } => {
                    points.truncate(MAX_POINTS);
                    for p in points.iter_mut() {
                        fin(&mut p.x, xs, ys);
                        fin(&mut p.y, y_lo, y_hi);
                    }
                    fin(angle, -360.0, 360.0);
                    fin(spacing, 2.0, 200.0);
                    fin(width, 0.3, 20.0);
                    clean_hex(color);
                    fin(alpha, 0.0, 1.0);
                }
                Layer::Dabs { x, y, w, h, count, size, angle, color, jitter, .. } => {
                    fin(x, xs, ys);
                    fin(y, y_lo, y_hi);
                    fin(w, 0.0, 3.0 * W as f32);
                    fin(h, 0.0, 3.0 * H as f32);
                    *count = (*count).min(4000);
                    fin(size, 1.0, 80.0);
                    fin(angle, -360.0, 360.0);
                    clean_hex(color);
                    fin(jitter, 0.0, 1.0);
                }
            }
        }
        // something to look at, at the least
        if self.layers.is_empty() {
            self.layers.push(Layer::Gradient { top: "#202438".into(), bottom: "#4a4458".into(), y0: 0.0, y1: H as f32 });
        }
    }

    /// Paint it, at the game's size.
    pub fn render(&self) -> Canvas {
        let mut c = Canvas::new(W, H);
        c.clear(rgb(0, 0, 0));
        let mut relief = crate::brush::Relief::new(W, H);
        // Brushwork is the costly part (about 11 pixel touches for each pixel of length
        // times width): past a budget, the hairs touch down further apart (the same
        // marks, coarser), so that any painting draws in a moment.
        let work: f32 = self
            .layers
            .iter()
            .map(|l| match l {
                Layer::Stroke { points, width, brush, .. } => {
                    let len: f32 = points.windows(2).map(|w| (w[1].x - w[0].x).hypot(w[1].y - w[0].y)).sum();
                    crate::brush::stroke_cost(len, *width, &crate::brush::brush(brush))
                }
                Layer::Dabs { count, size, .. } => *count as f32 * crate::brush::stroke_cost(size * 1.3, size * 0.7, &crate::brush::Brush { hairs: 10, ..crate::brush::brush("round") }),
                _ => 0.0,
            })
            .sum();
        let coarse = (work / 1.5e8).max(1.0);
        // and washes and hatching over more than twenty times the frame's area in all are left out
        let mut area_left = 20.0 * (W * H) as f32;
        // polygons are filled by testing each pixel of their box against every edge: past
        // about a dozen frame-sized polygons of many corners, the rest are left out
        let mut poly_left = 12.0 * (W * H) as f64 * MAX_POINTS as f64;
        let mut poly_ok = |points: &[Pt]| {
            let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
            for p in points {
                x0 = x0.min(p.x);
                y0 = y0.min(p.y);
                x1 = x1.max(p.x);
                y1 = y1.max(p.y);
            }
            let area = ((x1.min(W as f32) - x0.max(0.0)).max(0.0) * (y1.min(H as f32) - y0.max(0.0)).max(0.0)) as f64;
            poly_left -= area * points.len() as f64;
            poly_left >= 0.0
        };
        let mut area_ok = |points: &[Pt]| {
            let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
            for p in points {
                x0 = x0.min(p.x);
                y0 = y0.min(p.y);
                x1 = x1.max(p.x);
                y1 = y1.max(p.y);
            }
            let area = (x1.min(W as f32) - x0.max(0.0)).max(0.0) * (y1.min(H as f32) - y0.max(0.0)).max(0.0);
            area_left -= area;
            area_left >= 0.0
        };
        let a8 = |a: f32| (a.clamp(0.0, 1.0) * 255.0).round() as u32;
        for l in &self.layers {
            match l {
                Layer::Gradient { top, bottom, y0, y1 } => {
                    let (t, b) = (colour(top), colour(bottom));
                    let (from, to) = (*y0 as i32, (*y1 as i32).max(*y0 as i32 + 1));
                    for y in from.max(0)..to.min(H) {
                        let k = (y - from) as f32 / (to - from).max(1) as f32;
                        c.rect(0, y, W, 1, crate::gfx::lerp(t, b, k));
                    }
                }
                Layer::Rect { x, y, w, h, color, alpha } => c.rect_blend(*x as i32, *y as i32, *w as i32, *h as i32, with_alpha(colour(color), a8(*alpha))),
                Layer::Soft { x, y, w, h, feather, color, alpha } => crate::art::paint::soft_rect(&mut c, *x, *y, *w, *h, feather.max(0.5), with_alpha(colour(color), a8(*alpha))),
                Layer::Ellipse { x, y, rx, ry, color, alpha } => c.ellipse_aa(*x, *y, *rx, *ry, with_alpha(colour(color), a8(*alpha))),
                Layer::Poly { points, color, alpha } => {
                    if points.len() >= 3 && poly_ok(points) {
                        let pts: Vec<(f32, f32)> = points.iter().map(|p| (p.x, p.y)).collect();
                        c.poly_aa(&pts, with_alpha(colour(color), a8(*alpha)));
                    }
                }
                Layer::Line { x0, y0, x1, y1, width, color, alpha } => c.capsule(*x0, *y0, *x1, *y1, width / 2.0, with_alpha(colour(color), a8(*alpha))),
                Layer::Glow { x, y, r, color, strength } => c.glow(*x, *y, *r, colour(color), *strength),
                Layer::Stars { count, y0, y1, color, seed } => {
                    let col = colour(color);
                    for i in 0..*count as i32 {
                        let sx = (crate::gfx::hash(i, 0, *seed) % W as u32) as i32;
                        let span = (*y1 - *y0).max(1.0) as u32;
                        let sy = *y0 as i32 + (crate::gfx::hash(i, 1, *seed) % span) as i32;
                        let bright = 0.35 + crate::gfx::hashf(i, 2, *seed) * 0.65;
                        c.cover(sx, sy, col, bright);
                    }
                }
                Layer::Stroke { points, width, color, brush, load, alpha, seed } => {
                    let mut b = crate::brush::brush(brush);
                    b.coarse = coarse;
                    let path: Vec<crate::brush::Touch> = points.iter().map(|p| crate::brush::Touch { x: p.x, y: p.y, p: p.p }).collect();
                    crate::brush::stroke(&mut c, &mut relief, &path, *width, colour(color), &b, *load, *alpha, *seed);
                }
                Layer::Wash { points, .. } | Layer::Hatch { points, .. } if !area_ok(points) => {}
                Layer::Wash { points, color, alpha, wet, seed } => {
                    let pts: Vec<(f32, f32)> = points.iter().map(|p| (p.x, p.y)).collect();
                    crate::brush::wash(&mut c, &pts, colour(color), *alpha, *wet, *seed);
                }
                Layer::Hatch { points, angle, spacing, width, color, alpha, cross, seed } => {
                    let pts: Vec<(f32, f32)> = points.iter().map(|p| (p.x, p.y)).collect();
                    crate::brush::hatch(&mut c, &pts, *angle, *spacing, *width, colour(color), *alpha, *cross, *seed);
                }
                Layer::Dabs { x, y, w, h, count, size, angle, color, jitter, seed } => {
                    crate::brush::dabs(&mut c, &mut relief, *x, *y, *w, *h, *count, *size, *angle, colour(color), *jitter, *seed, coarse);
                }
            }
        }
        // the surface of the style
        match self.style.as_str() {
            "oil" => {
                crate::brush::light_relief(&mut c, &relief, 1.4);
                crate::brush::canvas_weave(&mut c, 0.07);
            }
            "impressionist" => {
                crate::brush::light_relief(&mut c, &relief, 0.8);
                crate::brush::canvas_weave(&mut c, 0.06);
            }
            "watercolor" => crate::brush::paper(&mut c, 0.1, 17),
            "ink" => crate::brush::paper(&mut c, 0.07, 23),
            "pixel" => crate::brush::palette(&mut c, 20, 18.0, 4),
            "poster" => {
                crate::brush::palette(&mut c, 9, 0.0, 1);
                crate::brush::paper(&mut c, 0.05, 31);
            }
            "woodblock" => {
                crate::brush::palette(&mut c, 7, 0.0, 1);
                crate::brush::paper(&mut c, 0.11, 37);
            }
            _ => crate::brush::light_relief(&mut c, &relief, 0.8),
        }
        if self.vignette > 0.0 {
            crate::art::paint::finish(&mut c, self.vignette, 0.0);
        }
        c
    }
}

/// Where a story keeps its own paintings: `paintings` beside the story file.
pub fn dir(story: &Path) -> PathBuf {
    story.parent().unwrap_or(Path::new(".")).join("paintings")
}

/// A name a story may give a place of its own: an id the game does not already use.
pub fn usable_name(name: &str) -> bool {
    crate::script::valid_id(name) && name.len() <= 40 && name.chars().all(|c| !c.is_ascii_uppercase()) && !crate::art::BACKGROUNDS.contains(&name)
}

/// The story's own places, by name, in order.
pub fn names(story: &Path) -> Vec<String> {
    let mut out: Vec<String> = std::fs::read_dir(dir(story))
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok())
        .filter_map(|e| e.file_name().to_string_lossy().strip_suffix(".json").map(str::to_string))
        .filter(|n| usable_name(n))
        .collect();
    out.sort();
    out
}

/// Every place a story can name: the game's own, then the story's.
pub fn backgrounds_for(story: Option<&Path>) -> Vec<String> {
    let mut out: Vec<String> = crate::art::BACKGROUNDS.iter().map(|s| s.to_string()).collect();
    if let Some(story) = story {
        out.extend(names(story));
    }
    out
}

/// One of the story's own paintings, cleaned; `None` if there is none (or it cannot be read).
pub fn load(story: &Path, name: &str) -> Option<PaintingData> {
    if !usable_name(name) {
        return None;
    }
    let text = std::fs::read_to_string(dir(story).join(format!("{}.json", name))).ok()?;
    let mut p: PaintingData = serde_json::from_str(&text).ok()?;
    p.clean();
    Some(p)
}

/// Save one of the story's own paintings (beside it, then renamed in: never half a file).
pub fn save(story: &Path, name: &str, painting: &PaintingData) -> Result<(), String> {
    if !usable_name(name) {
        return Err(format!("'{}' cannot name a new place (letters, digits and _, not one of the game's own)", name));
    }
    let d = dir(story);
    std::fs::create_dir_all(&d).map_err(|e| e.to_string())?;
    let tmp = d.join(format!("{}.json.tmp", name));
    std::fs::write(&tmp, serde_json::to_string_pretty(painting).unwrap_or_default() + "\n").map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, d.join(format!("{}.json", name))).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_painting_from_anywhere_is_cleaned_and_drawn() {
        let text = r##"{"title":"Rooftop | night","layers":[
            {"shape":"gradient","top":"#0a0c20","bottom":"#30284a","y0":0,"y1":400},
            {"shape":"stars","count":200,"y0":0,"y1":300,"color":"#ffffff","seed":3},
            {"shape":"poly","points":[{"x":0,"y":420},{"x":1152,"y":380},{"x":1152,"y":648},{"x":0,"y":648}],"color":"#202020"},
            {"shape":"rect","x":100,"y":300,"w":80,"h":120,"color":"#ffd080","alpha":0.8},
            {"shape":"glow","x":140,"y":360,"r":120,"color":"#ffb050","strength":0.6},
            {"shape":"ellipse","x":900,"y":120,"rx":40,"ry":40,"color":"#f0f0e0"},
            {"shape":"line","x0":0,"y0":500,"x1":1152,"y1":480,"width":6,"color":"#606060","alpha":1e9},
            {"shape":"soft","x":0,"y":350,"w":1152,"h":60,"feather":30,"color":"#8090c0","alpha":0.3}],"vignette":5}"##;
        let mut p: PaintingData = serde_json::from_str(text).unwrap();
        p.clean();
        assert_eq!(p.title, "Rooftop  night");
        assert_eq!(p.vignette, 1.0);
        assert!(matches!(p.layers[6], Layer::Line { alpha, .. } if alpha == 1.0));
        let c = p.render();
        assert_eq!((c.w, c.h), (W, H));
        // the lit window is warm, the sky dark, the ground darker still
        let at = |x: i32, y: i32| { let v = c.px[(y * W + x) as usize]; ((v >> 16) & 255, (v >> 8) & 255, v & 255) };
        let (wr, wg, wb) = at(140, 360);
        assert!(wr > 140 && wr > wb + 50, "window {:?}", (wr, wg, wb));
        let (sr, sg, sb) = at(600, 40);
        assert!(sr < 60 && sg < 60 && sb < 90, "sky {:?}", (sr, sg, sb));
        // nonsense stays harmless: wrong colours, huge sizes, too many points
        let mut odd = PaintingData { layers: vec![Layer::Rect { x: f32::NAN, y: 1e30, w: -5.0, h: 1e9, color: "red".into(), alpha: 2.0 }, Layer::Poly { points: (0..500).map(|i| Pt { x: i as f32, y: 0.0 }).collect(), color: "#fff".into(), alpha: 1.0 }], ..Default::default() };
        odd.clean();
        assert!(matches!(&odd.layers[0], Layer::Rect { color, alpha, .. } if color == "#808080" && *alpha == 1.0));
        assert!(matches!(&odd.layers[1], Layer::Poly { points, .. } if points.len() == MAX_POINTS));
        odd.render();
        // an empty painting is not a black hole
        let mut empty = PaintingData::default();
        empty.clean();
        assert_eq!(empty.layers.len(), 1);
    }

    #[test]
    fn every_style_paints_and_no_painting_takes_long() {
        let marks = r##"{"title":"Marks","style":"oil","layers":[
            {"shape":"rect","x":0,"y":0,"w":1152,"h":648,"color":"#8a7f6e"},
            {"shape":"stroke","points":[{"x":0,"y":100,"p":0.7},{"x":600,"y":120,"p":0.8},{"x":1152,"y":90,"p":0.6}],"width":120,"color":"#6a90c8","brush":"flat","load":0.28},
            {"shape":"stroke","points":[{"x":500,"y":400},{"x":560,"y":380}],"width":10,"color":"#ffe0a0","brush":"nonsense","load":0.9},
            {"shape":"wash","points":[{"x":0,"y":400},{"x":1152,"y":380},{"x":1152,"y":648},{"x":0,"y":648}],"color":"#40a060"},
            {"shape":"hatch","points":[{"x":0,"y":500},{"x":1152,"y":500},{"x":1152,"y":648},{"x":0,"y":648}],"angle":30,"spacing":6,"width":1,"color":"#101010","cross":true},
            {"shape":"dabs","x":100,"y":150,"w":300,"h":100,"count":200,"size":12,"angle":0,"color":"#e0c060","jitter":0.2}]}"##;
        let mut p: PaintingData = serde_json::from_str(marks).unwrap();
        p.clean();
        assert!(matches!(&p.layers[2], Layer::Stroke { brush, .. } if brush == "round"), "an unknown brush becomes a round one");
        let mut looks = Vec::new();
        for st in STYLES.iter() {
            p.style = st.id.to_string();
            p.clean();
            let c = p.render();
            // the sky stroke is blue in every style
            let v = c.px[(110 * W + 600) as usize];
            assert!(v & 255 > (v >> 16) & 255, "{}: the sky is blue ({:x})", st.id, v);
            looks.push(c.px);
        }
        for i in 1..looks.len() {
            assert!(looks[i] != looks[0], "{} looks different from {}", STYLES[i].id, STYLES[0].id);
        }
        let seeded: Layer = serde_json::from_str(r##"{"shape":"stroke","points":[{"x":1,"y":2}],"width":5,"color":"#ffffff","seed":-7}"##).unwrap();
        assert!(matches!(seeded, Layer::Stroke { seed, .. } if seed != 0), "any whole number is a seed");
        assert!(serde_json::from_str::<Layer>(r##"{"shape":"stars","count":3,"y0":0,"y1":9,"color":"#ffffff","seed":20260315123}"##).is_ok());
        let mut odd = PaintingData { style: "cubist".into(), ..Default::default() };
        odd.clean();
        assert_eq!(odd.style, "", "an unknown style is the matte one");
        // the most brushwork and washes a painting may hold still draws in a few seconds
        let big = Layer::Stroke { points: (0..MAX_POINTS).map(|i| Press { x: (i % 2) as f32 * 1152.0, y: i as f32 * 6.0, p: 1.0 }).collect(), width: 1e6, color: "#ffffff".into(), brush: "soft".into(), load: 1.0, alpha: 1.0, seed: 0 };
        let wash = Layer::Hatch { points: vec![Pt { x: 0.0, y: 0.0 }, Pt { x: 1152.0, y: 0.0 }, Pt { x: 1152.0, y: 648.0 }], angle: 10.0, spacing: 3.0, width: 1.0, color: "#000000".into(), alpha: 1.0, cross: true, seed: 1 };
        let mut heavy = PaintingData { layers: (0..MAX_LAYERS * 2).map(|i| if i % 2 == 0 { big.clone() } else { wash.clone() }).collect(), style: "oil".into(), ..Default::default() };
        heavy.clean();
        assert_eq!(heavy.layers.iter().filter(|l| matches!(l, Layer::Hatch { .. })).count(), MAX_AREA_LAYERS);
        // and dabs and polygons too
        let dab = Layer::Dabs { x: 0.0, y: 0.0, w: 1152.0, h: 648.0, count: 4000, size: 80.0, angle: 0.0, color: "#ff0000".into(), jitter: 0.5, seed: 1 };
        let star = Layer::Poly { points: (0..MAX_POINTS).map(|i| { let a = i as f32 * 0.0654; let r = if i % 2 == 0 { 2000.0 } else { 300.0 }; Pt { x: 576.0 + r * a.cos(), y: 324.0 + r * a.sin() } }).collect(), color: "#00ff00".into(), alpha: 0.01 };
        heavy.layers.truncate(MAX_LAYERS / 2);
        heavy.layers.extend((0..MAX_LAYERS / 2).map(|i| if i % 2 == 0 { dab.clone() } else { star.clone() }));
        heavy.clean();
        let t = std::time::Instant::now();
        heavy.render();
        assert!(t.elapsed().as_secs() < 25, "took {:?}", t.elapsed());
        eprintln!("the heaviest painting took {:?}", t.elapsed());
    }

    #[test]
    fn a_story_keeps_places_of_its_own() {
        let dir = std::env::temp_dir().join(format!("below-stairs-paint-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let story = dir.join("story.txt");
        let p = PaintingData { title: "Roof".into(), ..Default::default() };
        save(&story, "rooftop", &p).unwrap();
        assert!(save(&story, "pantry", &p).is_err(), "the game's own names are taken");
        assert!(save(&story, "../x", &p).is_err() && save(&story, "Roof", &p).is_err());
        std::fs::write(dir.join("paintings").join("broken.json"), "{").unwrap();
        assert_eq!(names(&story), vec!["broken".to_string(), "rooftop".to_string()]);
        assert!(load(&story, "rooftop").is_some() && load(&story, "broken").is_none());
        assert_eq!(backgrounds_for(Some(&story)).len(), crate::art::BACKGROUNDS.len() + 2);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
