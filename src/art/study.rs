//! Lord Varrow's study at midnight: walnut panelling, walls of books, the
//! ancestor's portrait, and the war table with its cyan hologram of two
//! cylinders and the comet between them. A warm lamp fights cold holo-light.

use super::paint::*;
use crate::gfx::*;

const FLOOR_Y: f32 = 426.0;
const LAMP: (f32, f32) = (338.0, 340.0);
const HOLO: (f32, f32) = (576.0, 326.0);

fn light(fx: f32, fy: f32) -> V3 {
    let warm = (-((fx - LAMP.0) / 250.0).powi(2) - ((fy - LAMP.1 - 30.0) / 210.0).powi(2)).exp();
    let cyan = (-((fx - HOLO.0) / 330.0).powi(2) - ((fy - HOLO.1) / 260.0).powi(2)).exp();
    [0.20 + 1.05 * warm + 0.10 * cyan, 0.19 + 0.78 * warm + 0.34 * cyan, 0.24 + 0.36 * warm + 0.44 * cyan]
}

fn room(c: &mut Canvas) {
    shade(c, |x, y| {
        let (fx, fy) = (x as f32, y as f32);
        if fy < FLOOR_Y {
            // walnut panelling
            let grain = fbm(fx * 0.30, fy * 0.014, 4, 5);
            let figure = fbm(fx * 0.02, fy * 0.006, 3, 6);
            let mut col = vscale(vmix([0.30, 0.17, 0.11], [0.44, 0.26, 0.16], figure), 0.72 + 0.5 * grain);
            let px = x.rem_euclid(96);
            let framed = px > 10 && px < 86 && ((y > 40 && y < 300) || (y > 324 && y < 410));
            let edge = px == 10 || px == 86 || y == 40 || y == 300 || y == 324 || y == 410;
            if framed && !edge {
                col = vscale(col, 0.86);
            }
            if edge && px >= 10 && px <= 86 {
                col = vscale(col, if px == 10 || y == 40 || y == 324 { 0.55 } else { 1.35 });
            }
            if (310..316).contains(&y) || y < 18 {
                col = vscale(col, 1.25);
            }
            col = vscale(col, 1.0 - 0.4 * smoothstep(400.0, FLOOR_Y, fy));
            return vmul(col, light(fx, fy));
        }
        // carpet in perspective
        let depth = 190.0 / (fy - 392.0);
        let (u, v) = ((fx - 576.0) * depth * 0.012, depth * 4.0);
        let pile = fbm(fx * 0.4, fy * 0.4, 2, 9);
        let motif = ((u * 2.0).sin() * (v * 2.0).sin()).abs();
        let medal = smoothstep(0.55, 0.75, motif);
        let mut col = vmix([0.44, 0.07, 0.11], [0.62, 0.38, 0.14], medal * 0.55);
        if (u.abs() - 5.2).abs() < 0.12 {
            col = [0.66, 0.46, 0.18];
        }
        col = vscale(col, 0.75 + 0.3 * pile);
        vscale(vmul(col, light(fx, FLOOR_Y + (fy - FLOOR_Y) * 0.3)), 1.0 - 0.3 * smoothstep(445.0, FLOOR_Y, fy))
    });
}

