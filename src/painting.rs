//! Backgrounds as data: a painting described as layers of simple shapes,
//! drawn by the game's own renderer. The game's seven built-in places are
//! painted by code (`art`); a story can add places of its own as painting
//! files in a `paintings` folder beside its story file
//! (`paintings/<name>.json`), made by hand or generated in the editor.
//!
//! The canvas is the game's frame, 1152 x 648, x to the right and y down.
//! Layers are drawn in order, first at the back.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::gfx::{rgb, with_alpha, Canvas, H, W};

/// The most layers, and the most corners of one polygon, a painting may have.
pub const MAX_LAYERS: usize = 600;
pub const MAX_POINTS: usize = 96;

fn one() -> f32 {
    1.0
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Pt {
    pub x: f32,
    pub y: f32,
}

/// One layer of a painting.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "shape", rename_all = "lowercase")]
pub enum Layer {
    /// A band from `y0` to `y1`, shading from `top` to `bottom` colour, across the whole width.
    Gradient { top: String, bottom: String, y0: f32, y1: f32 },
    Rect { x: f32, y: f32, w: f32, h: f32, color: String, #[serde(default = "one")] alpha: f32 },
    /// A rectangle with soft edges `feather` pixels wide (haze, light through a window).
    Soft { x: f32, y: f32, w: f32, h: f32, feather: f32, color: String, #[serde(default = "one")] alpha: f32 },
    Ellipse { x: f32, y: f32, rx: f32, ry: f32, color: String, #[serde(default = "one")] alpha: f32 },
    Poly { points: Vec<Pt>, color: String, #[serde(default = "one")] alpha: f32 },
    /// A line with round ends, `width` pixels thick.
    Line { x0: f32, y0: f32, x1: f32, y1: f32, width: f32, color: String, #[serde(default = "one")] alpha: f32 },
    /// Light: a soft round glow that brightens what is under it.
    Glow { x: f32, y: f32, r: f32, color: String, strength: f32 },
    /// Scattered points of light between `y0` and `y1` (stars, dust, distant windows).
    Stars { count: u32, y0: f32, y1: f32, color: String, seed: u32 },
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PaintingData {
    #[serde(default)]
    pub title: String,
    /// What it shows, in words: the prompt for ComfyUI to paint it from.
    #[serde(default)]
    pub prompt: String,
    #[serde(default)]
    pub layers: Vec<Layer>,
    /// Darken the corners a little, as the game's own paintings do (0 to 1).
    #[serde(default)]
    pub vignette: f32,
}

/// "#rrggbb" as the canvas's colour; mid grey if it is not one.
pub fn colour(hex: &str) -> u32 {
    let h = hex.trim().trim_start_matches('#');
    match (h.len(), u32::from_str_radix(h, 16)) {
        (6, Ok(n)) => rgb((n >> 16) as u8, (n >> 8) as u8, n as u8),
        _ => rgb(128, 128, 128),
    }
}

fn clean_hex(c: &mut String) {
    let h = c.trim().trim_start_matches('#').to_lowercase();
    *c = if h.len() == 6 && h.chars().all(|c| c.is_ascii_hexdigit()) { format!("#{}", h) } else { "#808080".into() };
}

fn fin(v: &mut f32, lo: f32, hi: f32) {
    *v = if v.is_finite() { v.clamp(lo, hi) } else { lo.max(0.0).min(hi) };
}

impl PaintingData {
    /// Bring a painting from anywhere within what the renderer will draw.
    pub fn clean(&mut self) {
        self.title = self.title.chars().filter(|c| !c.is_control() && *c != '|').take(80).collect::<String>().trim().to_string();
        self.prompt = self.prompt.chars().filter(|c| !c.is_control()).take(600).collect::<String>().trim().to_string();
        fin(&mut self.vignette, 0.0, 1.0);
        self.layers.truncate(MAX_LAYERS);
        // (a little way beyond the frame is allowed: shapes may run off its edges)
        let (xs, ys) = (-(W as f32), 2.0 * W as f32);
        let (y_lo, y_hi) = (-(H as f32), 2.0 * H as f32);
        for l in &mut self.layers {
            match l {
                Layer::Gradient { top, bottom, y0, y1 } => {
                    clean_hex(top);
                    clean_hex(bottom);
                    fin(y0, 0.0, H as f32);
                    fin(y1, 0.0, H as f32);
                    if *y1 < *y0 {
                        std::mem::swap(y0, y1);
                    }
                }
                Layer::Rect { x, y, w, h, color, alpha } | Layer::Soft { x, y, w, h, color, alpha, .. } => {
                    fin(x, xs, ys);
                    fin(y, y_lo, y_hi);
                    fin(w, 0.0, 3.0 * W as f32);
                    fin(h, 0.0, 3.0 * H as f32);
                    clean_hex(color);
                    fin(alpha, 0.0, 1.0);
                }
                Layer::Ellipse { x, y, rx, ry, color, alpha } => {
                    fin(x, xs, ys);
                    fin(y, y_lo, y_hi);
                    fin(rx, 0.0, 2.0 * W as f32);
                    fin(ry, 0.0, 2.0 * W as f32);
                    clean_hex(color);
                    fin(alpha, 0.0, 1.0);
                }
                Layer::Poly { points, color, alpha } => {
                    points.truncate(MAX_POINTS);
                    for p in points.iter_mut() {
                        fin(&mut p.x, xs, ys);
                        fin(&mut p.y, y_lo, y_hi);
                    }
                    clean_hex(color);
                    fin(alpha, 0.0, 1.0);
                }
                Layer::Line { x0, y0, x1, y1, width, color, alpha } => {
                    for v in [x0, x1] {
                        fin(v, xs, ys);
                    }
                    for v in [y0, y1] {
                        fin(v, y_lo, y_hi);
                    }
                    fin(width, 0.5, 200.0);
                    clean_hex(color);
                    fin(alpha, 0.0, 1.0);
                }
                Layer::Glow { x, y, r, color, strength } => {
                    fin(x, xs, ys);
                    fin(y, y_lo, y_hi);
                    fin(r, 1.0, 2.0 * W as f32);
                    clean_hex(color);
                    fin(strength, 0.0, 3.0);
                }
                Layer::Stars { count, y0, y1, color, .. } => {
                    *count = (*count).min(3000);
                    fin(y0, 0.0, H as f32);
                    fin(y1, 0.0, H as f32);
                    if *y1 < *y0 {
                        std::mem::swap(y0, y1);
                    }
                    clean_hex(color);
                }
            }
        }
        // something to look at, at the least
        if self.layers.is_empty() {
            self.layers.push(Layer::Gradient { top: "#202438".into(), bottom: "#4a4458".into(), y0: 0.0, y1: H as f32 });
        }
    }

    /// Paint it, at the game's size.
    pub fn render(&self) -> Canvas {
        let mut c = Canvas::new(W, H);
        c.clear(rgb(0, 0, 0));
        let a8 = |a: f32| (a.clamp(0.0, 1.0) * 255.0).round() as u32;
        for l in &self.layers {
            match l {
                Layer::Gradient { top, bottom, y0, y1 } => {
                    let (t, b) = (colour(top), colour(bottom));
                    let (from, to) = (*y0 as i32, (*y1 as i32).max(*y0 as i32 + 1));
                    for y in from.max(0)..to.min(H) {
                        let k = (y - from) as f32 / (to - from).max(1) as f32;
                        c.rect(0, y, W, 1, crate::gfx::lerp(t, b, k));
                    }
                }
                Layer::Rect { x, y, w, h, color, alpha } => c.rect_blend(*x as i32, *y as i32, *w as i32, *h as i32, with_alpha(colour(color), a8(*alpha))),
                Layer::Soft { x, y, w, h, feather, color, alpha } => crate::art::paint::soft_rect(&mut c, *x, *y, *w, *h, feather.max(0.5), with_alpha(colour(color), a8(*alpha))),
                Layer::Ellipse { x, y, rx, ry, color, alpha } => c.ellipse_aa(*x, *y, *rx, *ry, with_alpha(colour(color), a8(*alpha))),
                Layer::Poly { points, color, alpha } => {
                    if points.len() >= 3 {
                        let pts: Vec<(f32, f32)> = points.iter().map(|p| (p.x, p.y)).collect();
                        c.poly_aa(&pts, with_alpha(colour(color), a8(*alpha)));
                    }
                }
                Layer::Line { x0, y0, x1, y1, width, color, alpha } => c.capsule(*x0, *y0, *x1, *y1, width / 2.0, with_alpha(colour(color), a8(*alpha))),
                Layer::Glow { x, y, r, color, strength } => c.glow(*x, *y, *r, colour(color), *strength),
                Layer::Stars { count, y0, y1, color, seed } => {
                    let col = colour(color);
                    for i in 0..*count as i32 {
                        let sx = (crate::gfx::hash(i, 0, *seed) % W as u32) as i32;
                        let span = (*y1 - *y0).max(1.0) as u32;
                        let sy = *y0 as i32 + (crate::gfx::hash(i, 1, *seed) % span) as i32;
                        let bright = 0.35 + crate::gfx::hashf(i, 2, *seed) * 0.65;
                        c.cover(sx, sy, col, bright);
                    }
                }
            }
        }
        if self.vignette > 0.0 {
            crate::art::paint::finish(&mut c, self.vignette, 0.0);
        }
        c
    }
}

/// Where a story keeps its own paintings: `paintings` beside the story file.
pub fn dir(story: &Path) -> PathBuf {
    story.parent().unwrap_or(Path::new(".")).join("paintings")
}

/// A name a story may give a place of its own: an id the game does not already use.
pub fn usable_name(name: &str) -> bool {
    crate::script::valid_id(name) && name.len() <= 40 && name.chars().all(|c| !c.is_ascii_uppercase()) && !crate::art::BACKGROUNDS.contains(&name)
}

/// The story's own places, by name, in order.
pub fn names(story: &Path) -> Vec<String> {
    let mut out: Vec<String> = std::fs::read_dir(dir(story))
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok())
        .filter_map(|e| e.file_name().to_string_lossy().strip_suffix(".json").map(str::to_string))
        .filter(|n| usable_name(n))
        .collect();
    out.sort();
    out
}

/// Every place a story can name: the game's own, then the story's.
pub fn backgrounds_for(story: Option<&Path>) -> Vec<String> {
    let mut out: Vec<String> = crate::art::BACKGROUNDS.iter().map(|s| s.to_string()).collect();
    if let Some(story) = story {
        out.extend(names(story));
    }
    out
}

/// One of the story's own paintings, cleaned; `None` if there is none (or it cannot be read).
pub fn load(story: &Path, name: &str) -> Option<PaintingData> {
    if !usable_name(name) {
        return None;
    }
    let text = std::fs::read_to_string(dir(story).join(format!("{}.json", name))).ok()?;
    let mut p: PaintingData = serde_json::from_str(&text).ok()?;
    p.clean();
    Some(p)
}

/// Save one of the story's own paintings (beside it, then renamed in: never half a file).
pub fn save(story: &Path, name: &str, painting: &PaintingData) -> Result<(), String> {
    if !usable_name(name) {
        return Err(format!("'{}' cannot name a new place (letters, digits and _, not one of the game's own)", name));
    }
    let d = dir(story);
    std::fs::create_dir_all(&d).map_err(|e| e.to_string())?;
    let tmp = d.join(format!("{}.json.tmp", name));
    std::fs::write(&tmp, serde_json::to_string_pretty(painting).unwrap_or_default() + "\n").map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, d.join(format!("{}.json", name))).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_painting_from_anywhere_is_cleaned_and_drawn() {
        let text = r##"{"title":"Rooftop | night","layers":[
            {"shape":"gradient","top":"#0a0c20","bottom":"#30284a","y0":0,"y1":400},
            {"shape":"stars","count":200,"y0":0,"y1":300,"color":"#ffffff","seed":3},
            {"shape":"poly","points":[{"x":0,"y":420},{"x":1152,"y":380},{"x":1152,"y":648},{"x":0,"y":648}],"color":"#202020"},
            {"shape":"rect","x":100,"y":300,"w":80,"h":120,"color":"#ffd080","alpha":0.8},
            {"shape":"glow","x":140,"y":360,"r":120,"color":"#ffb050","strength":0.6},
            {"shape":"ellipse","x":900,"y":120,"rx":40,"ry":40,"color":"#f0f0e0"},
            {"shape":"line","x0":0,"y0":500,"x1":1152,"y1":480,"width":6,"color":"#606060","alpha":1e9},
            {"shape":"soft","x":0,"y":350,"w":1152,"h":60,"feather":30,"color":"#8090c0","alpha":0.3}],"vignette":5}"##;
        let mut p: PaintingData = serde_json::from_str(text).unwrap();
        p.clean();
        assert_eq!(p.title, "Rooftop  night");
        assert_eq!(p.vignette, 1.0);
        assert!(matches!(p.layers[6], Layer::Line { alpha, .. } if alpha == 1.0));
        let c = p.render();
        assert_eq!((c.w, c.h), (W, H));
        // the lit window is warm, the sky dark, the ground darker still
        let at = |x: i32, y: i32| { let v = c.px[(y * W + x) as usize]; ((v >> 16) & 255, (v >> 8) & 255, v & 255) };
        let (wr, wg, wb) = at(140, 360);
        assert!(wr > 140 && wr > wb + 50, "window {:?}", (wr, wg, wb));
        let (sr, sg, sb) = at(600, 40);
        assert!(sr < 60 && sg < 60 && sb < 90, "sky {:?}", (sr, sg, sb));
        // nonsense stays harmless: wrong colours, huge sizes, too many points
        let mut odd = PaintingData { layers: vec![Layer::Rect { x: f32::NAN, y: 1e30, w: -5.0, h: 1e9, color: "red".into(), alpha: 2.0 }, Layer::Poly { points: (0..500).map(|i| Pt { x: i as f32, y: 0.0 }).collect(), color: "#fff".into(), alpha: 1.0 }], ..Default::default() };
        odd.clean();
        assert!(matches!(&odd.layers[0], Layer::Rect { color, alpha, .. } if color == "#808080" && *alpha == 1.0));
        assert!(matches!(&odd.layers[1], Layer::Poly { points, .. } if points.len() == MAX_POINTS));
        odd.render();
        // an empty painting is not a black hole
        let mut empty = PaintingData::default();
        empty.clean();
        assert_eq!(empty.layers.len(), 1);
    }

    #[test]
    fn a_story_keeps_places_of_its_own() {
        let dir = std::env::temp_dir().join(format!("below-stairs-paint-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let story = dir.join("story.txt");
        let p = PaintingData { title: "Roof".into(), ..Default::default() };
        save(&story, "rooftop", &p).unwrap();
        assert!(save(&story, "pantry", &p).is_err(), "the game's own names are taken");
        assert!(save(&story, "../x", &p).is_err() && save(&story, "Roof", &p).is_err());
        std::fs::write(dir.join("paintings").join("broken.json"), "{").unwrap();
        assert_eq!(names(&story), vec!["broken".to_string(), "rooftop".to_string()]);
        assert!(load(&story, "rooftop").is_some() && load(&story, "broken").is_none());
        assert_eq!(backgrounds_for(Some(&story)).len(), crate::art::BACKGROUNDS.len() + 2);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
