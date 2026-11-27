//! Software renderer: a 1152x648 ARGB canvas with anti-aliased primitives,
//! alpha compositing, noise, blur, resampling, PNG I/O and a bitmap font.
//! Everything in the game is drawn with this.

#![allow(dead_code)] // this is a toolkit; not every primitive is used by every scene

use font8x8::legacy::BASIC_LEGACY;

/// Frame size. All art and UI coordinates are in this space.
pub const W: i32 = 1152;
pub const H: i32 = 648;

/// Opaque colour. Pixels are 0xAARRGGBB; alpha 0 is fully transparent.
pub const fn rgb(r: u8, g: u8, b: u8) -> u32 {
    0xFF00_0000 | ((r as u32) << 16) | ((g as u32) << 8) | b as u32
}

pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> u32 {
    ((a as u32) << 24) | ((r as u32) << 16) | ((g as u32) << 8) | b as u32
}

/// Replace a colour's alpha (0..=255).
pub fn with_alpha(c: u32, a: u32) -> u32 {
    (c & 0x00FF_FFFF) | (a.min(255) << 24)
}

pub const BLACK: u32 = rgb(0, 0, 0);
pub const WHITE: u32 = rgb(248, 248, 248);
pub const GOLD: u32 = rgb(240, 200, 88);
pub const OUTLINE: u32 = rgb(24, 20, 40);

/// Blend opaque `a` towards `b`; `t` runs 0..=256. The result is opaque.
pub fn mix(a: u32, b: u32, t: u32) -> u32 {
    let t = t.min(256);
    let s = 256 - t;
    let ch = |sh: u32| ((((a >> sh) & 0xFF) * s + ((b >> sh) & 0xFF) * t) >> 8) << sh;
    0xFF00_0000 | ch(16) | ch(8) | ch(0)
}

/// Floating point colour interpolation, `t` in 0.0..=1.0. Opaque result.
pub fn lerp(a: u32, b: u32, t: f32) -> u32 {
    let t = t.clamp(0.0, 1.0);
    let ch = |sh: u32| {
        let (x, y) = (((a >> sh) & 0xFF) as f32, ((b >> sh) & 0xFF) as f32);
        ((x + (y - x) * t + 0.5) as u32).min(255) << sh
    };
    0xFF00_0000 | ch(16) | ch(8) | ch(0)
}

/// Multiply a colour's brightness (1.0 = unchanged); keeps alpha.
pub fn scale_rgb(c: u32, f: f32) -> u32 {
    let ch = |sh: u32| ((((c >> sh) & 0xFF) as f32 * f) as u32).min(255) << sh;
    (c & 0xFF00_0000) | ch(16) | ch(8) | ch(0)
}

/// Porter-Duff "over": composite `src` on top of `dst`, honouring both alphas.
pub fn over(dst: u32, src: u32) -> u32 {
    let sa = src >> 24;
    if sa == 255 {
        return src;
    }
    if sa == 0 {
        return dst;
    }
    let da = dst >> 24;
    let db = da * (255 - sa) / 255;
    let oa = sa + db;
    let ch = |sh: u32| ((((src >> sh) & 0xFF) * sa + ((dst >> sh) & 0xFF) * db) / oa.max(1)) << sh;
    (oa << 24) | ch(16) | ch(8) | ch(0)
}

const BAYER: [[u32; 4]; 4] = [[0, 8, 2, 10], [12, 4, 14, 6], [3, 11, 1, 9], [15, 7, 13, 5]];

/// 4x4 ordered-dither threshold, 0..16.
pub fn bayer(x: i32, y: i32) -> u32 {
    BAYER[(y & 3) as usize][(x & 3) as usize]
}

/// Cheap deterministic integer hash for procedural art.
pub fn hash(a: i32, b: i32, s: u32) -> u32 {
    let mut h = (a as u32).wrapping_mul(374_761_393)
        ^ (b as u32).wrapping_mul(668_265_263)
        ^ s.wrapping_mul(2_246_822_519);
    h = (h ^ (h >> 13)).wrapping_mul(1_274_126_177);
    h ^ (h >> 16)
}

/// Hash to a float in 0.0..1.0.
pub fn hashf(a: i32, b: i32, s: u32) -> f32 {
    (hash(a, b, s) & 0xFFFF) as f32 / 65536.0
}

