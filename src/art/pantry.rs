//! The servants' hall: brick walls, a cast-iron stove throwing warm light,
//! copper pots, shelves of preserves and the brass bell board.
//! Key light is the fire (warm, centre); the fill is a cool ambient.

use super::paint::*;
use crate::gfx::*;

const FLOOR_Y: i32 = 428;
const FIRE: (f32, f32) = (576.0, 346.0);
/// The bell that is ringing: (column, row).
const RINGING: (i32, i32) = (4, 2);

fn room(c: &mut Canvas) {
    shade(c, |x, y| {
        let (fx, fy) = (x as f32, y as f32);
        let d2 = ((fx - FIRE.0) / 430.0).powi(2) + ((fy - FIRE.1 - 30.0) / 300.0).powi(2);
        let warm = 1.0 / (1.0 + d2 * 2.4);
        let light = [0.30 + 0.98 * warm, 0.30 + 0.66 * warm, 0.37 + 0.34 * warm];
        let col = if y < 38 {
            // timber beam
            let grain = fbm(fx * 0.012, fy * 0.22, 4, 3);
            let k = 0.55 + 0.6 * grain - if y > 30 { (y - 30) as f32 * 0.05 } else { 0.0 };
            vscale([0.30, 0.19, 0.13], k)
        } else if y < FLOOR_Y {
            let row = (y - 38) / 28;
            let by = (y - 38) % 28;
            let bxs = x + if row % 2 == 0 { 0 } else { 39 };
            let (bi, bx) = (bxs / 78, bxs % 78);
            let rough = fbm(fx * 0.16, fy * 0.16, 3, 9);
            let mut col = if bx < 3 || by < 3 {
                vscale([0.46, 0.41, 0.37], 0.75 + 0.4 * rough)
            } else {
                let tone = hashf(bi, row, 21);
                let base = vmix([0.50, 0.26, 0.20], [0.66, 0.40, 0.30], tone);
                let mut k = 0.80 + 0.36 * rough;
                if by < 6 {
                    k += 0.10;
                }
                if by > 24 || bx > 74 {
                    k -= 0.14;
                }
                // the odd soot-darkened brick
                if hash(bi, row, 22) % 9 == 0 {
                    k -= 0.16;
                }
                vscale(base, k)
            };
            // beam shadow and grime where wall meets floor
            col = vscale(col, 1.0 - 0.45 * smoothstep(70.0, 38.0, fy) - 0.35 * smoothstep(396.0, 428.0, fy));
            col
        } else {
            // tiled floor in perspective
            let depth = 210.0 / (fy - 386.0);
            let (u, v) = ((fx - 576.0) * depth * 0.0125, depth * 5.0);
            let (fu, fv) = (u - u.floor(), v - v.floor());
            let blur = (depth * 0.012).min(0.2);
            let grout = smoothstep(0.05 + blur, 0.02, fu.min(1.0 - fu)).max(smoothstep(0.05 + blur, 0.02, fv.min(1.0 - fv)));
            let checker = ((u.floor() + v.floor()) as i32).rem_euclid(2) == 0;
            let base = if checker { [0.62, 0.33, 0.23] } else { [0.48, 0.24, 0.19] };
            let wear = fbm(u * 2.3, v * 2.3, 3, 41);
            let mut col = vmix(vscale(base, 0.78 + 0.4 * wear), [0.25, 0.20, 0.18], grout * 0.8);
            // sheen of firelight on the glazed tiles
            let sheen = (-((fx - FIRE.0) / 200.0).powi(2)).exp() * smoothstep(648.0, 430.0, fy);
            col = vadd(col, vscale([0.30, 0.14, 0.04], sheen * (0.4 + 0.6 * wear)));
            vscale(col, 1.0 - 0.4 * smoothstep(446.0, 428.0, fy))
        };
        vmul(col, light)
    });
}

