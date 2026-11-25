//! All artwork is procedural pixel art: backgrounds and character sprites
//! are drawn in code with a limited palette and ordered dithering.

use crate::gfx::*;
use std::f32::consts::{PI, TAU};

pub const BACKGROUNDS: &[&str] = &["pantry", "axis", "dock", "vista", "vista_night", "ballroom", "study"];
pub const SPRITES: &[&str] = &["p", "v", "s", "q", "t", "c", "a"];
pub const EXPRS: &[&str] = &["neutral", "smile", "worried", "angry", "smug", "shock"];

pub const SPR_W: i32 = 112;
pub const SPR_H: i32 = 152;

pub fn draw_bg(c: &mut Canvas, name: &str, t: f32) {
    match name {
        "pantry" => pantry(c, t),
        "axis" => axis(c, t),
        "dock" => dock(c, t),
        "vista" => vista(c, t, false),
        "vista_night" => vista(c, t, true),
        "ballroom" => ballroom(c, t),
        "study" => study(c, t),
        _ => c.clear(BLACK),
    }
}

fn stars(c: &mut Canvas, x: i32, y: i32, t: f32) {
    let h = hash(x, y, 3);
    if h % 53 == 0 {
        let twinkle = (h / 53 + (t * 3.0) as u32) % 6 == 0;
        let col = if twinkle { rgb(120, 130, 170) } else if h % 5 == 0 { rgb(255, 240, 200) } else { rgb(220, 228, 255) };
        c.put(x, y, col);
    }
}

fn glow(c: &mut Canvas, cx: i32, cy: i32, r: i32, col: u32, strength: u32) {
    for y in -r..=r {
        for x in -r..=r {
            let d2 = x * x + y * y;
            if d2 < r * r {
                let d = (d2 as f32).sqrt() / r as f32;
                c.tint(cx + x, cy + y, col, ((1.0 - d) * strength as f32) as u32);
            }
        }
    }
}

/// The interior of the cylinder, looking down its length: three strips of
/// land and three of window curving overhead, with the estate in front.
fn vista(c: &mut Canvas, t: f32, night: bool) {
    let haze = if night { rgb(20, 24, 60) } else { rgb(170, 210, 238) };
    let (cx, cy) = (192.0f32, 76.0f32);
    for y in 0..H {
        for x in 0..W {
            let (dx, dy) = (x as f32 - cx, (y as f32 - cy) * 1.12);
            let r = (dx * dx + dy * dy).sqrt().max(0.5);
            let a = (dy.atan2(dx) - PI / 2.0 + PI / 6.0).rem_euclid(TAU) / (PI / 3.0);
            let sector = (a as i32).rem_euclid(6);
            let frac = a - a.floor();
            let zl = (900.0 / r).ln() * 5.0;
            let zi = zl.floor() as i32;
            let mut col = if sector % 2 == 0 {
                let strip = (frac * 5.0) as i32;
                let h = hash(sector * 8 + strip, zi, 1) % 10;
                let mut g = match h {
                    0..=3 => rgb(84, 168, 80),
                    4..=6 => rgb(60, 140, 72),
                    7 => rgb(206, 190, 96),
                    8 => rgb(36, 104, 64),
                    _ => rgb(72, 140, 208),
                };
                if (frac * 5.0).fract() < 0.07 || zl.fract() < 0.1 {
                    g = mix(g, rgb(24, 64, 40), 120);
                }
                if night {
                    g = mix(g, rgb(10, 14, 44), 190);
                    if hash(x, y, 7) % 67 == 0 {
                        g = rgb(255, 214, 120);
                    }
                }
                g
            } else if night {
                rgb(8, 10, 30)
            } else if (frac * 3.0).fract() < 0.05 || zl.fract() < 0.08 {
                rgb(132, 156, 190)
            } else {
                rgb(216, 238, 255)
            };
            if night && sector % 2 == 1 {
                c.put(x, y, col);
                stars(c, x, y, t);
                col = c.get(x, y);
            }
            let far = (1.0 - r / 180.0).clamp(0.0, 1.0);
            c.put(x, y, mix(col, haze, (far * far * 236.0) as u32));
        }
    }
    // the sunline, seen end-on, and the far end cap
    let sun = if night { rgb(150, 160, 230) } else { rgb(255, 250, 214) };
    glow(c, 192, 76, if night { 22 } else { 34 }, sun, 250);
    c.ellipse(192, 76, 3, 3, sun);
    if !night {
        // clouds gather round the axis
        for i in 0..4 {
            let x = ((i * 97) as f32 + t * (3.0 + i as f32)) % 440.0 - 30.0;
            let y = 30 + i * 13;
            for (ox, oy, rx) in [(0, 0, 18), (12, 2, 12), (-10, 3, 10)] {
                for yy in -4..=4i32 {
                    for xx in -rx..=rx {
                        if xx * xx * 16 + yy * yy * rx * rx <= rx * rx * 16 {
                            c.tint(x as i32 + ox + xx, y + oy + yy, WHITE, 150);
                        }
                    }
                }
            }
        }
    }
    // lawn
    let lawn: [u32; 3] = if night {
        [rgb(20, 52, 56), rgb(14, 40, 50), rgb(10, 28, 44)]
    } else {
        [rgb(96, 184, 84), rgb(70, 156, 72), rgb(48, 128, 64)]
    };
    c.vgrad(0, 144, W, H - 144, &lawn);
    // the manor
    let (wall, wall_sh, roof, col_c, win) = if night {
        (rgb(58, 62, 100), rgb(40, 42, 76), rgb(28, 30, 56), rgb(92, 98, 140), rgb(255, 208, 104))
    } else {
        (rgb(238, 224, 196), rgb(204, 186, 160), rgb(92, 100, 128), WHITE, rgb(72, 104, 150))
    };
    c.rect(104, 100, 176, 46, wall);
    c.rect(104, 100, 34, 46, wall_sh);
    c.rect(246, 100, 34, 46, wall_sh);
    c.rect(100, 94, 184, 6, roof);
    c.tri(150, 94, 234, 94, 192, 72, wall);
    c.line(150, 94, 192, 72, roof);
    c.line(234, 94, 192, 72, roof);
    c.ellipse(192, 86, 4, 4, win);
    for i in 0..6 {
        let x = 154 + i * 14;
        c.rect(x, 100, 5, 46, col_c);
        c.vline(x + 4, 100, 46, wall_sh);
    }
    for (i, x) in [110, 124, 252, 266].iter().enumerate() {
        for row in 0..2 {
            let lit = !night || hash(i as i32, row, 9) % 4 != 0;
            c.rect(*x, 106 + row * 20, 7, 11, if lit { win } else { roof });
        }
    }
    c.rect(187, 124, 11, 22, if night { win } else { rgb(110, 70, 48) });
    c.rect(146, 146, 92, 3, col_c);
    if night {
        glow(c, 192, 134, 30, rgb(255, 208, 104), 90);
    }
    // gravel path and topiary
    for y in 149..H {
        let half = 7 + (y - 149) * 2 / 3;
        c.hline(192 - half, y, half * 2, if night { rgb(70, 72, 104) } else { rgb(226, 208, 164) });
    }
    let (bush, bush_sh) = if night { (rgb(14, 44, 48), rgb(8, 28, 40)) } else { (rgb(40, 116, 60), rgb(24, 84, 52)) };
    for x in [70, 314] {
        c.rect(x - 1, 138, 3, 12, rgb(80, 56, 40));
        c.ellipse(x, 130, 11, 11, bush);
        c.ellipse(x + 3, 133, 7, 7, bush_sh);
        c.ellipse(x, 114, 6, 6, bush);
    }
    for x in [30, 354] {
        c.tri(x - 9, 150, x + 9, 150, x, 112, bush);
        c.tri(x + 2, 150, x + 9, 150, x, 112, bush_sh);
    }
}