/// Smooth value noise in 0.0..1.0; features are about one unit across.
pub fn noise2(x: f32, y: f32, seed: u32) -> f32 {
    let (xi, yi) = (x.floor(), y.floor());
    let (fx, fy) = (x - xi, y - yi);
    let (u, v) = (fx * fx * (3.0 - 2.0 * fx), fy * fy * (3.0 - 2.0 * fy));
    let (xi, yi) = (xi as i32, yi as i32);
    let top = hashf(xi, yi, seed) * (1.0 - u) + hashf(xi + 1, yi, seed) * u;
    let bot = hashf(xi, yi + 1, seed) * (1.0 - u) + hashf(xi + 1, yi + 1, seed) * u;
    top * (1.0 - v) + bot * v
}

/// Fractal noise in 0.0..1.0: `octaves` layers of `noise2`.
pub fn fbm(x: f32, y: f32, octaves: u32, seed: u32) -> f32 {
    let (mut sum, mut amp, mut freq, mut norm) = (0.0, 0.5, 1.0, 0.0);
    for o in 0..octaves {
        sum += noise2(x * freq, y * freq, seed.wrapping_add(o * 101)) * amp;
        norm += amp;
        amp *= 0.5;
        freq *= 2.0;
    }
    sum / norm
}

#[derive(Clone)]
pub struct Canvas {
    pub w: i32,
    pub h: i32,
    pub px: Vec<u32>,
}

/// Largest picture that will be opened, along either side.
pub const MAX_PNG_SIDE: u32 = 8192;

impl Canvas {
    /// A fully transparent canvas.
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

    /// Overwrite a pixel (no blending).
    pub fn put(&mut self, x: i32, y: i32, c: u32) {
        if x >= 0 && y >= 0 && x < self.w && y < self.h {
            self.px[(y * self.w + x) as usize] = c;
        }
    }

    /// Composite a colour over a pixel using the colour's own alpha.
    pub fn blend(&mut self, x: i32, y: i32, c: u32) {
        if x >= 0 && y >= 0 && x < self.w && y < self.h {
            let i = (y * self.w + x) as usize;
            self.px[i] = over(self.px[i], c);
        }
    }

    /// Composite a colour over a pixel with coverage `t` (0..=256).
    pub fn tint(&mut self, x: i32, y: i32, c: u32, t: u32) {
        let a = (c >> 24) * t.min(256) / 256;
        self.blend(x, y, with_alpha(c, a));
    }

    /// Composite with floating point coverage 0.0..=1.0.
    pub fn cover(&mut self, x: i32, y: i32, c: u32, coverage: f32) {
        if coverage > 0.0 {
            self.tint(x, y, c, (coverage.min(1.0) * 256.0) as u32);
        }
    }

    pub fn rect(&mut self, x: i32, y: i32, w: i32, h: i32, c: u32) {
        for yy in y.max(0)..(y + h).min(self.h) {
            for xx in x.max(0)..(x + w).min(self.w) {
                self.px[(yy * self.w + xx) as usize] = c;
            }
        }
    }