fn shelves(c: &mut Canvas) {
    let jars: [V3; 6] = [
        [0.72, 0.20, 0.14],
        [0.82, 0.60, 0.16],
        [0.34, 0.52, 0.26],
        [0.80, 0.78, 0.70],
        [0.50, 0.22, 0.42],
        [0.78, 0.40, 0.12],
    ];
    for (s, y) in [186, 270, 354].into_iter().enumerate() {
        soft_rect(c, 48.0, y as f32 + 8.0, 300.0, 16.0, 10.0, rgba(0, 0, 0, 120));
        for i in 0..7 {
            let h = hash(i, s as i32, 4);
            let (x, w) = (56.0 + i as f32 * 41.0, 27.0 + (h % 5) as f32);
            let jh = 34.0 + (h / 7 % 14) as f32;
            let top = y as f32 - jh;
            soft_rect(c, x + 9.0, top + 6.0, w, jh, 7.0, rgba(0, 0, 0, 70));
            cyl_v(c, x, top, w, jh, jars[(h / 3 % 6) as usize], 0.9);
            // paper label, glass highlight, cloth-covered lid
            c.rect_blend(x as i32 + 6, top as i32 + 13, w as i32 - 11, 11, rgba(236, 226, 200, 215));
            c.rect_blend(x as i32 + 5, top as i32 + 4, 2, jh as i32 - 8, rgba(255, 255, 255, 80));
            cyl_v(c, x - 1.5, top - 7.0, w + 3.0, 8.0, [0.84, 0.80, 0.66], 0.2);
        }
        slab(c, 40, y, 306, 10, [0.42, 0.26, 0.16], 2, 0.5);
        for bx in [62, 318] {
            c.poly_aa(&[(bx as f32, y as f32 + 10.0), (bx as f32 + 6.0, y as f32 + 10.0), (bx as f32, y as f32 + 30.0)], rgb(30, 22, 20));
        }
    }
}

fn pots(c: &mut Canvas) {
    for i in 0..5 {
        soft_ellipse(c, 122.0 + i as f32 * 66.0, 118.0, 34.0, 28.0, rgba(0, 0, 0, 120), 0.8);
    }
    cyl_h(c, 70.0, 60.0, 336.0, 6.0, [0.24, 0.23, 0.27], 0.6);
    for i in 0..5 {
        let x = 108.0 + i as f32 * 66.0;
        let r = 23.0 + (i % 2) as f32 * 4.0;
        c.capsule(x, 64.0, x, 84.0, 1.6, rgb(40, 36, 40));
        c.capsule(x, 82.0, x, 96.0, 3.0, rgb(120, 70, 40));
        orb(c, x, 110.0, r, r * 0.78, [0.84, 0.44, 0.22], 1.0, 14.0);
        // tinned rim and the warm bounce of the fire on the underside
        c.capsule(x - r * 0.8, 97.0, x + r * 0.8, 97.0, 1.5, rgba(240, 200, 160, 190));
        soft_ellipse(c, x + 6.0, 121.0, r * 0.6, 6.0, rgba(255, 150, 70, 110), 1.0);
    }
}

fn stove(c: &mut Canvas) {
    soft_rect(c, 430.0, 222.0, 300.0, 214.0, 18.0, rgba(0, 0, 0, 120));
    // flue
    soft_rect(c, 566.0, 40.0, 50.0, 170.0, 10.0, rgba(0, 0, 0, 90));
    cyl_v(c, 552.0, 36.0, 48.0, 172.0, [0.22, 0.22, 0.27], 0.5);
    for y in [70.0, 150.0] {
        cyl_v(c, 549.0, y, 54.0, 7.0, [0.30, 0.30, 0.36], 0.6);
    }
    // body
    slab(c, 438, 216, 276, 214, [0.17, 0.17, 0.21], 6, 0.3);
    slab(c, 428, 204, 296, 14, [0.26, 0.26, 0.32], 3, 0.2);
    for i in 0..9 {
        orb(c, 452.0 + i as f32 * 31.0, 226.0, 2.6, 2.6, [0.34, 0.34, 0.40], 1.0, 12.0);
    }
    slab(c, 474, 268, 204, 132, [0.11, 0.11, 0.14], 5, 0.2);
    slab(c, 492, 300, 168, 76, [0.05, 0.04, 0.05], 3, 0.0);
    // ash door and feet
    slab(c, 520, 406, 112, 16, [0.13, 0.13, 0.16], 3, 0.2);
    orb(c, 576.0, 414.0, 4.0, 4.0, [0.66, 0.5, 0.22], 1.0, 16.0);
    c.capsule(692.0, 318.0, 692.0, 352.0, 3.5, rgb(170, 130, 60));
    c.capsule(691.0, 318.0, 691.0, 350.0, 1.2, rgb(250, 220, 150));
    // kettle on the hob
    soft_ellipse(c, 494.0, 204.0, 40.0, 7.0, rgba(0, 0, 0, 140), 0.7);
    c.capsule(516.0, 186.0, 542.0, 166.0, 4.5, rgb(132, 136, 150));
    c.capsule(516.0, 185.0, 541.0, 165.0, 1.5, rgb(214, 218, 230));
    orb(c, 490.0, 184.0, 32.0, 22.0, [0.60, 0.62, 0.68], 1.0, 26.0);
    orb(c, 490.0, 162.0, 5.0, 4.0, [0.20, 0.18, 0.18], 0.6, 12.0);
    for i in 0..14 {
        let a = std::f32::consts::PI * (1.0 + i as f32 / 13.0);
        let (x0, y0) = (490.0 + a.cos() * 22.0, 168.0 + a.sin() * 24.0);
        c.ellipse_aa(x0, y0, 2.0, 2.0, rgb(52, 46, 48));
    }
}

