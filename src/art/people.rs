//! The six human portraits, all painted by one parameterised renderer so
//! they share a hand: a shaded head built from facial anatomy (sockets,
//! nose, lips, jaw), eyes with lids, iris and catchlight, hair as lit
//! volumes with strand texture, and tailored clothing.
//! Key light from the upper left, a cool rim light on the right.
//! Only brows, eyes and mouth change between expressions.

use super::paint::*;
use crate::gfx::*;
use std::collections::BTreeMap;
use std::f32::consts::PI;

const CX: f32 = 168.0;
const TOP: f32 = 86.0;
const CY: f32 = 160.0;
const EYE_Y: f32 = 168.0;
const EYE_DX: f32 = 24.5;
const BROW_Y: f32 = 151.0;
const MOUTH_Y: f32 = 217.0;
const NECK_Y: f32 = 286.0;

#[derive(Clone, Copy, PartialEq)]
enum Style {
    Heir,
    Lord,
    Rival,
    Steward,
    Footman,
    Cook,
}

struct Look {
    style: Style,
    skin: V3,
    hair: V3,
    brow: V3,
    iris: V3,
    coat: V3,
    /// Half-width of the face at the cheekbones.
    rx: f32,
    /// Jaw width as a fraction of `rx`.
    jaw: f32,
    chin: f32,
    age: f32,
    blush: f32,
    shoulders: f32,
    lashes: f32,
    lip_tint: f32,
}

fn look(who: &str) -> Look {
    match who {
        "p" => Look {
            style: Style::Heir,
            skin: [0.97, 0.80, 0.70],
            hair: [0.93, 0.74, 0.34],
            brow: [0.62, 0.44, 0.20],
            iris: [0.30, 0.52, 0.80],
            coat: [0.56, 0.44, 0.78],
            rx: 54.0,
            jaw: 0.76,
            chin: 238.0,
            age: 0.0,
            blush: 0.55,
            shoulders: 118.0,
            lashes: 0.3,
            lip_tint: 0.45,
        },
        "v" => Look {
            style: Style::Lord,
            skin: [0.90, 0.71, 0.61],
            hair: [0.74, 0.75, 0.79],
            brow: [0.52, 0.52, 0.56],
            iris: [0.42, 0.48, 0.56],
            coat: [0.58, 0.11, 0.17],
            rx: 58.0,
            jaw: 0.88,
            chin: 241.0,
            age: 0.85,
            blush: 0.25,
            shoulders: 126.0,
            lashes: 0.0,
            lip_tint: 0.30,
        },
        "s" => Look {
            style: Style::Rival,
            skin: [0.97, 0.89, 0.85],
            hair: [0.93, 0.92, 0.97],
            brow: [0.60, 0.58, 0.66],
            iris: [0.46, 0.62, 0.50],
            coat: [0.10, 0.50, 0.50],
            rx: 51.0,
            jaw: 0.70,
            chin: 245.0,
            age: 0.6,
            blush: 0.15,
            shoulders: 116.0,
            lashes: 0.2,
            lip_tint: 0.40,
        },
        "q" => Look {
            style: Style::Steward,
            skin: [0.52, 0.34, 0.25],
            hair: [0.09, 0.08, 0.11],
            brow: [0.07, 0.06, 0.08],
            iris: [0.22, 0.13, 0.08],
            coat: [0.27, 0.29, 0.36],
            rx: 52.0,
            jaw: 0.74,
            chin: 238.0,
            age: 0.3,
            blush: 0.1,
            shoulders: 106.0,
            lashes: 1.0,
            lip_tint: 0.55,
        },
        "t" => Look {
            style: Style::Footman,
            skin: [0.93, 0.74, 0.60],
            hair: [0.82, 0.36, 0.14],
            brow: [0.60, 0.28, 0.12],
            iris: [0.30, 0.56, 0.36],
            coat: [0.14, 0.54, 0.54],
            rx: 52.0,
            jaw: 0.78,
            chin: 235.0,
            age: 0.0,
            blush: 0.3,
            shoulders: 106.0,
            lashes: 0.2,
            lip_tint: 0.40,
        },
        _ => Look {
            style: Style::Cook,
            skin: [0.68, 0.47, 0.35],
            hair: [0.50, 0.48, 0.50],
            brow: [0.30, 0.27, 0.27],
            iris: [0.26, 0.15, 0.08],
            coat: [0.90, 0.90, 0.93],
            rx: 61.0,
            jaw: 0.93,
            chin: 239.0,
            age: 0.5,
            blush: 0.5,
            shoulders: 130.0,
            lashes: 0.7,
            lip_tint: 0.50,
        },
    }
}

struct Expr {
    brow_in: f32,
    brow_out: f32,
    /// Extra lift of the screen-right brow.
    cocked: f32,
    open: f32,
    /// Eyes squeezed shut in a happy arc.
    arcs: bool,
    lid: f32,
    curve: f32,
    smirk: bool,
    teeth: f32,
    gape: f32,
    thin: f32,
}

fn expr(e: &str) -> Expr {
    let base = Expr { brow_in: 0.0, brow_out: 0.0, cocked: 0.0, open: 1.0, arcs: false, lid: 0.0, curve: 0.12, smirk: false, teeth: 0.0, gape: 0.0, thin: 1.0 };
    match e {
        "smile" => Expr { brow_in: -3.0, brow_out: -2.0, arcs: true, curve: 1.0, teeth: 1.0, ..base },
        "worried" => Expr { brow_in: -7.0, brow_out: 3.5, open: 1.08, curve: -0.55, ..base },
        "angry" => Expr { brow_in: 7.0, brow_out: -3.0, open: 0.82, lid: 1.0, curve: -0.9, thin: 0.7, ..base },
        "smug" => Expr { brow_in: 1.0, cocked: -7.0, open: 0.58, lid: 0.4, curve: 0.5, smirk: true, ..base },
        "shock" => Expr { brow_in: -9.0, brow_out: -8.0, open: 1.5, gape: 1.0, curve: 0.0, ..base },
        _ => base,
    }
}

/// Half-width of the head outline at height `y` (0 outside it), grown by `g`.
fn head_hw(l: &Look, y: f32, g: f32) -> f32 {
    if y < CY {
        let t = (CY - y) / (CY - TOP + g);
        if t >= 1.0 {
            0.0
        } else {
            (l.rx + g) * (1.0 - t * t).sqrt()
        }
    } else {
        let t = (y - CY) / (l.chin - CY);
        if t >= 1.0 {
            0.0
        } else {
            l.rx * (1.0 - (1.0 - l.jaw) * t.powf(1.7)) * (1.0 - t.powf(5.0)).powf(0.45) + g
        }
    }
}

/// Soft form lighting: a wrapped Lambert term in 0..1.
fn wrap(n: V3) -> f32 {
    let len = dot(n, n).sqrt().max(0.001);
    ((dot(n, LIGHT) / len + 0.24) / 1.24).clamp(0.0, 1.0)
}

fn skin_tone(l: &Look, d: f32) -> V3 {
    let col = vscale(l.skin, 0.36 + 0.78 * d);
    // blood under the skin warms the edge of the shadow
    let term = (-((d - 0.36) / 0.16).powi(2)).exp();
    vadd(col, vscale([0.09, 0.012, 0.0], term * (0.4 + l.skin[0])))
}

