//! Painting by hand, simulated: the marks a painting can be made of besides
//! flat shapes, and the finishes that give each art style its surface.
//!
//! The brush stroke follows the approach of aibrush
//! (github.com/neokrasav4ik/aibrush): there are no stamps or textures. A
//! stroke is a path with pressure, and the mark it leaves comes from the
//! brush's hairs, each carrying its own paint, which runs out as it travels,
//! picks up the wet paint it passes over, skips across the canvas tooth when
//! it runs dry, and leaves ridges of paint (impasto) that catch the light.
//! This is a much smaller model than aibrush's, written for this renderer.
//!
//! Colours are mixed in linear light, as paint and light mix.

use crate::gfx::{hash, hashf, noise2, Canvas};

// ------------------------------------------------------------- colour

fn to_lin(v: u32) -> f32 {
    let c = v as f32 / 255.0;
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

fn from_lin(c: f32) -> u32 {
    let c = c.clamp(0.0, 1.0);
    let s = if c <= 0.003_130_8 { c * 12.92 } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 };
    (s * 255.0 + 0.5) as u32
}

/// A table for the way in (256 entries), so that reading a pixel is cheap.
fn lin_table() -> &'static [f32; 256] {
    static T: std::sync::OnceLock<[f32; 256]> = std::sync::OnceLock::new();
    T.get_or_init(|| {
        let mut t = [0.0; 256];
        for (i, v) in t.iter_mut().enumerate() {
            *v = to_lin(i as u32);
        }
        t
    })
}

pub type Lin = [f32; 3];

pub fn lin(c: u32) -> Lin {
    let t = lin_table();
    [t[((c >> 16) & 255) as usize], t[((c >> 8) & 255) as usize], t[(c & 255) as usize]]
}

/// The way out, by table too (fine enough steps that no two neighbouring
/// 8-bit values are confused).
fn out_table() -> &'static [u8; 8192] {
    static T: std::sync::OnceLock<[u8; 8192]> = std::sync::OnceLock::new();
    T.get_or_init(|| {
        let mut t = [0u8; 8192];
        for (i, v) in t.iter_mut().enumerate() {
            *v = from_lin((i as f32 + 0.5) / 8192.0) as u8;
        }
        t
    })
}

pub fn pack(l: Lin) -> u32 {
    let t = out_table();
    let o = |c: f32| {
        // (the darkest values change fastest: worked out exactly)
        if c < 0.004 {
            from_lin(c)
        } else {
            t[((c.min(1.0) * 8192.0) as usize).min(8191)] as u32
        }
    };
    0xFF00_0000 | o(l[0]) << 16 | o(l[1]) << 8 | o(l[2])
}

fn mix(a: Lin, b: Lin, t: f32) -> Lin {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]
}

// -------------------------------------------------------------- brushes

/// What a brush is like. (After aibrush's presets: the same names, in English.)
#[derive(Clone, Copy, Debug)]
pub struct Brush {
    /// How many hairs it lays down side by side.
    pub hairs: usize,
    /// A flat brush keeps its width whatever the pressure; a round one swells with it.
    pub flat: bool,
    /// How sharply a round brush's width follows pressure (0: not at all).
    pub tip: f32,
    /// How much the hairs' tones differ.
    pub tone: f32,
    /// How far the paint lasts, in pixels of travel at full load.
    pub reservoir: f32,
    /// How readily the hairs skip over the canvas (dry brush).
    pub gaps: f32,
    /// How soft each hair's mark is (0 crisp, 1 soft).
    pub soft: f32,
    /// Hairs spread apart, with gaps between them (a fan brush).
    pub spread: f32,
    /// Hairs out of line and of different lengths (a worn brush).
    pub ragged: f32,
    /// A palette knife: one flat blade, not hairs.
    pub knife: bool,
    /// Spacing between the hairs' touches, as a multiple of the usual (more is
    /// quicker and coarser: for a painting with very much brushwork).
    pub coarse: f32,
}

/// The brushes a stroke can be made with.
pub const BRUSHES: [&str; 9] = ["flat", "narrow_flat", "round", "soft", "dry", "liner", "fan", "ragged", "knife"];