fn bookcase(c: &mut Canvas, x0: i32) {
    let leathers: [V3; 7] = [
        [0.48, 0.10, 0.12],
        [0.12, 0.30, 0.20],
        [0.12, 0.17, 0.38],
        [0.58, 0.42, 0.16],
        [0.32, 0.13, 0.30],
        [0.10, 0.32, 0.36],
        [0.30, 0.18, 0.10],
    ];
    let l = |x: i32, y: i32| light(x as f32, y as f32);
    soft_rect(c, x0 as f32 + 6.0, 54.0, 296.0, 372.0, 14.0, rgba(0, 0, 0, 120));
    slab(c, x0, 44, 292, 376, vmul([0.30, 0.18, 0.12], l(x0 + 146, 230)), 6, 0.4);
    for shelf in 0..5 {
        let (top, base) = (56 + shelf * 72, 118 + shelf * 72);
        c.rect(x0 + 10, top, 272, 62, pack(vmul([0.07, 0.04, 0.04], l(x0 + 146, top + 30))));
        let mut x = x0 + 12;
        while x < x0 + 272 {
            let h = hash(x, shelf + x0, 6);
            let bw = 9 + (h % 9) as i32;
            let bh = 42 + (h / 7 % 17) as i32;
            if h % 13 == 0 {
                x += bw; // a gap where a volume is out on loan
                continue;
            }
            if x + bw > x0 + 280 {
                break;
            }
            let col = vmul(leathers[(h / 3 % 7) as usize], l(x, base - bh / 2));
            cyl_v(c, x as f32, (base - bh) as f32, bw as f32, bh as f32, col, 0.25);
            let gilt = pack(vmul([0.84, 0.66, 0.28], l(x, base - bh / 2)));
            c.rect_blend(x + 1, base - bh + 6, bw - 2, 2, with_alpha(gilt, 210));
            c.rect_blend(x + 1, base - 9, bw - 2, 2, with_alpha(gilt, 210));
            if h % 4 == 0 {
                c.rect_blend(x + 2, base - bh + 14, bw - 4, 7, with_alpha(gilt, 150));
            }
            x += bw + 1;
        }
        // each shelf throws a shadow on the books below
        soft_rect(c, x0 as f32 + 10.0, top as f32 - 2.0, 272.0, 9.0, 6.0, rgba(0, 0, 0, 150));
        slab(c, x0 + 8, base, 276, 8, vmul([0.34, 0.20, 0.13], l(x0 + 146, base)), 2, 0.3);
    }
}

fn portrait(c: &mut Canvas) {
    // lit mostly by the lamp; keep the gilt warm
    let l = vmul(light(576.0, 150.0), [1.5, 1.3, 0.9]);
    soft_rect(c, 490.0, 52.0, 196.0, 228.0, 14.0, rgba(0, 0, 0, 140));
    slab(c, 478, 38, 196, 228, vmul([0.82, 0.63, 0.26], l), 9, 0.2);
    slab(c, 488, 48, 176, 208, vmul([0.50, 0.38, 0.14], l), 3, 0.1);
    // the ancestor himself, in oils, painted on his own canvas so nothing strays past the frame
    let mut oil = Canvas::new(164, 196);
    let dim = |v: V3| vmul(v, vscale(l, 1.5));
    region(&mut oil, 0, 0, 164, 196, |x, y| {
        let (u, v) = ((x - 70.0) / 82.0, (y - 80.0) / 98.0);
        Some((dim(vmix([0.20, 0.24, 0.20], [0.05, 0.06, 0.07], (u * u + v * v).sqrt())), 1.0))
    });
    orb(&mut oil, 82.0, 200.0, 76.0, 56.0, dim([0.52, 0.10, 0.13]), 0.15, 8.0);
    orb(&mut oil, 82.0, 138.0, 12.0, 14.0, dim([0.62, 0.46, 0.38]), 0.1, 8.0);
    orb(&mut oil, 82.0, 96.0, 29.0, 37.0, dim([0.80, 0.60, 0.50]), 0.15, 10.0);
    orb(&mut oil, 82.0, 68.0, 30.0, 16.0, dim([0.66, 0.66, 0.70]), 0.2, 10.0);
    for sx in [-1.0f32, 1.0] {
        oil.ellipse_aa(82.0 + sx * 11.0, 94.0, 4.0, 2.0, pack(dim([0.16, 0.12, 0.12])));
        oil.capsule(82.0 + sx * 3.0, 112.0, 82.0 + sx * 16.0, 115.0, 3.0, pack(dim([0.70, 0.70, 0.74])));
        orb(&mut oil, 82.0 + sx * 56.0, 160.0, 15.0, 7.0, dim([0.80, 0.62, 0.24]), 0.4, 10.0);
    }
    oil.capsule(52.0, 150.0, 108.0, 196.0, 6.0, pack(dim([0.16, 0.24, 0.50])));
    c.blit(&oil, 494, 54, false, 256);
    // old varnish catching the lamp
    soft_ellipse(c, 530.0, 92.0, 60.0, 40.0, rgba(255, 240, 200, 26), 1.0);
}