fn bell(x: f32, y: f32, cx: f32, cy: f32, rx: f32, ry: f32) -> f32 {
    (-((x - cx) / rx).powi(2) - ((y - cy) / ry).powi(2)).exp()
}

fn seg_dist(px: f32, py: f32, a: (f32, f32), b: (f32, f32)) -> f32 {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let t = (((px - a.0) * dx + (py - a.1) * dy) / (dx * dx + dy * dy).max(1e-6)).clamp(0.0, 1.0);
    ((px - a.0 - dx * t).powi(2) + (py - a.1 - dy * t).powi(2)).sqrt()
}

fn head(c: &mut Canvas, l: &Look) {
    // ears first, tucked behind the cheeks
    for sx in [-1.0f32, 1.0] {
        let ex = CX + sx * (head_hw(l, 172.0, 0.0) + 2.0);
        let tone = skin_tone(l, if sx < 0.0 { 0.62 } else { 0.22 });
        c.ellipse_aa(ex, 174.0, 8.0, 17.0, pack(tone));
        c.ellipse_aa(ex + sx * 1.0, 174.0, 4.0, 10.0, pack(vscale(tone, 0.72)));
    }
    region(c, (CX - l.rx) as i32 - 2, TOP as i32 - 1, (CX + l.rx) as i32 + 3, l.chin as i32 + 1, |x, y| {
        let hw = head_hw(l, y, 0.0);
        let dx = x - CX;
        let a = (hw - dx.abs() + 0.5).min(y - TOP + 0.5).min((l.chin - y) * 0.8 + 0.5).min(1.0);
        if a <= 0.0 {
            return None;
        }
        let u = dx / hw.max(1.0);
        let v = if y < CY { (y - CY) / (CY - TOP) } else { (y - CY) / (l.chin - CY) };
        let nz = (1.0 - u * u * 0.76 - v * v * 0.30).max(0.03).sqrt();
        let mut d = wrap([u * 0.88, v * 0.52, nz]);
        // --- anatomy as light and shade ---
        let sock = bell(x, y, CX - EYE_DX, EYE_Y - 4.0, 17.0, 11.0) + bell(x, y, CX + EYE_DX, EYE_Y - 4.0, 17.0, 11.0);
        d -= 0.13 * sock * (1.0 + 0.6 * l.age);
        // brow ridge and forehead catch the light
        d += 0.07 * bell(x, y, CX - 14.0, 122.0, 34.0, 20.0);
        d += 0.05 * bell(x, y, CX - EYE_DX, BROW_Y - 7.0, 18.0, 5.0);
        // nose: lit bridge, shaded far side, rounded tip, shadow beneath
        let nose_span = smoothstep(156.0, 172.0, y) * smoothstep(208.0, 198.0, y);
        d += 0.10 * (-((dx + 2.0) / 3.6).powi(2)).exp() * nose_span;
        d -= 0.24 * (-((dx - 8.0) / 5.0).powi(2)).exp() * nose_span * smoothstep(160.0, 190.0, y);
        // the wings of the nostrils
        d += 0.07 * bell(x, y, CX - 9.5, 199.0, 4.0, 4.0);
        d -= 0.12 * bell(x, y, CX + 11.0, 199.5, 3.0, 4.5);
        d -= 0.10 * bell(x, y, CX - 13.5, 201.0, 2.0, 4.0);
        d += 0.17 * bell(x, y, CX - 2.5, 195.0, 5.0, 5.0);
        d -= 0.10 * bell(x, y, CX - 10.5, 198.0, 3.5, 5.0);
        d -= 0.24 * bell(x, y, CX + 1.0, 205.5, 11.0, 3.2);
        d -= 0.30 * (bell(x, y, CX - 5.5, 202.5, 2.6, 1.8) + bell(x, y, CX + 6.0, 202.5, 2.6, 1.8));
        // cheekbone, the dip under the lower lip, the ball of the chin
        d += 0.06 * bell(x, y, CX - 31.0, 184.0, 15.0, 10.0);
        d -= 0.13 * bell(x, y, CX, MOUTH_Y + 11.0, 13.0, 3.5);
        d += 0.06 * bell(x, y, CX - 3.0, l.chin - 11.0, 12.0, 7.0);
        // the years
        if l.age > 0.0 {
            for (i, ly) in [112.0f32, 121.0, 130.0].into_iter().enumerate() {
                let wob = (x * 0.07 + i as f32).sin() * 1.5;
                d -= 0.07 * l.age * (-((y - ly - wob) / 1.2).powi(2)).exp() * smoothstep(40.0, 20.0, dx.abs());
            }
            for sx in [-1.0f32, 1.0] {
                let fold = seg_dist(x, y, (CX + sx * 12.0, 199.0), (CX + sx * 24.0, 224.0));
                d -= 0.16 * l.age * (-(fold / 2.0).powi(2)).exp();
                d -= 0.10 * l.age * bell(x, y, CX + sx * EYE_DX, EYE_Y + 11.0, 12.0, 2.6);
                d -= 0.06 * l.age * bell(x, y, CX + sx * 30.0, 222.0, 9.0, 12.0);
            }
        }
        let mut col = skin_tone(l, d.clamp(0.0, 1.0));
        let blush = (bell(x, y, CX - 31.0, 197.0, 17.0, 11.0) + bell(x, y, CX + 31.0, 197.0, 17.0, 11.0)) * l.blush;
        col = vmix(col, vmul(col, [1.12, 0.74, 0.74]), blush * 0.7);
        col = vmix(col, vmul(col, [1.05, 0.86, 0.84]), 0.5 * bell(x, y, CX, 197.0, 9.0, 7.0));
        if l.style == Style::Footman {
            // freckles across the nose and cheeks
            let f = hash((x / 3.0) as i32, (y / 3.0) as i32, 71);
            let band = bell(x, y, CX, 190.0, 44.0, 12.0);
            if f % 5 == 0 && band > 0.35 && (x as i32 % 3 == 1) && (y as i32 % 3 == 1) {
                col = vmix(col, [0.66, 0.36, 0.20], 0.85 * band);
            }
        }
        if l.style == Style::Rival {
            col = vmix(col, [0.16, 0.10, 0.10], (bell(x, y, CX - 27.0, 211.0, 1.9, 1.9) * 2.2).min(1.0));
        }
        Some((col, a))
    });
}

fn neck(c: &mut Canvas, l: &Look) {
    let w = if l.style == Style::Cook || l.style == Style::Lord { 27.0 } else { 22.0 };
    region(c, (CX - w) as i32 - 2, 214, (CX + w) as i32 + 3, 312, |x, y| {
        let hw = w + (y - 250.0).max(0.0) * 0.12;
        let u = (x - CX) / hw;
        if u.abs() > 1.0 {
            return None;
        }
        let mut d = wrap([u * 0.9, 0.1, (1.0 - u * u * 0.8).sqrt()]);
        // the jaw throws the throat into shadow
        d *= 0.45 + 0.55 * smoothstep(l.chin - 4.0, l.chin + 34.0, y);
        let a = ((1.0 - u.abs()) * hw + 0.5).min(1.0) * smoothstep(NECK_Y + 10.0, NECK_Y + 2.0, y);
        Some((skin_tone(l, d), a))
    });
}

