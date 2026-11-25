//! Tiny software renderer: a 384x216 canvas, pixel primitives, ordered
//! dithering and an 8x8 bitmap font. Everything in the game is drawn with this.

use font8x8::legacy::BASIC_LEGACY;

pub const W: i32 = 384;
pub const H: i32 = 216;

/// Opaque colour. A pixel value of 0 means "transparent" in sprites.
pub const fn rgb(r: u8, g: u8, b: u8) -> u32 {
    0xFF00_0000 | ((r as u32) << 16) | ((g as u32) << 8) | b as u32
}

pub const BLACK: u32 = rgb(0, 0, 0);
pub const WHITE: u32 = rgb(248, 248, 248);
pub const GOLD: u32 = rgb(240, 200, 88);
pub const OUTLINE: u32 = rgb(24, 20, 40);

/// Blend `a` towards `b`; `t` runs 0..=256.
pub fn mix(a: u32, b: u32, t: u32) -> u32 {
    let t = t.min(256);
    let s = 256 - t;
    let ch = |sh: u32| ((((a >> sh) & 0xFF) * s + ((b >> sh) & 0xFF) * t) >> 8) << sh;
    0xFF00_0000 | ch(16) | ch(8) | ch(0)
}

const BAYER: [[u32; 4]; 4] = [[0, 8, 2, 10], [12, 4, 14, 6], [3, 11, 1, 9], [15, 7, 13, 5]];

/// 4x4 ordered-dither threshold, 0..16.
pub fn bayer(x: i32, y: i32) -> u32 {
    BAYER[(y & 3) as usize][(x & 3) as usize]
}

/// Cheap deterministic hash for procedural art.
pub fn hash(a: i32, b: i32, s: u32) -> u32 {
    let mut h = (a as u32).wrapping_mul(374_761_393)
        ^ (b as u32).wrapping_mul(668_265_263)
        ^ s.wrapping_mul(2_246_822_519);
    h = (h ^ (h >> 13)).wrapping_mul(1_274_126_177);
    h ^ (h >> 16)
}

#[derive(Clone)]
pub struct Canvas {
    pub w: i32,
    pub h: i32,
    pub px: Vec<u32>,
}

impl Canvas {
    pub fn new(w: i32, h: i32) -> Self {
        Canvas { w, h, px: vec![0; (w * h) as usize] }
    }

    pub fn clear(&mut self, c: u32) {
        self.px.fill(c);
    }

    pub fn get(&self, x: i32, y: i32) -> u32 {
        if x < 0 || y < 0 || x >= self.w || y >= self.h {
            0
        } else {
            self.px[(y * self.w + x) as usize]
        }
    }

    pub fn put(&mut self, x: i32, y: i32, c: u32) {
        if x >= 0 && y >= 0 && x < self.w && y < self.h {
            self.px[(y * self.w + x) as usize] = c;
        }
    }

    /// Blend a colour over the existing pixel (t 0..=256).
    pub fn tint(&mut self, x: i32, y: i32, c: u32, t: u32) {
        if x >= 0 && y >= 0 && x < self.w && y < self.h {
            let i = (y * self.w + x) as usize;
            self.px[i] = mix(self.px[i], c, t);
        }
    }

    pub fn rect(&mut self, x: i32, y: i32, w: i32, h: i32, c: u32) {
        for yy in y.max(0)..(y + h).min(self.h) {
            for xx in x.max(0)..(x + w).min(self.w) {
                self.px[(yy * self.w + xx) as usize] = c;
            }
        }
    }

    pub fn hline(&mut self, x: i32, y: i32, w: i32, c: u32) {
        self.rect(x, y, w, 1, c);
    }

    pub fn vline(&mut self, x: i32, y: i32, h: i32, c: u32) {
        self.rect(x, y, 1, h, c);
    }

    pub fn frame(&mut self, x: i32, y: i32, w: i32, h: i32, c: u32) {
        self.hline(x, y, w, c);
        self.hline(x, y + h - 1, w, c);
        self.vline(x, y, h, c);
        self.vline(x + w - 1, y, h, c);
    }

    pub fn line(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, c: u32) {
        let (dx, dy) = ((x1 - x0).abs(), -(y1 - y0).abs());
        let (sx, sy) = (if x0 < x1 { 1 } else { -1 }, if y0 < y1 { 1 } else { -1 });
        let (mut x, mut y, mut err) = (x0, y0, dx + dy);
        loop {
            self.put(x, y, c);
            if x == x1 && y == y1 {
                break;
            }
            let e2 = 2 * err;
            if e2 >= dy {
                err += dy;
                x += sx;
            }
            if e2 <= dx {
                err += dx;
                y += sy;
            }
        }
    }

    pub fn ellipse(&mut self, cx: i32, cy: i32, rx: i32, ry: i32, c: u32) {
        let (rx2, ry2) = ((rx * rx).max(1) as i64, (ry * ry).max(1) as i64);
        for y in -ry..=ry {
            for x in -rx..=rx {
                if (x * x) as i64 * ry2 + (y * y) as i64 * rx2 <= rx2 * ry2 {
                    self.put(cx + x, cy + y, c);
                }
            }
        }
    }

    /// One pixel wide circle outline.
    pub fn ring(&mut self, cx: i32, cy: i32, r: i32, c: u32) {
        for y in -r - 1..=r + 1 {
            for x in -r - 1..=r + 1 {
                let d = x * x + y * y;
                if d <= r * r + r && d >= (r - 1) * (r - 1) + r - 1 {
                    self.put(cx + x, cy + y, c);
                }
            }
        }
    }