pub fn brush(name: &str) -> Brush {
    let b = Brush { hairs: 40, flat: true, tip: 0.3, tone: 0.06, reservoir: 900.0, gaps: 0.06, soft: 0.45, spread: 1.0, ragged: 0.0, knife: false, coarse: 1.0 };
    match name {
        "narrow_flat" => Brush { hairs: 22, ..b },
        "round" => Brush { hairs: 30, flat: false, tip: 1.0, tone: 0.05, reservoir: 800.0, ..b },
        "soft" => Brush { hairs: 56, flat: false, tip: 0.6, tone: 0.025, gaps: 0.0, soft: 1.0, reservoir: 1100.0, ..b },
        "dry" => Brush { hairs: 30, tone: 0.1, reservoir: 260.0, gaps: 0.5, soft: 0.25, ..b },
        "liner" => Brush { hairs: 6, flat: false, tip: 1.2, tone: 0.03, reservoir: 2600.0, gaps: 0.02, soft: 0.4, ..b },
        "fan" => Brush { hairs: 24, tone: 0.08, gaps: 0.3, soft: 0.2, spread: 1.7, reservoir: 600.0, ..b },
        "ragged" => Brush { hairs: 34, tone: 0.12, gaps: 0.32, soft: 0.3, ragged: 0.6, reservoir: 500.0, ..b },
        "knife" => Brush { hairs: 1, tone: 0.0, gaps: 0.12, soft: 0.0, knife: true, reservoir: 700.0, ..b },
        _ => b,
    }
}

/// About how many pixels a stroke touches (its cost to draw), for a path
/// `length` long and `width` wide.
pub fn stroke_cost(length: f32, width: f32, b: &Brush) -> f32 {
    let hairs = if b.knife { 1.0 } else { b.hairs.max(1) as f32 };
    let r = if b.knife { width / 2.0 } else { (width / hairs * (1.1 + b.soft * 1.2) * b.spread.recip().max(0.5)).max(0.7) };
    let step = (r * 0.55).max(0.6);
    hairs * (length / step) * (2.0 * r + 3.0).powi(2)
}

/// One point of a stroke's path: where, and how hard the brush presses (0 to 1).
#[derive(Clone, Copy, Debug)]
pub struct Touch {
    pub x: f32,
    pub y: f32,
    pub p: f32,
}

/// The paint's height above the canvas, for impasto: one value per pixel.
pub struct Relief {
    pub w: i32,
    pub h: i32,
    pub z: Vec<f32>,
    pub used: bool,
}

impl Relief {
    pub fn new(w: i32, h: i32) -> Relief {
        Relief { w, h, z: vec![0.0; (w * h) as usize], used: false }
    }
}

/// The path, evenly spaced every `step` pixels, smoothed through its points
/// (Catmull-Rom), with pressure carried along.
fn resample(pts: &[Touch], step: f32) -> Vec<Touch> {
    if pts.len() < 2 {
        return pts.to_vec();
    }
    // a smooth curve through the points first, finely
    let mut fine = Vec::new();
    for i in 0..pts.len() - 1 {
        let p0 = pts[i.saturating_sub(1)];
        let (p1, p2) = (pts[i], pts[i + 1]);
        let p3 = pts[(i + 2).min(pts.len() - 1)];
        let seg = ((p2.x - p1.x).hypot(p2.y - p1.y) / 2.0).ceil().clamp(1.0, 400.0) as usize;
        for k in 0..seg {
            let t = k as f32 / seg as f32;
            let (t2, t3) = (t * t, t * t * t);
            let cr = |a: f32, b: f32, c: f32, d: f32| 0.5 * (2.0 * b + (-a + c) * t + (2.0 * a - 5.0 * b + 4.0 * c - d) * t2 + (-a + 3.0 * b - 3.0 * c + d) * t3);
            fine.push(Touch { x: cr(p0.x, p1.x, p2.x, p3.x), y: cr(p0.y, p1.y, p2.y, p3.y), p: p1.p + (p2.p - p1.p) * t });
        }
    }
    fine.push(*pts.last().unwrap());
    // then evenly along it
    let mut out = vec![fine[0]];
    let mut carry = 0.0;
    for w in fine.windows(2) {
        let (a, b) = (w[0], w[1]);
        let len = (b.x - a.x).hypot(b.y - a.y);
        let mut d = step - carry;
        while d <= len {
            let t = d / len.max(1e-6);
            out.push(Touch { x: a.x + (b.x - a.x) * t, y: a.y + (b.y - a.y) * t, p: a.p + (b.p - a.p) * t });
            d += step;
        }
        carry = len - (d - step);
        if out.len() > 20_000 {
            break;
        }
    }
    out
}