fn eye(c: &mut Canvas, l: &Look, e: &Expr, sx: f32) {
    let (ex, ey) = (CX + sx * EYE_DX, EYE_Y);
    let lash = pack(vscale(l.brow, 0.35));
    if e.arcs {
        // squeezed shut with delight
        let mut prev = (ex - 13.0, ey + 2.5);
        for i in 1..=10 {
            let t = i as f32 / 10.0;
            let p = (ex - 13.0 + 26.0 * t, ey + 2.5 - 6.5 * (t * PI).sin());
            c.capsule(prev.0, prev.1, p.0, p.1, 1.5 + 0.4 * l.lashes, lash);
            prev = p;
        }
        c.capsule(ex - 11.0, ey - 6.0, ex + 11.0, ey - 6.0, 1.0, packa(vscale(l.skin, 0.55), 0.35));
        return;
    }
    let (half, up, lo) = (13.0, 6.4 * e.open, 4.4 * e.open.min(1.25));
    // angry or heavy lids press the upper lid down, most of all at the inner corner
    let top_at = |t: f32| -> f32 {
        let shape = (1.0 - t * t).max(0.0).powf(0.72);
        let press = e.lid * (0.25 + 0.30 * (-t * sx).max(0.0));
        ey + sx * t * -1.0 - up * shape * (1.0 - press)
    };
    let bot_at = |t: f32| ey + sx * t * -1.0 + lo * (1.0 - t * t).max(0.0).powf(0.8);
    let (ix, iy, ir) = (ex, ey + 0.2 - 0.6 * (e.open - 1.0), 6.7);
    region(c, (ex - half) as i32 - 1, (ey - up) as i32 - 3, (ex + half) as i32 + 2, (ey + lo) as i32 + 3, |x, y| {
        let t = (x - ex) / half;
        if t.abs() >= 1.0 {
            return None;
        }
        let (top, bot) = (top_at(t), bot_at(t));
        let a = (y - top + 0.5).min(bot - y + 0.5).min((1.0 - t.abs()) * half + 0.3).min(1.0);
        if a <= 0.0 {
            return None;
        }
        // the lid shades the top of the eyeball
        let shade = 0.66 + 0.34 * smoothstep(top, top + 5.0, y);
        let mut col = vscale([0.94, 0.92, 0.89], shade * (1.0 - 0.22 * t.abs().powi(3)));
        let d = ((x - ix).powi(2) + (y - iy).powi(2)).sqrt();
        if d < ir + 0.6 {
            let r = d / ir;
            let fibre = 0.85 + 0.3 * hashf(((y - iy).atan2(x - ix) * 9.0) as i32, 0, 3);
            let mut iris = vscale(vmix(vscale(l.iris, 1.35), vscale(l.iris, 0.6), r * r), fibre * shade);
            // dark limbal ring, and light pooling opposite the catchlight
            iris = vscale(iris, 1.0 - 0.55 * smoothstep(0.78, 1.0, r));
            iris = vadd(iris, vscale(l.iris, 0.5 * bell(x, y, ix + 2.2, iy + 2.6, 2.8, 2.2)));
            if d < 2.9 {
                iris = vmix([0.03, 0.02, 0.03], iris, smoothstep(2.3, 2.9, d));
            }
            col = vmix(col, iris, (ir + 0.6 - d).min(1.0));
        }
        Some((col, a))
    });
    if up > 3.0 {
        c.ellipse_aa(ix - 2.3, iy - 2.5, 1.7, 1.7, rgba(255, 255, 255, 235));
    }
    // lids: a firm upper lash line, a soft lower one, and the crease above
    let mut prev = (ex - half, top_at(-1.0));
    for i in 1..=12 {
        let t = -1.0 + i as f32 / 6.0;
        let p = (ex + half * t, top_at(t));
        c.capsule(prev.0, prev.1, p.0, p.1, 1.15 + 0.55 * l.lashes, lash);
        c.capsule(prev.0, prev.1 - 4.0 - up * 0.25, p.0, p.1 - 4.0 - up * 0.25, 0.8, packa(vscale(l.skin, 0.5), 0.28));
        let (q0, q1) = ((prev.0, bot_at(t - 1.0 / 6.0)), (p.0, bot_at(t)));
        c.capsule(q0.0, q0.1 + 0.3, q1.0, q1.1 + 0.3, 0.6, packa(vscale(l.skin, 0.5), 0.45));
        prev = p;
    }
    if l.lashes > 0.5 {
        let outer = (ex + sx * half, top_at(sx));
        c.capsule(outer.0, outer.1, outer.0 + sx * 3.5, outer.1 - 2.5, 0.9, lash);
    }
}

fn brow(c: &mut Canvas, l: &Look, e: &Expr, sx: f32) {
    let ex = CX + sx * EYE_DX;
    let lift = if sx > 0.0 { e.cocked } else { 0.0 };
    let thick = if l.style == Style::Lord { 3.6 } else if l.lashes > 0.5 { 2.0 } else { 2.7 };
    let inner = (ex - sx * 13.0, BROW_Y + 1.0 + e.brow_in + lift * 0.6);
    let mid = (ex + sx * 3.0, BROW_Y - 3.5 + (e.brow_in + e.brow_out) * 0.5 + lift);
    let outer = (ex + sx * 17.0, BROW_Y + 1.5 + e.brow_out + lift * 0.7);
    let col = pack(l.brow);
    c.capsule(inner.0, inner.1, mid.0, mid.1, thick, col);
    c.capsule(mid.0, mid.1, outer.0, outer.1, thick * 0.62, col);
    c.capsule(inner.0, inner.1 - 1.0, mid.0, mid.1 - 1.2, thick * 0.4, packa(vadd(l.brow, [0.2, 0.2, 0.2]), 0.4));
}

