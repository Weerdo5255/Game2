//! The gala: plum damask walls, gilded pilasters, tall arched windows on to
//! the night and the fleet, crystal chandeliers, and a marble floor that
//! mirrors all of it. Warm candlelight against cold starlight.

use super::paint::*;
use crate::gfx::*;

const FLOOR_Y: f32 = 426.0;
const WINDOWS: [f32; 4] = [150.0, 435.0, 717.0, 1002.0];
const CHANDELIERS: [(f32, f32); 3] = [(288.0, 100.0), (864.0, 100.0), (576.0, 72.0)];

/// Candlelight reaching a point on the wall.
fn light(fx: f32, fy: f32) -> V3 {
    let mut l = 0.0;
    for (cx, cy) in CHANDELIERS {
        l += (-((fx - cx) / 300.0).powi(2) - ((fy - cy) / 330.0).powi(2)).exp();
    }
    let l = l.min(1.35);
    [0.24 + 0.92 * l, 0.22 + 0.74 * l, 0.30 + 0.46 * l]
}

/// Inside a window? Returns the distance in pixels from the frame edge.
fn window(fx: f32, fy: f32) -> Option<(f32, f32)> {
    for cx in WINDOWS {
        let dx = (fx - cx).abs();
        if dx > 68.0 || fy > 384.0 || fy < 76.0 {
            continue;
        }
        let edge = if fy >= 134.0 {
            (66.0 - dx).min(384.0 - fy)
        } else {
            (1.0 - ((dx / 66.0).powi(2) + ((134.0 - fy) / 56.0).powi(2)).sqrt()) * 56.0
        };
        if edge > 0.0 {
            return Some((edge, fx - cx));
        }
    }
    None
}

fn wall(x: i32, y: i32) -> V3 {
    let (fx, fy) = (x as f32, y as f32);
    let l = light(fx, fy);
    if let Some((edge, dx)) = window(fx, fy) {
        let gold = [0.80, 0.62, 0.26];
        if edge < 8.0 {
            // gilt frame, rounded
            let u = edge / 4.0 - 1.0;
            return vmul(vscale(gold, 0.55 + 0.6 * (1.0 - u * u)), l);
        }
        if dx.abs() < 2.5 || (fy - 252.0).abs() < 2.5 || (fy - 134.0).abs() < 2.0 {
            return vmul(vscale(gold, 0.6), l);
        }
        // night beyond the glass, with a breath of the room reflected in it
        let mut col = vadd(space(x, y, 71), [0.035, 0.022, 0.03]);
        col = vadd(col, vscale([0.10, 0.07, 0.05], smoothstep(384.0, 150.0, fy) * 0.25));
        return col;
    }
    if fy < 26.0 {
        // cornice
        let u = fy / 26.0;
        return vmul(vscale([0.80, 0.62, 0.26], 0.5 + 0.7 * (u * 9.0).sin().abs() * (1.0 - 0.4 * u)), l);
    }
    if fy > 388.0 {
        // dado: dark panelling under a gilt rail
        if fy < 394.0 {
            return vmul([0.86, 0.68, 0.30], l);
        }
        let panel = ((fx / 96.0).fract() - 0.5).abs() < 0.42 && fy > 400.0 && fy < 420.0;
        return vmul(vscale([0.20, 0.10, 0.22], if panel { 1.25 } else { 0.9 }), l);
    }
    // damask: a woven diamond pattern that only shows as a change of sheen
    let (u, v) = (fx / 34.0, fy / 46.0);
    let motif = ((u + v) * std::f32::consts::PI).sin() * ((u - v) * std::f32::consts::PI).sin();
    let leaf = smoothstep(0.25, 0.6, motif.abs());
    let weave = fbm(fx * 0.5, fy * 0.5, 2, 13);
    let col = vscale([0.34, 0.15, 0.40], 0.80 + 0.22 * leaf + 0.10 * weave);
    // each pilaster shades the wall beside it
    let near = (fx - 9.0).rem_euclid(285.0);
    let ao = 1.0 - 0.35 * smoothstep(40.0, 12.0, near.min(285.0 - near));
    vmul(vscale(col, ao), l)
}