    pub fn tri(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, x2: i32, y2: i32, c: u32) {
        let edge = |ax: i32, ay: i32, bx: i32, by: i32, px: i32, py: i32| {
            (bx - ax) * (py - ay) - (by - ay) * (px - ax)
        };
        for y in y0.min(y1).min(y2)..=y0.max(y1).max(y2) {
            for x in x0.min(x1).min(x2)..=x0.max(x1).max(x2) {
                let (a, b, d) = (
                    edge(x0, y0, x1, y1, x, y),
                    edge(x1, y1, x2, y2, x, y),
                    edge(x2, y2, x0, y0, x, y),
                );
                if (a >= 0 && b >= 0 && d >= 0) || (a <= 0 && b <= 0 && d <= 0) {
                    self.put(x, y, c);
                }
            }
        }
    }

    /// Vertical gradient through `cols`, banded with ordered dithering.
    pub fn vgrad(&mut self, x: i32, y: i32, w: i32, h: i32, cols: &[u32]) {
        let n = cols.len() as i32 - 1;
        for yy in 0..h {
            let f = yy * n * 16 / h.max(1);
            let (i, frac) = ((f / 16) as usize, (f % 16) as u32);
            for xx in 0..w {
                let c = if frac > bayer(x + xx, y + yy) { cols[(i + 1).min(n as usize)] } else { cols[i] };
                self.put(x + xx, y + yy, c);
            }
        }
    }

    /// Replace `base` with `shade` to the right of `xt`, with a dithered edge.
    pub fn shade_right(&mut self, base: u32, shade: u32, xt: i32) {
        for y in 0..self.h {
            for x in xt..self.w {
                if self.get(x, y) == base && (x > xt + 2 || (x + y) % 2 == 0) {
                    self.put(x, y, shade);
                }
            }
        }
    }

    /// Dark outline around every opaque region (for sprites).
    pub fn outline(&mut self, c: u32) {
        let src = self.clone();
        for y in 0..self.h {
            for x in 0..self.w {
                if src.get(x, y) == 0
                    && (src.get(x - 1, y) != 0
                        || src.get(x + 1, y) != 0
                        || src.get(x, y - 1) != 0
                        || src.get(x, y + 1) != 0)
                {
                    self.put(x, y, c);
                }
            }
        }
    }

    /// Draw a sprite. `alpha` (0..=16) dissolves it in with a dither pattern;
    /// `dim` darkens it (used for characters who are not speaking).
    pub fn blit(&mut self, src: &Canvas, x: i32, y: i32, dim: bool, alpha: u32) {
        for sy in 0..src.h {
            for sx in 0..src.w {
                let p = src.px[(sy * src.w + sx) as usize];
                if p == 0 || bayer(sx, sy) >= alpha {
                    continue;
                }
                self.put(x + sx, y + sy, if dim { mix(p, rgb(16, 16, 48), 110) } else { p });
            }
        }
    }

    /// Fade the whole canvas towards black in chunky steps.
    pub fn fade(&mut self, amount: u32) {
        let a = (amount.min(256) / 32) * 32;
        if a == 0 {
            return;
        }
        for p in self.px.iter_mut() {
            *p = mix(*p, BLACK, a);
        }
    }

    /// Snap colours to 15-bit (5 bits per channel), like the consoles of the era.
    pub fn quantize(&mut self) {
        for p in self.px.iter_mut() {
            *p = 0xFF00_0000 | (*p & 0x00F8_F8F8);
        }
    }

    pub fn text_scaled(&mut self, x: i32, y: i32, s: &str, c: u32, scale: i32) {
        let mut cx = x;
        for ch in s.chars() {
            let idx = if (ch as u32) < 128 { ch as usize } else { b'?' as usize };
            for (row, bits) in BASIC_LEGACY[idx].iter().enumerate() {
                for col in 0..8 {
                    if bits & (1 << col) != 0 {
                        self.rect(cx + col * scale, y + row as i32 * scale, scale, scale, c);
                    }
                }
            }
            cx += 8 * scale;
        }
    }

    pub fn text(&mut self, x: i32, y: i32, s: &str, c: u32) {
        self.text_scaled(x, y, s, c, 1);
    }

    /// Text with a drop shadow, readable over any background.
    pub fn text_sh(&mut self, x: i32, y: i32, s: &str, c: u32, scale: i32) {
        self.text_scaled(x + scale, y + scale, s, OUTLINE, scale);
        self.text_scaled(x, y, s, c, scale);
    }

    /// Centred shadowed text.
    pub fn text_c(&mut self, cx: i32, y: i32, s: &str, c: u32, scale: i32) {
        let w = s.chars().count() as i32 * 8 * scale;
        self.text_sh(cx - w / 2, y, s, c, scale);
    }

    /// JRPG-style window panel.
    pub fn panel(&mut self, x: i32, y: i32, w: i32, h: i32) {
        self.vgrad(x, y, w, h, &[rgb(32, 44, 112), rgb(16, 20, 64), rgb(10, 10, 40)]);
        self.frame(x, y, w, h, rgb(240, 240, 255));
        self.frame(x + 1, y + 1, w - 2, h - 2, rgb(112, 128, 208));
        self.frame(x - 1, y - 1, w + 2, h + 2, OUTLINE);
    }
}
