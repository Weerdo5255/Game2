//! The estate's private dock: Lord Solberg's yacht in the cradle, an open
//! bay on to the stars, cold strip lighting on steel, and the warm glow
//! of an idling engine.

use super::paint::*;
use crate::gfx::*;

const DECK_Y: i32 = 372;
const BAY: (i32, i32, i32, i32) = (168, 78, 984, 354);

fn room(c: &mut Canvas) {
    shade(c, |x, y| {
        let (fx, fy) = (x as f32, y as f32);
        if x >= BAY.0 && x < BAY.2 && y >= BAY.1 && y < BAY.3 {
            let edge = (x - BAY.0).min(BAY.2 - 1 - x);
            if edge < 14 {
                return vscale(hazard(fx, fy), 0.8);
            }
            return space(x, y, 44);
        }
        // strip lights over the bay
        let mut lamp = 0.0f32;
        for lx in [300.0, 576.0, 852.0] {
            lamp += (-((fx - lx) / 190.0).powi(2)).exp();
        }
        if y < DECK_Y {
            let brush = fbm(fx * 0.6, fy * 0.008, 3, 23);
            let blotch = fbm(fx * 0.012, fy * 0.012, 3, 25);
            let mut col = vscale([0.31, 0.35, 0.45], 0.76 + 0.22 * brush + 0.2 * blotch);
            let seam = x.rem_euclid(96);
            if seam < 2 {
                col = vscale(col, 0.5);
            } else if seam < 4 {
                col = vscale(col, 1.25);
            }
            // heavy frame round the bay
            let fd = (BAY.0 - x).max(x - BAY.2 + 1).max(BAY.1 - y).max(y - BAY.3 + 1);
            if fd <= 14 {
                col = vscale([0.44, 0.48, 0.58], 1.15 - fd as f32 * 0.04 + 0.1 * brush);
            } else if fd < 40 {
                col = vscale(col, 0.62 + 0.38 * (fd - 14) as f32 / 26.0);
            }
            let l = 0.42 + 0.55 * lamp * smoothstep(420.0, 40.0, fy) + 0.25 * smoothstep(110.0, 0.0, fy);
            vmul(col, [l * 0.95, l, l * 1.1])
        } else if y < DECK_Y + 22 {
            vscale(hazard(fx, fy * 1.6), 0.55 + 0.35 * lamp.min(1.0))
        } else {
            // deck plates in perspective
            let depth = 190.0 / (fy - 352.0);
            let (u, v) = ((fx - 576.0) * depth * 0.011, depth * 4.2);
            let (fu, fv) = (u - u.floor(), v - v.floor());
            let seam = smoothstep(0.035, 0.012, fu.min(1.0 - fu)).max(smoothstep(0.05, 0.02, fv.min(1.0 - fv)));
            let scuff = fbm(u * 3.0, v * 1.2, 4, 51);
            let mut col = vscale([0.29, 0.31, 0.39], 0.70 + 0.5 * scuff);
            col = vmix(col, [0.10, 0.11, 0.15], seam * 0.8);
            // the pale hull mirrored dimly in the plating
            let mirror = (-((fx - 588.0) / 300.0).powi(2)).exp() * smoothstep(620.0, 400.0, fy);
            col = vadd(col, vscale([0.20, 0.20, 0.19], mirror * (0.3 + 0.7 * scuff)));
            let l = 0.50 + 0.45 * lamp.min(1.2);
            vmul(col, [l * 0.95, l, l * 1.1])
        }
    });
    cyl_h(c, 0.0, 14.0, W as f32, 13.0, [0.42, 0.46, 0.56], 0.6);
    cyl_h(c, 0.0, 38.0, W as f32, 7.0, [0.72, 0.44, 0.22], 0.9);
    for x in (60..W).step_by(180) {
        slab(c, x, 10, 10, 40, [0.26, 0.28, 0.36], 2, 0.1);
    }
    for lx in [300.0, 576.0, 852.0] {
        c.capsule(lx - 60.0, 62.0, lx + 60.0, 62.0, 3.5, rgb(235, 245, 255));
        c.glow(lx, 62.0, 90.0, rgb(190, 215, 255), 0.35);
    }
}

