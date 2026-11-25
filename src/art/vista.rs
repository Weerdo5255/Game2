//! The interior of the cylinder seen from the estate lawn, looking down the
//! axis: three strips of land and three of window curving up and overhead,
//! converging on the far end cap, with the sunline blazing along the axis.
//! By day the key light is the sunline; by night it dims to moonlight, the
//! land overhead becomes a sky of town lights and the manor glows.

use super::paint::*;
use crate::gfx::*;
use std::f32::consts::{PI, TAU};

/// Vanishing point of the axis. We stand on the inner surface, so it sits
/// on the horizon and the whole 360 degrees of the cylinder fans out above
/// it: a strip at angle phi around the hull appears at screen angle phi/2.
const VP: (f32, f32) = (576.0, 424.0);
const HORIZON: f32 = 432.0;

/// Colour of the land at cylinder coordinates (s around, d along).
fn land(s: f32, d: f32, detail: f32, night: bool) -> V3 {
    // big regions: forest, farmland, water
    let region = fbm(s * 1.5, d * 0.9, 4, 101);
    let moist = fbm(s * 0.9 + 9.0, d * 0.55, 3, 103);
    let (cs, cd) = (s * 8.0, d * 4.6);
    let (fs, fd) = (cs - cs.floor(), cd - cd.floor());
    let id = hash(cs.floor() as i32, cd.floor() as i32, 105);
    let field = match id % 7 {
        0 | 1 => [0.36, 0.56, 0.24],
        2 => [0.46, 0.62, 0.26],
        3 => [0.74, 0.68, 0.36],
        4 => [0.30, 0.48, 0.22],
        5 => [0.58, 0.62, 0.30],
        _ => [0.46, 0.38, 0.24],
    };
    let avg = [0.42, 0.55, 0.27];
    // furrows inside a field, hedgerows and lanes between them
    let furrow = 0.95 + 0.05 * ((if id % 2 == 0 { fs } else { fd }) * 22.0).sin() * (1.0 - detail);
    let hedge = smoothstep(0.07, 0.025, fs.min(1.0 - fs).min(fd.min(1.0 - fd)));
    let mut col = vmix(vscale(field, furrow), [0.16, 0.30, 0.14], hedge * 0.8);
    col = vmix(col, avg, detail);
    let forest = smoothstep(0.60, 0.68, region);
    let canopy = 0.70 + 0.6 * fbm(s * 70.0, d * 40.0, 3, 107) * (1.0 - detail);
    col = vmix(col, vscale([0.11, 0.29, 0.15], canopy), forest);
    let water = smoothstep(0.37, 0.34, region) * smoothstep(0.42, 0.52, moist);
    col = vmix(col, [0.24, 0.48, 0.68], water);
    // towns: pale roofs by day, lights by night
    let town = smoothstep(0.60, 0.70, fbm(s * 4.0 + 30.0, d * 2.4, 3, 109)) * (1.0 - water) * (1.0 - forest);
    if night {
        col = vmul(col, [0.07, 0.10, 0.20]);
        col = vadd(col, vscale([0.10, 0.07, 0.03], town));
    } else {
        let roofs = fbm(s * 130.0, d * 75.0, 2, 111);
        col = vmix(col, vmix([0.82, 0.76, 0.68], [0.68, 0.42, 0.32], roofs), town * 0.75 * (1.0 - detail * 0.6));
    }
    col
}