fn mouth(c: &mut Canvas, l: &Look, e: &Expr) {
    let lip = vmix(vscale(l.skin, 0.9), [0.74, 0.26, 0.28], l.lip_tint);
    let dark = pack(vscale(lip, 0.32));
    if e.gape > 0.5 {
        // an O of astonishment
        orb(c, CX, MOUTH_Y + 4.0, 12.0, 14.5, lip, 0.25, 12.0);
        c.ellipse_aa(CX, MOUTH_Y + 4.0, 8.0, 10.5, rgb(40, 12, 16));
        c.ellipse_aa(CX, MOUTH_Y + 9.5, 5.5, 4.0, rgb(150, 60, 64));
        c.ellipse_aa(CX, MOUTH_Y - 4.0, 5.5, 2.0, rgb(238, 232, 222));
        return;
    }
    let half = 17.0 + 4.5 * e.curve.max(0.0);
    let (left, right) = if e.smirk { (MOUTH_Y + 1.0, MOUTH_Y - 6.0) } else { (MOUTH_Y - 5.0 * e.curve, MOUTH_Y - 5.0 * e.curve) };
    let sag = if e.smirk { 1.0 } else { 3.2 * e.curve };
    let line = |t: f32| left + (right - left) * (t + 1.0) * 0.5 + sag * (1.0 - t * t);
    let gap = |t: f32| 4.2 * e.teeth * (1.0 - t * t).max(0.0).sqrt();
    region(c, (CX - half) as i32 - 2, (MOUTH_Y - 14.0) as i32, (CX + half) as i32 + 3, (MOUTH_Y + 20.0) as i32, |x, y| {
        let t = (x - CX) / half;
        if t.abs() >= 1.0 {
            return None;
        }
        let m = line(t);
        let fall = (1.0 - t.abs().powf(1.6)).max(0.0);
        let up = 4.6 * e.thin * fall * (1.0 - 0.28 * (-(t / 0.16).powi(2)).exp());
        let low = 6.6 * e.thin * (1.0 - t * t).powf(0.8);
        let g = gap(t);
        let edge = ((1.0 - t.abs()) * half).min(1.0);
        if y < m - up - 0.5 || y > m + g + low + 0.5 {
            return None;
        }
        if y < m {
            // upper lip, turned away from the light
            let a = (y - (m - up) + 0.5).min(1.0);
            return Some((vscale(lip, 0.70 + 0.12 * ((y - (m - up)) / up.max(0.5))), a * edge));
        }
        if y < m + g {
            let tooth = 0.88 + 0.10 * ((x - CX) / 5.5 * PI).cos().abs().powf(0.3) - 0.25 * t.abs();
            return Some((vscale([0.97, 0.95, 0.90], tooth), edge));
        }
        // lower lip, full, with a highlight
        let v = (y - m - g) / low.max(0.5);
        let a = ((m + g + low) - y + 0.5).min(1.0);
        let shine = 0.22 * bell(x, y, CX - 3.0, m + g + low * 0.38, 6.0, 1.8);
        Some((vadd(vscale(lip, 1.02 - 0.26 * v), [shine, shine, shine]), a * edge))
    });
    let mut prev = (CX - half, line(-1.0));
    for i in 1..=12 {
        let t = -1.0 + i as f32 / 6.0;
        let p = (CX + half * t, line(t));
        c.capsule(prev.0, prev.1, p.0, p.1, 0.95, dark);
        prev = p;
    }
    // corners tucked into the cheeks
    c.ellipse_aa(CX - half, line(-1.0), 1.8, 1.8, with_alpha(dark, 200));
    c.ellipse_aa(CX + half, line(1.0), 1.8, 1.8, with_alpha(dark, 200));
}

/// How far the shoulder line falls from the base of the neck to its point.
const DROP: f32 = 36.0;

/// Half-width of the torso at height `y`.
fn body_hw(sw: f32, y: f32) -> f32 {
    if y < NECK_Y {
        0.0
    } else if y < NECK_Y + DROP {
        let t = (y - NECK_Y) / DROP;
        34.0 + (sw - 34.0) * (1.0 - (1.0 - t) * (1.0 - t)).powf(0.42)
    } else {
        sw + (y - NECK_Y - DROP) * 0.04
    }
}

fn body(c: &mut Canvas, l: &Look) {
    let sw = l.shoulders;
    region(c, 0, NECK_Y as i32 - 1, SPR_W_I, SPR_H_I, |x, y| {
        let hw = body_hw(sw, y);
        let dx = x - CX;
        // anti-alias the rounded shoulder against its top edge as well as its side
        let k = ((dx.abs() - 34.0) / (sw - 34.0)).clamp(0.0, 1.0);
        let top = NECK_Y + DROP * (1.0 - (1.0 - k.powf(1.0 / 0.42)).max(0.0).sqrt());
        let a = (hw - dx.abs() + 0.5).min(y - top + 0.5).min(1.0);
        if a <= 0.0 {
            return None;
        }
        let u = dx / hw.max(1.0);
        let slope = smoothstep(NECK_Y + 70.0, NECK_Y, y);
        let n = [u * 0.92, -0.25 - 0.5 * slope, (1.0 - u * u * 0.8).max(0.05).sqrt()];
        let mut d = wrap(n);
        // the arm hangs in front of the flank: a soft crease where they meet
        let crease = (-((dx.abs() - sw * 0.70) / 3.0).powi(2)).exp() * smoothstep(NECK_Y + 40.0, NECK_Y + 80.0, y);
        d -= 0.20 * crease;
        d += 0.06 * (-((dx.abs() - sw * 0.70 - 7.0) / 5.0).powi(2)).exp() * smoothstep(NECK_Y + 40.0, NECK_Y + 80.0, y);
        // drapery: long soft folds falling from the shoulders
        let fold = fbm(dx * 0.035 + (y * 0.004), y * 0.011, 3, 201) - 0.5;
        let weave = fbm(x * 0.7, y * 0.7, 2, 203) - 0.5;
        let mut base = l.coat;
        match l.style {
            Style::Rival => {
                // brocade: a woven figure that shows as a change of sheen
                let (p, q) = (x / 15.0, y / 19.0);
                let fig = ((p + q) * PI).sin() * ((p - q) * PI).sin();
                base = vscale(base, 0.94 + 0.16 * smoothstep(0.2, 0.6, fig.abs()));
            }
            Style::Lord => {
                // sash from the right shoulder to the left hip
                let s = (x - (CX - sw + 34.0)) - (y - 306.0) * 0.86;
                if (0.0..36.0).contains(&s) && y > 292.0 {
                    base = if !(4.0..32.0).contains(&s) { [0.86, 0.68, 0.28] } else { [0.16, 0.26, 0.62] };
                }
            }
            Style::Steward | Style::Footman => {
                let pipe = if l.style == Style::Steward { [0.12, 0.72, 0.66] } else { [0.82, 0.86, 0.90] };
                if (dx - 3.0).abs() < 1.8 && y > 300.0 {
                    base = pipe;
                }
            }
            _ => {}
        }
        let mut col = vscale(base, (0.36 + 0.74 * d) * (1.0 + 0.30 * fold + 0.08 * weave));
        if l.style == Style::Heir {
            // velvet lights up at grazing angles
            col = vadd(col, vscale(l.coat, 0.35 * (1.0 - n[2]).powi(2)));
        }
        // the head's shadow across collar and chest
        let under = bell(x, y, CX + 6.0, NECK_Y + 2.0, 60.0, 30.0);
        col = vscale(col, 1.0 - 0.38 * under);
        Some((col, a))
    });
}

const SPR_W_I: i32 = 336;
const SPR_H_I: i32 = 456;