/// The servants' hall: brick, copper pots, a stove and the bell board.
fn pantry(c: &mut Canvas, t: f32) {
    c.rect(0, 0, W, 142, rgb(112, 84, 72));
    for row in 0..15 {
        let y = row * 10;
        c.hline(0, y, W, rgb(84, 60, 54));
        let mut x = if row % 2 == 0 { 0 } else { 12 };
        while x < W {
            c.vline(x, y, 10, rgb(84, 60, 54));
            if hash(x, row, 2) % 5 == 0 {
                c.rect(x + 1, y + 1, 23, 9, rgb(124, 92, 76));
            }
            x += 24;
        }
    }
    c.rect(0, 0, W, 12, rgb(64, 44, 40));
    c.hline(0, 12, W, rgb(40, 28, 28));
    // copper pots on a rail
    c.hline(24, 22, 110, rgb(60, 52, 60));
    for i in 0..5 {
        let x = 36 + i * 22;
        c.vline(x, 22, 7, rgb(60, 52, 60));
        c.ellipse(x, 35, 8, 6, rgb(206, 120, 64));
        c.ellipse(x + 2, 37, 5, 3, rgb(160, 84, 48));
        c.rect(x - 4, 31, 3, 2, rgb(250, 190, 130));
    }
    // shelves of jars
    for (s, y) in [62, 90, 118].iter().enumerate() {
        c.rect(14, *y, 100, 3, rgb(72, 48, 36));
        c.hline(14, y + 3, 100, rgb(50, 34, 30));
        for i in 0..7 {
            let h = hash(i, s as i32, 4);
            let jar = [rgb(200, 80, 60), rgb(220, 170, 60), rgb(90, 150, 90), rgb(190, 200, 210), rgb(140, 80, 130)][(h % 5) as usize];
            let jh = 9 + (h % 4) as i32;
            c.rect(18 + i * 14, y - jh, 9, jh, jar);
            c.vline(19 + i * 14, y - jh + 1, jh - 2, mix(jar, WHITE, 110));
            c.rect(18 + i * 14, y - jh - 2, 9, 2, rgb(230, 224, 200));
        }
    }
    // stove
    c.rect(184, 12, 16, 60, rgb(52, 52, 66));
    c.vline(186, 12, 60, rgb(84, 84, 104));
    c.rect(146, 72, 92, 70, rgb(44, 44, 58));
    c.rect(146, 72, 92, 4, rgb(84, 84, 104));
    c.rect(158, 96, 68, 36, rgb(26, 26, 36));
    c.frame(158, 96, 68, 36, rgb(84, 84, 104));
    let tick = (t * 9.0) as u32;
    for y in 104..126 {
        for x in 166..218 {
            let h = hash(x / 2, y / 2, tick) % 8;
            let heat = (y - 104) as u32 / 3 + h;
            let col = match heat {
                0..=4 => rgb(60, 20, 24),
                5..=8 => rgb(200, 70, 30),
                9..=11 => rgb(248, 150, 40),
                _ => rgb(255, 232, 120),
            };
            c.put(x, y, col);
        }
    }
    glow(c, 192, 116, 46, rgb(255, 150, 60), 70);
    // kettle
    c.ellipse(170, 64, 10, 8, rgb(150, 156, 176));
    c.rect(164, 58, 5, 2, WHITE);
    c.line(180, 62, 186, 56, rgb(150, 156, 176));
    for i in 0..3 {
        let y = 50 - ((t * 10.0) as i32 + i * 6) % 18;
        c.tint(188 + (i % 2), y, WHITE, 140);
        c.tint(189 + (i % 2), y - 1, WHITE, 90);
    }
    // bell board
    c.rect(258, 30, 112, 62, rgb(72, 46, 36));
    c.frame(258, 30, 112, 62, rgb(200, 160, 72));
    for row in 0..3 {
        for i in 0..6 {
            let ringing = row == 2 && i == 4;
            let sway = if ringing { ((t * 14.0).sin() * 2.0) as i32 } else { 0 };
            let (x, y) = (272 + i * 17, 42 + row * 19);
            c.vline(x, y - 5, 3, rgb(40, 28, 28));
            c.ellipse(x + sway, y, 4, 4, if ringing { rgb(255, 224, 110) } else { rgb(208, 164, 72) });
            c.put(x + sway - 1, y - 1, WHITE);
            c.put(x + sway * 2, y + 5, rgb(40, 28, 28));
            c.rect(x - 4, y + 8, 9, 3, if ringing { GOLD } else { rgb(226, 220, 200) });
        }
    }
    // floor
    for y in 142..H {
        for x in 0..W {
            let tile = ((x + (y - 142) / 2) / 16 + (y - 142) / 12) % 2 == 0;
            c.put(x, y, if tile { rgb(150, 84, 64) } else { rgb(122, 64, 54) });
        }
    }
    c.hline(0, 142, W, rgb(50, 34, 30));
}