fn yacht(c: &mut Canvas) {
    let (cx, cy, rx, ry) = (588.0, 232.0, 282.0, 56.0);
    // tail fin and engine first, so the hull overlaps them
    c.poly_aa(&[(306.0, 226.0), (322.0, 118.0), (372.0, 124.0), (440.0, 214.0)], rgb(204, 196, 176));
    c.poly_aa(&[(318.0, 222.0), (330.0, 126.0), (366.0, 130.0), (426.0, 212.0)], rgb(236, 230, 212));
    c.capsule(322.0, 119.0, 372.0, 125.0, 2.0, rgb(214, 170, 70));
    cyl_h(c, 284.0, 212.0, 52.0, 44.0, [0.34, 0.35, 0.42], 0.8);
    c.ellipse_aa(284.0, 234.0, 7.0, 21.0, rgb(30, 26, 30));
    // hull: a long tapered body of revolution
    region(c, (cx - rx) as i32 - 2, (cy - ry) as i32 - 2, (cx + rx) as i32 + 2, (cy + ry) as i32 + 2, |x, y| {
        let (nx, ny) = ((x - cx) / rx, (y - cy) / ry);
        let taper = (1.0 - nx.abs().powf(2.6)).max(0.0).sqrt();
        if taper <= 0.0 {
            return None;
        }
        let v = ny / taper;
        let a = ((1.0 - v.abs()) * taper * ry + 0.5).min(1.0);
        if a <= 0.0 {
            return None;
        }
        let nz = (1.0 - v * v).max(0.0).sqrt();
        let (d, s) = lit([nx * 0.25, v, nz], 40.0);
        let mut base = [0.93, 0.90, 0.81];
        // gold coachlines
        if (v - 0.10).abs() < 0.035 || (v - 0.24).abs() < 0.012 {
            base = [0.86, 0.66, 0.24];
        }
        let mut col = vscale(base, 0.34 + 0.72 * d);
        // deck colour bounced on to the belly, a band of stars on the flank
        col = vadd(col, vscale([0.05, 0.07, 0.12], v.max(0.0)));
        col = vadd(col, vscale([1.0, 1.0, 0.96], s * 0.8 + 0.10 * (-((v + 0.45) / 0.07).powi(2)).exp()));
        Some((col, a))
    });
    for i in 0..9 {
        let x = 408.0 + i as f32 * 36.0;
        c.ellipse_aa(x, 213.0, 9.0, 6.5, rgb(190, 150, 60));
        c.ellipse_aa(x, 213.0, 7.0, 4.8, rgb(22, 40, 70));
        c.ellipse_aa(x - 2.0, 211.5, 2.6, 1.3, rgba(170, 210, 255, 200));
    }
    // cockpit canopy
    region(c, 716, 186, 848, 232, |x, y| {
        let (nx, ny) = ((x - 782.0) / 62.0, (y - 209.0) / 19.0);
        let d = nx * nx + ny * ny;
        if d > 1.0 {
            return None;
        }
        let glass = vmix([0.06, 0.14, 0.24], [0.30, 0.55, 0.72], (0.5 - ny * 0.5) * (0.6 - nx * 0.4));
        Some((glass, ((1.0 - d.sqrt()) * 19.0 + 0.5).min(1.0)))
    });
    c.capsule(742.0, 199.0, 800.0, 196.0, 1.6, rgba(230, 245, 255, 190));
}

fn cradle(c: &mut Canvas) {
    soft_ellipse(c, 588.0, 396.0, 330.0, 26.0, rgba(0, 0, 0, 150), 0.7);
    for x in [410.0, 760.0] {
        tube(c, x, 280.0, x, 384.0, 9.0, [0.44, 0.48, 0.58], 0.7);
        slab(c, x as i32 - 26, 272, 52, 12, [0.50, 0.54, 0.64], 2, 0.1);
        slab(c, x as i32 - 34, 380, 68, 12, [0.34, 0.37, 0.46], 2, 0.1);
    }
}

fn crates(c: &mut Canvas) {
    soft_rect(c, 962.0, 392.0, 190.0, 20.0, 14.0, rgba(0, 0, 0, 150));
    for (x, y) in [(952, 332), (1040, 332), (996, 262)] {
        slab(c, x, y, 82, 70, [0.14, 0.50, 0.50], 4, 0.3);
        c.rect_blend(x + 4, y + 30, 74, 6, rgba(10, 40, 46, 150));
        slab(c, x + 10, y + 10, 46, 14, [0.90, 0.90, 0.86], 1, 0.1);
        c.rect_blend(x + 14, y + 14, 30, 2, rgba(40, 60, 70, 200));
        c.rect_blend(x + 14, y + 18, 20, 2, rgba(40, 60, 70, 160));
    }
}

fn sedan_chair(c: &mut Canvas) {
    soft_rect(c, 30.0, 392.0, 130.0, 18.0, 12.0, rgba(0, 0, 0, 150));
    cyl_h(c, 0.0, 356.0, 190.0, 8.0, [0.78, 0.60, 0.26], 0.9);
    slab(c, 46, 286, 92, 110, [0.42, 0.20, 0.54], 4, 0.3);
    slab(c, 62, 304, 38, 44, [0.07, 0.07, 0.13], 2, 0.0);
    c.capsule(68.0, 310.0, 82.0, 308.0, 1.4, rgba(200, 220, 255, 110));
    c.frame(60, 302, 42, 48, rgb(214, 170, 70));
    slab(c, 38, 274, 108, 14, [0.82, 0.64, 0.26], 3, 0.1);
    for x in [44.0, 140.0] {
        orb(c, x, 270.0, 6.0, 6.0, [0.90, 0.72, 0.30], 1.0, 16.0);
    }
    c.rect_blend(108, 300, 2, 90, rgba(214, 170, 70, 200));
}

pub fn paint(c: &mut Canvas) {
    room(c);
    yacht(c);
    cradle(c);
    crates(c);
    sedan_chair(c);
    finish(c, 0.55, 0.035);
}

pub fn animate(c: &mut Canvas, t: f32) {
    // the engine, idling
    let f = 0.8 + 0.2 * (t * 17.0).sin() * (t * 7.0).cos();
    soft_ellipse(c, 262.0 - 10.0 * f, 234.0, 34.0 + 14.0 * f, 15.0, rgba(255, 150, 60, 200), 1.0);
    soft_ellipse(c, 272.0, 234.0, 16.0, 8.0, rgba(255, 244, 200, 230), 0.8);
    c.glow(270.0, 234.0, 70.0, rgb(255, 140, 60), 0.16 + 0.08 * f);
    // bay beacon
    if (t * 1.2).fract() < 0.5 {
        c.glow(576.0, 70.0, 16.0, rgb(255, 60, 50), 0.9);
        c.ellipse_aa(576.0, 70.0, 3.0, 3.0, rgb(255, 200, 190));
    }
}
