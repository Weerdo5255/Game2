//! The zero-g observatory at the hub: brushed steel, a great brass-rimmed
//! viewport on to deep space, a telescope, and breakfast in free fall.
//! Lit by warm overhead lamps with cold starlight spilling from the glass.

use super::paint::*;
use crate::gfx::*;

const PORT: (f32, f32, f32) = (576.0, 252.0, 246.0);

fn room(c: &mut Canvas) {
    shade(c, |x, y| {
        let (fx, fy) = (x as f32, y as f32);
        let (dx, dy) = (fx - PORT.0, fy - PORT.1);
        let d = (dx * dx + dy * dy).sqrt();
        if d < PORT.2 {
            // the glass: space, darkened a touch at the rim, with a ghost of reflection
            let mut col = space(x, y, 3);
            let rim = smoothstep(PORT.2 - 30.0, PORT.2, d);
            col = vadd(vscale(col, 1.0 - 0.5 * rim), vscale([0.10, 0.08, 0.05], rim * rim));
            let refl = (-((fx - 440.0) / 150.0).powi(2) - ((fy - 120.0) / 46.0).powi(2)).exp();
            return vadd(col, vscale([0.10, 0.12, 0.17], refl));
        }
        if d < PORT.2 + 22.0 {
            // brass ring, shaded as a torus
            let u = (d - PORT.2 - 11.0) / 11.0;
            let nz = (1.0 - u * u).max(0.0).sqrt();
            let (dl, s) = lit([dx / d * u, dy / d * u, nz], 18.0);
            let grime = fbm(fx * 0.05, fy * 0.05, 3, 8);
            let base = vscale([0.80, 0.58, 0.24], 0.8 + 0.3 * grime);
            return vadd(vscale(base, 0.22 + 0.9 * dl), vscale([1.0, 0.92, 0.7], s * 0.9));
        }
        // brushed steel wall panels
        let brush = fbm(fx * 0.008, fy * 0.7, 3, 17);
        let blotch = fbm(fx * 0.01, fy * 0.012, 3, 19);
        let mut col = vscale([0.33, 0.37, 0.47], 0.78 + 0.22 * brush + 0.16 * blotch);
        let seam = (x + 72).rem_euclid(144);
        if seam < 2 || (y - 86).abs() < 1 || (y - 566).abs() < 1 {
            col = vscale(col, 0.45);
        } else if seam < 4 || y == 88 || y == 568 {
            col = vscale(col, 1.3);
        }
        // warm lamps above, cold starlight near the glass, shadow under the ring
        let lamp = 0.50 + 0.62 * smoothstep(648.0, -80.0, fy) * (0.75 + 0.25 * (fx * 0.011).cos());
        let star_spill = smoothstep(PORT.2 + 150.0, PORT.2 + 22.0, d);
        let ao = 1.0 - 0.55 * smoothstep(PORT.2 + 60.0, PORT.2 + 22.0, d) * (0.5 + 0.5 * ((dx + dy) / d * 0.7).max(0.0));
        col = vmul(col, [lamp * 1.08, lamp, lamp * 0.9]);
        vscale(vadd(col, vscale([0.04, 0.07, 0.14], star_spill)), ao)
    });
    for sx in (72..W).step_by(144) {
        for sy in [30, 150, 290, 430, 600] {
            for off in [-12.0, 14.0] {
                let (px, py) = (sx as f32 + off, sy as f32);
                if ((px - PORT.0).powi(2) + (py - PORT.1).powi(2)).sqrt() > PORT.2 + 30.0 {
                    orb(c, px, py, 3.2, 3.2, [0.46, 0.50, 0.60], 0.9, 14.0);
                }
            }
        }
    }
}