/// The zero-g observatory at the hub, with breakfast in orbit.
fn axis(c: &mut Canvas, t: f32) {
    c.vgrad(0, 0, W, H, &[rgb(52, 60, 88), rgb(36, 42, 66), rgb(24, 28, 48)]);
    for x in (0..W).step_by(48) {
        c.vline(x, 0, H, rgb(22, 26, 44));
        c.vline(x + 1, 0, H, rgb(70, 80, 112));
        for y in (10..H).step_by(40) {
            c.put(x + 4, y, rgb(120, 130, 160));
            c.put(x - 3, y, rgb(120, 130, 160));
        }
    }
    let (cx, cy, r) = (192, 84, 82);
    for y in -r - 6..=r + 6 {
        for x in -r - 6..=r + 6 {
            let d2 = x * x + y * y;
            let (px, py) = (cx + x, cy + y);
            if d2 <= r * r {
                let neb = ((px as f32 * 0.05).sin() + (py as f32 * 0.07 + px as f32 * 0.02).cos()) * 8.0 + 8.0;
                c.put(px, py, if neb as u32 > bayer(px, py) + 6 { rgb(30, 18, 56) } else { rgb(8, 10, 28) });
                stars(c, px, py, t);
            } else if d2 <= (r + 6) * (r + 6) {
                let brass = if d2 > (r + 4) * (r + 4) { rgb(120, 88, 40) } else if x + y < 0 { rgb(236, 196, 104) } else { rgb(184, 140, 64) };
                c.put(px, py, brass);
            }
        }
    }
    // Meridian Spindle, far off
    c.rect(222, 50, 34, 9, rgb(150, 158, 178));
    c.rect(222, 55, 34, 4, rgb(104, 110, 136));
    c.ellipse(222, 54, 2, 4, rgb(150, 158, 178));
    c.ellipse(256, 54, 2, 4, rgb(104, 110, 136));
    for i in 0..7 {
        c.put(226 + i * 4, 52 + (i % 2), rgb(255, 230, 140));
    }
    c.line(214, 46, 222, 51, rgb(104, 110, 136));
    c.line(264, 62, 256, 57, rgb(104, 110, 136));
    // the Veil
    for i in 0..26 {
        let a = 200u32.saturating_sub(i as u32 * 8);
        c.tint(150 - i, 104 - i / 2, rgb(190, 230, 255), a);
        c.tint(150 - i, 105 - i / 2, rgb(190, 230, 255), a / 2);
    }
    c.rect(150, 103, 2, 2, WHITE);
    // the fleet on picket
    for i in 0..12 {
        let x = 138 + i * 9;
        let y = 132 + (i % 3) * 4;
        c.hline(x, y, 4, rgb(170, 176, 196));
        c.put(x + 4, y, if (t * 2.0) as i32 % 12 == i { rgb(255, 120, 80) } else { rgb(110, 116, 140) });
    }
    // telescope
    for i in 0..5 {
        c.line(38, 150 + i, 92, 104 + i, if i < 2 { rgb(236, 196, 104) } else { rgb(170, 124, 56) });
    }
    c.ellipse(94, 105, 4, 5, rgb(120, 88, 40));
    c.line(60, 138, 50, 180, rgb(90, 70, 50));
    c.line(62, 138, 76, 180, rgb(90, 70, 50));
    // breakfast, in orbit
    let bob = |ph: f32, amp: f32| ((t * 0.9 + ph).sin() * amp) as i32;
    let (x, y) = (62 + bob(0.0, 7.0), 46 + bob(1.5, 5.0));
    for i in -9..=9i32 {
        let h = 5 - (i * i) / 18;
        c.vline(x + i, y - h + (i * i) / 12, 4, if i % 4 == 0 { rgb(184, 120, 48) } else { rgb(226, 164, 72) });
    }
    let (x, y) = (316 + bob(2.0, 5.0), 60 + bob(0.7, 8.0));
    c.rect(x, y, 12, 9, WHITE);
    c.rect(x + 9, y, 3, 9, rgb(200, 204, 224));
    c.hline(x, y + 2, 12, rgb(90, 120, 220));
    c.frame(x + 12, y + 2, 4, 5, WHITE);
    c.hline(x + 1, y, 10, rgb(150, 90, 50));
    let (x, y) = (338 + bob(4.0, 6.0), 96 + bob(3.0, 6.0));
    c.ellipse(x, y, 10, 3, WHITE);
    c.hline(x - 6, y + 1, 12, rgb(200, 204, 224));
    let (x, y) = (300 + bob(5.0, 4.0), 140 + bob(0.2, 7.0));
    c.rect(x, y, 10, 12, rgb(240, 140, 40));
    c.vline(x + 2, y + 2, 8, rgb(255, 200, 110));
    c.rect(x - 1, y - 3, 12, 3, rgb(210, 60, 60));
    for (i, (bx, by)) in [(330, 40), (286, 110), (350, 124)].iter().enumerate() {
        let r = 2 + (i as i32 % 2);
        c.ellipse(bx + bob(i as f32, 5.0), by + bob(i as f32 * 2.0, 5.0), r, r, rgb(170, 100, 50));
    }
}

