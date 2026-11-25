//! Shading helpers shared by the scene painters: float colour maths, a
//! parallel per-pixel shader, and lit primitives (spheres, cylinders,
//! bevelled slabs, soft shadows) so that nothing is a flat fill.

use crate::gfx::*;

pub type V3 = [f32; 3];

/// Key light direction used by every interior prop and portrait:
/// from the upper left, towards the viewer.
pub const LIGHT: V3 = [-0.48, -0.60, 0.64];

pub fn v3(c: u32) -> V3 {
    [((c >> 16) & 255) as f32 / 255.0, ((c >> 8) & 255) as f32 / 255.0, (c & 255) as f32 / 255.0]
}

pub fn pack(v: V3) -> u32 {
    let ch = |x: f32| (x.clamp(0.0, 1.0) * 255.0 + 0.5) as u32;
    0xFF00_0000 | ch(v[0]) << 16 | ch(v[1]) << 8 | ch(v[2])
}

pub fn packa(v: V3, a: f32) -> u32 {
    with_alpha(pack(v), (a.clamp(0.0, 1.0) * 255.0 + 0.5) as u32)
}

pub fn vmix(a: V3, b: V3, t: f32) -> V3 {
    let t = t.clamp(0.0, 1.0);
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]
}

pub fn vscale(a: V3, k: f32) -> V3 {
    [a[0] * k, a[1] * k, a[2] * k]
}