/// Open coat over a lace jabot (the heir and Lord Solberg).
fn open_coat(c: &mut Canvas, l: &Look) {
    let trim = if l.style == Style::Heir { [0.90, 0.72, 0.30] } else { [0.84, 0.87, 0.92] };
    let depth = 420.0;
    // shirt front
    region(c, (CX - 40.0) as i32, NECK_Y as i32 - 4, (CX + 40.0) as i32, depth as i32, |x, y| {
        let half = 34.0 * (depth - y) / (depth - NECK_Y);
        let a = (half - (x - CX).abs() + 0.5).min(1.0);
        if a <= 0.0 || y < NECK_Y - 2.0 {
            return None;
        }
        let d = 0.62 + 0.3 * (1.0 - (x - CX + 8.0).abs() / 40.0) - 0.3 * bell(x, y, CX + 6.0, NECK_Y, 40.0, 22.0);
        Some((vscale([0.96, 0.96, 0.98], d), a))
    });
    // lapels, with a rolled edge and metallic piping
    for sx in [-1.0f32, 1.0] {
        let lit_k = if sx < 0.0 { 1.12 } else { 0.78 };
        let pts = [
            (CX + sx * 33.0, NECK_Y - 6.0),
            (CX + sx * 2.0, depth),
            (CX + sx * 20.0, depth + 20.0),
            (CX + sx * 62.0, NECK_Y + 62.0),
            (CX + sx * 50.0, NECK_Y + 6.0),
        ];
        c.poly_aa(&pts, pack(vscale(l.coat, 0.78 * lit_k)));
        c.capsule(pts[0].0, pts[0].1, pts[1].0, pts[1].1, 2.2, pack(vscale(trim, lit_k)));
        c.capsule(pts[0].0 - sx * 0.8, pts[0].1, pts[1].0 - sx * 0.8, pts[1].1, 0.8, pack(vadd(vscale(trim, lit_k), [0.12, 0.12, 0.12])));
        c.capsule(pts[4].0, pts[4].1, pts[3].0, pts[3].1, 1.2, pack(vscale(l.coat, 0.5)));
        // high collar standing behind the neck
        c.poly_aa(&[(CX + sx * 24.0, NECK_Y - 28.0), (CX + sx * 44.0, NECK_Y - 20.0), (CX + sx * 52.0, NECK_Y + 8.0), (CX + sx * 30.0, NECK_Y - 2.0)], pack(vscale(l.coat, 0.9 * lit_k)));
    }
    // jabot: tiers of gathered lace
    for tier in 0..5 {
        let (y, w) = (NECK_Y + 4.0 + tier as f32 * 21.0, 27.0 - tier as f32 * 3.4);
        soft_ellipse(c, CX + 2.0, y + 12.0, w + 6.0, 10.0, rgba(40, 40, 70, 90), 1.0);
        let n = 5 - tier / 2;
        for i in 0..n {
            let x = CX - w + 2.0 * w * (i as f32 + 0.5) / n as f32;
            orb(c, x, y + ((i + tier) % 2) as f32 * 2.0, w / n as f32 + 2.5, 10.0, [0.97, 0.97, 1.0], 0.2, 10.0);
        }
    }
    let pin = if l.style == Style::Heir { [0.80, 0.08, 0.14] } else { [0.30, 0.80, 0.76] };
    orb(c, CX, NECK_Y + 2.0, 5.5, 5.5, pin, 1.4, 30.0);
    c.ring(CX as i32, NECK_Y as i32 + 2, 7, pack(trim));
    for (i, y) in [depth + 6.0, depth + 26.0].into_iter().enumerate() {
        if y < 452.0 {
            orb(c, CX + 2.0, y + i as f32, 4.5, 4.5, trim, 1.0, 20.0);
        }
    }
}

/// Lord Varrow's military dress coat: stand collar, epaulettes, orders.
fn dress_coat(c: &mut Canvas, l: &Look) {
    let gold = [0.88, 0.69, 0.28];
    let sw = l.shoulders;
    // white stock at the throat, then the collar closing over it
    c.poly_aa(&[(CX - 20.0, NECK_Y - 16.0), (CX + 20.0, NECK_Y - 16.0), (CX + 12.0, NECK_Y + 8.0), (CX - 12.0, NECK_Y + 8.0)], rgb(232, 232, 238));
    for sx in [-1.0f32, 1.0] {
        let k = if sx < 0.0 { 1.1 } else { 0.78 };
        let pts = [(CX + sx * 2.0, NECK_Y + 12.0), (CX + sx * 8.0, NECK_Y - 14.0), (CX + sx * 36.0, NECK_Y - 26.0), (CX + sx * 44.0, NECK_Y + 6.0)];
        c.poly_aa(&pts, pack(vscale(l.coat, 0.95 * k)));
        c.capsule(pts[1].0, pts[1].1, pts[2].0, pts[2].1, 1.8, pack(vscale(gold, k)));
        c.capsule(pts[0].0, pts[0].1, pts[1].0, pts[1].1, 1.8, pack(vscale(gold, k)));
        // epaulette: a padded gold crescent with bullion fringe
        let ex = CX + sx * (sw - 30.0);
        for i in 0..9 {
            let fx = ex - 24.0 + i as f32 * 6.0;
            tube(c, fx, 306.0, fx + sx * 1.5, 326.0 - (i as f32 - 4.0).abs() * 1.2, 2.3, vscale(gold, 0.9 * k), 0.9);
        }
        orb(c, ex, 300.0, 32.0, 11.0, vscale(gold, k), 0.9, 14.0);
        orb(c, ex - sx * 2.0, 297.0, 20.0, 5.5, vscale([0.96, 0.80, 0.40], k), 0.6, 14.0);
    }
    // front closure and buttons
    c.capsule(CX + 3.0, NECK_Y + 12.0, CX + 5.0, 456.0, 1.0, pack(vscale(l.coat, 0.45)));
    for i in 0..4 {
        orb(c, CX + 14.0, NECK_Y + 36.0 + i as f32 * 36.0, 5.0, 5.0, gold, 1.2, 24.0);
    }
    // the star of some order or other
    let (mx, my) = (CX - 58.0, 350.0);
    for i in 0..8 {
        let a = i as f32 * PI / 4.0;
        c.capsule(mx, my, mx + a.cos() * 13.0, my + a.sin() * 13.0, 1.8, pack(vscale(gold, 1.05)));
    }
    orb(c, mx, my, 6.5, 6.5, [0.88, 0.90, 0.96], 1.2, 24.0);
}

/// Closed uniform jacket with piping (Quince and Tobe).
fn livery(c: &mut Canvas, l: &Look) {
    let steward = l.style == Style::Steward;
    let pipe = if steward { [0.12, 0.72, 0.66] } else { [0.82, 0.86, 0.90] };
    c.poly_aa(&[(CX - 18.0, NECK_Y - 20.0), (CX + 18.0, NECK_Y - 20.0), (CX + 10.0, NECK_Y + 4.0), (CX - 10.0, NECK_Y + 4.0)], rgb(236, 238, 242));
    for sx in [-1.0f32, 1.0] {
        let k = if sx < 0.0 { 1.12 } else { 0.8 };
        let pts = [(CX + sx * 2.0, NECK_Y + 10.0), (CX + sx * 9.0, NECK_Y - 12.0), (CX + sx * 30.0, NECK_Y - 22.0), (CX + sx * 40.0, NECK_Y + 6.0)];
        c.poly_aa(&pts, pack(vscale(l.coat, 1.05 * k)));
        c.capsule(pts[1].0, pts[1].1, pts[2].0, pts[2].1, 1.5, pack(vscale(pipe, k)));
        c.capsule(pts[0].0, pts[0].1, pts[1].0, pts[1].1, 1.5, pack(vscale(pipe, k)));
        // shoulder seam
        c.capsule(CX + sx * 46.0, NECK_Y + 8.0, CX + sx * (l.shoulders - 14.0), NECK_Y + 34.0, 0.8, packa(vscale(l.coat, 0.5), 0.6));
    }
    if steward {
        // one discreet badge of office
        orb(c, CX - 46.0, 340.0, 6.0, 6.0, [0.80, 0.84, 0.90], 1.3, 26.0);
        c.ellipse_aa(CX - 46.0, 340.0, 2.4, 2.4, pack(pipe));
        for i in 0..4 {
            orb(c, CX + 12.0, NECK_Y + 36.0 + i as f32 * 36.0, 3.6, 3.6, [0.30, 0.32, 0.40], 1.0, 20.0);
        }
    } else {
        for i in 0..4 {
            for sx in [-1.0f32, 1.0] {
                orb(c, CX + 3.0 + sx * 20.0, NECK_Y + 40.0 + i as f32 * 36.0, 4.6, 4.6, [0.84, 0.87, 0.92], 1.3, 24.0);
            }
        }
    }
}