/// The private dock, with Lord Solberg's yacht in the cradle.
fn dock(c: &mut Canvas, t: f32) {
    c.vgrad(0, 0, W, 124, &[rgb(84, 96, 120), rgb(64, 74, 98), rgb(48, 54, 78)]);
    for x in (0..W).step_by(32) {
        c.vline(x, 0, 124, rgb(40, 46, 66));
    }
    c.rect(0, 5, W, 4, rgb(110, 122, 146));
    c.hline(0, 9, W, rgb(40, 46, 66));
    c.rect(0, 13, W, 2, rgb(176, 120, 60));
    // bay door on to space
    c.rect(52, 22, 280, 100, rgb(30, 34, 52));
    for y in 26..118 {
        for x in 56..328 {
            c.put(x, y, rgb(8, 10, 28));
            stars(c, x, y, t);
        }
    }
    for x in [56, 324] {
        for y in 26..118 {
            c.rect(x, y, 4, 1, if (y / 5) % 2 == 0 { rgb(240, 200, 50) } else { rgb(30, 30, 36) });
        }
    }
    // the Unimpeachable
    c.tri(96, 76, 132, 70, 104, 44, rgb(200, 190, 170));
    c.tri(104, 44, 132, 70, 118, 70, rgb(236, 228, 208));
    c.ellipse(196, 78, 92, 19, rgb(240, 234, 216));
    for y in 80..98 {
        for x in 100..290 {
            if c.get(x, y) == rgb(240, 234, 216) && (y > 84 || bayer(x, y) < 8) {
                c.put(x, y, rgb(196, 186, 168));
            }
        }
    }
    c.hline(110, 78, 172, GOLD);
    c.hline(112, 80, 168, rgb(176, 136, 56));
    c.ellipse(256, 70, 18, 6, rgb(40, 96, 140));
    c.hline(244, 68, 16, rgb(140, 200, 236));
    for i in 0..8 {
        c.rect(140 + i * 12, 70, 5, 4, rgb(40, 96, 140));
    }
    c.rect(98, 70, 10, 16, rgb(70, 72, 90));
    let flick = (t * 12.0) as i32 % 2;
    c.rect(92 - flick, 73, 6 + flick, 10, rgb(255, 170, 70));
    c.rect(95, 75, 3, 6, rgb(255, 240, 180));
    glow(c, 94, 78, 14, rgb(255, 150, 60), 120);
    // cradle
    for x in [136, 252] {
        c.rect(x, 94, 7, 30, rgb(120, 130, 152));
        c.vline(x + 6, 94, 30, rgb(60, 66, 88));
        c.rect(x - 6, 92, 19, 4, rgb(150, 160, 180));
    }
    // deck
    c.vgrad(0, 124, W, H - 124, &[rgb(72, 78, 98), rgb(54, 58, 78), rgb(38, 40, 60)]);
    for y in 124..132 {
        for x in 0..W {
            c.put(x, y, if ((x + y) / 7) % 2 == 0 { rgb(240, 200, 50) } else { rgb(30, 30, 36) });
        }
    }
    for i in -6..=6 {
        c.line(192 + i * 22, 132, 192 + i * 60, H, rgb(44, 48, 68));
    }
    // the Grand Reserve
    for (x, y) in [(318, 112), (346, 112), (332, 90)] {
        c.rect(x, y, 26, 22, rgb(36, 124, 124));
        c.rect(x + 20, y, 6, 22, rgb(24, 88, 96));
        c.frame(x, y, 26, 22, rgb(16, 56, 64));
        c.rect(x + 3, y + 5, 15, 3, WHITE);
        c.rect(x + 3, y + 10, 10, 2, rgb(190, 220, 220));
    }
    // sedan chair
    c.rect(14, 96, 30, 36, rgb(112, 60, 140));
    c.rect(36, 96, 8, 36, rgb(80, 40, 108));
    c.rect(20, 102, 12, 14, rgb(30, 26, 50));
    c.rect(12, 92, 34, 5, GOLD);
    c.rect(2, 120, 56, 3, rgb(176, 136, 56));
}

