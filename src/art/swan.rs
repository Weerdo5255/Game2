//! Admiral Featherstonehaugh: a mute swan in profile, facing left, in a
//! bicorne hat, with a cravat and a decoration. White plumage shaded in
//! cool blue-greys under the same upper-left key light as the people.

use super::paint::*;
use crate::gfx::*;
use std::f32::consts::PI;

const WHITE_PLUME: V3 = [0.98, 0.98, 1.0];
const SHADE_PLUME: V3 = [0.56, 0.62, 0.78];
const HEAD: (f32, f32) = (104.0, 150.0);

/// A lit ellipsoid of white feathers; `rows` adds overlapping feather tips.
fn plume(c: &mut Canvas, cx: f32, cy: f32, rx: f32, ry: f32, rows: f32) {
    let rmin = rx.min(ry);
    region(c, (cx - rx) as i32 - 1, (cy - ry) as i32 - 1, (cx + rx) as i32 + 2, (cy + ry) as i32 + 2, |x, y| {
        let (nx, ny) = ((x - cx) / rx, (y - cy) / ry);
        let d2 = nx * nx + ny * ny;
        let a = (0.5 - (d2.sqrt() - 1.0) * rmin).min(1.0);
        if a <= 0.0 {
            return None;
        }
        let nz = (1.0 - d2.min(1.0)).sqrt();
        let mut d = ((dot([nx, ny, nz], LIGHT) + 0.35) / 1.35).clamp(0.0, 1.0);
        if rows > 0.0 {
            // scalloped rows: each feather tip casts a sliver of shadow on the next
            let row = (y + (x * 0.5)) / rows;
            let col_i = x / (rows * 1.5) + if (row.floor() as i32) % 2 == 0 { 0.0 } else { 0.5 };
            let (fr, fc) = (row.fract(), col_i.fract() - 0.5);
            let tip = (fr - (1.0 - fc * fc * 3.2)).max(0.0);
            d -= 0.5 * tip.min(0.22);
            d += 0.04 * (1.0 - fr);
        }
        d -= 0.05 * (fbm(x * 0.3, y * 0.3, 2, 91) - 0.5);
        Some((vmix(SHADE_PLUME, WHITE_PLUME, smoothstep(0.05, 0.75, d)), a))
    });
}

/// Centre and radius of the neck at parameter t (0 = breast, 1 = head).
fn neck_at(t: f32) -> (f32, f32, f32) {
    let x = 118.0 + 30.0 * (t * PI).sin() * (1.0 - 0.35 * t) - 16.0 * t;
    let y = 356.0 - 204.0 * t;
    (x, y, 27.0 - 11.0 * t)
}

fn body(c: &mut Canvas, angry: bool) {
    // tail, body, then the raised wing with its long flight feathers
    c.poly_aa(&[(280.0, 352.0), (330.0, 296.0), (318.0, 392.0)], pack(vmix(SHADE_PLUME, WHITE_PLUME, 0.6)));
    c.poly_aa(&[(286.0, 356.0), (322.0, 312.0), (312.0, 380.0)], pack(WHITE_PLUME));
    plume(c, 196.0, 412.0, 134.0, 86.0, 15.0);
    soft_ellipse(c, 226.0, 408.0, 110.0, 66.0, rgba(70, 84, 130, 70), 1.0);
    plume(c, 222.0, 380.0, 100.0, 60.0, 13.0);
    for i in 0..6 {
        let (x0, y0) = (228.0 + i as f32 * 13.0, 352.0 + i as f32 * 5.0);
        c.capsule(x0, y0, x0 + 52.0, y0 + 30.0, 1.0, rgba(96, 110, 156, 110));
    }
    // neck: an S of overlapping segments, tapering to the head
    for i in 0..=120 {
        let t = i as f32 / 120.0;
        let (x, y, r) = neck_at(t);
        if angry && i % 12 == 0 && (0.25..0.8).contains(&t) {
            // hackles up along the back of the neck
            c.poly_aa(&[(x + r - 5.0, y - 5.0), (x + r + 6.0, y - 9.0), (x + r - 3.0, y + 5.0)], pack(vmix(SHADE_PLUME, WHITE_PLUME, 0.8)));
        }
        // tall overlapping segments, so the shading runs across the neck rather than in rings
        plume(c, x, y, r, r * 2.4, 0.0);
    }
}