/// Cook's double-breasted jacket and neckerchief.
fn whites(c: &mut Canvas, l: &Look) {
    // the overlapping front panel
    c.capsule(CX + 34.0, NECK_Y + 30.0, CX + 40.0, 456.0, 1.0, rgba(120, 124, 150, 150));
    c.capsule(CX - 30.0, NECK_Y + 6.0, CX + 34.0, NECK_Y + 30.0, 1.0, rgba(120, 124, 150, 150));
    for i in 0..4 {
        for sx in [-1.0f32, 1.0] {
            orb(c, CX + 4.0 + sx * 24.0, NECK_Y + 56.0 + i as f32 * 34.0, 5.0, 5.0, [0.86, 0.86, 0.90], 0.8, 16.0);
        }
    }
    for sx in [-1.0f32, 1.0] {
        let k = if sx < 0.0 { 1.0 } else { 0.84 };
        c.poly_aa(&[(CX + sx * 6.0, NECK_Y + 10.0), (CX + sx * 22.0, NECK_Y - 18.0), (CX + sx * 44.0, NECK_Y - 8.0), (CX + sx * 40.0, NECK_Y + 12.0)], pack(vscale(l.coat, k)));
    }
    // red neckerchief, knotted
    let red = [0.76, 0.14, 0.14];
    c.poly_aa(&[(CX - 30.0, NECK_Y - 16.0), (CX + 30.0, NECK_Y - 16.0), (CX + 20.0, NECK_Y + 6.0), (CX - 20.0, NECK_Y + 6.0)], pack(vscale(red, 0.8)));
    c.poly_aa(&[(CX - 4.0, NECK_Y + 6.0), (CX - 22.0, NECK_Y + 40.0), (CX - 4.0, NECK_Y + 34.0)], pack(vscale(red, 0.95)));
    c.poly_aa(&[(CX + 4.0, NECK_Y + 6.0), (CX + 16.0, NECK_Y + 44.0), (CX + 2.0, NECK_Y + 32.0)], pack(vscale(red, 0.7)));
    orb(c, CX, NECK_Y + 6.0, 9.0, 7.5, red, 0.3, 10.0);
}

/// Paint a mass of hair: `inside` gives coverage, `strand` maps a pixel to
/// (across, along) strand coordinates, and the volume is lit as an ellipsoid.
fn hair_mass(
    c: &mut Canvas,
    l: &Look,
    bbox: (i32, i32, i32, i32),
    vol: (f32, f32, f32, f32),
    gloss: f32,
    inside: impl Fn(f32, f32) -> f32,
    strand: impl Fn(f32, f32) -> (f32, f32),
) {
    region(c, bbox.0, bbox.1, bbox.2, bbox.3, |x, y| {
        let a = inside(x, y).min(1.0);
        if a <= 0.0 {
            return None;
        }
        let (nx, ny) = (((x - vol.0) / vol.2).clamp(-1.0, 1.0), ((y - vol.1) / vol.3).clamp(-1.0, 1.0));
        let nz = (1.0 - nx * nx * 0.8 - ny * ny * 0.6).max(0.04).sqrt();
        let d = wrap([nx * 0.9, ny * 0.8, nz]);
        let (sa, sb) = strand(x, y);
        let lock = fbm(sa * 0.11, sb * 0.012, 3, 301);
        let fine = fbm(sa * 0.9, sb * 0.03, 2, 303);
        let mut col = vscale(l.hair, (0.34 + 0.80 * d) * (0.62 + 0.55 * lock + 0.22 * fine));
        // a band of sheen where the curve of the head turns to the light
        let band = (-((ny + 0.42) / 0.20).powi(2)).exp() * (0.4 + 0.6 * nz);
        col = vadd(col, vscale(vadd(l.hair, [0.25, 0.25, 0.28]), gloss * band * (0.3 + 0.7 * fine)));
        Some((col, a))
    });
}

/// Coverage of the hair cap: the skull grown by `g`, above `hairline(x)`.
fn cap(l: &Look, x: f32, y: f32, g: f32, hairline: f32) -> f32 {
    let hw = head_hw(l, y, g);
    (hw - (x - CX).abs() + 0.5).min(y - (TOP - g) + 0.5).min(hairline - y + 0.5)
}

fn ell(x: f32, y: f32, cx: f32, cy: f32, rx: f32, ry: f32) -> f32 {
    (1.0 - (((x - cx) / rx).powi(2) + ((y - cy) / ry).powi(2)).sqrt()) * rx.min(ry) + 0.5
}

/// Hair that sits behind the head and shoulders.
fn hair_back(c: &mut Canvas, l: &Look) {
    match l.style {
        Style::Steward => {
            hair_mass(c, l, (120, 30, 216, 110), (CX - 4.0, 72.0, 34.0, 30.0), 0.9, |x, y| ell(x, y, CX, 72.0, 31.0, 28.0), |x, y| {
                let (dx, dy) = (x - CX, y - 72.0);
                (dy.atan2(dx) * 40.0, (dx * dx + dy * dy).sqrt())
            });
        }
        Style::Rival => {
            // the great powdered wig: a tower above, a fall of curls behind
            hair_mass(c, l, (60, 6, 276, 290), (CX - 6.0, 110.0, 104.0, 110.0), 0.25, |x, y| {
                ell(x, y, CX, 120.0, 90.0, 84.0).max(ell(x, y, CX, 64.0, 56.0, 52.0)).max(ell(x, y, CX, 214.0, 84.0, 66.0))
            }, |x, y| (x + (y * 0.16).sin() * 9.0, y * 6.0));
        }
        _ => {}
    }
}