/// Meridian Spindle, far off: a small O'Neill cylinder with mirror vanes.
fn meridian(c: &mut Canvas) {
    let (x, y) = (654.0, 152.0);
    for (a, b) in [((x + 4.0, y + 2.0), (x - 30.0, y - 22.0)), ((x + 104.0, y + 26.0), (x + 138.0, y + 48.0))] {
        c.capsule(a.0, a.1, b.0, b.1, 1.6, rgb(110, 118, 140));
        c.capsule(a.0, a.1 + 1.0, b.0, b.1 + 1.0, 0.6, rgb(210, 220, 240));
    }
    cyl_h(c, x, y, 108.0, 28.0, [0.66, 0.68, 0.76], 0.5);
    c.ellipse_aa(x, y + 14.0, 5.0, 14.0, rgb(150, 156, 176));
    c.ellipse_aa(x + 108.0, y + 14.0, 5.0, 14.0, rgb(70, 74, 96));
    for row in 0..3 {
        c.rect_blend(x as i32 + 6, y as i32 + 5 + row * 8, 96, 2, rgba(30, 40, 70, 200));
    }
    for i in 0..22 {
        let h = hash(i, 0, 31);
        let (px, py) = (x + 8.0 + (h % 92) as f32, y + 3.0 + (h / 97 % 22) as f32);
        c.ellipse_aa(px, py, 0.9, 0.9, rgb(255, 226, 150));
    }
    c.capsule(x - 16.0, y + 14.0, x, y + 14.0, 1.0, rgb(150, 156, 176));
    c.glow(x + 54.0, y + 14.0, 70.0, rgb(150, 170, 220), 0.10);
}

/// The Veil: a comet of excellent ice.
fn veil(c: &mut Canvas) {
    let (hx, hy) = (452.0, 314.0);
    let (ux, uy) = (-0.83, -0.56);
    region(c, 320, 210, 470, 330, |x, y| {
        let (rx, ry) = (x - hx, y - hy);
        let along = rx * ux + ry * uy;
        if along < -2.0 || along > 130.0 {
            return None;
        }
        let side = -rx * uy + ry * ux;
        let width = 2.0 + along * 0.13;
        let a = (-(side / width).powi(2)).exp() * (1.0 - along / 130.0).max(0.0).powf(1.4) * 0.75;
        Some((vmix([0.80, 0.93, 1.0], [0.45, 0.65, 1.0], along / 130.0), a))
    });
    c.glow(hx, hy, 16.0, rgb(200, 235, 255), 0.9);
    c.ellipse_aa(hx, hy, 2.6, 2.6, rgb(255, 255, 255));
}

fn fleet(c: &mut Canvas) {
    for i in 0..12 {
        let (x, y) = (418.0 + i as f32 * 27.0, 398.0 + (i % 3) as f32 * 11.0 + (i / 4) as f32 * 3.0);
        c.capsule(x, y, x + 11.0, y - 1.0, 1.7, rgb(150, 158, 180));
        c.capsule(x + 2.0, y - 0.6, x + 9.0, y - 1.6, 0.6, rgb(225, 230, 245));
        c.ellipse_aa(x - 1.0, y, 1.1, 1.1, rgb(120, 190, 255));
    }
}

fn telescope(c: &mut Canvas) {
    let brass = [0.82, 0.60, 0.24];
    soft_ellipse(c, 210.0, 420.0, 130.0, 80.0, rgba(0, 0, 0, 70), 1.0);
    for (x1, y1) in [(140.0, 600.0), (246.0, 606.0), (196.0, 624.0)] {
        tube(c, 190.0, 404.0, x1, y1, 4.5, [0.32, 0.21, 0.14], 0.4);
    }
    tube(c, 104.0, 462.0, 236.0, 348.0, 15.0, brass, 1.0);
    tube(c, 232.0, 351.0, 290.0, 301.0, 11.0, brass, 1.0);
    tube(c, 286.0, 304.0, 298.0, 294.0, 13.5, [0.56, 0.40, 0.16], 1.0);
    tube(c, 92.0, 472.0, 108.0, 458.0, 9.0, [0.30, 0.22, 0.12], 0.8);
    tube(c, 150.0, 421.0, 160.0, 412.0, 16.5, [0.56, 0.40, 0.16], 1.0);
    orb(c, 190.0, 402.0, 10.0, 10.0, [0.50, 0.36, 0.16], 1.0, 16.0);
}