    /// Rectangle composited with the colour's alpha.
    pub fn rect_blend(&mut self, x: i32, y: i32, w: i32, h: i32, c: u32) {
        for yy in y.max(0)..(y + h).min(self.h) {
            for xx in x.max(0)..(x + w).min(self.w) {
                let i = (yy * self.w + xx) as usize;
                self.px[i] = over(self.px[i], c);
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

    /// Hard one pixel line.
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

    /// Hard-edged filled ellipse.
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

    /// Anti-aliased filled ellipse, composited with the colour's alpha.
    pub fn ellipse_aa(&mut self, cx: f32, cy: f32, rx: f32, ry: f32, c: u32) {
        let (rx, ry) = (rx.max(0.5), ry.max(0.5));
        let rmin = rx.min(ry);
        for y in (cy - ry - 1.0).floor() as i32..=(cy + ry + 1.0).ceil() as i32 {
            for x in (cx - rx - 1.0).floor() as i32..=(cx + rx + 1.0).ceil() as i32 {
                let (nx, ny) = ((x as f32 + 0.5 - cx) / rx, (y as f32 + 0.5 - cy) / ry);
                let d = ((nx * nx + ny * ny).sqrt() - 1.0) * rmin;
                self.cover(x, y, c, 0.5 - d);
            }
        }
    }

    /// Anti-aliased thick line with round caps (`r` is the half-width).
    pub fn capsule(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, r: f32, c: u32) {
        let (dx, dy) = (x1 - x0, y1 - y0);
        let len2 = (dx * dx + dy * dy).max(1e-6);
        let (lx, hx) = ((x0.min(x1) - r - 1.0).floor() as i32, (x0.max(x1) + r + 1.0).ceil() as i32);
        let (ly, hy) = ((y0.min(y1) - r - 1.0).floor() as i32, (y0.max(y1) + r + 1.0).ceil() as i32);
        for y in ly..=hy {
            for x in lx..=hx {
                let (px, py) = (x as f32 + 0.5 - x0, y as f32 + 0.5 - y0);
                let t = ((px * dx + py * dy) / len2).clamp(0.0, 1.0);
                let (ex, ey) = (px - dx * t, py - dy * t);
                self.cover(x, y, c, 0.5 - ((ex * ex + ey * ey).sqrt() - r));
            }
        }
    }

    /// Anti-aliased filled polygon (even-odd rule), composited with alpha.
    pub fn poly_aa(&mut self, pts: &[(f32, f32)], c: u32) {
        if pts.len() < 3 {
            return;
        }
        let (mut lx, mut hx, mut ly, mut hy) = (f32::MAX, f32::MIN, f32::MAX, f32::MIN);
        for &(x, y) in pts {
            lx = lx.min(x);
            hx = hx.max(x);
            ly = ly.min(y);
            hy = hy.max(y);
        }
        let inside = |x: f32, y: f32| {
            let mut odd = false;
            let mut j = pts.len() - 1;
            for i in 0..pts.len() {
                let ((xi, yi), (xj, yj)) = (pts[i], pts[j]);
                if (yi > y) != (yj > y) && x < (xj - xi) * (y - yi) / (yj - yi) + xi {
                    odd = !odd;
                }
                j = i;
            }
            odd
        };
        for y in (ly.floor() as i32).max(0)..=(hy.ceil() as i32).min(self.h - 1) {
            for x in (lx.floor() as i32).max(0)..=(hx.ceil() as i32).min(self.w - 1) {
                let mut hits = 0;
                for (ox, oy) in [(0.25, 0.25), (0.75, 0.25), (0.25, 0.75), (0.75, 0.75)] {
                    hits += inside(x as f32 + ox, y as f32 + oy) as u32;
                }
                if hits > 0 {
                    self.tint(x, y, c, hits * 64);
                }
            }
        }
    }

    /// Soft radial glow: full `strength` (0.0..=1.0) at the centre, fading
    /// smoothly to nothing at radius `r`.
    pub fn glow(&mut self, cx: f32, cy: f32, r: f32, c: u32, strength: f32) {
        for y in (cy - r).floor() as i32..=(cy + r).ceil() as i32 {
            for x in (cx - r).floor() as i32..=(cx + r).ceil() as i32 {
                let d = ((x as f32 - cx).powi(2) + (y as f32 - cy).powi(2)).sqrt() / r;
                if d < 1.0 {
                    let f = 1.0 - d;
                    self.cover(x, y, c, f * f * strength);
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

    /// Hard-edged filled triangle.
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

    /// Smooth vertical gradient through `cols` (opaque).
    pub fn vgrad_smooth(&mut self, x: i32, y: i32, w: i32, h: i32, cols: &[u32]) {
        let n = (cols.len() - 1) as f32;
        for yy in 0..h {
            let f = yy as f32 / (h - 1).max(1) as f32 * n;
            let i = (f as usize).min(cols.len() - 1);
            let c = lerp(cols[i], cols[(i + 1).min(cols.len() - 1)], f - i as f32);
            self.hline(x, y + yy, w, c);
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

    /// Dark one pixel outline around every opaque region.
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

    /// Box blur of the given radius (run it twice for a softer falloff).
    /// Alpha-aware, so it is safe on sprites.
    pub fn box_blur(&mut self, r: i32) {
        if r < 1 {
            return;
        }
        let (w, h) = (self.w as usize, self.h as usize);
        // premultiplied float channels
        let mut buf: Vec<[f32; 4]> = self
            .px
            .iter()
            .map(|&p| {
                let a = (p >> 24) as f32 / 255.0;
                [((p >> 16) & 255) as f32 * a, ((p >> 8) & 255) as f32 * a, (p & 255) as f32 * a, a * 255.0]
            })
            .collect();
        let mut tmp = buf.clone();
        let pass = |src: &Vec<[f32; 4]>, dst: &mut Vec<[f32; 4]>, len: usize, lines: usize, stride: usize, step: usize| {
            for l in 0..lines {
                let at = |i: i32| src[l * stride + (i.clamp(0, len as i32 - 1) as usize) * step];
                let mut acc = [0.0f32; 4];
                for i in -r..=r {
                    let v = at(i);
                    for k in 0..4 {
                        acc[k] += v[k];
                    }
                }
                let n = (2 * r + 1) as f32;
                for i in 0..len as i32 {
                    dst[l * stride + i as usize * step] = [acc[0] / n, acc[1] / n, acc[2] / n, acc[3] / n];
                    let (add, sub) = (at(i + r + 1), at(i - r));
                    for k in 0..4 {
                        acc[k] += add[k] - sub[k];
                    }
                }
            }
        };
        pass(&buf, &mut tmp, w, h, w, 1);
        pass(&tmp, &mut buf, h, w, 1, w);
        for (p, v) in self.px.iter_mut().zip(buf.iter()) {
            let a = v[3].clamp(0.0, 255.0);
            let un = if a > 0.0 { 255.0 / a } else { 0.0 };
            let ch = |x: f32| ((x * un + 0.5) as u32).min(255);
            *p = ((a + 0.5) as u32) << 24 | ch(v[0]) << 16 | ch(v[1]) << 8 | ch(v[2]);
        }
    }

    /// Resample to a new size: area-averaged when shrinking, bilinear when
    /// enlarging. Alpha-aware.
    pub fn resized(&self, w: i32, h: i32) -> Canvas {
        let mut out = Canvas::new(w, h);
        let (sx, sy) = (self.w as f32 / w as f32, self.h as f32 / h as f32);
        let fetch = |x: i32, y: i32| {
            let p = self.px[(y.clamp(0, self.h - 1) * self.w + x.clamp(0, self.w - 1)) as usize];
            let a = (p >> 24) as f32;
            [((p >> 16) & 255) as f32 * a, ((p >> 8) & 255) as f32 * a, (p & 255) as f32 * a, a]
        };
        for y in 0..h {
            for x in 0..w {
                let mut acc = [0.0f32; 4];
                let mut wsum = 0.0;
                if sx > 1.0 || sy > 1.0 {
                    let (x0, x1) = ((x as f32 * sx) as i32, (((x + 1) as f32 * sx).ceil() as i32).max((x as f32 * sx) as i32 + 1));
                    let (y0, y1) = ((y as f32 * sy) as i32, (((y + 1) as f32 * sy).ceil() as i32).max((y as f32 * sy) as i32 + 1));
                    for yy in y0..y1 {
                        for xx in x0..x1 {
                            let v = fetch(xx, yy);
                            for k in 0..4 {
                                acc[k] += v[k];
                            }
                            wsum += 1.0;
                        }
                    }
                } else {
                    let (fx, fy) = ((x as f32 + 0.5) * sx - 0.5, (y as f32 + 0.5) * sy - 0.5);
                    let (x0, y0) = (fx.floor(), fy.floor());
                    let (tx, ty) = (fx - x0, fy - y0);
                    for (dx, dy, wt) in [(0, 0, (1.0 - tx) * (1.0 - ty)), (1, 0, tx * (1.0 - ty)), (0, 1, (1.0 - tx) * ty), (1, 1, tx * ty)] {
                        let v = fetch(x0 as i32 + dx, y0 as i32 + dy);
                        for k in 0..4 {
                            acc[k] += v[k] * wt;
                        }
                    }
                    wsum = 1.0;
                }
                let a = acc[3] / wsum;
                let un = if acc[3] > 0.0 { 1.0 / acc[3] } else { 0.0 };
                let ch = |v: f32| ((v * un + 0.5) as u32).min(255);
                out.px[(y * w + x) as usize] = ((a + 0.5) as u32).min(255) << 24 | ch(acc[0]) << 16 | ch(acc[1]) << 8 | ch(acc[2]);
            }
        }
        out
    }

    /// Composite a sprite at (x, y). `opacity` runs 0..=256; `dim` darkens
    /// the sprite (used for characters who are not speaking).
    pub fn blit(&mut self, src: &Canvas, x: i32, y: i32, dim: bool, opacity: u32) {
        let opacity = opacity.min(256);
        for sy in 0..src.h {
            let dy = y + sy;
            if dy < 0 || dy >= self.h {
                continue;
            }
            for sx in 0..src.w {
                let dx = x + sx;
                let mut p = src.px[(sy * src.w + sx) as usize];
                let a = (p >> 24) * opacity / 256;
                if a == 0 || dx < 0 || dx >= self.w {
                    continue;
                }
                if dim {
                    p = mix(p, rgb(16, 16, 48), 110);
                }
                let i = (dy * self.w + dx) as usize;
                self.px[i] = over(self.px[i], with_alpha(p, a));
            }
        }
    }

    /// Fade the whole canvas towards black; `amount` runs 0..=256.
    pub fn fade(&mut self, amount: u32) {
        if amount == 0 {
            return;
        }
        for p in self.px.iter_mut() {
            *p = mix(*p, BLACK, amount);
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

    /// Translucent window panel for the UI.
    pub fn panel(&mut self, x: i32, y: i32, w: i32, h: i32) {
        for yy in 0..h {
            let c = lerp(rgb(30, 42, 104), rgb(8, 10, 36), yy as f32 / h.max(1) as f32);
            self.rect_blend(x, y + yy, w, 1, with_alpha(c, 226));
        }
        for i in 0..2 {
            self.frame(x + i, y + i, w - 2 * i, h - 2 * i, rgb(232, 236, 255));
            self.frame(x + 2 + i, y + 2 + i, w - 4 - 2 * i, h - 4 - 2 * i, rgb(104, 122, 204));
        }
        self.frame(x - 1, y - 1, w + 2, h + 2, OUTLINE);
    }

    /// Load an 8 or 16 bit PNG of any colour type.
    pub fn load_png(path: &std::path::Path) -> Result<Canvas, String> {
        let file = std::fs::File::open(path).map_err(|e| format!("{}: {}", path.display(), e))?;
        let mut dec = png::Decoder::new(std::io::BufReader::new(file));
        dec.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
        let mut reader = dec.read_info().map_err(|e| format!("{}: {}", path.display(), e))?;
        // a picture's header can claim any size: refuse before allocating for it
        let (w, h) = (reader.info().width, reader.info().height);
        if w == 0 || h == 0 || w > MAX_PNG_SIDE || h > MAX_PNG_SIDE {
            return Err(format!("{}: {}x{} is too large a picture (the limit is {} a side)", path.display(), w, h, MAX_PNG_SIDE));
        }
        let mut buf = vec![0; reader.output_buffer_size()];
        let info = reader.next_frame(&mut buf).map_err(|e| format!("{}: {}", path.display(), e))?;
        let n = match info.color_type {
            png::ColorType::Grayscale => 1,
            png::ColorType::GrayscaleAlpha => 2,
            png::ColorType::Rgb => 3,
            png::ColorType::Rgba => 4,
            other => return Err(format!("{}: unsupported colour type {:?}", path.display(), other)),
        };
        let mut c = Canvas::new(info.width as i32, info.height as i32);
        for (p, s) in c.px.iter_mut().zip(buf.chunks_exact(n)) {
            *p = match n {
                1 => rgb(s[0], s[0], s[0]),
                2 => rgba(s[0], s[0], s[0], s[1]),
                3 => rgb(s[0], s[1], s[2]),
                _ => rgba(s[0], s[1], s[2], s[3]),
            };
        }
        Ok(c)
    }

    /// Encode as PNG; RGBA if `alpha`, otherwise RGB.
    pub fn png_bytes(&self, alpha: bool) -> Result<Vec<u8>, String> {
        let mut data = Vec::with_capacity(self.px.len() * 4);
        for &p in &self.px {
            data.extend_from_slice(&[(p >> 16) as u8, (p >> 8) as u8, p as u8]);
            if alpha {
                data.push((p >> 24) as u8);
            }
        }
        let mut out = Vec::new();
        let mut enc = png::Encoder::new(&mut out, self.w as u32, self.h as u32);
        enc.set_color(if alpha { png::ColorType::Rgba } else { png::ColorType::Rgb });
        enc.set_depth(png::BitDepth::Eight);
        enc.write_header().and_then(|mut w| w.write_image_data(&data)).map_err(|e| e.to_string())?;
        Ok(out)
    }

    /// Write a PNG file; RGBA if `alpha`, otherwise RGB.
    pub fn save_png(&self, path: &std::path::Path, alpha: bool) -> Result<(), String> {
        let bytes = self.png_bytes(alpha)?;
        std::fs::write(path, bytes).map_err(|e| format!("{}: {}", path.display(), e))
    }
}