/// Lay down one round mark of a hair: `amount` of `paint`, mixed with what
/// is there in proportion to `pick` (wet paint picked up and dragged).
fn dab(c: &mut Canvas, relief: &mut Relief, x: f32, y: f32, r: f32, paint: Lin, amount: f32, soft: f32, ridge: f32) {
    let (x0, x1) = ((x - r - 1.0).floor() as i32, (x + r + 1.0).ceil() as i32);
    let (y0, y1) = ((y - r - 1.0).floor() as i32, (y + r + 1.0).ceil() as i32);
    for py in y0.max(0)..=y1.min(c.h - 1) {
        for px in x0.max(0)..=x1.min(c.w - 1) {
            let d = ((px as f32 + 0.5 - x).powi(2) + (py as f32 + 0.5 - y).powi(2)).sqrt();
            let edge = (r + 0.5 - d).clamp(0.0, 1.0);
            if edge <= 0.0 {
                continue;
            }
            let fall = if soft > 0.0 { (1.0 - (d / (r + 0.5)).min(1.0)).powf(0.4 + soft * 1.2) } else { 1.0 };
            let a = (amount * edge * fall).clamp(0.0, 1.0);
            if a <= 0.002 {
                continue;
            }
            let i = (py * c.w + px) as usize;
            c.px[i] = pack(mix(lin(c.px[i]), paint, a));
            if ridge != 0.0 {
                // paint piles up, but not without end: a new layer mostly covers the ridges under it
                let z = &mut relief.z[i];
                *z = *z * (1.0 - a * 0.5) + ridge * a;
            }
        }
    }
}

/// A brush stroke along `path`, `width` pixels wide at full pressure.
/// `load` is how much paint the brush carries (0: a clean, damp brush that
/// only moves the paint already there; 0.1 a scrub; 0.3 ordinary; 0.9
/// thick, impasto). `opacity` is how much the paint hides what is under it
/// (low: a glaze).
#[allow(clippy::too_many_arguments)]
pub fn stroke(c: &mut Canvas, relief: &mut Relief, path: &[Touch], width: f32, color: u32, b: &Brush, load: f32, opacity: f32, seed: u32) {
    if path.is_empty() || width <= 0.0 {
        return;
    }
    let paint = lin(color);
    let load = load.clamp(0.0, 1.0);
    let hairs = if b.knife { 1 } else { b.hairs.max(1) };
    let hair_r = |w: f32| if b.knife { w / 2.0 } else { (w / hairs as f32 * (1.1 + b.soft * 1.2) * b.spread.recip().max(0.5)).max(0.7) };
    let step = (hair_r(width) * 0.55).max(0.6) * b.coarse.clamp(1.0, 5000.0);
    let pts = resample(path, step);
    if pts.len() < 2 && !pts.is_empty() {
        // a touch, not a stroke: one dab of each hair
        let t = pts[0];
        let w = width * if b.flat { 0.8 } else { 0.25 + 0.75 * t.p.clamp(0.0, 1.0) };
        dab(c, relief, t.x, t.y, w / 2.0, paint, opacity * (0.4 + 0.6 * load.max(0.05)), b.soft, load * 0.5);
        relief.used = true;
        return;
    }
    // each hair: where it sits across the brush, its tone, how much paint, when it touches
    struct Hair {
        at: f32,
        paint: Lin,
        store: f32,
        carried: Option<Lin>,
        late: f32,
        early: f32,
    }
    let travel_total = (pts.len() as f32) * step;
    let mut set: Vec<Hair> = (0..hairs)
        .map(|i| {
            let i32_ = i as i32;
            let u = if hairs == 1 { 0.0 } else { i as f32 / (hairs - 1) as f32 - 0.5 };
            let jitter = (hashf(i32_, 1, seed) - 0.5) / hairs as f32 + (hashf(i32_, 2, seed) - 0.5) * b.ragged * 0.25;
            let k = 1.0 + (hashf(i32_, 3, seed) - 0.5) * 2.0 * b.tone;
            let tint = (hashf(i32_, 4, seed) - 0.5) * b.tone * 0.6;
            Hair {
                at: (u + jitter) * b.spread.min(1.0),
                paint: [paint[0] * (k + tint), paint[1] * k, paint[2] * (k - tint)],
                // (the reservoir is the travel an ordinary load, 0.3, lasts)
                store: (load.max(0.02) / 0.3).powf(0.8) * b.reservoir * (0.75 + 0.5 * hashf(i32_, 5, seed)),
                carried: None,
                // hairs are not all the same length: they touch down and lift a little apart,
                // the outer hairs of a round brush later and sooner (a pointed mark)
                late: hashf(i32_, 6, seed) * (b.ragged * travel_total * 0.25 + width * 0.12) + if b.flat { 0.0 } else { u.abs().powi(2) * width * 1.2 },
                early: hashf(i32_, 7, seed) * (b.ragged * travel_total * 0.25 + width * 0.12) + if b.flat { 0.0 } else { u.abs().powi(2) * width * 1.0 },
            }
        })
        .collect();
    for (k, t) in pts.iter().enumerate() {
        let prev = pts[k.saturating_sub(1)];
        let next = pts[(k + 1).min(pts.len() - 1)];
        let (mut tx, mut ty) = (next.x - prev.x, next.y - prev.y);
        let len = tx.hypot(ty).max(1e-6);
        tx /= len;
        ty /= len;
        let (nx, ny) = (-ty, tx);
        let p = t.p.clamp(0.0, 1.0);
        let w = width * if b.flat { 0.72 + 0.28 * p } else { (0.12 + 0.88 * p.powf(b.tip.max(0.05))).min(1.0) };
        let travel = k as f32 * step;
        // the first and last moments of a stroke are lighter: the brush lands and lifts
        let land = ((travel / (width * 0.25 + 4.0)).min(1.0)) * (((travel_total - travel) / (width * 0.2 + 4.0)).min(1.0));
        let r = hair_r(w);
        for (i, h) in set.iter_mut().enumerate() {
            if travel < h.late || travel > travel_total - h.early {
                continue;
            }
            let x = t.x + nx * h.at * w;
            let y = t.y + ny * h.at * w;
            let (px, py) = (x as i32, y as i32);
            if px < -50 || py < -50 || px > c.w + 50 || py > c.h + 50 {
                continue;
            }
            let under = if px >= 0 && py >= 0 && px < c.w && py < c.h { lin(c.px[(py * c.w + px) as usize]) } else { paint };
            // how much paint this hair still has, from full to none
            let left = (h.store / (b.reservoir * 0.35).max(1.0)).clamp(0.0, 1.0);
            h.store -= step * (0.5 + 0.5 * p);
            // dry brush: the hair skips over the canvas's tooth (the weave's raised threads), more as it runs
            // dry and the lighter it presses: streaks along the stroke, broken by the weave
            let weave = ((px as f32 * 1.35).sin() * (py as f32 * 1.35).sin()) * 0.5 + 0.5;
            let tooth = noise2(travel * 0.05 + i as f32 * 13.7, i as f32 * 3.1, seed) * 0.8 + weave * 0.2;
            if tooth < b.gaps + (1.0 - left) * 0.55 - p * 0.15 {
                continue;
            }
            // what it lays down: its own paint, and some of the wet paint it has been dragging
            let carried = h.carried.get_or_insert(under);
            // (all of it with a clean brush; about a quarter with an ordinary load; none with thick paint;
            // and more as the hair runs dry)
            let pick = (1.0 - load).powi(4);
            let colour = mix(h.paint, *carried, pick + (1.0 - pick) * (1.0 - left) * 0.5);
            *carried = mix(*carried, under, 0.06);
            let amount = opacity * land * (0.3 + 0.7 * p) * if load <= 0.0 { 0.6 } else { 0.35 + 0.65 * left.max(0.15) };
            // each hair leaves its own height of paint: the grooves a bristle brush ploughs
            let ridge = if b.knife { load * 0.25 } else { load * 0.55 * left * (0.3 + 1.4 * hashf(i as i32, 8, seed)) };
            dab(c, &mut *relief, x, y, r, colour, amount, b.soft, ridge);
        }
        if b.knife {
            // the blade leaves a raised edge on each side
            for side in [-0.5f32, 0.5] {
                let (x, y) = (t.x + nx * side * w, t.y + ny * side * w);
                dab(c, &mut *relief, x, y, 1.2, mix(paint, [paint[0] * 0.85, paint[1] * 0.85, paint[2] * 0.85], 0.5), opacity * land * 0.4, 0.0, load * 0.8);
            }
        }
    }
    relief.used = relief.used || load > 0.0;
}