/// Hair that falls in front of the head.
fn hair_front(c: &mut Canvas, l: &Look) {
    let radial = |x: f32, y: f32| {
        let (dx, dy) = (x - CX, y - 210.0);
        (dy.atan2(dx) * 150.0, (dx * dx + dy * dy).sqrt())
    };
    match l.style {
        Style::Heir => {
            // floppy, side-swept, catching the light
            let sweep = |x: f32, y: f32| (x * 0.50 + y * 0.87, x * 0.87 - y * 0.50);
            hair_mass(c, l, (90, 50, 250, 196), (CX - 6.0, 122.0, 74.0, 64.0), 0.7, |x, y| {
                let line = 147.0 - (x - 112.0) * 0.27 + 5.0 * (x * 0.13).sin();
                cap(l, x, y, 11.0, line)
                    .max(ell(x, y, 142.0, 98.0, 40.0, 24.0))
                    .max(ell(x, y, 192.0, 90.0, 44.0, 25.0))
                    .max(ell(x, y, 226.0, 114.0, 22.0, 28.0))
                    .max(ell(x, y, 108.0, 148.0, 11.0, 34.0))
                    .max(ell(x, y, 230.0, 150.0, 9.0, 26.0))
            }, sweep);
        }
        Style::Lord => {
            hair_mass(c, l, (100, 76, 236, 200), (CX - 6.0, 130.0, 66.0, 60.0), 0.8, |x, y| {
                let dx = (x - CX).abs();
                let line = if dx > head_hw(l, y, 4.0) - 9.0 { 196.0 } else { 110.0 + dx * dx / 120.0 };
                cap(l, x, y, 4.0, line)
            }, radial);
        }
        Style::Steward => {
            hair_mass(c, l, (106, 76, 230, 190), (CX - 6.0, 128.0, 62.0, 58.0), 1.3, |x, y| {
                let dx = (x - CX).abs();
                let line = if dx > head_hw(l, y, 3.0) - 7.0 { 180.0 } else { 113.0 + dx * dx / 110.0 };
                cap(l, x, y, 3.0, line)
            }, radial);
        }
        Style::Footman => {
            // a shock of tufts going wherever it likes
            for i in 0..9 {
                let bx = 116.0 + i as f32 * 13.0;
                let h = hash(i, 0, 81);
                let lean = (i as f32 - 4.0) * 3.4 + (h % 13) as f32 - 6.0;
                let top = 74.0 + (i as f32 - 4.0).abs() * 3.0 - (h / 17 % 12) as f32;
                let tone = vscale(l.hair, if i % 2 == 0 { 1.08 } else { 0.78 });
                // each tuft is a curved, tapering lock
                for k in 0..8 {
                    let t = k as f32 / 7.0;
                    let (x, y) = (bx + lean * t * t, 116.0 + (top - 116.0) * t);
                    c.ellipse_aa(x, y, 9.5 * (1.0 - t * 0.85), 7.0, pack(vscale(tone, 0.9 + 0.25 * t)));
                }
            }
            hair_mass(c, l, (100, 70, 236, 176), (CX - 6.0, 124.0, 66.0, 56.0), 0.5, |x, y| {
                let ragged = fbm(x * 0.11, 0.0, 3, 83) + 0.5 * ((x / 15.0).fract() - 0.5).abs();
                let dx = (x - CX).abs();
                let line = if dx > head_hw(l, y, 7.0) - 9.0 { 170.0 } else { 117.0 + 20.0 * ragged };
                cap(l, x, y, 7.0, line)
            }, |x, y| (x * 0.8 + y * 0.3, y * 2.0 - x * 0.4));
        }
        Style::Rival => {
            hair_mass(c, l, (100, 70, 236, 170), (CX - 6.0, 120.0, 70.0, 60.0), 0.25, |x, y| {
                let dx = (x - CX).abs();
                cap(l, x, y, 9.0, 113.0 + dx * dx / 80.0)
            }, |x, y| (x + (y * 0.2).sin() * 6.0, y * 6.0));
            // rolled side curls, and the bow
            for sx in [-1.0f32, 1.0] {
                for k in 0..4 {
                    let (rx, ry) = (CX + sx * (70.0 + k as f32 * 2.5), 150.0 + k as f32 * 31.0);
                    let tone = vscale(l.hair, if sx < 0.0 { 1.0 } else { 0.84 });
                    soft_ellipse(c, rx + 3.0, ry + 12.0, 30.0, 12.0, rgba(60, 56, 90, 110), 1.0);
                    orb(c, rx, ry, 27.0, 15.0, tone, 0.25, 10.0);
                    for i in 0..5 {
                        let t = (i as f32 - 2.0) / 2.0;
                        c.capsule(rx + t * 21.0, ry - 12.0 * (1.0 - t * t).sqrt(), rx + t * 21.0 + sx * 4.0, ry + 12.0 * (1.0 - t * t).sqrt(), 0.8, rgba(120, 116, 160, 90));
                    }
                    c.ellipse_aa(rx + sx * 23.0, ry, 4.5, 8.0, rgba(110, 106, 150, 120));
                }
            }
            for sx in [-1.0f32, 1.0] {
                c.poly_aa(&[(CX, 22.0), (CX + sx * 24.0, 10.0), (CX + sx * 22.0, 34.0)], pack(vscale(l.coat, if sx < 0.0 { 1.25 } else { 0.9 })));
            }
            orb(c, CX, 22.0, 6.0, 6.0, vscale(l.coat, 1.2), 0.6, 14.0);
        }
        Style::Cook => {
            hair_mass(c, l, (96, 100, 240, 196), (CX - 6.0, 136.0, 70.0, 56.0), 0.4, |x, y| {
                let dx = (x - CX).abs();
                let line = if dx > head_hw(l, y, 3.0) - 10.0 { 184.0 } else { 122.0 + dx * dx / 200.0 };
                cap(l, x, y, 3.0, line)
            }, radial);
            // the toque: a pleated band under a billow of starched linen
            let linen = [0.95, 0.95, 0.97];
            for (ox, oy, rx, ry) in [(-36.0, 62.0, 42.0, 34.0), (34.0, 58.0, 46.0, 38.0), (0.0, 48.0, 48.0, 36.0), (-8.0, 80.0, 60.0, 28.0)] {
                orb(c, CX + ox, oy, rx, ry, linen, 0.1, 8.0);
            }
            region(c, 98, 92, 240, 128, |x, y| {
                let dx = x - CX;
                let (top, bot) = (96.0 + dx * dx / 600.0, 123.0 + dx * dx / 420.0);
                let a = (y - top + 0.5).min(bot - y + 0.5).min(66.0 - dx.abs() + 0.5).min(1.0);
                if a <= 0.0 {
                    return None;
                }
                let pleat = 0.9 + 0.1 * (x * 0.9).sin();
                let d = wrap([dx / 70.0, -0.2, (1.0 - (dx / 70.0).powi(2) * 0.8).sqrt()]);
                Some((vscale(linen, (0.5 + 0.55 * d) * pleat * (1.0 - 0.2 * smoothstep(bot - 5.0, bot, y))), a))
            });
        }
    }
}