fn sky(c: &mut Canvas, night: bool) {
    let haze: V3 = if night { [0.05, 0.07, 0.16] } else { [0.70, 0.83, 0.94] };
    shade(c, |x, y| {
        let (dx, dy) = (x as f32 - VP.0, (VP.1 - y as f32).max(0.5));
        let r = (dx * dx + dy * dy).sqrt();
        let psi = dy.atan2(dx); // 0..PI across the sky, right to left
        let a = (2.0 * psi + PI / 6.0).rem_euclid(TAU) / (PI / 3.0);
        let sector = (a as i32).rem_euclid(6);
        let frac = a - a.floor();
        let depth = 900.0 * psi.sin().max(0.02) / r;
        // fine detail is averaged away once a field is smaller than a pixel
        let detail = smoothstep(3.0, 11.0, depth);
        let far = 0.16 + 0.84 * (1.0 - (-depth / 7.0).exp());
        let arc = (PI / 6.0) * r;
        let edge_px = frac.min(1.0 - frac) * arc;
        let mut col = if sector % 2 == 0 {
            let s = sector as f32 * 2.0 + frac;
            let mut l = land(s, depth, detail, night);
            if !night {
                // drifting cloud shadow, and fuller light where the land faces the sunline squarely
                let shadow = fbm(s * 1.4 + 3.0, depth * 0.9, 3, 113);
                l = vscale(l, 0.94 + 0.12 * (1.0 - (frac - 0.5).abs() * 2.0) - 0.20 * smoothstep(0.55, 0.7, shadow));
            }
            // raised rim where land meets window
            vmix(l, if night { [0.03, 0.04, 0.08] } else { [0.56, 0.58, 0.62] }, smoothstep(3.5, 1.5, edge_px))
        } else {
            // window strip: slender glazing bars over daylight glare, or the night outside
            let q = (frac * 4.0).fract();
            let long_bar = smoothstep(2.2, 0.8, q.min(1.0 - q) * arc / 4.0);
            let zf = (depth * 2.0).fract();
            let ring_bar = smoothstep(2.2, 0.8, zf.min(1.0 - zf) * 0.5 * r / depth) * (1.0 - detail);
            let bars = long_bar.max(ring_bar).max(smoothstep(3.5, 1.5, edge_px));
            if night {
                vmix(space(x, y, 5), [0.07, 0.08, 0.12], bars * 0.8)
            } else {
                let glare = 0.5 + 0.5 * (frac * PI).sin();
                vmix(vmix([0.56, 0.76, 0.97], [0.86, 0.94, 1.0], glare), [0.50, 0.58, 0.68], bars * 0.6)
            }
        };
        col = vmix(col, haze, far);
        if night && sector % 2 == 0 && far < 0.95 {
            // lamps of the towns overhead
            let town = fbm(dx * 0.02 + 7.0, dy * 0.02, 3, 117);
            let h = hash(x, y, 119);
            if h % (if town > 0.55 { 23 } else { 230 }) == 0 {
                let warm = if h % 5 == 0 { [0.75, 0.85, 1.0] } else { [1.0, 0.78, 0.42] };
                col = vadd(col, vscale(warm, (1.0 - far) * (0.5 + (h >> 8 & 63) as f32 / 90.0)));
            }
        }
        // the sunline: a rod of light along the axis, overhead and running away to the far cap
        let sun: V3 = if night { [0.40, 0.48, 0.85] } else { [1.0, 0.96, 0.84] };
        let core = (-(dx / (1.5 + r * 0.012)).powi(2)).exp();
        let halo = (-(dx / (30.0 + r * 0.16)).powi(2)).exp();
        let end = (-(r / 150.0).powi(2)).exp();
        let k = if night { 0.5 * core + 0.10 * halo + 0.25 * end } else { 1.0 * core + 0.42 * halo + 0.6 * end };
        col = vadd(col, vscale(sun, k));
        if !night {
            // high cloud gathered round the axis
            let cl = fbm(dx * 0.007 + 3.0, dy * 0.016, 5, 121);
            let band = (-(dx / 330.0).powi(2)).exp() * smoothstep(40.0, 150.0, dy);
            col = vmix(col, [1.0, 1.0, 1.0], smoothstep(0.50, 0.72, cl) * band * 0.8);
        }
        col
    });
}

fn grounds(c: &mut Canvas, night: bool) {
    let amb: V3 = if night { [0.10, 0.14, 0.26] } else { [1.0, 1.0, 1.0] };
    region(c, 0, HORIZON as i32 - 34, W, H, |x, y| {
        // distant parkland trees closing off the lawn
        let top = HORIZON - 16.0 - 16.0 * fbm(x * 0.02, 0.0, 3, 131) - 6.0 * fbm(x * 0.11, 0.0, 2, 133);
        if y < HORIZON {
            if y < top {
                return None;
            }
            let leaf = fbm(x * 0.25, y * 0.25, 3, 135);
            let col = vscale([0.14, 0.30, 0.16], 0.6 + 0.6 * leaf + 0.5 * smoothstep(HORIZON, top, y));
            return Some((vmul(vmix(col, [0.60, 0.74, 0.80], 0.25), amb), smoothstep(0.0, 2.0, y - top)));
        }
        let depth = 150.0 / (y - HORIZON + 14.0);
        let u = (x - 576.0) * depth * 0.02;
        let blades = fbm(x * 0.5, y * 0.9, 3, 137);
        let patch = fbm(u * 0.8, depth * 2.0, 3, 139);
        // mown stripes running away from us
        let stripe = 0.94 + 0.08 * ((u * 1.6).floor() as i32).rem_euclid(2) as f32;
        let mut col = vscale([0.30, 0.56, 0.22], (0.72 + 0.26 * blades + 0.2 * patch) * stripe);
        let path = (4.6 + 0.5 * (depth * 3.0).sin() * 0.0 - u.abs()) / 0.25;
        if path > 0.0 {
            let gravel = vscale([0.80, 0.72, 0.58], 0.75 + 0.4 * fbm(x * 0.9, y * 0.9, 2, 141));
            col = vmix(col, gravel, path.min(1.0));
        }
        col = vmix(col, [0.52, 0.70, 0.62], smoothstep(2.0, 9.0, depth) * 0.45);
        Some((vmul(col, amb), 1.0))
    });
}