// ------------------------------------------------------- other marks

/// How much of each pixel of a box lies inside a polygon (0 to 1), and the box.
fn coverage(points: &[(f32, f32)], pad: i32, cw: i32, ch: i32) -> Option<(i32, i32, i32, i32, Vec<f32>)> {
    if points.len() < 3 {
        return None;
    }
    let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
    for &(x, y) in points {
        x0 = x0.min(x);
        y0 = y0.min(y);
        x1 = x1.max(x);
        y1 = y1.max(y);
    }
    let bx = (x0.floor() as i32 - pad).max(-pad).max(0);
    let by = (y0.floor() as i32 - pad).max(0);
    let bw = ((x1.ceil() as i32 + pad).min(cw) - bx).max(0);
    let bh = ((y1.ceil() as i32 + pad).min(ch) - by).max(0);
    if bw == 0 || bh == 0 {
        return None;
    }
    let mut m = Canvas::new(bw, bh);
    m.clear(0xFF00_0000);
    let local: Vec<(f32, f32)> = points.iter().map(|&(x, y)| (x - bx as f32, y - by as f32)).collect();
    m.poly_aa(&local, 0xFFFF_FFFF);
    Some((bx, by, bw, bh, m.px.iter().map(|p| ((p >> 16) & 255) as f32 / 255.0).collect()))
}

