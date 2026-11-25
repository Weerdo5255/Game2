//! Artwork registry.
//!
//! Every background and character is painted procedurally in code (one
//! module per scene). Those paintings are also exported as *guides* for the
//! ComfyUI enhancement suite in `comfyui/`; whatever that suite produces is
//! dropped into `assets/enhanced/` and loaded here in place of the
//! procedural version, file by file.

mod axis;
mod ballroom;
mod dock;
mod paint;
mod pantry;
mod people;
mod study;
mod swan;
mod vista;

use crate::gfx::*;
use std::path::{Path, PathBuf};

pub const BACKGROUNDS: &[&str] = &["pantry", "axis", "dock", "vista", "vista_night", "ballroom", "study"];
pub const SPRITES: &[&str] = &["p", "v", "s", "q", "t", "c", "a"];
pub const EXPRS: &[&str] = &["neutral", "smile", "worried", "angry", "smug", "shock"];

/// Character sprite size, in frame pixels.
pub const SPR_W: i32 = 336;
pub const SPR_H: i32 = 456;

/// Where enhanced art is looked for, in order: `$BELOW_STAIRS_ART`,
/// `./assets/enhanced`, then `assets/enhanced` beside the executable.
pub fn find_enhanced_dir() -> Option<PathBuf> {
    let mut candidates = Vec::new();
    if let Ok(p) = std::env::var("BELOW_STAIRS_ART") {
        candidates.push(PathBuf::from(p));
    }
    candidates.push(PathBuf::from("assets/enhanced"));
    if let Some(dir) = std::env::current_exe().ok().and_then(|e| e.parent().map(Path::to_path_buf)) {
        candidates.push(dir.join("assets/enhanced"));
    }
    candidates.into_iter().find(|p| p.is_dir())
}

fn load_enhanced(dir: Option<&Path>, file: &str, w: i32, h: i32) -> Option<Canvas> {
    let path = dir?.join(file);
    if !path.is_file() {
        return None;
    }
    match Canvas::load_png(&path) {
        Ok(img) if img.w == w && img.h == h => Some(img),
        Ok(img) => Some(img.resized(w, h)),
        Err(e) => {
            eprintln!("ignoring enhanced art: {}", e);
            None
        }
    }
}

/// Paint the static part of a background at full frame size. Returns the
/// canvas and whether it is the procedural version (which may be animated).
pub fn background(name: &str, enhanced: Option<&Path>) -> (Canvas, bool) {
    if let Some(mut img) = load_enhanced(enhanced, &format!("bg_{}.png", name), W, H) {
        for p in img.px.iter_mut() {
            *p |= 0xFF00_0000;
        }
        return (img, false);
    }
    let mut c = Canvas::new(W, H);
    c.clear(BLACK);
    match name {
        "pantry" => pantry::paint(&mut c),
        "axis" => axis::paint(&mut c),
        "dock" => dock::paint(&mut c),
        "vista" => vista::paint(&mut c, false),
        "vista_night" => vista::paint(&mut c, true),
        "ballroom" => ballroom::paint(&mut c),
        "study" => study::paint(&mut c),
        _ => {}
    }
    (c, true)
}

/// Per-frame animated overlay for a procedural background (cheap: it runs
/// every frame on top of a copy of the cached static painting).
pub fn animate(c: &mut Canvas, name: &str, t: f32) {
    match name {
        "pantry" => pantry::animate(c, t),
        "axis" => axis::animate(c, t),
        "dock" => dock::animate(c, t),
        "vista" => vista::animate(c, t, false),
        "vista_night" => vista::animate(c, t, true),
        "ballroom" => ballroom::animate(c, t),
        "study" => study::animate(c, t),
        _ => {}
    }
}

fn procedural_sprite(who: &str, expr: &str) -> Canvas {
    let mut c = Canvas::new(SPR_W, SPR_H);
    if who == "a" {
        swan::paint(&mut c, expr);
    } else {
        people::paint(&mut c, who, expr);
    }
    c
}

/// A character sprite (SPR_W x SPR_H, transparent background).
pub fn sprite(who: &str, expr: &str, enhanced: Option<&Path>) -> Canvas {
    load_enhanced(enhanced, &format!("spr_{}_{}.png", who, expr), SPR_W, SPR_H)
        .unwrap_or_else(|| procedural_sprite(who, expr))
}