/// Things worn on the face: moustache, spectacles, monocle.
fn extras(c: &mut Canvas, l: &Look) {
    match l.style {
        Style::Lord => {
            // a moustache of some consequence
            for sx in [-1.0f32, 1.0] {
                let tone = vscale(l.hair, if sx < 0.0 { 1.0 } else { 0.82 });
                for i in 0..7 {
                    let t = i as f32 / 6.0;
                    let (x, y) = (CX + sx * (3.0 + t * 27.0), 209.0 + 5.0 * (t * PI * 0.9).sin() - t * t * 5.0);
                    let r = 6.2 * (1.0 - t * 0.72);
                    orb(c, x, y, r + 1.5, r, tone, 0.2, 8.0);
                }
                for i in 0..6 {
                    let t = i as f32 / 5.0;
                    c.capsule(CX + sx * (2.0 + t * 8.0), 205.0, CX + sx * (10.0 + t * 20.0), 214.0 - t * 5.0, 0.6, rgba(90, 92, 104, 110));
                }
            }
        }
        Style::Steward => {
            let silver = rgb(176, 184, 202);
            for sx in [-1.0f32, 1.0] {
                let ex = CX + sx * EYE_DX;
                let (x0, y0, x1, y1) = (ex - 18.0, EYE_Y - 11.0, ex + 18.0, EYE_Y + 11.0);
                c.rect_blend(x0 as i32, y0 as i32, 36, 22, rgba(200, 225, 255, 22));
                c.capsule(x0 + 4.0, y0 + 3.0, x0 + 13.0, y0 + 18.0, 1.6, rgba(255, 255, 255, 38));
                for (a, b) in [((x0, y0), (x1, y0)), ((x1, y0), (x1, y1)), ((x1, y1), (x0, y1)), ((x0, y1), (x0, y0))] {
                    c.capsule(a.0, a.1, b.0, b.1, 1.0, silver);
                }
                c.capsule(x0, y0 - 0.6, x1, y0 - 0.6, 0.6, rgb(250, 252, 255));
                c.capsule(ex + sx * 18.0, EYE_Y - 7.0, ex + sx * 30.0, EYE_Y - 4.0, 1.3, silver);
            }
            c.capsule(CX - 6.5, EYE_Y - 4.0, CX + 6.5, EYE_Y - 4.0, 1.3, silver);
        }
        Style::Rival => {
            let (ex, gold) = (CX + EYE_DX, rgb(226, 182, 80));
            c.ellipse_aa(ex, EYE_Y, 16.5, 16.5, rgba(210, 230, 255, 26));
            for i in 0..40 {
                let a = i as f32 / 40.0 * 2.0 * PI;
                let b = (i + 1) as f32 / 40.0 * 2.0 * PI;
                c.capsule(ex + a.cos() * 17.0, EYE_Y + a.sin() * 17.0, ex + b.cos() * 17.0, EYE_Y + b.sin() * 17.0, 1.7, gold);
            }
            c.capsule(ex - 10.0, EYE_Y - 9.0, ex - 3.0, EYE_Y - 13.0, 1.3, rgba(255, 255, 255, 90));
            // fine chain down to the lapel
            for i in 0..34 {
                let t = i as f32 / 33.0;
                let (x, y) = (ex + 15.0 + 26.0 * (t * PI * 0.5).sin() - t * t * 30.0, EYE_Y + 9.0 + t * 150.0);
                c.ellipse_aa(x, y, 1.2, 1.2, if i % 2 == 0 { gold } else { rgb(160, 120, 50) });
            }
        }
        _ => {}
    }
}

/// Paint character `who` with expression `expr` on a transparent
/// SPR_W x SPR_H (336x456) canvas.
pub fn paint(out: &mut Canvas, who: &str, overrides: &BTreeMap<String, String>, expr_name: &str) {
    let mut l = look(who);
    apply_overrides(&mut l, overrides);
    let e = expr(expr_name);
    // paint on a working canvas, then enlarge the bust to fill the frame
    let mut work = Canvas::new(SPR_W_I, SPR_H_I);
    let c = &mut work;
    hair_back(c, &l);
    body(c, &l);
    neck(c, &l);
    // collars and neckwear sit over the neck
    match l.style {
        Style::Heir | Style::Rival => open_coat(c, &l),
        Style::Lord => dress_coat(c, &l),
        Style::Steward | Style::Footman => livery(c, &l),
        Style::Cook => whites(c, &l),
    }
    head(c, &l);
    for sx in [-1.0f32, 1.0] {
        eye(c, &l, &e, sx);
        brow(c, &l, &e, sx);
    }
    mouth(c, &l, &e);
    hair_front(c, &l);
    extras(c, &l);
    rim_light(c);
    let (cw, ch) = ((SPR_W_I as f32 / ZOOM) as i32, (SPR_H_I as f32 / ZOOM) as i32);
    let mut crop = Canvas::new(cw, ch);
    crop.blit(&work, -(SPR_W_I - cw) / 2, 0, false, 256);
    *out = crop.resized(out.w, out.h);
}

fn hex(v: V3) -> String {
    let p = pack(v);
    format!("#{:06x}", p & 0x00FF_FFFF)
}

/// The colour and proportion settings a story may override, by key.
fn look_fields(l: &mut Look) -> (Vec<(&'static str, &mut V3)>, Vec<(&'static str, &mut f32, f32, f32)>) {
    (
        vec![("skin", &mut l.skin), ("hair", &mut l.hair), ("brow", &mut l.brow), ("iris", &mut l.iris), ("coat", &mut l.coat)],
        vec![
            ("width", &mut l.rx, 46.0, 64.0),
            ("jaw", &mut l.jaw, 0.62, 0.98),
            ("age", &mut l.age, 0.0, 1.0),
            ("blush", &mut l.blush, 0.0, 1.0),
            ("shoulders", &mut l.shoulders, 96.0, 132.0),
            ("lashes", &mut l.lashes, 0.0, 1.0),
            ("lips", &mut l.lip_tint, 0.0, 1.0),
        ],
    )
}

fn apply_overrides(l: &mut Look, overrides: &BTreeMap<String, String>) {
    let (colours, numbers) = look_fields(l);
    for (key, field) in colours {
        if let Some(c) = overrides.get(key).and_then(|v| crate::script::parse_color(v)) {
            *field = v3(c);
        }
    }
    for (key, field, lo, hi) in numbers {
        if let Some(n) = overrides.get(key).and_then(|v| v.parse::<f32>().ok()).filter(|n| n.is_finite()) {
            *field = n.clamp(lo, hi);
        }
    }
}

/// Default value of every overridable setting for a built-in portrait.
/// Numbers are written "value:min:max" so an editor can build a slider.
pub fn look_defaults(who: &str) -> BTreeMap<String, String> {
    let mut l = look(who);
    let (colours, numbers) = look_fields(&mut l);
    let mut out = BTreeMap::new();
    for (key, field) in colours {
        out.insert(key.to_string(), hex(*field));
    }
    for (key, field, lo, hi) in numbers {
        out.insert(key.to_string(), format!("{}:{}:{}", *field, lo, hi));
    }
    out
}

/// Enlargement applied to the working canvas (anchored top centre).
const ZOOM: f32 = 1.2;

/// The region of the sprite that changes between expressions (brows, eyes,
/// mouth): (x, y, w, h) in sprite pixels.
pub fn face_rect(_who: &str) -> (i32, i32, i32, i32) {
    let z = |v: f32| (v * ZOOM) as i32;
    (z(118.0 - 28.0), z(130.0), z(100.0), z(112.0))
}