fn blurred(v: &[f32], w: i32, h: i32, r: i32) -> Vec<f32> {
    let mut a = v.to_vec();
    let mut b = vec![0.0; v.len()];
    for _ in 0..2 {
        for y in 0..h {
            let mut acc = 0.0;
            let at = |x: i32| a[(y * w + x.clamp(0, w - 1)) as usize];
            for x in -r..=r {
                acc += at(x);
            }
            for x in 0..w {
                b[(y * w + x) as usize] = acc / (2 * r + 1) as f32;
                acc += at(x + r + 1) - at(x - r);
            }
        }
        for x in 0..w {
            let mut acc = 0.0;
            let at = |y: i32| b[(y.clamp(0, h - 1) * w + x) as usize];
            for y in -r..=r {
                acc += at(y);
            }
            for y in 0..h {
                a[(y * w + x) as usize] = acc / (2 * r + 1) as f32;
                acc += at(y + r + 1) - at(y - r);
            }
        }
    }
    a
}

/// A watercolour wash over a polygon: transparent pigment that darkens what
/// is under it (light passes through it and back off the paper), pooling at
/// the wash's edge as it dries, settling into the paper's grain. `wet` from
/// 0 (a crisp wash on dry paper) to 1 (wet into wet: soft, blooming edges).
pub fn wash(c: &mut Canvas, points: &[(f32, f32)], color: u32, alpha: f32, wet: f32, seed: u32) {
    let wet = wet.clamp(0.0, 1.0);
    let spread = (2.0 + wet * 14.0) as i32;
    let Some((bx, by, bw, bh, mask)) = coverage(points, spread * 2 + 2, c.w, c.h) else { return };
    let soft = blurred(&mask, bw, bh, spread);
    let pigment = lin(color);
    for y in 0..bh {
        for x in 0..bw {
            let i = (y * bw + x) as usize;
            let m = mask[i] + (soft[i] - mask[i]) * (0.25 + wet * 0.7);
            if m <= 0.003 {
                continue;
            }
            // the rim: inside the shape, near its edge (less of one when the paper was wet)
            let rim = (mask[i] - soft[i]).max(0.0) * (2.2 - wet * 1.4);
            let (gx, gy) = (bx + x, by + y);
            let grain = noise2(gx as f32 * 0.21, gy as f32 * 0.21, seed) * 0.6 + hashf(gx, gy, seed ^ 0x55) * 0.4;
            let bloom = if wet > 0.4 { (noise2(gx as f32 * 0.02, gy as f32 * 0.02, seed + 9) - 0.5) * wet * 0.5 } else { 0.0 };
            let a = (alpha * (m * (0.78 + 0.32 * grain + bloom) + rim * 0.9)).clamp(0.0, 1.0);
            let j = (gy * c.w + gx) as usize;
            let under = lin(c.px[j]);
            // a glaze: light through the pigment, multiplied
            let over = [under[0] * pigment[0], under[1] * pigment[1], under[2] * pigment[2]];
            c.px[j] = pack(mix(under, over, a));
        }
    }
}

/// Lines of ink across a polygon, `spacing` apart at `angle` degrees (and
/// across them too, if `cross`), each `width` wide and a little unsteady.
#[allow(clippy::too_many_arguments)]
pub fn hatch(c: &mut Canvas, points: &[(f32, f32)], angle: f32, spacing: f32, width: f32, color: u32, alpha: f32, cross: bool, seed: u32) {
    let Some((bx, by, bw, bh, mask)) = coverage(points, 1, c.w, c.h) else { return };
    let ink = lin(color);
    let spacing = spacing.max(2.0);
    let half = (width / 2.0).max(0.3);
    let dirs: Vec<f32> = if cross { vec![angle, angle + 90.0] } else { vec![angle] };
    for y in 0..bh {
        for x in 0..bw {
            let i = (y * bw + x) as usize;
            if mask[i] <= 0.0 {
                continue;
            }
            let (gx, gy) = ((bx + x) as f32, (by + y) as f32);
            let mut cover: f32 = 0.0;
            for (k, a) in dirs.iter().enumerate() {
                let (s, co) = a.to_radians().sin_cos();
                let along = gx * co + gy * s;
                let across = -gx * s + gy * co;
                let wobble = (noise2(along * 0.015, across / spacing, seed + k as u32) - 0.5) * spacing * 0.35;
                let line = ((across + wobble) / spacing).round();
                let d = (across + wobble - line * spacing).abs();
                // pen pressure varies along each line; some lines break off
                let press = 0.6 + 0.4 * noise2(along * 0.03, line * 1.7, seed + 31 + k as u32);
                if noise2(along * 0.01, line * 2.3, seed + 57 + k as u32) < 0.12 {
                    continue;
                }
                cover = cover.max((half * press + 0.5 - d).clamp(0.0, 1.0));
            }
            if cover > 0.0 {
                let j = ((by + y) * c.w + bx + x) as usize;
                c.px[j] = pack(mix(lin(c.px[j]), ink, (cover * mask[i] * alpha).clamp(0.0, 1.0)));
            }
        }
    }
}