fn room(c: &mut Canvas) {
    shade(c, |x, y| {
        let (fx, fy) = (x as f32, y as f32);
        if fy < FLOOR_Y {
            return wall(x, y);
        }
        // checkered marble in perspective
        let depth = 190.0 / (fy - 392.0);
        let (u, v) = ((fx - 576.0) * depth * 0.0115, depth * 4.0);
        let checker = ((u.floor() + v.floor()) as i32).rem_euclid(2) == 0;
        let vein = fbm(u * 2.2 + fbm(u * 1.3, v * 1.3, 3, 3) * 2.0, v * 2.2, 4, 7);
        let streak = smoothstep(0.50, 0.54, vein) * smoothstep(0.60, 0.56, vein);
        let base = if checker {
            vmix([0.82, 0.74, 0.76], [0.56, 0.46, 0.52], streak)
        } else {
            vmix([0.30, 0.16, 0.32], [0.52, 0.36, 0.52], streak)
        };
        let mut col = vmul(vscale(base, 0.75 + 0.3 * vein), light(fx, 300.0));
        // the room mirrored in the polish, breaking up towards the viewer
        let my = (2.0 * FLOOR_Y - fy - (fy - FLOOR_Y) * 0.35) as i32;
        let ripple = ((vein - 0.5) * 14.0) as i32;
        let mirror = wall(x + ripple, my.max(0));
        col = vmix(col, mirror, 0.34 * smoothstep(660.0, FLOOR_Y, fy) + 0.08);
        for (cx, _) in CHANDELIERS {
            let s = (-((fx - cx) / 46.0).powi(2)).exp() * smoothstep(648.0, FLOOR_Y, fy);
            col = vadd(col, vscale([0.42, 0.30, 0.14], s * (0.5 + 0.5 * vein)));
        }
        vscale(col, 1.0 - 0.35 * smoothstep(440.0, FLOOR_Y, fy))
    });
}

fn pilasters(c: &mut Canvas) {
    for x in [-9.0f32, 276.0, 561.0, 846.0, 1131.0] {
        soft_rect(c, x + 22.0, 26.0, 16.0, 362.0, 10.0, rgba(0, 0, 0, 90));
        region(c, x as i32, 26, x as i32 + 30, 388, |px, py| {
            let u = (px - x) / 30.0;
            let flute = ((u * 5.0 * std::f32::consts::PI).sin() * 0.5 + 0.5).powf(0.7);
            let k = (0.45 + 0.65 * flute) * (1.15 - 0.5 * u);
            Some((vmul(vscale([0.82, 0.63, 0.26], k), light(px, py)), 1.0))
        });
        for (y, h) in [(26, 22), (366, 22)] {
            slab(c, x as i32 - 5, y, 40, h, [0.86, 0.68, 0.30], 3, 0.1);
        }
    }
}