fn foliage(c: &mut Canvas, cx: f32, cy: f32, r: f32, night: bool) {
    region(c, (cx - r) as i32 - 1, (cy - r) as i32 - 1, (cx + r) as i32 + 2, (cy + r) as i32 + 2, |x, y| {
        let (nx, ny) = ((x - cx) / r, (y - cy) / r);
        let d = nx * nx + ny * ny;
        if d > 1.0 {
            return None;
        }
        let (dl, _) = lit([nx, ny, (1.0 - d).sqrt()], 8.0);
        let leaf = fbm(x * 0.45, y * 0.45, 3, 151);
        let col = vscale([0.14, 0.38, 0.17], 0.35 + 0.95 * dl + 0.45 * (leaf - 0.5));
        let col = if night { vmul(col, [0.16, 0.22, 0.38]) } else { col };
        Some((col, ((1.0 - d.sqrt()) * r + 0.5).min(1.0)))
    });
}

fn cone(c: &mut Canvas, cx: f32, base: f32, h: f32, w: f32, night: bool) {
    region(c, (cx - w) as i32 - 1, (base - h) as i32 - 1, (cx + w) as i32 + 2, base as i32 + 1, |x, y| {
        let half = w * (y - (base - h)) / h;
        let u = (x - cx) / half.max(0.5);
        if u.abs() > 1.0 {
            return None;
        }
        let (dl, _) = lit([u, -0.2, (1.0 - u * u).sqrt()], 8.0);
        let leaf = fbm(x * 0.45, y * 0.45, 3, 153);
        let col = vscale([0.12, 0.34, 0.16], 0.35 + 0.95 * dl + 0.45 * (leaf - 0.5));
        let col = if night { vmul(col, [0.16, 0.22, 0.38]) } else { col };
        Some((col, ((1.0 - u.abs()) * half + 0.5).min(1.0)))
    });
}

fn manor(c: &mut Canvas, night: bool) {
    // stone under daylight, or under the dimmed sunline
    let tone = |k: f32| -> V3 {
        let stone = vscale([0.93, 0.87, 0.74], k);
        if night { vmul(stone, [0.20, 0.24, 0.40]) } else { stone }
    };
    let glass: V3 = if night { [1.0, 0.80, 0.42] } else { [0.20, 0.30, 0.42] };
    let lit_window = |i: i32, row: i32| !night || hash(i, row, 9) % 5 != 0;
    // long shadow on the lawn by day, pools of window light by night
    if night {
        soft_ellipse(c, 576.0, 474.0, 330.0, 46.0, rgba(255, 200, 110, 70), 1.0);
    } else {
        soft_rect(c, 330.0, 440.0, 560.0, 16.0, 16.0, rgba(10, 30, 20, 110));
    }
    // wings
    for (x, k) in [(300, 0.88), (714, 0.80)] {
        slab(c, x, 300, 138, 140, tone(k), 2, 0.12);
        slab(c, x - 6, 288, 150, 14, tone(k * 1.08), 2, 0.1);
        slab(c, x + 10, 262, 26, 28, tone(k * 0.8), 2, 0.1);
        for (i, wx) in [x + 22, x + 86].into_iter().enumerate() {
            for row in 0..2 {
                let (wy, on) = (316 + row * 62, lit_window(wx + i as i32, row));
                slab(c, wx - 4, wy - 4, 38, 52, tone(1.04), 2, 0.05);
                let g = if on { glass } else { [0.05, 0.06, 0.12] };
                region(c, wx, wy, wx + 30, wy + 44, |px, py| {
                    let sky = if night { 1.0 } else { 1.0 + 0.9 * smoothstep(wy as f32 + 44.0, wy as f32, py) * (0.6 + 0.4 * fbm(px * 0.05, py * 0.05, 2, 161)) };
                    Some((vscale(g, sky), 1.0))
                });
                c.rect(wx + 14, wy, 2, 44, pack(tone(1.0)));
                c.rect(wx, wy + 21, 30, 2, pack(tone(1.0)));
                if night && on {
                    c.glow(wx as f32 + 15.0, wy as f32 + 22.0, 46.0, rgb(255, 200, 110), 0.30);
                }
            }
        }
    }
    // roof and central block, the portico in shadow behind its columns
    slab(c, 426, 270, 300, 32, if night { [0.07, 0.08, 0.15] } else { [0.36, 0.40, 0.50] }, 2, 0.2);
    slab(c, 438, 300, 276, 138, tone(0.50), 0, 0.1);
    if night {
        c.glow(576.0, 400.0, 90.0, rgb(255, 200, 110), 0.45);
    }
    // door with fanlight
    slab(c, 556, 362, 40, 76, if night { [0.24, 0.14, 0.08] } else { [0.26, 0.15, 0.10] }, 2, 0.3);
    c.ellipse_aa(576.0, 362.0, 20.0, 14.0, pack(vscale(glass, if night { 1.0 } else { 0.9 })));
    c.rect(556, 362, 40, 3, pack(tone(1.0)));
    for i in 0..6 {
        let x = 452.0 + i as f32 * 46.0;
        soft_rect(c, x + 14.0, 306.0, 12.0, 130.0, 6.0, rgba(0, 0, 0, 60));
        cyl_v(c, x, 306.0, 18.0, 128.0, tone(1.10), 0.15);
        slab(c, x as i32 - 3, 300, 24, 8, tone(1.12), 1, 0.05);
        slab(c, x as i32 - 3, 432, 24, 7, tone(1.0), 1, 0.05);
    }
    // entablature and pediment
    slab(c, 432, 282, 288, 20, tone(1.06), 3, 0.08);
    c.rect_blend(438, 302, 276, 5, rgba(0, 0, 0, 80));
    c.poly_aa(&[(426.0, 282.0), (726.0, 282.0), (576.0, 208.0)], pack(tone(1.10)));
    c.poly_aa(&[(456.0, 276.0), (696.0, 276.0), (576.0, 218.0)], pack(tone(0.84)));
    c.poly_aa(&[(456.0, 276.0), (696.0, 276.0), (576.0, 226.0)], pack(tone(0.94)));
    c.ellipse_aa(576.0, 256.0, 11.0, 11.0, pack(tone(1.1)));
    c.ellipse_aa(576.0, 256.0, 8.0, 8.0, pack(glass));
    // steps
    for (i, w) in [300.0f32, 320.0, 340.0].into_iter().enumerate() {
        slab(c, (576.0 - w / 2.0) as i32, 438 + i as i32 * 5, w as i32, 6, tone(1.0 - i as f32 * 0.06), 1, 0.05);
    }
    if night {
        for x in [540.0, 612.0] {
            c.glow(x, 384.0, 26.0, rgb(255, 220, 150), 0.9);
            c.ellipse_aa(x, 384.0, 3.0, 4.0, rgb(255, 245, 210));
        }
    }
}