/// The gala: chandeliers, tall windows and a crowd in silhouette.
fn ballroom(c: &mut Canvas, t: f32) {
    c.vgrad(0, 0, W, 142, &[rgb(88, 44, 100), rgb(68, 32, 84), rgb(48, 24, 68)]);
    for cxw in [50, 145, 239, 334] {
        c.ellipse(cxw, 44, 24, 20, rgb(190, 150, 70));
        c.rect(cxw - 24, 44, 49, 86, rgb(190, 150, 70));
        for y in 24..128 {
            for x in cxw - 22..=cxw + 22 {
                let dy = (44 - y).max(0);
                if dy == 0 || (x - cxw) * (x - cxw) * 324 + dy * dy * 484 <= 484 * 324 {
                    c.put(x, y, rgb(10, 12, 36));
                    stars(c, x, y, t);
                }
            }
        }
        // the fleet, keeping station
        for i in 0..5 {
            let x = cxw - 18 + i * 9;
            c.hline(x, 78 + (i % 2) * 5, 4, rgb(120, 128, 160));
            c.put(x + 4, 78 + (i % 2) * 5, rgb(255, 210, 110));
        }
        c.vline(cxw, 24, 104, rgb(150, 112, 50));
        c.hline(cxw - 22, 86, 45, rgb(150, 112, 50));
        for side in [-1, 1] {
            for y in 22..132 {
                let wv = ((y as f32 * 0.25).sin() * 2.0) as i32;
                let x = cxw + side * (25 + (y - 22) / 28) + wv;
                c.rect(x - 4, y, 9, 1, rgb(160, 32, 56));
                c.put(x + 2, y, rgb(112, 20, 44));
                c.put(x - 3, y, rgb(200, 60, 80));
            }
        }
    }
    for x in [0, 95, 190, 284, 378] {
        c.rect(x, 0, 6, 142, rgb(208, 168, 76));
        c.vline(x + 5, 0, 142, rgb(140, 104, 44));
        c.rect(x - 2, 0, 10, 6, rgb(236, 200, 110));
    }
    // floor
    for y in 142..H {
        for x in 0..W {
            let d = y - 142;
            let tile = ((x - 192) * 20 / (d + 20) + 100) / 6 + (d * 40 / (d + 40)) / 5;
            c.put(x, y, if tile % 2 == 0 { rgb(132, 88, 120) } else { rgb(98, 60, 98) });
        }
    }
    c.hline(0, 142, W, rgb(236, 200, 110));
    // crowd
    for i in 0..15 {
        let h = hash(i, 0, 11);
        let x = 8 + i * 26 + (h % 9) as i32;
        let bobv = ((t * 2.2 + i as f32 * 1.3).sin() * 1.6) as i32;
        let col = [rgb(34, 18, 46), rgb(44, 22, 54), rgb(28, 20, 56)][(h % 3) as usize];
        c.ellipse(x, 136 + bobv, 9, 16, col);
        c.ellipse(x, 114 + bobv, 5, 6, col);
        if h % 4 == 0 {
            c.ellipse(x, 108 + bobv, 6, 4, col);
        }
        if h % 3 == 0 {
            c.tri(x - 12, 152, x + 12, 152, x, 126 + bobv, col);
        }
    }
    // chandeliers
    for (cxc, y0) in [(96, 22), (288, 22), (192, 12)] {
        c.vline(cxc, 0, y0 - 4, rgb(150, 112, 50));
        c.ellipse(cxc, y0, 18, 4, rgb(208, 168, 76));
        c.ellipse(cxc, y0 + 5, 10, 3, rgb(150, 112, 50));
        for i in -2..=2 {
            let x = cxc + i * 8;
            c.rect(x, y0 - 8, 2, 6, WHITE);
            let f = hash(x, (t * 10.0) as i32, 5) % 3;
            c.put(x, y0 - 10 - (f == 0) as i32, rgb(255, 220, 110));
            c.put(x + 1, y0 - 9, rgb(255, 170, 60));
            c.put(x - (i % 2), y0 + 8 + (i.abs() % 2) * 2, rgb(190, 230, 255));
        }
        glow(c, cxc, y0 - 4, 46, rgb(255, 214, 130), 120);
        for y in 146..H {
            if bayer(cxc, y) < 9 {
                c.tint(cxc, y, rgb(255, 214, 130), 110);
                c.tint(cxc + 1, y, rgb(255, 214, 130), 60);
            }
        }
    }
}

/// Lord Varrow's study, and the war table.
fn study(c: &mut Canvas, t: f32) {
    c.vgrad(0, 0, W, 142, &[rgb(84, 52, 42), rgb(66, 40, 34), rgb(50, 30, 28)]);
    for x in (0..W).step_by(32) {
        c.vline(x, 0, 142, rgb(44, 26, 24));
        c.vline(x + 1, 0, 142, rgb(96, 62, 48));
    }
    for x0 in [8, 280] {
        c.rect(x0, 16, 96, 122, rgb(40, 24, 22));
        c.frame(x0, 16, 96, 122, rgb(110, 72, 48));
        for shelf in 0..4 {
            let base = 44 + shelf * 30;
            c.rect(x0 + 2, base, 92, 3, rgb(110, 72, 48));
            let mut x = x0 + 4;
            while x < x0 + 90 {
                let h = hash(x, shelf + x0, 6);
                let bw = 4 + (h % 3) as i32;
                let bh = 17 + (h / 7 % 6) as i32;
                let col = [rgb(150, 40, 48), rgb(44, 110, 76), rgb(40, 60, 130), rgb(196, 150, 60), rgb(110, 50, 110), rgb(40, 120, 130)][(h / 3 % 6) as usize];
                if h % 11 != 0 {
                    c.rect(x, base - bh, bw, bh, col);
                    c.vline(x + bw - 1, base - bh, bh, mix(col, BLACK, 90));
                    c.hline(x, base - bh + 3, bw - 1, GOLD);
                }
                x += bw + 1;
            }
        }
    }
    // the ancestor
    c.rect(160, 14, 64, 74, rgb(208, 168, 76));
    c.frame(160, 14, 64, 74, rgb(140, 104, 44));
    c.rect(165, 19, 54, 64, rgb(30, 60, 66));
    c.ellipse(192, 80, 22, 16, rgb(120, 30, 44));
    c.ellipse(192, 48, 11, 13, rgb(214, 170, 140));
    c.ellipse(192, 39, 12, 6, rgb(190, 190, 200));
    c.hline(186, 53, 13, rgb(190, 190, 200));
    c.rect(165, 83, 54, 1, rgb(30, 60, 66));
    c.rect(160, 84, 64, 4, rgb(208, 168, 76));
    // floor and carpet
    c.vgrad(0, 142, W, H - 142, &[rgb(120, 30, 44), rgb(96, 24, 40), rgb(70, 18, 36)]);
    for x in (0..W).step_by(16) {
        c.rect(x + 6, 146, 4, 2, rgb(200, 150, 70));
    }
    c.hline(0, 142, W, rgb(30, 18, 18));
    // lamp
    c.rect(118, 118, 2, 14, rgb(176, 136, 56));
    c.ellipse(119, 116, 9, 4, rgb(40, 150, 100));
    glow(c, 119, 122, 22, rgb(255, 230, 150), 100);
    // war table
    c.rect(92, 130, 200, 12, rgb(64, 38, 32));
    c.hline(92, 130, 200, rgb(208, 168, 76));
    c.hline(92, 131, 200, rgb(110, 72, 48));
    let flick = hash((t * 15.0) as i32, 0, 8) % 6 == 0;
    for y in 92..130 {
        for x in 110..274 {
            let spread = (130 - y) * 2 / 3;
            if x > 128 - spread && x < 256 + spread && bayer(x, y + (t * 6.0) as i32) < 3 {
                c.tint(x, y, rgb(80, 220, 240), if flick { 50 } else { 80 });
            }
        }
    }
    let spin = (t * 6.0) as i32;
    for (x0, col, hi) in [(134, rgb(240, 150, 60), rgb(255, 220, 150)), (224, rgb(40, 200, 190), rgb(190, 255, 245))] {
        c.rect(x0, 106, 28, 9, col);
        c.ellipse(x0, 110, 2, 4, col);
        c.ellipse(x0 + 28, 110, 2, 4, mix(col, BLACK, 80));
        for i in 0..4 {
            c.vline(x0 + (spin + i * 7) % 28, 106, 9, hi);
        }
    }
    for i in 0..6 {
        c.put(130 + i * 5, 120 + (i % 2) * 2, rgb(255, 220, 150));
    }
    let pulse = ((t * 3.0).sin() * 2.0 + 5.0) as i32;
    c.ring(193, 104, pulse, rgb(190, 240, 255));
    c.rect(192, 103, 3, 3, WHITE);
    for x in (166..=186).step_by(4) {
        c.put(x, 108, rgb(255, 90, 90));
        c.put(384 - x, 108, rgb(255, 90, 90));
    }
    c.text(177, 90, "VEIL", rgb(190, 240, 255));
}