fn bell(c: &mut Canvas, i: i32, row: i32, sway: f32, lit_up: bool) {
    let (bx, by) = (816.0 + i as f32 * 51.0, 128.0 + row as f32 * 56.0);
    c.rect(bx as i32 - 2, by as i32 - 20, 4, 5, rgb(34, 26, 24));
    c.capsule(bx, by - 15.0, bx + sway, by - 6.0, 1.3, rgb(70, 60, 54));
    soft_ellipse(c, bx + sway + 5.0, by + 5.0, 15.0, 15.0, rgba(0, 0, 0, 130), 1.0);
    let brass = if lit_up { [0.98, 0.80, 0.34] } else { [0.84, 0.64, 0.26] };
    orb(c, bx + sway, by, 11.5, 11.5, brass, 1.0, 16.0);
    orb(c, bx + sway * 1.9, by + 12.5, 2.4, 2.4, [0.25, 0.20, 0.16], 0.8, 10.0);
    slab(c, bx as i32 - 14, by as i32 + 19, 28, 10, if lit_up { [0.98, 0.84, 0.44] } else { [0.86, 0.82, 0.70] }, 1, 0.1);
    c.rect_blend(bx as i32 - 9, by as i32 + 23, 18, 2, rgba(60, 44, 36, 150));
}

fn bell_board(c: &mut Canvas) {
    soft_rect(c, 786.0, 104.0, 340.0, 188.0, 16.0, rgba(0, 0, 0, 130));
    slab(c, 772, 88, 340, 190, [0.72, 0.55, 0.24], 5, 0.15);
    slab(c, 783, 99, 318, 168, [0.30, 0.18, 0.13], 3, 0.45);
    for row in 0..3 {
        for i in 0..6 {
            if (i, row) != RINGING {
                bell(c, i, row, 0.0, false);
            }
        }
    }
}

pub fn paint(c: &mut Canvas) {
    room(c);
    shelves(c);
    pots(c);
    stove(c);
    bell_board(c);
    // firelight spilling over the hearth and floor
    c.glow(FIRE.0, FIRE.1, 250.0, rgb(255, 150, 60), 0.30);
    soft_ellipse(c, FIRE.0, 470.0, 250.0, 50.0, rgba(255, 140, 60, 70), 1.2);
    finish(c, 0.62, 0.04);
}

pub fn animate(c: &mut Canvas, t: f32) {
    // the fire behind the stove door
    let flick = 0.85 + 0.15 * (t * 9.0).sin() * (t * 5.3).cos();
    region(c, 496, 304, 656, 372, |x, y| {
        let ny = (y - 304.0) / 68.0;
        let n = fbm(x * 0.035, y * 0.05 + t * 1.6, 3, 5);
        let heat = (ny * 0.95 + n * 0.9 - 0.42) * flick;
        let col = if heat < 0.25 {
            vmix([0.06, 0.02, 0.02], [0.60, 0.10, 0.04], heat / 0.25)
        } else if heat < 0.55 {
            vmix([0.60, 0.10, 0.04], [1.0, 0.50, 0.08], (heat - 0.25) / 0.3)
        } else {
            vmix([1.0, 0.50, 0.08], [1.0, 0.92, 0.55], (heat - 0.55) / 0.35)
        };
        Some((col, 1.0))
    });
    for i in 0..4 {
        c.rect_blend(526 + i * 33, 302, 5, 72, rgba(14, 12, 14, 235));
    }
    c.glow(FIRE.0, FIRE.1 - 6.0, 110.0, rgb(255, 160, 70), 0.10 + 0.08 * flick);
    // steam from the kettle
    for i in 0..5 {
        let p = (t * 0.35 + i as f32 * 0.2).fract();
        let x = 546.0 + (p * 9.0 + i as f32).sin() * 7.0 + p * 10.0;
        soft_ellipse(c, x, 160.0 - p * 78.0, 6.0 + p * 14.0, 6.0 + p * 12.0, rgba(235, 235, 240, (70.0 * (1.0 - p)) as u8), 1.0);
    }
    // somebody upstairs wants something
    bell(c, RINGING.0, RINGING.1, (t * 22.0).sin() * 3.5, true);
    c.glow(816.0 + RINGING.0 as f32 * 51.0, 128.0 + RINGING.1 as f32 * 56.0, 26.0, rgb(255, 230, 150), 0.25);
}