fn war_table(c: &mut Canvas) {
    soft_rect(c, 262.0, 430.0, 628.0, 40.0, 22.0, rgba(0, 0, 0, 160));
    slab(c, 282, 406, 588, 60, vmul([0.24, 0.13, 0.09], light(576.0, 430.0)), 4, 0.4);
    for x in [300, 828] {
        slab(c, x, 440, 24, 208, vmul([0.20, 0.11, 0.08], light(x as f32, 480.0)), 3, 0.4);
    }
    region(c, 268, 386, 884, 408, |x, y| {
        let grain = fbm(x * 0.02, y * 0.5, 3, 4);
        let col = vscale([0.40, 0.22, 0.13], 0.8 + 0.4 * grain + if y < 389.0 { 0.35 } else { 0.0 });
        Some((vmul(col, light(x, 380.0)), 1.0))
    });
    c.rect_blend(280, 396, 592, 2, rgba(214, 170, 70, 220));
    // projector lens set into the top, and the cone of light above it
    c.ellipse_aa(576.0, 392.0, 170.0, 5.0, rgb(10, 30, 40));
    c.ellipse_aa(576.0, 392.0, 150.0, 3.5, rgb(70, 220, 240));
    region(c, 330, 250, 822, 392, |x, y| {
        let h = (392.0 - y) / 142.0;
        let half = 150.0 + 90.0 * h;
        let d = (x - 576.0).abs() / half;
        if d >= 1.0 {
            return None;
        }
        Some(([0.30, 0.85, 0.95], 0.11 * (1.0 - d * d) * (1.0 - h * 0.7)))
    });
}

fn lamp(c: &mut Canvas) {
    soft_ellipse(c, LAMP.0, 390.0, 46.0, 7.0, rgba(0, 0, 0, 150), 0.8);
    orb(c, LAMP.0, 384.0, 22.0, 6.0, [0.70, 0.52, 0.22], 1.0, 16.0);
    tube(c, LAMP.0, 338.0, LAMP.0, 382.0, 3.0, [0.78, 0.60, 0.26], 1.0);
    c.glow(LAMP.0, 356.0, 70.0, rgb(255, 225, 150), 0.55);
    // green glass shade, glowing where the bulb sits behind it
    region(c, 286, 314, 392, 346, |x, y| {
        let (nx, ny) = ((x - LAMP.0) / 50.0, (y - 344.0) / 28.0);
        let d = nx * nx + ny * ny;
        if d > 1.0 || ny > 0.0 {
            return None;
        }
        let nz = (1.0 - d).sqrt();
        let (dl, s) = lit([nx, ny, nz], 30.0);
        let glass = vadd(vscale([0.06, 0.46, 0.28], 0.5 + 0.8 * dl + 0.6 * (1.0 + ny)), vscale([1.0, 1.0, 0.9], s));
        Some((glass, ((1.0 - d.sqrt()) * 28.0 + 0.5).min(1.0)))
    });
    c.capsule(LAMP.0 - 50.0, 345.0, LAMP.0 + 50.0, 345.0, 1.6, rgb(200, 160, 70));
    soft_ellipse(c, LAMP.0, 396.0, 120.0, 14.0, rgba(255, 220, 150, 70), 1.0);
}

pub fn paint(c: &mut Canvas) {
    room(c);
    bookcase(c, 18);
    bookcase(c, 842);
    portrait(c);
    war_table(c);
    lamp(c);
    finish(c, 0.70, 0.04);
}