// ------------------------------------------------------------------ sprites

#[derive(PartialEq)]
enum Extra {
    Moustache,
    Monocle,
    Glasses,
    Cap,
    Epaulettes,
    Sash,
    Cravat,
    Freckles,
    BeautyMark,
    Blush,
}

struct Look {
    skin: [u32; 2],
    hair: [u32; 2],
    coat: [u32; 2],
    shirt: u32,
    trim: u32,
    hair_style: u8,
    shoulders: i32,
    extras: &'static [Extra],
}

fn look(who: &str) -> Look {
    match who {
        // Peregrine Varrow, the heir
        "p" => Look {
            skin: [rgb(244, 206, 176), rgb(214, 166, 142)],
            hair: [rgb(240, 200, 88), rgb(196, 148, 52)],
            coat: [rgb(152, 124, 208), rgb(112, 86, 164)],
            shirt: rgb(250, 250, 255),
            trim: GOLD,
            hair_style: 2,
            shoulders: 42,
            extras: &[Extra::Cravat, Extra::Blush],
        },
        // Lord Cassius Varrow
        "v" => Look {
            skin: [rgb(232, 184, 158), rgb(196, 144, 124)],
            hair: [rgb(176, 180, 194), rgb(124, 128, 148)],
            coat: [rgb(156, 36, 52), rgb(110, 24, 42)],
            shirt: rgb(250, 250, 255),
            trim: GOLD,
            hair_style: 0,
            shoulders: 46,
            extras: &[Extra::Moustache, Extra::Epaulettes, Extra::Sash],
        },
        // Lord Hadrian Solberg of Meridian Spindle
        "s" => Look {
            skin: [rgb(248, 224, 210), rgb(216, 184, 176)],
            hair: [rgb(238, 234, 248), rgb(190, 186, 216)],
            coat: [rgb(30, 136, 136), rgb(20, 96, 104)],
            shirt: rgb(250, 250, 255),
            trim: rgb(214, 224, 236),
            hair_style: 4,
            shoulders: 42,
            extras: &[Extra::Monocle, Extra::Cravat, Extra::BeautyMark],
        },
        // Quince, Solberg's steward
        "q" => Look {
            skin: [rgb(144, 94, 68), rgb(110, 68, 52)],
            hair: [rgb(44, 38, 56), rgb(26, 22, 36)],
            coat: [rgb(78, 82, 100), rgb(54, 56, 74)],
            shirt: rgb(236, 240, 244),
            trim: rgb(40, 190, 176),
            hair_style: 1,
            shoulders: 40,
            extras: &[Extra::Glasses],
        },
        // Tobe, Solberg's footman
        "t" => Look {
            skin: [rgb(226, 176, 138), rgb(190, 136, 106)],
            hair: [rgb(208, 88, 44), rgb(156, 56, 34)],
            coat: [rgb(44, 148, 146), rgb(30, 106, 110)],
            shirt: rgb(236, 240, 244),
            trim: rgb(214, 224, 236),
            hair_style: 3,
            shoulders: 38,
            extras: &[Extra::Freckles],
        },
        // Cook
        _ => Look {
            skin: [rgb(184, 128, 96), rgb(148, 96, 74)],
            hair: [rgb(120, 112, 124), rgb(84, 78, 92)],
            coat: [rgb(228, 228, 236), rgb(186, 188, 206)],
            shirt: rgb(206, 72, 72),
            trim: rgb(160, 44, 52),
            hair_style: 1,
            shoulders: 48,
            extras: &[Extra::Cap, Extra::Blush],
        },
    }
}

pub fn sprite(who: &str, expr: &str) -> Canvas {
    let mut c = Canvas::new(SPR_W, SPR_H);
    if who == "a" {
        swan(&mut c, expr);
    } else {
        person(&mut c, &look(who), expr);
    }
    c.outline(OUTLINE);
    c
}

fn thick_line(c: &mut Canvas, x0: i32, y0: i32, x1: i32, y1: i32, col: u32) {
    c.line(x0, y0, x1, y1, col);
    c.line(x0, y0 + 1, x1, y1 + 1, col);
}