fn drapes(c: &mut Canvas) {
    for cx in WINDOWS {
        // swagged pelmet over the arch
        region(c, cx as i32 - 100, 58, cx as i32 + 100, 132, |px, py| {
            let u = (px - cx) / 100.0;
            let sag = 74.0 + 34.0 * (1.0 - (u * std::f32::consts::PI * 1.5).cos().abs());
            if py > sag + 22.0 * (1.0 - u.abs()) {
                return None;
            }
            let fold = ((px * 0.22 + py * 0.05).sin() * 0.5 + 0.5).powf(1.5);
            Some((vmul(vscale([0.50, 0.07, 0.13], 0.36 + 0.70 * fold), light(px, py)), smoothstep(0.0, 3.0, sag + 22.0 * (1.0 - u.abs()) - py)))
        });
        for side in [-1.0f32, 1.0] {
            region(c, (cx + side * 80.0) as i32 - 50, 60, (cx + side * 80.0) as i32 + 50, 414, |px, py| {
                // gathered by a tie-back two thirds of the way down
                let tie = (-((py - 292.0) / 60.0).powi(2)).exp();
                let inner = 50.0 + 26.0 * tie + 6.0 * (py * 0.03).sin();
                let outer = 106.0 - 10.0 * tie;
                let d = (px - cx) * side;
                if d < inner || d > outer {
                    return None;
                }
                let u = (d - inner) / (outer - inner);
                let fold = ((u * 4.5 * std::f32::consts::PI + py * 0.012).sin() * 0.5 + 0.5).powf(1.4);
                let sheen = fold.powf(6.0) * 0.12;
                let col = vadd(vscale([0.48, 0.07, 0.13], 0.30 + 0.72 * fold), [sheen, sheen * 0.5, sheen * 0.5]);
                let a = smoothstep(0.0, 2.0, (d - inner).min(outer - d));
                Some((vmul(col, light(px, py)), a))
            });
            orb(c, cx + side * 84.0, 292.0, 9.0, 6.0, [0.88, 0.70, 0.30], 1.0, 16.0);
        }
    }
}

/// Lights of the fleet on station, seen through each window.
fn fleet(c: &mut Canvas) {
    for (w, cx) in WINDOWS.into_iter().enumerate() {
        for i in 0..5 {
            let h = hash(i, w as i32, 15);
            let (x, y) = (cx - 46.0 + i as f32 * 21.0 + (h % 9) as f32, 196.0 + (h / 11 % 40) as f32);
            if (x - cx).abs() < 5.0 {
                continue;
            }
            c.capsule(x, y, x + 8.0, y - 0.5, 1.3, rgb(120, 128, 160));
            c.glow(x + 9.0, y, 5.0, rgb(255, 210, 120), 0.9);
        }
    }
}

fn crowd(c: &mut Canvas) {
    for i in 0..17 {
        let h = hash(i, 0, 11);
        let x = 30.0 + i as f32 * 67.0 + (h % 23) as f32;
        let head_y = 336.0 - (h / 29 % 20) as f32 + (h / 7 % 6) as f32;
        let tone = [[0.11, 0.06, 0.15], [0.17, 0.06, 0.10], [0.06, 0.09, 0.17], [0.09, 0.07, 0.10]][(h % 4) as usize];
        let rim = vmul(vadd(tone, [0.26, 0.19, 0.12]), light(x, head_y));
        let (dark, glint) = (pack(tone), packa(rim, 0.55));
        soft_ellipse(c, x, 432.0, 40.0, 8.0, rgba(0, 0, 0, 130), 0.8);
        let sh = head_y + 26.0; // shoulder line
        if h % 3 != 0 {
            // a lady: bare shoulders, fitted bodice, wide skirt
            c.ellipse_aa(x, sh + 4.0, 15.0, 7.0, dark);
            c.poly_aa(&[(x - 12.0, sh + 4.0), (x + 12.0, sh + 4.0), (x + 8.0, sh + 36.0), (x + 34.0, 432.0), (x - 34.0, 432.0), (x - 8.0, sh + 36.0)], dark);
            c.poly_aa(&[(x - 8.0, sh + 36.0), (x - 3.0, sh + 36.0), (x - 22.0, 432.0), (x - 34.0, 432.0)], glint);
            c.ellipse_aa(x + 1.0, head_y - 9.0, 7.0, 6.0, dark);
        } else {
            // a gentleman: square shoulders, tailcoat
            c.ellipse_aa(x, sh + 6.0, 19.0, 8.0, dark);
            c.poly_aa(&[(x - 19.0, sh + 6.0), (x + 19.0, sh + 6.0), (x + 14.0, sh + 50.0), (x + 15.0, 432.0), (x - 15.0, 432.0), (x - 14.0, sh + 50.0)], dark);
            c.poly_aa(&[(x - 19.0, sh + 6.0), (x - 13.0, sh + 4.0), (x - 12.0, sh + 50.0), (x - 14.0, sh + 50.0)], glint);
        }
        c.capsule(x, head_y + 10.0, x, sh, 4.0, dark);
        c.ellipse_aa(x, head_y + 4.0, 9.0, 11.0, dark);
        // candlelight catching hair and shoulders
        c.ellipse_aa(x - 3.0, head_y - 3.0, 5.0, 2.6, glint);
        c.ellipse_aa(x - 8.0, sh + 1.0, 6.0, 2.0, glint);
    }
    // haze, so the crowd sits back in the room
    soft_rect(c, 0.0, 310.0, W as f32, 124.0, 26.0, rgba(96, 54, 84, 70));
}