/// Many short touches of a round brush scattered over a box, each a little
/// different in colour (broken colour, as the Impressionists laid it).
#[allow(clippy::too_many_arguments)]
pub fn dabs(c: &mut Canvas, relief: &mut Relief, x: f32, y: f32, w: f32, h: f32, count: u32, size: f32, angle: f32, color: u32, jitter: f32, seed: u32, coarse: f32) {
    let base = lin(color);
    let b = Brush { hairs: 10, coarse, ..brush("round") };
    for i in 0..count.min(4000) as i32 {
        let px = x + hashf(i, 11, seed) * w;
        let py = y + hashf(i, 12, seed) * h;
        let a = (angle + (hashf(i, 13, seed) - 0.5) * 50.0).to_radians();
        let len = size * (0.8 + hashf(i, 14, seed) * 0.9);
        let (dx, dy) = (a.cos() * len * 0.5, a.sin() * len * 0.5);
        let k = |n: i32| 1.0 + (hashf(i, n, seed) - 0.5) * 2.0 * jitter;
        let col = pack([base[0] * k(15), base[1] * k(16), base[2] * k(17)]);
        let path = [Touch { x: px - dx, y: py - dy, p: 0.7 }, Touch { x: px + dx, y: py + dy, p: 0.5 }];
        stroke(c, relief, &path, size * 0.7, col, &b, 0.4, 0.92, hash(i, 18, seed));
    }
}

// --------------------------------------------------------- finishes

/// Light across the paint's ridges, from the upper left (impasto).
pub fn light_relief(c: &mut Canvas, relief: &Relief, strength: f32) {
    if !relief.used {
        return;
    }
    let (w, h) = (relief.w, relief.h);
    // a little softening, so ridges are paint, not pixels
    let z = blurred(&relief.z, w, h, 1);
    for y in 1..h - 1 {
        for x in 1..w - 1 {
            let i = (y * w + x) as usize;
            let slope = z[i - w as usize - 1] - z[i + w as usize + 1];
            if slope.abs() < 1e-4 {
                continue;
            }
            let k = (1.0 + slope * strength).clamp(0.55, 1.6);
            let l = lin(c.px[i]);
            // where the ridge faces the light it also shines a little
            let shine = (slope * strength - 0.25).max(0.0) * 0.12;
            c.px[i] = pack([l[0] * k + shine, l[1] * k + shine, l[2] * k + shine]);
        }
    }
}

/// Darken or lighten each pixel by `f(x, y)` (a texture: canvas weave, paper).
fn texture(c: &mut Canvas, f: impl Fn(i32, i32) -> f32) {
    for y in 0..c.h {
        for x in 0..c.w {
            let i = (y * c.w + x) as usize;
            let l = lin(c.px[i]);
            let k = f(x, y);
            c.px[i] = pack([l[0] * k, l[1] * k, l[2] * k]);
        }
    }
}

/// The weave of a primed linen canvas.
pub fn canvas_weave(c: &mut Canvas, strength: f32) {
    texture(c, |x, y| {
        let warp = ((x as f32 * 1.35).sin() * 0.5 + 0.5) * ((y as f32 * 0.45 + (x / 3) as f32).sin() * 0.5 + 0.5);
        let weft = ((y as f32 * 1.35).sin() * 0.5 + 0.5) * ((x as f32 * 0.45 + (y / 3) as f32).sin() * 0.5 + 0.5);
        1.0 - strength * (warp.max(weft) * 0.7 + hashf(x, y, 4242) * 0.3)
    });
}

/// The grain and fibres of paper.
pub fn paper(c: &mut Canvas, strength: f32, seed: u32) {
    texture(c, |x, y| {
        let grain = noise2(x as f32 * 0.35, y as f32 * 0.35, seed) * 0.5 + noise2(x as f32 * 0.06, y as f32 * 0.06, seed + 1) * 0.3 + hashf(x, y, seed + 2) * 0.2;
        1.0 - strength * (1.0 - grain)
    });
}