fn person(c: &mut Canvas, l: &Look, expr: &str) {
    let cx = 56;
    let has = |e: Extra| l.extras.contains(&e);
    let (hair, hair_sh) = (l.hair[0], l.hair[1]);

    // hair that sits behind the head
    match l.hair_style {
        1 => c.ellipse(cx, 30, 10, 9, hair),
        4 => {
            c.ellipse(cx, 24, 20, 20, hair);
            c.ellipse(cx, 46, 32, 30, hair);
            for k in 0..4 {
                for x in [cx - 31, cx + 31] {
                    c.ellipse(x, 66 + k * 11, 9, 6, hair);
                    c.hline(x - 6, 70 + k * 11, 13, hair_sh);
                }
            }
            c.rect(cx - 5, 5, 11, 4, l.coat[0]);
            c.rect(cx - 1, 4, 3, 6, l.trim);
        }
        _ => {}
    }

    // shoulders and coat
    let sw = l.shoulders;
    c.ellipse(cx, 118, sw, 18, l.coat[0]);
    c.rect(cx - sw, 118, sw * 2 + 1, SPR_H - 118, l.coat[0]);
    c.shade_right(l.coat[0], l.coat[1], cx + sw / 3);
    if has(Extra::Sash) {
        for y in 100..SPR_H {
            for x in 0..14 {
                let px = 24 + (y - 100) + x;
                let cur = c.get(px, y);
                if cur == l.coat[0] || cur == l.coat[1] {
                    c.put(px, y, if x < 2 || x > 11 { GOLD } else { rgb(60, 90, 190) });
                }
            }
        }
    }
    for y in 98..130 {
        let half = (130 - y) * 11 / 32;
        c.hline(cx - half, y, half * 2 + 1, l.shirt);
        c.put(cx - half - 1, y, l.trim);
        c.put(cx - half - 2, y, l.trim);
        c.put(cx + half + 1, y, l.trim);
        c.put(cx + half + 2, y, l.trim);
    }
    for y in [136, 146] {
        c.rect(cx - 1, y, 3, 3, l.trim);
    }
    if has(Extra::Epaulettes) {
        for x in [cx - sw + 10, cx + sw - 10] {
            c.ellipse(x, 104, 11, 5, GOLD);
            c.hline(x - 8, 106, 17, rgb(176, 136, 56));
            for i in -4..=4 {
                c.vline(x + i * 2, 108, 5, if i % 2 == 0 { GOLD } else { rgb(176, 136, 56) });
            }
        }
    }

    // neck, ears, head
    c.rect(cx - 7, 82, 15, 18, l.skin[0]);
    c.rect(cx - 7, 82, 15, 7, l.skin[1]);
    if has(Extra::Cravat) {
        c.ellipse(cx, 98, 10, 5, WHITE);
        c.ellipse(cx, 106, 8, 5, rgb(216, 220, 240));
        c.ellipse(cx, 113, 6, 4, WHITE);
        c.rect(cx - 1, 97, 3, 3, rgb(220, 60, 80));
    }
    c.ellipse(cx - 21, 62, 3, 5, l.skin[0]);
    c.ellipse(cx + 21, 62, 3, 5, l.skin[1]);
    c.ellipse(cx, 60, 21, 25, l.skin[0]);
    c.shade_right(l.skin[0], l.skin[1], cx + 11);

    // hair in front
    let hairline = |x: i32| -> i32 {
        let d = x - cx;
        match l.hair_style {
            0 => 45 + d * d / 60,
            1 => 44 + d * d / 48,
            2 => 53 - (x - 34) / 4 + ((x as f32 * 0.7).sin() * 2.0) as i32,
            3 => 46 + ((x / 4) % 2) * 3,
            _ => 43 + d * d / 40,
        }
    };
    let (hrx, hry) = if l.hair_style == 2 { (25, 30) } else { (23, 28) };
    for y in 28..70 {
        for x in cx - hrx..=cx + hrx {
            let (dx, dy) = (x - cx, y - 58);
            if dx * dx * hry * hry + dy * dy * hrx * hrx <= hrx * hrx * hry * hry && y < hairline(x) {
                c.put(x, y, hair);
            }
        }
    }
    match l.hair_style {
        0 => {
            c.rect(cx - 22, 48, 3, 14, hair);
            c.rect(cx + 20, 48, 3, 14, hair);
        }
        1 => c.vline(cx, 33, 10, hair_sh),
        2 => {
            c.ellipse(44, 34, 10, 7, hair);
            c.ellipse(60, 31, 12, 7, hair);
            c.ellipse(73, 38, 8, 7, hair);
            c.rect(31, 48, 5, 16, hair);
            c.line(40, 40, 52, 46, hair_sh);
            c.line(56, 36, 66, 42, hair_sh);
        }
        3 => {
            for i in 0..7 {
                let x = 35 + i * 7;
                c.tri(x - 4, 40, x + 5, 40, x + (i % 3) - 1, 27 - (i % 2) * 4, hair);
            }
            c.rect(cx - 22, 46, 3, 10, hair);
            c.rect(cx + 20, 46, 3, 10, hair);
        }
        _ => {}
    }
    c.shade_right(hair, hair_sh, cx + 9);
    if has(Extra::Cap) {
        c.ellipse(cx, 28, 25, 13, WHITE);
        c.ellipse(cx + 8, 31, 14, 8, rgb(214, 216, 232));
        c.rect(cx - 21, 36, 43, 6, WHITE);
        c.hline(cx - 21, 41, 43, rgb(214, 216, 232));
    }

    // face
    let ink = rgb(48, 32, 44);
    let brow = if l.hair_style == 4 { rgb(150, 146, 176) } else { hair_sh };
    let (ly, ry) = match expr {
        "smile" => ((53, 53), (53, 53)),
        "worried" => ((55, 52), (52, 55)),
        "angry" => ((52, 55), (55, 52)),
        "smug" => ((54, 54), (51, 50)),
        "shock" => ((50, 50), (50, 50)),
        _ => ((54, 54), (54, 54)),
    };
    thick_line(c, 44, ly.0, 50, ly.1, brow);
    thick_line(c, 62, ry.0, 68, ry.1, brow);
    for ex in [45, 63] {
        match expr {
            "smile" => {
                c.put(ex, 61, ink);
                c.hline(ex + 1, 60, 3, ink);
                c.put(ex + 4, 61, ink);
            }
            "smug" => {
                c.hline(ex - 1, 59, 7, ink);
                c.rect(ex, 60, 5, 2, WHITE);
                c.rect(ex + 2, 60, 2, 2, ink);
            }
            "shock" => {
                c.hline(ex, 56, 5, ink);
                c.rect(ex, 57, 5, 5, WHITE);
                c.rect(ex + 2, 59, 2, 2, ink);
            }
            _ => {
                c.hline(ex - 1, 58, 7, ink);
                c.rect(ex, 59, 5, 3, WHITE);
                c.rect(ex + 2, 59, 2, 3, ink);
                if expr == "angry" {
                    c.hline(ex, 59, 5, ink);
                }
            }
        }
    }
    c.vline(cx + 1, 64, 5, l.skin[1]);
    c.hline(cx - 1, 69, 3, l.skin[1]);
    if has(Extra::Blush) {
        for (x, y) in [(43, 67), (45, 68), (47, 67), (65, 67), (67, 68), (69, 67)] {
            c.put(x, y, rgb(240, 140, 140));
        }
    }
    if has(Extra::Freckles) {
        for (x, y) in [(44, 66), (47, 67), (50, 66), (63, 66), (66, 67), (69, 66)] {
            c.put(x, y, l.skin[1]);
        }
    }
    if has(Extra::BeautyMark) {
        c.rect(46, 71, 2, 2, ink);
    }
    let lip = rgb(140, 48, 60);
    match expr {
        "smile" => {
            c.hline(51, 75, 11, lip);
            c.put(50, 74, lip);
            c.put(62, 74, lip);
            c.hline(53, 76, 7, WHITE);
            c.hline(54, 77, 5, lip);
        }
        "worried" => {
            c.hline(53, 75, 7, lip);
            c.put(52, 76, lip);
            c.put(60, 76, lip);
        }
        "angry" => {
            c.hline(52, 75, 9, lip);
            c.put(51, 76, lip);
            c.put(61, 76, lip);
            c.put(50, 77, lip);
            c.put(62, 77, lip);
        }
        "smug" => {
            c.hline(52, 75, 8, lip);
            c.put(60, 74, lip);
            c.put(61, 73, lip);
        }
        "shock" => {
            c.ellipse(cx, 76, 3, 3, ink);
            c.rect(cx - 1, 76, 3, 2, rgb(200, 80, 90));
        }
        _ => c.hline(52, 75, 9, lip),
    }
    if has(Extra::Moustache) {
        c.ellipse(50, 72, 6, 2, hair);
        c.ellipse(62, 72, 6, 2, hair_sh);
        c.put(43, 71, hair);
        c.put(42, 70, hair);
        c.put(69, 71, hair_sh);
        c.put(70, 70, hair_sh);
    }
    if has(Extra::Monocle) {
        c.ring(65, 60, 7, GOLD);
        for y in 66..104 {
            if y % 2 == 0 {
                c.put(72 + (y - 66) / 8, y, GOLD);
            }
        }
    }
    if has(Extra::Glasses) {
        let fr = rgb(200, 220, 230);
        c.frame(42, 55, 11, 9, fr);
        c.frame(60, 55, 11, 9, fr);
        c.hline(53, 58, 7, fr);
        c.put(44, 57, WHITE);
        c.put(62, 57, WHITE);
    }
}