fn chandelier(c: &mut Canvas, cx: f32, cy: f32) {
    c.rect_blend(cx as i32 - 1, 0, 3, cy as i32 - 50, rgba(120, 90, 40, 255));
    c.glow(cx, cy, 230.0, rgb(255, 205, 120), 0.42);
    for (tier, (rx, n)) in [(84.0f32, 11), (56.0, 8), (30.0, 5)].into_iter().enumerate() {
        let y = cy - tier as f32 * 22.0;
        c.capsule(cx - rx, y, cx + rx, y, 2.5, rgb(150, 110, 44));
        c.capsule(cx - rx, y - 1.0, cx + rx, y - 1.0, 0.9, rgb(250, 215, 130));
        for i in 0..n {
            let x = cx - rx + 2.0 * rx * i as f32 / (n - 1) as f32;
            cyl_v(c, x - 2.0, y - 15.0, 4.0, 13.0, [0.96, 0.93, 0.84], 0.2);
            // swags of crystal between the arms, and drops beneath
            if i + 1 < n {
                let step = 2.0 * rx / (n - 1) as f32;
                for k in 1..6 {
                    let f = k as f32 / 6.0;
                    c.ellipse_aa(x + step * f, y + 3.0 + 9.0 * (f * std::f32::consts::PI).sin(), 1.3, 1.3, rgba(235, 245, 255, 200));
                }
            }
            for k in 0..3 {
                let dy = 6.0 + k as f32 * 6.0 + (hash(i, k, tier as u32) % 4) as f32;
                c.ellipse_aa(x + (k as f32 - 1.0) * 2.0, y + dy, 1.4, 2.4, rgba(225, 240, 255, 215));
            }
        }
    }
    orb(c, cx, cy + 16.0, 7.0, 9.0, [0.82, 0.92, 1.0], 1.4, 30.0);
    c.capsule(cx, cy - 52.0, cx, cy, 2.5, rgb(170, 128, 52));
}

pub fn paint(c: &mut Canvas) {
    room(c);
    fleet(c);
    drapes(c);
    pilasters(c);
    crowd(c);
    for (cx, cy) in CHANDELIERS {
        chandelier(c, cx, cy);
    }
    finish(c, 0.55, 0.035);
}

pub fn animate(c: &mut Canvas, t: f32) {
    for (j, (cx, cy)) in CHANDELIERS.into_iter().enumerate() {
        for (tier, (rx, n)) in [(84.0f32, 11), (56.0, 8), (30.0, 5)].into_iter().enumerate() {
            let y = cy - tier as f32 * 22.0 - 15.0;
            for i in 0..n {
                let x = cx - rx + 2.0 * rx * i as f32 / (n - 1) as f32;
                let f = (t * 11.0 + i as f32 * 2.1 + j as f32).sin() * 0.5 + 0.5;
                c.ellipse_aa(x + (f - 0.5) * 0.8, y - 4.0 - f, 1.8, 4.0 + f, rgb(255, 214, 120));
                c.ellipse_aa(x, y - 2.5, 0.9, 2.0, rgb(255, 250, 230));
                c.glow(x, y - 4.0, 9.0, rgb(255, 200, 110), 0.5);
            }
        }
        let pulse = 0.05 + 0.03 * (t * 6.0 + j as f32 * 2.0).sin();
        c.glow(cx, cy - 10.0, 80.0, rgb(255, 215, 140), pulse);
    }
}