/// The painting in `k` colours, found from the painting itself (k-means),
/// so that hues stay true; with `block` > 1 in big square pixels, and with
/// `dither` an ordered pattern between neighbouring colours.
pub fn palette(c: &mut Canvas, k: usize, dither: f32, block: i32) {
    let k = k.clamp(2, 64);
    let block = block.max(1);
    let (w, h) = (c.w, c.h);
    let rgbf = |p: u32| [((p >> 16) & 255) as f32, ((p >> 8) & 255) as f32, (p & 255) as f32];
    // block averages first (the big pixels)
    let (bw, bh) = ((w + block - 1) / block, (h + block - 1) / block);
    let mut cells = vec![[0.0f32; 3]; (bw * bh) as usize];
    for by in 0..bh {
        for bx in 0..bw {
            let (mut sum, mut n) = ([0.0f32; 3], 0.0);
            for y in by * block..((by + 1) * block).min(h) {
                for x in bx * block..((bx + 1) * block).min(w) {
                    let v = rgbf(c.px[(y * w + x) as usize]);
                    for j in 0..3 {
                        sum[j] += v[j];
                    }
                    n += 1.0;
                }
            }
            cells[(by * bw + bx) as usize] = [sum[0] / n, sum[1] / n, sum[2] / n];
        }
    }
    // k-means on a spread-out sample, started from colours spaced through the range of lightness
    let stride = (cells.len() / 6000).max(1);
    let sample: Vec<[f32; 3]> = cells.iter().step_by(stride).copied().collect();
    // ("redmean": a cheap distance between colours close to how different they look)
    let dist = |c: &[f32; 3], v: &[f32; 3]| {
        let r = (c[0] + v[0]) / 2.0;
        (2.0 + r / 256.0) * (c[0] - v[0]).powi(2) + 4.0 * (c[1] - v[1]).powi(2) + (2.0 + (255.0 - r) / 256.0) * (c[2] - v[2]).powi(2)
    };
    let near = |centres: &[[f32; 3]], v: [f32; 3]| {
        let mut best = (0, f32::MAX);
        for (i, c) in centres.iter().enumerate() {
            let d = dist(c, &v);
            if d < best.1 {
                best = (i, d);
            }
        }
        best.0
    };
    // started from the most distinct colours (each the farthest from those already chosen),
    // so that a small accent, a red roof, keeps a colour of its own
    let mean = sample.iter().fold([0.0f32; 3], |a, v| [a[0] + v[0], a[1] + v[1], a[2] + v[2]]).map(|x| x / sample.len().max(1) as f32);
    let mut centres: Vec<[f32; 3]> = vec![mean];
    let mut gap: Vec<f32> = sample.iter().map(|v| dist(&mean, v)).collect();
    while centres.len() < k {
        let (far, d) = gap.iter().enumerate().fold((0, -1.0f32), |b, (i, &d)| if d > b.1 { (i, d) } else { b });
        if d <= 1.0 {
            break;
        }
        let c = sample[far];
        centres.push(c);
        for (g, v) in gap.iter_mut().zip(&sample) {
            *g = g.min(dist(&c, v));
        }
    }
    for _ in 0..10 {
        let mut acc = vec![[0.0f32; 4]; k];
        for v in &sample {
            let i = near(&centres, *v);
            for j in 0..3 {
                acc[i][j] += v[j];
            }
            acc[i][3] += 1.0;
        }
        for (c, a) in centres.iter_mut().zip(&acc) {
            if a[3] > 0.0 {
                *c = [a[0] / a[3], a[1] / a[3], a[2] / a[3]];
            }
        }
    }
    for by in 0..bh {
        for bx in 0..bw {
            let mut v = cells[(by * bw + bx) as usize];
            if dither > 0.0 {
                let t = ((crate::gfx::bayer(bx, by) as f32 + 0.5) / 16.0 - 0.5) * dither;
                for x in v.iter_mut() {
                    *x += t;
                }
            }
            let col = centres[near(&centres, v)];
            let packed = 0xFF00_0000 | (col[0].round().clamp(0.0, 255.0) as u32) << 16 | (col[1].round().clamp(0.0, 255.0) as u32) << 8 | col[2].round().clamp(0.0, 255.0) as u32;
            for y in by * block..((by + 1) * block).min(h) {
                for x in bx * block..((bx + 1) * block).min(w) {
                    c.px[(y * w + x) as usize] = packed;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(c: &Canvas, x: i32, y: i32) -> (u32, u32, u32) {
        let v = c.px[(y * c.w + x) as usize];
        ((v >> 16) & 255, (v >> 8) & 255, v & 255)
    }

    #[test]
    fn a_stroke_lays_down_paint_that_runs_out_and_stands_up() {
        let mut c = Canvas::new(400, 120);
        c.clear(crate::gfx::rgb(150, 141, 126));
        let mut relief = Relief::new(400, 120);
        let path = [Touch { x: 20.0, y: 60.0, p: 0.7 }, Touch { x: 200.0, y: 55.0, p: 0.8 }, Touch { x: 380.0, y: 62.0, p: 0.5 }];
        stroke(&mut c, &mut relief, &path, 40.0, crate::gfx::rgb(200, 40, 30), &brush("dry"), 0.3, 1.0, 7);
        // red where it began, with gaps of canvas showing further along as the paint ran out
        let red = |x0: i32, x1: i32| (x0..x1).flat_map(|x| (45..75).map(move |y| (x, y))).filter(|&(x, y)| { let (r, g, _) = at(&c, x, y); r > g + 40 }).count();
        let (start, end) = (red(40, 100), red(300, 360));
        assert!(start > 600, "the stroke starts with paint: {}", start);
        assert!(end < start, "and runs dry: {} then {}", start, end);
        assert!(relief.used && relief.z.iter().any(|&z| z > 0.05), "it leaves paint standing on the canvas");
        // off the stroke, the canvas is untouched
        assert_eq!(at(&c, 200, 5), (150, 141, 126));
        // a clean brush (no load) only smears what is there: nothing red appears where there was none
        let mut d = Canvas::new(100, 60);
        d.clear(crate::gfx::rgb(20, 20, 200));
        let mut rel = Relief::new(100, 60);
        stroke(&mut d, &mut rel, &[Touch { x: 10.0, y: 30.0, p: 0.8 }, Touch { x: 90.0, y: 30.0, p: 0.8 }], 20.0, crate::gfx::rgb(255, 0, 0), &brush("soft"), 0.0, 1.0, 1);
        let (r, _, b) = at(&d, 50, 30);
        assert!(r < 120 && b > 120, "a clean brush drags the blue, it does not paint red: {:?}", (r, b));
        // every brush draws, whatever it is given
        for name in BRUSHES {
            let mut e = Canvas::new(200, 100);
            e.clear(0xFF000000);
            let mut rel = Relief::new(200, 100);
            stroke(&mut e, &mut rel, &[Touch { x: 10.0, y: 50.0, p: 1.0 }, Touch { x: 190.0, y: 50.0, p: 1.0 }], 30.0, 0xFFFFFFFF, &brush(name), 0.5, 1.0, 3);
            assert!(e.px.iter().any(|&p| p & 0xFF > 100), "{} leaves a mark", name);
            stroke(&mut e, &mut rel, &[Touch { x: -500.0, y: 1e6, p: 9.0 }], 1e4, 0xFFFFFFFF, &brush(name), 9.0, 9.0, 3);
        }
    }

    #[test]
    fn washes_hatching_and_finishes() {
        let mut c = Canvas::new(300, 200);
        c.clear(crate::gfx::rgb(240, 236, 226));
        let square = [(50.0, 50.0), (250.0, 50.0), (250.0, 150.0), (50.0, 150.0)];
        wash(&mut c, &square, crate::gfx::rgb(60, 110, 200), 0.8, 0.0, 5);
        let (mid, rim) = (at(&c, 150, 100), at(&c, 53, 100));
        assert!(mid.2 > mid.0 + 30, "a blue wash: {:?}", mid);
        assert!(rim.0 < mid.0, "darker where it pooled at the edge: {:?} {:?}", rim, mid);
        assert_eq!(at(&c, 10, 10), (240, 236, 226), "and none outside");
        let mut d = Canvas::new(200, 200);
        d.clear(0xFFFFFFFF);
        hatch(&mut d, &[(0.0, 0.0), (200.0, 0.0), (200.0, 200.0), (0.0, 200.0)], 45.0, 8.0, 1.5, 0xFF000000, 1.0, false, 2);
        let dark = d.px.iter().filter(|&&p| p & 0xFF < 128).count();
        assert!(dark > 2000 && dark < 30000, "lines, not a fill: {}", dark);
        // a palette of the picture's own colours: a red and blue picture stays red and blue
        let mut f = Canvas::new(80, 40);
        for (i, p) in f.px.iter_mut().enumerate() {
            let x = (i % 80) as u32;
            *p = if x < 40 { 0xFF000000 | (150 + x) << 16 | 20 } else { 0xFF000000 | 30 << 8 | (150 + x) };
        }
        palette(&mut f, 2, 0.0, 1);
        let colours: std::collections::BTreeSet<u32> = f.px.iter().copied().collect();
        assert_eq!(colours.len(), 2);
        let (l, r) = (f.px[0], f.px[79]);
        assert!((l >> 16) & 255 > 140 && l & 255 < 40 && r & 255 > 140, "{:x} {:x}", l, r);
    }
}