/// One of the two cylinders floating over the table.
fn holo_cylinder(c: &mut Canvas, x: f32, y: f32, tone: V3, t: f32) {
    let (w, h) = (96.0, 30.0);
    region(c, x as i32 - 8, y as i32 - 2, (x + w) as i32 + 8, (y + h) as i32 + 2, |px, py| {
        let v = (py - y) / h * 2.0 - 1.0;
        let u = (px - x) / w;
        if v.abs() > 1.0 || !(-0.06..=1.06).contains(&u) {
            return None;
        }
        // rounded end caps
        let cap = if u < 0.0 { u / 0.06 } else if u > 1.0 { (u - 1.0) / 0.06 } else { 0.0 };
        if cap * cap + v * v > 1.0 {
            return None;
        }
        let shell = 0.35 + 0.65 * v.abs().powf(3.0);
        // land and window strips sliding round as the cylinder spins
        let strip = (((v.asin() / std::f32::consts::PI + 0.5) * 3.0 + t * 0.35).fract() < 0.5) as i32 as f32;
        let scan = 0.8 + 0.2 * ((py * 1.6 + t * 30.0).sin());
        Some((vadd(tone, [0.25 * strip, 0.25 * strip, 0.25 * strip]), (shell * 0.70 + strip * 0.25).min(1.0) * scan))
    });
    c.glow(x + w / 2.0, y + h / 2.0, 70.0, pack(tone), 0.16);
}

pub fn animate(c: &mut Canvas, t: f32) {
    let flick = if hash((t * 14.0) as i32, 0, 8) % 9 == 0 { 0.75 } else { 1.0 };
    let amber = [0.98 * flick, 0.62 * flick, 0.22 * flick];
    let teal = [0.20 * flick, 0.86 * flick, 0.80 * flick];
    holo_cylinder(c, 398.0, 314.0, amber, t);
    holo_cylinder(c, 660.0, 314.0, teal, -t);
    // Halcyon's fleet, such as it is
    for i in 0..8 {
        let (x, y) = (392.0 + i as f32 * 13.0, 358.0 + (i % 3) as f32 * 5.0);
        c.capsule(x, y, x + 5.0, y, 1.0, packa(amber, 0.9));
    }
    // threat lines converging on the Veil
    for i in 0..9 {
        let p = ((i as f32 + (t * 1.5).fract()) / 9.0).min(1.0);
        for (x0, x1) in [(498.0, 566.0), (656.0, 588.0)] {
            c.ellipse_aa(x0 + (x1 - x0) * p, 329.0 - 14.0 * (p * std::f32::consts::PI).sin(), 1.6, 1.6, rgba(255, 90, 90, 220));
        }
    }
    let pulse = (t * 3.0).sin() * 0.5 + 0.5;
    c.glow(577.0, 316.0, 20.0 + 8.0 * pulse, rgb(190, 240, 255), 0.8);
    c.ring(577, 316, 9 + (pulse * 5.0) as i32, rgba(190, 240, 255, 200));
    c.ellipse_aa(577.0, 316.0, 3.2, 3.2, rgb(255, 255, 255));
    for i in 0..18 {
        let k = i as f32;
        c.ellipse_aa(577.0 - 5.0 - k * 2.4, 316.0 - 3.0 - k * 1.3, 1.6, 1.6, rgba(190, 235, 255, (190.0 - k * 10.0) as u8));
    }
    c.text_scaled(545, 276, "VEIL", rgba(190, 240, 255, 255), 2);
    c.text_scaled(404, 352 + 28, "HALCYON", packa(amber, 1.0), 1);
    c.text_scaled(676, 352 + 28, "MERIDIAN", packa(teal, 1.0), 1);
    // shimmer on the projector lens
    c.ellipse_aa(576.0, 392.0, 150.0, 3.5, rgba(70, 220, 240, (150.0 + 90.0 * pulse) as u8));
}