fn regalia(c: &mut Canvas) {
    // lavender cravat at the base of the neck
    let lav = [0.60, 0.48, 0.82];
    let (nx, ny, _) = neck_at(0.10);
    soft_ellipse(c, nx + 2.0, ny + 10.0, 40.0, 12.0, rgba(60, 70, 120, 90), 1.0);
    orb(c, nx - 2.0, ny, 34.0, 11.0, lav, 0.5, 12.0);
    c.poly_aa(&[(nx - 20.0, ny + 4.0), (nx - 34.0, ny + 34.0), (nx - 10.0, ny + 26.0)], pack(vscale(lav, 0.9)));
    c.poly_aa(&[(nx - 14.0, ny + 4.0), (nx - 4.0, ny + 38.0), (nx + 4.0, ny + 22.0)], pack(vscale(lav, 0.7)));
    orb(c, nx - 16.0, ny + 3.0, 8.0, 7.0, vscale(lav, 1.1), 0.6, 14.0);
    // a decoration on a red ribbon
    c.poly_aa(&[(150.0, 372.0), (166.0, 372.0), (164.0, 398.0), (158.0, 404.0), (152.0, 398.0)], rgb(196, 40, 50));
    c.rect_blend(156, 372, 3, 30, rgba(240, 220, 200, 120));
    orb(c, 158.0, 410.0, 9.0, 9.0, [0.90, 0.72, 0.28], 1.2, 22.0);
    c.ellipse_aa(158.0, 410.0, 4.0, 4.0, rgba(140, 100, 30, 140));
}

fn head(c: &mut Canvas, expr: &str) {
    let (hx, hy) = HEAD;
    let open = match expr {
        "shock" => 9.0,
        "angry" => 6.0,
        _ => 0.0,
    };
    // bill: lower mandible (dropped when hissing), then the upper
    let orange = [0.95, 0.50, 0.14];
    if open > 0.0 {
        c.poly_aa(&[(hx - 16.0, hy + 5.0), (hx - 60.0, hy + 12.0), (hx - 18.0, hy + 11.0)], rgb(120, 30, 40));
    }
    c.poly_aa(&[(hx - 14.0, hy + 6.0), (hx - 58.0, hy + 10.0 + open), (hx - 16.0, hy + 12.0 + open * 0.3)], pack(vscale(orange, 0.72)));
    region(c, (hx - 66.0) as i32, (hy - 10.0) as i32, (hx - 8.0) as i32, (hy + 14.0) as i32, |x, y| {
        let t = (hx - 12.0 - x) / 52.0;
        if !(0.0..=1.0).contains(&t) {
            return None;
        }
        let mid = hy + 1.0 + 7.0 * t;
        let half = 8.5 * (1.0 - t).powf(0.75) + 1.5;
        let v = (y - mid) / half;
        if v.abs() > 1.0 {
            return None;
        }
        let d = (0.55 - 0.5 * v + 0.2 * (1.0 - t)).clamp(0.0, 1.0);
        let mut col = vscale(orange, 0.55 + 0.6 * d);
        if t > 0.90 {
            col = [0.16, 0.12, 0.12]; // the nail at the tip
        }
        Some((col, ((1.0 - v.abs()) * half + 0.5).min(1.0)))
    });
    c.ellipse_aa(hx - 34.0, hy + 2.0, 3.0, 1.2, rgb(60, 30, 20));
    plume(c, hx, hy, 25.0, 21.0, 0.0);
    // black knob and lores running back to the eye
    orb(c, hx - 17.0, hy - 9.0, 8.0, 7.0, [0.10, 0.09, 0.10], 0.9, 20.0);
    c.poly_aa(&[(hx - 20.0, hy - 5.0), (hx - 12.0, hy + 6.0), (hx + 1.0, hy - 5.0), (hx - 8.0, hy - 9.0)], rgb(22, 20, 24));
    // eye
    let (ex, ey) = (hx - 1.0, hy - 5.0);
    match expr {
        "smile" => {
            for i in 0..6 {
                let t = i as f32 / 5.0;
                c.ellipse_aa(ex - 5.0 + 10.0 * t, ey + 2.0 - 4.0 * (t * PI).sin(), 1.4, 1.4, rgb(20, 18, 22));
            }
        }
        _ => {
            let (r, white) = match expr {
                "shock" => (5.2, 2.4),
                "smug" => (4.4, 0.0),
                _ => (4.6, 0.6),
            };
            if white > 0.0 {
                c.ellipse_aa(ex, ey, r + white, r + white, rgb(240, 236, 226));
            }
            c.ellipse_aa(ex, ey, r, r, rgb(46, 26, 16));
            c.ellipse_aa(ex, ey, r * 0.6, r * 0.6, rgb(10, 8, 10));
            c.ellipse_aa(ex - 1.6, ey - 1.8, 1.4, 1.4, rgb(255, 255, 255));
            if expr == "smug" {
                // a heavy lid
                c.poly_aa(&[(ex - 7.0, ey - 6.0), (ex + 7.0, ey - 6.0), (ex + 7.0, ey - 1.0), (ex - 7.0, ey - 0.5)], pack(vmix(SHADE_PLUME, WHITE_PLUME, 0.8)));
                c.capsule(ex - 6.0, ey - 0.5, ex + 6.0, ey - 1.0, 0.9, rgb(40, 40, 60));
            }
        }
    }
    // a feathery brow does the rest of the acting
    let brow = match expr {
        "angry" => Some(((ex - 9.0, ey - 5.0), (ex + 9.0, ey - 12.0))),
        "worried" => Some(((ex - 9.0, ey - 12.0), (ex + 9.0, ey - 6.0))),
        "smug" => Some(((ex - 8.0, ey - 10.0), (ex + 9.0, ey - 12.0))),
        "shock" => Some(((ex - 8.0, ey - 13.0), (ex + 8.0, ey - 14.0))),
        _ => None,
    };
    if let Some((a, b)) = brow {
        c.capsule(a.0, a.1, b.0, b.1, 2.0, rgba(70, 80, 120, 230));
    }
}