pub fn vadd(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

pub fn vmul(a: V3, b: V3) -> V3 {
    [a[0] * b[0], a[1] * b[1], a[2] * b[2]]
}

pub fn dot(a: V3, b: V3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

pub fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Fill the whole canvas from a per-pixel function, using all CPU cores.
pub fn shade(c: &mut Canvas, f: impl Fn(i32, i32) -> V3 + Sync) {
    let w = c.w as usize;
    let threads = std::thread::available_parallelism().map_or(2, |n| n.get()).min(8);
    let rows_per = (c.h as usize + threads - 1) / threads;
    std::thread::scope(|s| {
        for (i, chunk) in c.px.chunks_mut(rows_per * w).enumerate() {
            let f = &f;
            s.spawn(move || {
                for (j, p) in chunk.iter_mut().enumerate() {
                    *p = pack(f((j % w) as i32, (i * rows_per + j / w) as i32));
                }
            });
        }
    });
}

/// Blend a per-pixel function over a rectangle; the function returns the
/// colour and coverage, or `None` to leave the pixel alone.
pub fn region(c: &mut Canvas, x0: i32, y0: i32, x1: i32, y1: i32, f: impl Fn(f32, f32) -> Option<(V3, f32)>) {
    for y in y0.max(0)..y1.min(c.h) {
        for x in x0.max(0)..x1.min(c.w) {
            if let Some((col, a)) = f(x as f32 + 0.5, y as f32 + 0.5) {
                if a > 0.0 {
                    c.blend(x, y, packa(col, a));
                }
            }
        }
    }
}

/// Lambert + specular for a surface normal under the standard key light.
/// Returns (diffuse factor, specular factor).
pub fn lit(n: V3, gloss: f32) -> (f32, f32) {
    let d = dot(n, LIGHT).max(0.0);
    // half vector between the light and the viewer (0,0,1)
    let h = [LIGHT[0], LIGHT[1], LIGHT[2] + 1.0];
    let hl = dot(h, h).sqrt();
    let s = (dot(n, h) / hl).max(0.0).powf(gloss);
    (d, s)
}

/// A lit ellipsoid. `spec` is the highlight strength, `gloss` its tightness.
pub fn orb(c: &mut Canvas, cx: f32, cy: f32, rx: f32, ry: f32, base: V3, spec: f32, gloss: f32) {
    let rmin = rx.min(ry);
    region(c, (cx - rx - 1.0) as i32, (cy - ry - 1.0) as i32, (cx + rx + 2.0) as i32, (cy + ry + 2.0) as i32, |x, y| {
        let (nx, ny) = ((x - cx) / rx, (y - cy) / ry);
        let d2 = nx * nx + ny * ny;
        let edge = 0.5 - (d2.sqrt() - 1.0) * rmin;
        if edge <= 0.0 {
            return None;
        }
        let nz = (1.0 - d2.min(1.0)).sqrt();
        let (d, s) = lit([nx, ny, nz], gloss);
        // ambient with a little bounce from below, darker towards the rim
        let amb = 0.30 + 0.10 * ny.max(0.0) + 0.08 * nz;
        let col = vadd(vscale(base, amb + 0.80 * d), vscale([1.0, 0.97, 0.9], s * spec));
        Some((col, edge.min(1.0)))
    });
}

/// A lit upright cylinder (jars, pipes, columns, candles).
pub fn cyl_v(c: &mut Canvas, x: f32, y: f32, w: f32, h: f32, base: V3, spec: f32) {
    region(c, x as i32 - 1, y as i32 - 1, (x + w) as i32 + 2, (y + h) as i32 + 2, |px, py| {
        let u = ((px - x) / w * 2.0 - 1.0).clamp(-1.0, 1.0);
        let ax = (0.5 - (x - px)).min(0.5 - (px - x - w)).min(1.0);
        let ay = (0.5 - (y - py)).min(0.5 - (py - y - h)).min(1.0);
        if ax <= 0.0 || ay <= 0.0 {
            return None;
        }
        let nz = (1.0 - u * u).sqrt();
        let (d, s) = lit([u, -0.15, nz], 24.0);
        let col = vadd(vscale(base, 0.30 + 0.85 * d + 0.06 * nz), vscale([1.0, 0.97, 0.9], s * spec));
        Some((col, ax * ay))
    });
}

/// A lit horizontal cylinder (rails, rolled edges, hulls).
pub fn cyl_h(c: &mut Canvas, x: f32, y: f32, w: f32, h: f32, base: V3, spec: f32) {
    region(c, x as i32 - 1, y as i32 - 1, (x + w) as i32 + 2, (y + h) as i32 + 2, |px, py| {
        let v = ((py - y) / h * 2.0 - 1.0).clamp(-1.0, 1.0);
        let ax = (0.5 - (x - px)).min(0.5 - (px - x - w)).min(1.0);
        let ay = (0.5 - (y - py)).min(0.5 - (py - y - h)).min(1.0);
        if ax <= 0.0 || ay <= 0.0 {
            return None;
        }
        let nz = (1.0 - v * v).sqrt();
        let (d, s) = lit([-0.15, v, nz], 24.0);
        let col = vadd(vscale(base, 0.30 + 0.85 * d + 0.06 * nz), vscale([1.0, 0.97, 0.9], s * spec));
        Some((col, ax * ay))
    });
}

/// A flat-faced block with a top-to-bottom falloff, a lit top/left bevel,
/// a shaded bottom/right bevel and a little surface grain.
pub fn slab(c: &mut Canvas, x: i32, y: i32, w: i32, h: i32, base: V3, bevel: i32, grain: f32) {
    for yy in 0..h {
        for xx in 0..w {
            let fy = yy as f32 / h.max(1) as f32;
            let mut k = 1.08 - 0.22 * fy;
            let (dl, dt, dr, db) = (xx, yy, w - 1 - xx, h - 1 - yy);
            if dt < bevel || dl < bevel {
                k += 0.22 * (1.0 - dt.min(dl) as f32 / bevel as f32);
            }
            if db < bevel || dr < bevel {
                k -= 0.30 * (1.0 - db.min(dr) as f32 / bevel as f32);
            }
            k += (fbm((x + xx) as f32 * 0.09, (y + yy) as f32 * 0.09, 3, 77) - 0.5) * grain;
            c.put(x + xx, y + yy, pack(vscale(base, k)));
        }
    }
}

/// A rectangle of colour with feathered edges: for cast shadows, pools of
/// light and soft panels. The colour's alpha is the peak opacity.
pub fn soft_rect(c: &mut Canvas, x: f32, y: f32, w: f32, h: f32, feather: f32, col: u32) {
    let peak = (col >> 24) as f32 / 255.0;
    let f = feather.max(0.5);
    for py in (y - f) as i32..=(y + h + f) as i32 {
        for px in (x - f) as i32..=(x + w + f) as i32 {
            let (fx, fy) = (px as f32 + 0.5, py as f32 + 0.5);
            let dx = (x - fx).max(fx - (x + w)).max(-f);
            let dy = (y - fy).max(fy - (y + h)).max(-f);
            let a = smoothstep(f, -f, dx) * smoothstep(f, -f, dy);
            c.cover(px, py, with_alpha(col, 255), a * peak);
        }
    }
}

/// An ellipse of colour fading smoothly from the centre to its edge.
pub fn soft_ellipse(c: &mut Canvas, cx: f32, cy: f32, rx: f32, ry: f32, col: u32, power: f32) {
    let peak = (col >> 24) as f32 / 255.0;
    for py in (cy - ry) as i32..=(cy + ry) as i32 {
        for px in (cx - rx) as i32..=(cx + rx) as i32 {
            let (nx, ny) = ((px as f32 + 0.5 - cx) / rx, (py as f32 + 0.5 - cy) / ry);
            let d = (nx * nx + ny * ny).sqrt();
            if d < 1.0 {
                c.cover(px, py, with_alpha(col, 255), (1.0 - d).powf(power) * peak);
            }
        }
    }
}

/// Darken the corners of a finished painting and add a breath of film
/// grain, which hides gradient banding.
pub fn finish(c: &mut Canvas, vignette: f32, grain: f32) {
    let (w, h) = (c.w as f32, c.h as f32);
    for y in 0..c.h {
        for x in 0..c.w {
            let (nx, ny) = (x as f32 / w * 2.0 - 1.0, y as f32 / h * 2.0 - 1.0);
            let v = 1.0 - vignette * (nx * nx * 0.55 + ny * ny * 0.45).powf(1.3);
            let g = 1.0 + (hashf(x, y, 991) - 0.5) * grain;
            let i = (y * c.w + x) as usize;
            c.px[i] = pack(vscale(v3(c.px[i]), v * g));
        }
    }
}

/// Brightness of a starfield at a pixel (0 for empty sky).
pub fn star(x: i32, y: i32, seed: u32) -> f32 {
    let h = hash(x, y, seed);
    if h % 211 == 0 {
        0.35 + ((h >> 8) % 100) as f32 / 150.0
    } else if h % 1700 == 1 {
        1.4
    } else {
        0.0
    }
}

/// Deep-space colour at a pixel: near-black blue, faint nebula, stars.
pub fn space(x: i32, y: i32, seed: u32) -> V3 {
    let (fx, fy) = (x as f32, y as f32);
    let neb = fbm(fx * 0.006, fy * 0.008, 4, seed);
    let neb2 = fbm(fx * 0.012 + 40.0, fy * 0.010, 3, seed + 5);
    let mut col = vadd([0.012, 0.016, 0.040], vscale([0.10, 0.05, 0.16], smoothstep(0.50, 0.85, neb)));
    col = vadd(col, vscale([0.02, 0.07, 0.10], smoothstep(0.55, 0.9, neb2)));
    let s = star(x, y, seed);
    if s > 0.0 {
        let tint = if hash(x, y, seed + 1) % 3 == 0 { [1.0, 0.86, 0.70] } else { [0.82, 0.90, 1.0] };
        col = vadd(col, vscale(tint, s));
    }
    // soft halo around the brightest stars
    for (dx, dy) in [(-1, 0), (1, 0), (0, -1), (0, 1)] {
        if star(x + dx, y + dy, seed) > 1.0 {
            col = vadd(col, [0.35, 0.38, 0.45]);
        }
    }
    col
}

/// A lit cylinder between two points with flat ends (poles, legs, spouts).
pub fn tube(c: &mut Canvas, x0: f32, y0: f32, x1: f32, y1: f32, r: f32, base: V3, spec: f32) {
    let (dx, dy) = (x1 - x0, y1 - y0);
    let len = (dx * dx + dy * dy).sqrt().max(0.001);
    let (ux, uy) = (dx / len, dy / len);
    let (lx, hx) = ((x0.min(x1) - r - 1.0) as i32, (x0.max(x1) + r + 2.0) as i32);
    let (ly, hy) = ((y0.min(y1) - r - 1.0) as i32, (y0.max(y1) + r + 2.0) as i32);
    region(c, lx, ly, hx, hy, |px, py| {
        let (rx, ry) = (px - x0, py - y0);
        let along = rx * ux + ry * uy;
        let side = -rx * uy + ry * ux;
        let a = (0.5 + along.min(len - along)).min(0.5 + r - side.abs()).min(1.0);
        if a <= 0.0 {
            return None;
        }
        let u = (side / r).clamp(-1.0, 1.0);
        let nz = (1.0 - u * u).sqrt();
        let (d, s) = lit([-uy * u, ux * u, nz], 24.0);
        let col = vadd(vscale(base, 0.30 + 0.85 * d + 0.06 * nz), vscale([1.0, 0.97, 0.9], s * spec));
        Some((col, a))
    });
}

/// Yellow and black hazard chevrons with scuffed paint.
pub fn hazard(x: f32, y: f32) -> V3 {
    let stripe = ((x + y) / 14.0).floor() as i32;
    let wear = fbm(x * 0.2, y * 0.2, 3, 61);
    if stripe.rem_euclid(2) == 0 {
        vscale([0.92, 0.72, 0.10], 0.7 + 0.4 * wear)
    } else {
        vscale([0.10, 0.10, 0.12], 0.7 + 0.6 * wear)
    }
}

/// A cool rim light along the right-hand silhouette, so the figure
/// separates from dark backgrounds without a drawn outline.
pub fn rim_light(c: &mut Canvas) {
    let src = c.clone();
    for y in 0..c.h {
        for x in 0..c.w {
            let p = src.get(x, y);
            if p >> 24 < 200 {
                continue;
            }
            let gap = (1..=5).find(|d| src.get(x + d, y) >> 24 < 90);
            if let Some(d) = gap {
                c.tint(x, y, rgb(170, 196, 255), (70 - d as u32 * 11).min(70));
            }
        }
    }
}