fn topiary(c: &mut Canvas, night: bool) {
    let shadow = if night { rgba(0, 0, 0, 60) } else { rgba(10, 30, 20, 120) };
    for x in [206.0f32, 946.0] {
        soft_ellipse(c, x + 22.0, 452.0, 48.0, 9.0, shadow, 0.8);
        tube(c, x, 404.0, x, 452.0, 4.0, if night { [0.06, 0.05, 0.08] } else { [0.34, 0.24, 0.16] }, 0.2);
        foliage(c, x, 386.0, 34.0, night);
        foliage(c, x, 338.0, 19.0, night);
    }
    for x in [88.0f32, 1064.0] {
        soft_ellipse(c, x + 26.0, 458.0, 50.0, 9.0, shadow, 0.8);
        cone(c, x, 458.0, 128.0, 30.0, night);
    }
}

pub fn paint(c: &mut Canvas, night: bool) {
    sky(c, night);
    grounds(c, night);
    manor(c, night);
    topiary(c, night);
    finish(c, if night { 0.6 } else { 0.35 }, 0.03);
}

pub fn animate(c: &mut Canvas, t: f32, night: bool) {
    if night {
        // a handful of town lights and windows flicker
        for i in 0..40 {
            let h = hash(i, 0, 171);
            let (x, y) = ((h % W as u32) as i32, (h / 1153 % 400) as i32);
            if hash(i, (t * 2.0) as i32, 173) % 3 == 0 {
                c.cover(x, y, rgb(255, 215, 140), 0.8);
            }
        }
        let f = 0.5 + 0.5 * (t * 2.3).sin();
        c.glow(576.0, 400.0, 60.0, rgb(255, 200, 110), 0.05 + 0.05 * f);
    } else {
        // wisps of cloud drifting past the sunline
        for i in 0..4 {
            let x = ((t * (2.0 + i as f32) + i as f32 * 310.0) % 1300.0) - 70.0;
            let y = 60.0 + i as f32 * 34.0;
            soft_ellipse(c, x, y, 54.0, 13.0, rgba(255, 255, 255, 60), 1.0);
            soft_ellipse(c, x + 30.0, y + 6.0, 36.0, 10.0, rgba(255, 255, 255, 50), 1.0);
        }
    }
}