/// The part of a sprite that changes between expressions: (x, y, w, h).
fn face_rect(who: &str) -> (i32, i32, i32, i32) {
    if who == "a" {
        swan::face_rect()
    } else {
        people::face_rect(who)
    }
}

// ------------------------------------------------- prompts for the AI pass

/// What each background depicts, as a text-to-image prompt fragment.
pub fn bg_prompt(name: &str) -> &'static str {
    match name {
        "pantry" => "servants' hall below stairs in a grand manor inside a space habitat, warm brick walls, cast iron stove with glowing fire, hanging copper pots, wooden shelves of preserve jars, brass servant bell board on the wall, terracotta tiled floor, warm firelight, cosy, empty room, no people",
        "axis" => "zero gravity observatory at the hub of an O'Neill cylinder space habitat, huge round brass-rimmed viewport showing stars, a distant cylindrical space station and a comet, riveted steel wall panels, antique brass telescope, a croissant, teacup, saucer and marmalade jar floating weightless in the air, no people",
        "dock" => "private spacecraft docking bay of an aristocratic estate, elegant cream and gold luxury space yacht resting in a docking cradle, large open bay door showing a starfield, steel deck with yellow and black hazard stripes, stacked teal cargo crates, an ornate purple sedan chair, industrial lighting, no people",
        "vista" => "interior of a vast O'Neill cylinder space habitat seen from the ground looking down its length, farmland, lakes and towns curving up both sides and overhead, long window strips, bright sunline along the axis, clouds near the centre, in the foreground a neoclassical manor house with white columns and pediment, formal lawn, gravel path and topiary, bright morning, no people",
        "vista_night" => "interior of a vast O'Neill cylinder space habitat at night seen from the ground looking down its length, city lights scattered across land curving up both sides and overhead, long window strips showing stars, dim moonlike glow along the axis, in the foreground a neoclassical manor house with columns and warmly lit windows, dark formal lawn and topiary, no people",
        "ballroom" => "opulent ballroom during an evening gala in a space habitat manor, deep purple walls with gilded pilasters, tall arched windows with crimson drapes showing stars and distant ship lights, glittering crystal chandeliers, polished checkered marble floor with reflections, silhouettes of an elegant crowd in the background, warm candlelight",
        "study" => "aristocratic wood-panelled study at midnight, floor to ceiling bookshelves of leather-bound books, gilt-framed ancestral portrait, green banker's lamp, deep red carpet, in the centre a walnut war table projecting a glowing cyan hologram of two small cylindrical space habitats with a bright comet between them, moody low light, no people",
        _ => "",
    }
}

/// What each character looks like, as a prompt fragment.
pub fn sprite_prompt(who: &str) -> &'static str {
    match who {
        "p" => "portrait of a cheerful twenty year old aristocratic young man, floppy golden blond hair, fair skin, rosy cheeks, lavender velvet tailcoat with gold trim, white lace cravat with ruby pin",
        "v" => "portrait of a stern aristocratic lord in his sixties, slicked back silver hair, large grey moustache, wine red military dress coat, gold fringed epaulettes, blue sash with gold edging, white shirt",
        "s" => "portrait of a haughty pale aristocratic lord in his fifties, towering white powdered wig with side curls and a small teal bow, gold monocle on a chain, beauty mark, teal brocade coat with silver trim, white lace cravat",
        "q" => "portrait of a composed woman in her forties, dark brown skin, black hair in a neat high bun, rectangular silver glasses, charcoal grey steward's uniform jacket with teal piping, white collar",
        "t" => "portrait of a friendly young footman of about nineteen, messy spiky ginger red hair, freckles, light tan skin, teal servant's livery jacket with silver piping, white collar",
        "c" => "portrait of a sturdy warm-faced cook in her fifties, brown skin, grey hair under a white cook's cap, rosy cheeks, white double-breasted chef's jacket with a red neckerchief",
        "a" => "a majestic white mute swan seen in profile facing left, wearing a tiny navy blue admiral's bicorne hat with gold trim and red cockade, small lavender cravat, gold medal on a red ribbon",
        _ => "",
    }
}