pub fn paint(c: &mut Canvas) {
    room(c);
    meridian(c);
    veil(c);
    fleet(c);
    telescope(c);
    finish(c, 0.55, 0.035);
}

fn croissant(c: &mut Canvas, x: f32, y: f32, tilt: f32) {
    // a crescent of overlapping, tapering segments
    for i in [0, 6, 1, 5, 2, 4, 3] {
        let k = i as f32 - 3.0;
        let a = tilt + k * 0.30;
        let r = 17.0 - k.abs() * 3.6;
        let (px, py) = (x + a.sin() * 34.0, y - a.cos() * 34.0 + 34.0);
        let tone = if i % 2 == 0 { [0.84, 0.55, 0.20] } else { [0.74, 0.44, 0.14] };
        orb(c, px, py, r * 1.15, r, tone, 0.3, 8.0);
    }
}

fn teacup(c: &mut Canvas, x: f32, y: f32) {
    c.capsule(x + 36.0, y + 6.0, x + 44.0, y + 12.0, 2.2, rgb(214, 216, 228));
    c.capsule(x + 44.0, y + 12.0, x + 36.0, y + 20.0, 2.2, rgb(196, 198, 214));
    cyl_v(c, x, y, 36.0, 26.0, [0.93, 0.93, 0.96], 0.9);
    c.rect_blend(x as i32, y as i32 + 8, 36, 3, rgba(60, 90, 200, 190));
    c.ellipse_aa(x + 18.0, y, 18.0, 5.5, rgb(238, 238, 244));
    c.ellipse_aa(x + 18.0, y + 0.5, 15.0, 4.0, rgb(120, 66, 30));
    c.ellipse_aa(x + 13.0, y - 0.5, 5.0, 1.2, rgba(255, 220, 170, 150));
}

pub fn animate(c: &mut Canvas, t: f32) {
    let bob = |ph: f32, amp: f32| (t * 0.9 + ph).sin() * amp;
    croissant(c, 180.0 + bob(0.0, 16.0), 140.0 + bob(1.5, 12.0), 0.3 + bob(0.8, 0.25));
    teacup(c, 940.0 + bob(2.0, 12.0), 176.0 + bob(0.7, 20.0));
    // saucer, adrift from its cup
    let (sx, sy) = (1010.0 + bob(4.0, 14.0), 296.0 + bob(3.0, 16.0));
    orb(c, sx, sy, 34.0, 9.0, [0.92, 0.92, 0.96], 0.8, 20.0);
    c.ellipse_aa(sx, sy - 1.0, 17.0, 4.0, rgba(150, 156, 180, 120));
    // marmalade
    let (mx, my) = (896.0 + bob(5.0, 10.0), 410.0 + bob(0.2, 18.0));
    cyl_v(c, mx, my, 32.0, 36.0, [0.95, 0.52, 0.10], 1.0);
    c.rect_blend(mx as i32 + 5, my as i32 + 12, 22, 13, rgba(240, 232, 205, 225));
    cyl_v(c, mx - 2.0, my - 9.0, 36.0, 10.0, [0.78, 0.16, 0.16], 0.7);
    // wandering spheres of tea
    for (i, (bx, by)) in [(990.0, 120.0), (860.0, 330.0), (1050.0, 380.0), (250.0, 220.0)].into_iter().enumerate() {
        let r = 5.0 + (i % 3) as f32 * 2.5;
        orb(c, bx + bob(i as f32 * 1.7, 13.0), by + bob(i as f32 * 2.3, 13.0), r, r, [0.62, 0.34, 0.12], 1.2, 30.0);
    }
    // a picket ship's running light, and Meridian's beacon
    let i = (t * 0.8) as i32 % 12;
    c.glow(418.0 + i as f32 * 27.0 + 11.0, 397.0 + (i % 3) as f32 * 11.0 + (i / 4) as f32 * 3.0, 6.0, rgb(255, 90, 70), 0.9);
    if (t * 1.5) as i32 % 2 == 0 {
        c.glow(638.0, 166.0, 7.0, rgb(255, 80, 80), 0.9);
    }
}