fn bicorne(c: &mut Canvas) {
    let (hx, hy) = HEAD;
    let navy = [0.12, 0.16, 0.36];
    let gold = rgb(232, 186, 80);
    let pts = [(hx - 46.0, hy - 16.0), (hx - 30.0, hy - 40.0), (hx - 2.0, hy - 66.0), (hx + 28.0, hy - 42.0), (hx + 46.0, hy - 18.0), (hx + 30.0, hy - 24.0), (hx - 30.0, hy - 22.0)];
    soft_ellipse(c, hx, hy - 12.0, 40.0, 9.0, rgba(40, 50, 90, 120), 1.0);
    c.poly_aa(&pts, pack(navy));
    c.poly_aa(&[pts[0], pts[1], pts[2], (hx - 6.0, hy - 28.0), pts[6]], pack(vscale(navy, 1.5)));
    for w in [(0, 1), (1, 2), (2, 3), (3, 4)] {
        c.capsule(pts[w.0].0, pts[w.0].1, pts[w.1].0, pts[w.1].1, 1.8, gold);
    }
    c.capsule(pts[0].0, pts[0].1, pts[6].0, pts[6].1, 1.4, gold);
    c.capsule(pts[6].0, pts[6].1, pts[5].0, pts[5].1, 1.4, gold);
    c.capsule(pts[5].0, pts[5].1, pts[4].0, pts[4].1, 1.4, gold);
    orb(c, hx - 2.0, hy - 42.0, 7.0, 7.0, [0.80, 0.12, 0.16], 0.8, 18.0);
    c.ellipse_aa(hx - 2.0, hy - 42.0, 3.0, 3.0, gold);
}

/// Paint the Admiral with expression `expr` on a transparent
/// SPR_W x SPR_H (336x456) canvas.
pub fn paint(c: &mut Canvas, expr: &str) {
    body(c, expr == "angry");
    regalia(c);
    head(c, expr);
    bicorne(c);
    rim_light(c);
}

/// The region of the sprite that changes between expressions: (x, y, w, h).
pub fn face_rect() -> (i32, i32, i32, i32) {
    (34, 122, 110, 58)
}