/// How each expression reads, as a prompt fragment.
pub fn expr_prompt(expr: &str) -> &'static str {
    match expr {
        "smile" => "warm happy smile, eyes closed in delight",
        "worried" => "worried anxious expression, raised inner eyebrows, slight frown",
        "angry" => "angry scowling expression, furrowed brows, frown",
        "smug" => "smug self-satisfied smirk, half-lidded eyes, one raised eyebrow",
        "shock" => "shocked astonished expression, wide eyes, open mouth, raised eyebrows",
        _ => "calm neutral expression, closed mouth",
    }
}

fn json_str(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

/// Write every procedural painting as a PNG guide, plus masks and a
/// `manifest.json` describing them, for the ComfyUI suite.
pub fn export_guides(dir: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {}", dir.display(), e))?;
    let mut entries = Vec::new();
    for name in BACKGROUNDS {
        let start = std::time::Instant::now();
        let (mut c, _) = background(name, None);
        let paint_ms = start.elapsed().as_millis();
        let start = std::time::Instant::now();
        animate(&mut c, name, 0.5);
        let file = format!("bg_{}.png", name);
        println!("{:<28} paint {:>5} ms, animate {:>3} ms", file, paint_ms, start.elapsed().as_millis());
        c.save_png(&dir.join(&file), false)?;
        entries.push(format!(
            "    {{\"id\": {}, \"kind\": \"background\", \"file\": {}, \"output\": {}, \"width\": {}, \"height\": {}, \"prompt\": {}}}",
            json_str(&format!("bg_{}", name)), json_str(&file), json_str(&file), W, H, json_str(bg_prompt(name))
        ));
    }
    for who in SPRITES {
        // the region that differs between expressions, for inpainting
        let (fx, fy, fw, fh) = face_rect(who);
        let mut face = Canvas::new(SPR_W, SPR_H);
        face.clear(BLACK);
        face.ellipse(fx + fw / 2, fy + fh / 2, fw / 2, fh / 2, rgb(255, 255, 255));
        let face_file = format!("face_{}.png", who);
        face.save_png(&dir.join(&face_file), false)?;
        for expr in EXPRS {
            let c = procedural_sprite(who, expr);
            let file = format!("spr_{}_{}.png", who, expr);
            c.save_png(&dir.join(&file), true)?;
            // silhouette: white where the character is
            let mut mask = Canvas::new(SPR_W, SPR_H);
            for (m, p) in mask.px.iter_mut().zip(c.px.iter()) {
                let a = (p >> 24) as u8;
                *m = rgb(a, a, a);
            }
            let mask_file = format!("mask_{}_{}.png", who, expr);
            mask.save_png(&dir.join(&mask_file), false)?;
            let base = if *expr == "neutral" { "null".to_string() } else { json_str(&format!("spr_{}_neutral", who)) };
            entries.push(format!(
                "    {{\"id\": {}, \"kind\": \"sprite\", \"file\": {}, \"output\": {}, \"width\": {}, \"height\": {}, \"character\": {}, \"expression\": {}, \"base\": {}, \"mask\": {}, \"face_mask\": {}, \"face_rect\": [{}, {}, {}, {}], \"prompt\": {}, \"expression_prompt\": {}}}",
                json_str(&format!("spr_{}_{}", who, expr)), json_str(&file), json_str(&file), SPR_W, SPR_H,
                json_str(who), json_str(expr), base, json_str(&mask_file), json_str(&face_file),
                fx, fy, fw, fh, json_str(sprite_prompt(who)), json_str(expr_prompt(expr))
            ));
        }
        println!("spr_{}_*.png", who);
    }
    let manifest = format!(
        "{{\n  \"version\": 1,\n  \"frame\": [{}, {}],\n  \"sprite\": [{}, {}],\n  \"assets\": [\n{}\n  ]\n}}\n",
        W, H, SPR_W, SPR_H, entries.join(",\n")
    );
    std::fs::write(dir.join("manifest.json"), manifest).map_err(|e| e.to_string())?;
    println!("wrote {} guides and manifest.json to {}", entries.len(), dir.display());
    Ok(())
}