/// Admiral Featherstonehaugh.
fn swan(c: &mut Canvas, expr: &str) {
    let (white, sh) = (rgb(246, 246, 252), rgb(200, 206, 228));
    c.tri(84, 112, 108, 94, 96, 128, white);
    c.ellipse(62, 124, 36, 24, white);
    c.ellipse(70, 124, 24, 14, sh);
    c.ellipse(68, 121, 23, 12, white);
    c.line(52, 124, 86, 126, sh);
    c.line(56, 129, 82, 131, sh);
    c.shade_right(white, sh, 84);
    for i in 0..=48 {
        let t = i as f32 / 48.0;
        let x = 34.0 + (t * PI).sin() * 9.0 - t * 4.0;
        let y = 114.0 - t * 64.0;
        c.ellipse(x as i32, y as i32, 6 - (t * 1.5) as i32, 5, white);
        c.put(x as i32 + 4, y as i32, sh);
    }
    c.ellipse(31, 48, 9, 8, white);
    c.tri(24, 46, 8, 53, 24, 54, rgb(240, 130, 40));
    c.line(10, 53, 23, 51, rgb(180, 80, 30));
    c.rect(22, 44, 4, 4, OUTLINE);
    c.rect(28, 46, 3, 3, OUTLINE);
    c.put(28, 46, WHITE);
    if expr == "angry" {
        thick_line(c, 25, 41, 33, 44, OUTLINE);
    }
    // bicorne
    let navy = rgb(36, 44, 96);
    c.tri(16, 39, 48, 39, 32, 22, navy);
    c.ellipse(32, 39, 17, 4, navy);
    c.hline(15, 40, 35, GOLD);
    c.line(17, 38, 32, 23, GOLD);
    c.line(47, 38, 32, 23, GOLD);
    c.rect(31, 30, 3, 3, rgb(220, 60, 70));
    // cravat and decoration
    c.ellipse(36, 100, 8, 3, rgb(150, 124, 208));
    c.rect(34, 99, 3, 3, GOLD);
    c.rect(44, 114, 3, 6, rgb(220, 60, 70));
    c.ellipse(45, 122, 3, 3, GOLD);
}
