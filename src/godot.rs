//! Writes the game as a Godot 4 project: the engine that shows it, and the
//! editor its pictures and places are worked on in.
//!
//!   cargo run --release -- --export-godot godot [--story FILE] [--force]
//!
//! What goes where in the project:
//!
//!   project.godot, main.tscn, scenes/, scripts/   the game itself (GDScript); written if missing
//!   story/story.json        the story, as the story editor keeps it; always written
//!   art/bg/<place>.png      the paintings behind the story
//!   art/portraits/<who>_<expression>.png
//!   places/<place>.tscn     each 3D place as a Godot scene: meshes, materials,
//!                           lights, the air, markers for the start and the cast,
//!                           and collision for what the player walks around
//!   audio/, models/         recordings, music and model files the story has
//!
//! Scenes and pictures are the user's to change in Godot, so a file that is
//! already there is left alone unless `--force` is given. The story file is
//! data, not something edited in Godot: it is written every time.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use crate::places::{self, PlaceData};
use crate::script::Doc;
use crate::{art, audio};

/// The project's own files: the game as GDScript and scenes.
const TEMPLATE: &[(&str, &str)] = &[
    ("project.godot", include_str!("../godot/project.godot")),
    ("main.tscn", include_str!("../godot/main.tscn")),
    ("scenes/hud.tscn", include_str!("../godot/scenes/hud.tscn")),
    ("scenes/stage_2d.tscn", include_str!("../godot/scenes/stage_2d.tscn")),
    ("scenes/player.tscn", include_str!("../godot/scenes/player.tscn")),
    ("scenes/character_3d.tscn", include_str!("../godot/scenes/character_3d.tscn")),
    ("scripts/game.gd", include_str!("../godot/scripts/game.gd")),
    ("scripts/story_vm.gd", include_str!("../godot/scripts/story_vm.gd")),
    ("scripts/hud.gd", include_str!("../godot/scripts/hud.gd")),
    ("scripts/stage_2d.gd", include_str!("../godot/scripts/stage_2d.gd")),
    ("scripts/world_3d.gd", include_str!("../godot/scripts/world_3d.gd")),
    ("scripts/player.gd", include_str!("../godot/scripts/player.gd")),
    ("scripts/character_3d.gd", include_str!("../godot/scripts/character_3d.gd")),
    ("scripts/place.gd", include_str!("../godot/scripts/place.gd")),
    ("tests/run_story.gd", include_str!("../godot/tests/run_story.gd")),
];

/// How the exported game shows its places.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    /// As the story sets them: some places paintings, some 3D worlds.
    Story,
    /// Everything as paintings: no 3D at all.
    Flat,
    /// Everything as 3D worlds, and the cast as 3D figures where they have no model of their own.
    Full,
}

impl Mode {
    pub fn parse(text: &str) -> Option<Mode> {
        match text {
            "" | "story" => Some(Mode::Story),
            "2d" | "flat" => Some(Mode::Flat),
            "3d" | "full" => Some(Mode::Full),
            _ => None,
        }
    }

    fn describe(self) -> &'static str {
        match self {
            Mode::Story => "places as the story sets them (some paintings, some 3D)",
            Mode::Flat => "everything 2D: every place a painting, every character a portrait",
            Mode::Full => "everything 3D: every place a 3D world, the cast as 3D figures (or their own models)",
        }
    }
}

/// What an export did, for telling the user.
#[derive(Default, Debug)]
pub struct Report {
    pub written: Vec<String>,
    pub kept: Vec<String>,
    pub notes: Vec<String>,
}

/// A number as Godot writes it: no more digits than it needs.
fn num(v: f32) -> String {
    let r = (v * 100_000.0).round() / 100_000.0;
    if r == r.trunc() && r.abs() < 1.0e9 {
        format!("{}", r as i64)
    } else {
        format!("{}", r)
    }
}

fn color(hex: &str, alpha: f32) -> String {
    let [r, g, b] = places::rgb(hex);
    format!("Color({}, {}, {}, {})", num(r), num(g), num(b), num(alpha))
}

/// The turn of a part as rows of a matrix: about y, then x, then z (Godot's own order for rotations).
fn turn(rot: [f32; 3]) -> [[f32; 3]; 3] {
    let [x, y, z] = rot.map(f32::to_radians);
    let (sx, cx, sy, cy, sz, cz) = (x.sin(), x.cos(), y.sin(), y.cos(), z.sin(), z.cos());
    [[cy * cz + sy * sx * sz, -cy * sz + sy * sx * cz, sy * cx], [cx * sz, cx * cz, -sx], [-sy * cz + cy * sx * sz, sy * sz + cy * sx * cz, cy * cx]]
}

/// `Transform3D(...)` as a scene file has it: the basis row by row (each column scaled), then the origin.
fn transform(rows: [[f32; 3]; 3], scale: [f32; 3], at: [f32; 3]) -> String {
    let mut out = String::from("Transform3D(");
    for row in rows {
        for (k, v) in row.iter().enumerate() {
            let _ = write!(out, "{}, ", num(v * scale[k]));
        }
    }
    let _ = write!(out, "{}, {}, {})", num(at[0]), num(at[1]), num(at[2]));
    out
}

const UPRIGHT: [[f32; 3]; 3] = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];

/// The basis of something at `from` looking at the origin (Godot looks along -z).
fn looking_in(from: [f32; 3]) -> [[f32; 3]; 3] {
    let len = (from[0] * from[0] + from[1] * from[1] + from[2] * from[2]).sqrt();
    if len < 1e-4 {
        return turn([-90.0, 0.0, 0.0]);
    }
    let z = from.map(|v| v / len);
    // straight up or down has no "up" to keep: lean on z instead
    let up = if z[1].abs() > 0.999 { [0.0, 0.0, -1.0] } else { [0.0, 1.0, 0.0] };
    let cross = |a: [f32; 3], b: [f32; 3]| [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
    let x = cross(up, z);
    let xl = (x[0] * x[0] + x[1] * x[1] + x[2] * x[2]).sqrt().max(1e-6);
    let x = x.map(|v| v / xl);
    let y = cross(z, x);
    [[x[0], y[0], z[0]], [x[1], y[1], z[1]], [x[2], y[2], z[2]]]
}

/// A name Godot will take for a node: letters, digits and underscores.
fn node_name(text: &str, fallback: &str) -> String {
    let clean: String = text.chars().map(|c| if c.is_ascii_alphanumeric() || c == '_' { c } else { '_' }).collect();
    let clean = clean.trim_matches('_').to_string();
    if clean.is_empty() {
        fallback.to_string()
    } else {
        clean
    }
}

/// One 3D place as a Godot scene.
///
/// `models`: the model files the project will have. A model part whose file
/// is not among them is left out (as the game does), since a scene that
/// refers to a missing file does not load in Godot at all.
pub fn place_scene(name: &str, place: &PlaceData, models_there: &[String]) -> String {
    let mut ext = String::new();
    let mut sub = String::new();
    let mut nodes = String::new();
    let _ = writeln!(ext, "[ext_resource type=\"Script\" path=\"res://scripts/place.gd\" id=\"place\"]");
    // one mesh of each plain shape, stretched to each part's size by its transform
    sub.push_str("[sub_resource type=\"BoxMesh\" id=\"box\"]\n\n");
    sub.push_str("[sub_resource type=\"CylinderMesh\" id=\"cylinder\"]\ntop_radius = 0.5\nbottom_radius = 0.5\nheight = 1.0\n\n");
    sub.push_str("[sub_resource type=\"SphereMesh\" id=\"sphere\"]\nradius = 0.5\nheight = 1.0\n\n");
    // parts of one colour and finish share a material
    let mut materials: BTreeMap<String, String> = BTreeMap::new();
    let mut capsules: BTreeMap<String, String> = BTreeMap::new();
    let mut models: BTreeMap<String, String> = BTreeMap::new();
    let mut names: BTreeMap<String, usize> = BTreeMap::new();
    let mut unique = |base: String| -> String {
        let n = names.entry(base.clone()).or_insert(0);
        *n += 1;
        if *n == 1 {
            base
        } else {
            format!("{}_{}", base, n)
        }
    };

    let air = &place.air;
    // (the game's own light units are an engine's; these are scaled to look alike in Godot)
    let _ = writeln!(
        sub,
        "[sub_resource type=\"Environment\" id=\"air\"]\nbackground_mode = 1\nbackground_color = {sky}\nambient_light_source = 2\nambient_light_color = {amb}\nambient_light_energy = {energy}\nfog_enabled = true\nfog_mode = 1\nfog_light_color = {sky}\nfog_density = 1.0\nfog_depth_begin = {f0}\nfog_depth_end = {f1}\ntonemap_mode = 2\n",
        sky = color(&air.sky, 1.0),
        amb = color(&air.ambient, 1.0),
        energy = num((air.brightness / 600.0).clamp(0.02, 4.0)),
        f0 = num(air.fog[0]),
        f1 = num(air.fog[1]),
    );
    let title = node_name(name, "Place");
    let _ = writeln!(nodes, "[node name=\"{}\" type=\"Node3D\"]\nscript = ExtResource(\"place\")\nfloating = {}\n", title, place.floating);
    let _ = writeln!(nodes, "[node name=\"Air\" type=\"WorldEnvironment\" parent=\".\"]\nenvironment = SubResource(\"air\")\n");

    // where you start, and where the cast stands
    let _ = writeln!(nodes, "[node name=\"Spawn\" type=\"Marker3D\" parent=\".\"]\ntransform = {}\n", transform(turn([0.0, place.yaw, 0.0]), [1.0; 3], place.spawn));
    for (marker, at) in ["CastLeft", "CastCentre", "CastRight"].iter().zip(place.anchors) {
        let _ = writeln!(nodes, "[node name=\"{}\" type=\"Marker3D\" parent=\".\"]\ntransform = {}\n", marker, transform(UPRIGHT, [1.0; 3], at));
    }

    nodes.push_str("[node name=\"Parts\" type=\"Node3D\" parent=\".\"]\n\n");
    for p in &place.parts {
        let base = node_name(if p.name.is_empty() { &p.shape } else { &p.name }, "part");
        let node = unique(base);
        if p.shape == "model" {
            if !models_there.contains(&p.file) {
                continue;
            }
            let next = models.len() + 1;
            let id = models.entry(p.file.clone()).or_insert_with(|| {
                let id = format!("model_{}", next);
                let _ = writeln!(ext, "[ext_resource type=\"PackedScene\" path=\"res://models/{}\" id=\"{}\"]", p.file, id);
                id
            });
            let _ = writeln!(nodes, "[node name=\"{}\" parent=\"Parts\" instance=ExtResource(\"{}\")]\ntransform = {}\n", node, id, transform(turn(p.rot), p.size, p.at));
            continue;
        }
        let key = format!("{} {} {} {}", p.color, p.rough, p.metal, p.glow);
        let next = materials.len() + 1;
        let material = materials
            .entry(key)
            .or_insert_with(|| {
                let id = format!("mat_{}", next);
                let _ = writeln!(sub, "[sub_resource type=\"StandardMaterial3D\" id=\"{}\"]\nalbedo_color = {}\nmetallic = {}\nroughness = {}", id, color(&p.color, 1.0), num(p.metal), num(p.rough));
                if p.glow > 0.0 {
                    let _ = writeln!(sub, "emission_enabled = true\nemission = {}\nemission_energy_multiplier = {}", color(&p.color, 1.0), num(p.glow));
                }
                sub.push('\n');
                id
            })
            .clone();
        // (a capsule's round ends would be squashed by stretching: it gets a mesh made to measure)
        let (mesh, scale) = if p.shape == "capsule" {
            let key = format!("{} {}", p.size[0], p.size[1]);
            let next = capsules.len() + 1;
            let id = capsules
                .entry(key)
                .or_insert_with(|| {
                    let id = format!("capsule_{}", next);
                    let _ = writeln!(sub, "[sub_resource type=\"CapsuleMesh\" id=\"{}\"]\nradius = {}\nheight = {}\n", id, num(p.size[0] / 2.0), num(p.size[1].max(p.size[0])));
                    id
                })
                .clone();
            (id, [1.0; 3])
        } else {
            (if ["cylinder", "sphere"].contains(&p.shape.as_str()) { p.shape.clone() } else { "box".to_string() }, p.size)
        };
        let _ = writeln!(nodes, "[node name=\"{}\" type=\"MeshInstance3D\" parent=\"Parts\"]\ntransform = {}\nmesh = SubResource(\"{}\")\nsurface_material_override/0 = SubResource(\"{}\")", node, transform(turn(p.rot), scale, p.at), mesh, material);
        if !p.shadow {
            nodes.push_str("cast_shadow = 0\n");
        }
        nodes.push('\n');
    }

    nodes.push_str("[node name=\"Lights\" type=\"Node3D\" parent=\".\"]\n\n");
    for l in &place.lights {
        if l.kind == "sun" {
            let node = unique("Sun".to_string());
            let _ = writeln!(
                nodes,
                "[node name=\"{}\" type=\"DirectionalLight3D\" parent=\"Lights\"]\ntransform = {}\nlight_color = {}\nlight_energy = {}\nshadow_enabled = {}\n",
                node,
                transform(looking_in(l.at), [1.0; 3], l.at),
                color(&l.color, 1.0),
                num((l.power / 9000.0).clamp(0.02, 8.0)),
                l.shadows
            );
        } else {
            let node = unique("Lamp".to_string());
            let _ = writeln!(
                nodes,
                "[node name=\"{}\" type=\"OmniLight3D\" parent=\"Lights\"]\ntransform = {}\nlight_color = {}\nlight_energy = {}\nomni_range = {}\nomni_attenuation = 2.0\nshadow_enabled = {}\n",
                node,
                transform(UPRIGHT, [1.0; 3], l.at),
                color(&l.color, 1.0),
                // (falling off with the square of the distance, as the game's lamps do)
                num((l.power / 40_000.0).clamp(0.05, 64.0)),
                num(if l.range > 0.0 { l.range } else { 30.0 }),
                l.shadows
            );
        }
    }

    // what the player walks around: the edge of the floor, and everything in the way on it
    nodes.push_str("[node name=\"Collision\" type=\"StaticBody3D\" parent=\".\"]\n\n");
    let mut shapes = 0;
    let mut wall = |nodes: &mut String, sub: &mut String, label: &str, centre: [f32; 2], half: [f32; 2]| {
        shapes += 1;
        let _ = writeln!(sub, "[sub_resource type=\"BoxShape3D\" id=\"shape_{}\"]\nsize = Vector3({}, 4, {})\n", shapes, num(half[0] * 2.0), num(half[1] * 2.0));
        let _ = writeln!(nodes, "[node name=\"{}\" type=\"CollisionShape3D\" parent=\"Collision\"]\ntransform = {}\nshape = SubResource(\"shape_{}\")\n", label, transform(UPRIGHT, [1.0; 3], [centre[0], place.spawn[1] + 2.0, centre[1]]), shapes);
    };
    let [min, max] = place.floor;
    let (mid, half) = ([(min[0] + max[0]) / 2.0, (min[1] + max[1]) / 2.0], [(max[0] - min[0]) / 2.0, (max[1] - min[1]) / 2.0]);
    wall(&mut nodes, &mut sub, "EdgeWest", [min[0] - 0.25, mid[1]], [0.25, half[1] + 0.5]);
    wall(&mut nodes, &mut sub, "EdgeEast", [max[0] + 0.25, mid[1]], [0.25, half[1] + 0.5]);
    wall(&mut nodes, &mut sub, "EdgeNorth", [mid[0], min[1] - 0.25], [half[0] + 0.5, 0.25]);
    wall(&mut nodes, &mut sub, "EdgeSouth", [mid[0], max[1] + 0.25], [half[0] + 0.5, 0.25]);
    for (i, (centre, half)) in place.obstacles().into_iter().enumerate() {
        wall(&mut nodes, &mut sub, &format!("Solid_{}", i + 1), centre, half);
    }

    format!("[gd_scene format=3]\n\n{}\n{}{}", ext, sub, nodes)
}

/// The story as the Godot game reads it: the editor's own document, with
/// each character's look filled in (their portrait's colours and their own changes).
pub fn story_json(doc: &Doc, mode: Mode) -> Value {
    let mut v = serde_json::to_value(doc).unwrap_or_else(|_| json!({}));
    match mode {
        Mode::Story => {}
        Mode::Flat => {
            v["render"] = json!("");
            v["locales"] = json!({});
        }
        Mode::Full => {
            v["render"] = json!("3d");
            v["locales"] = json!({});
            // a cut-out is a picture: in a game that is all 3D, whoever has a portrait and no model stands as a figure
            for c in v["characters"].as_array_mut().into_iter().flatten() {
                if c["model"].as_str().unwrap_or("").is_empty() && !c["sprite"].as_str().unwrap_or("").is_empty() {
                    c["model"] = json!("figure");
                    c["model_scale"] = json!(0.0);
                }
            }
        }
    }
    if let Some(list) = v["characters"].as_array_mut() {
        for (c, def) in list.iter_mut().zip(&doc.characters) {
            let mut look = art::look_defaults(&def.sprite);
            look.extend(def.look.clone());
            c["look_all"] = json!(look);
        }
    }
    v
}

fn put(path: &Path, bytes: &[u8], force: bool, always: bool, report: &mut Report, label: &str) -> Result<(), String> {
    if path.exists() && !force && !always {
        report.kept.push(label.to_string());
        return Ok(());
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {}", dir.display(), e))?;
    }
    std::fs::write(path, bytes).map_err(|e| format!("{}: {}", path.display(), e))?;
    report.written.push(label.to_string());
    Ok(())
}

/// Write (or refresh) a Godot project for a story.
pub fn export(dir: &Path, doc: &Doc, story: Option<&Path>, art_dir: Option<&Path>, force: bool, mode: Mode) -> Result<Report, String> {
    let mut report = Report::default();
    for (name, text) in TEMPLATE {
        put(&dir.join(name), text.as_bytes(), force, false, &mut report, name)?;
    }
    // the story: always as it now stands
    let story_text = serde_json::to_string_pretty(&story_json(doc, mode)).map_err(|e| e.to_string())? + "\n";
    put(&dir.join("story/story.json"), story_text.as_bytes(), force, true, &mut report, "story/story.json")?;

    // pictures: every place the game paints, and every expression of everyone with a portrait
    let backgrounds = crate::painting::backgrounds_for(story);
    for name in backgrounds.iter().map(String::as_str) {
        let label = format!("art/bg/{}.png", name);
        let path = dir.join(&label);
        if path.exists() && !force {
            report.kept.push(label);
            continue;
        }
        let (canvas, _) = art::background_of(name, art_dir, story);
        put(&path, &canvas.png_bytes(false)?, true, true, &mut report, &label)?;
    }
    for c in doc.characters.iter().filter(|c| !c.sprite.is_empty()) {
        for expr in art::EXPRS {
            let label = format!("art/portraits/{}_{}.png", c.id, expr);
            let path = dir.join(&label);
            if path.exists() && !force {
                report.kept.push(label);
                continue;
            }
            put(&path, &art::sprite(c, expr, art_dir).png_bytes(true)?, true, true, &mut report, &label)?;
        }
    }

    // places: the story's own where it has one, else the game's (none at all for a game that is all 2D)
    let world = story.map(places::world_dir).filter(|d| d.is_dir() && mode != Mode::Flat);
    // the models the project will have: the story's, and any already put there by hand
    let listed = |d: PathBuf| -> Vec<String> { std::fs::read_dir(d).into_iter().flatten().filter_map(|e| e.ok()).map(|e| e.file_name().to_string_lossy().to_string()).filter(|f| places::model_file(f)).collect() };
    let mut models_there = world.as_ref().map_or(Vec::new(), |w| listed(w.join("models")));
    models_there.extend(listed(dir.join("models")));
    for name in backgrounds.iter().map(String::as_str).filter(|_| mode != Mode::Flat) {
        if let Some(place) = places::load(world.as_deref(), name) {
            let label = format!("places/{}.tscn", name);
            let missing: Vec<&str> = place.parts.iter().filter(|p| p.shape == "model" && !models_there.contains(&p.file)).map(|p| p.file.as_str()).collect();
            if !missing.is_empty() {
                report.notes.push(format!("{}: left out {} part(s) whose model file is not in the story's world/models folder ({})", name, missing.len(), missing.join(", ")));
            }
            put(&dir.join(&label), place_scene(name, &place, &models_there).as_bytes(), force, false, &mut report, &label)?;
        }
    }

    // model files and sound the story has
    let copy_all = |from: PathBuf, to: &str, keep: &dyn Fn(&str) -> bool, report: &mut Report| -> Result<(), String> {
        for e in std::fs::read_dir(&from).into_iter().flatten().filter_map(|e| e.ok()).filter(|e| e.path().is_file()) {
            let file = e.file_name().to_string_lossy().to_string();
            if keep(&file) {
                let label = format!("{}/{}", to, file);
                let bytes = std::fs::read(e.path()).map_err(|e| e.to_string())?;
                put(&dir.join(&label), &bytes, force, false, report, &label)?;
            }
        }
        Ok(())
    };
    if let Some(world) = &world {
        // (with whatever sits beside them: a .gltf may keep its mesh and pictures in other files)
        copy_all(world.join("models"), "models", &|f| !f.starts_with('.'), &mut report)?;
    }
    if let Some(story) = story {
        let sounds = audio::audio_dir(story);
        let mut flac = 0;
        copy_all(
            sounds.clone(),
            "audio",
            &|f| {
                let ok = [".ogg", ".mp3", ".wav"].iter().any(|e| f.to_ascii_lowercase().ends_with(e));
                ok
            },
            &mut report,
        )?;
        for e in std::fs::read_dir(&sounds).into_iter().flatten().filter_map(|e| e.ok()) {
            if e.file_name().to_string_lossy().to_ascii_lowercase().ends_with(".flac") {
                flac += 1;
            }
        }
        // Godot does not play FLAC (what ComfyUI's audio savers write): such recordings go in as WAV, under the same names
        let mut unconverted = 0;
        for e in std::fs::read_dir(&sounds).into_iter().flatten().filter_map(|e| e.ok()) {
            let file = e.file_name().to_string_lossy().to_string();
            let Some(stem) = file.strip_suffix(".flac").or_else(|| file.strip_suffix(".FLAC")) else { continue };
            let label = format!("audio/{}.wav", stem);
            let path = dir.join(&label);
            if path.exists() && !force {
                report.kept.push(label);
                continue;
            }
            match to_wav(&e.path()) {
                Ok(bytes) => put(&path, &bytes, true, true, &mut report, &label)?,
                Err(_) => unconverted += 1,
            }
        }
        let _ = flac;
        if unconverted > 0 {
            report.notes.push(format!("{} FLAC recording(s) could not be converted for Godot (which does not play FLAC): convert them to Ogg Vorbis, MP3 or WAV under the same names", unconverted));
        }
    }
    // a character's model that is not there shows as a cut-out
    for c in doc.characters.iter().filter(|c| mode != Mode::Flat && places::model_file(&c.model)) {
        if !dir.join("models").join(&c.model).is_file() {
            report.notes.push(format!("{}'s model '{}' is not in the story's world/models folder: they will appear as a cut-out", c.id, c.model));
        }
    }
    // a note on what this is and how to open it, with anything worth knowing about this export
    let mut readme = format!(
        "{title}\n\nA Godot 4 project, exported from the story editor.\n\nShows: {how}.\n\nOpen it with Godot 4.3 or later: start Godot, choose Import, and pick project.godot\nin this folder (or: godot --path . --editor). Press F5 to play.\n\nThe story is story/story.json, with every scene and choice. Pictures are in art/,\n3D places are scenes in places/, sound in audio/, model files in models/.\nEdit the pictures and places here in Godot; edit the story in the story editor\nand export again (pictures and places changed here are kept unless you ask\nfor them to be written again).\n",
        title = if doc.title.is_empty() { "Untitled story" } else { &doc.title },
        how = mode.describe()
    );
    if !report.notes.is_empty() {
        readme.push_str("\nNotes from this export:\n");
        for n in &report.notes {
            readme.push_str(&format!("- {}\n", n));
        }
    }
    put(&dir.join("README.txt"), readme.as_bytes(), true, true, &mut report, "README.txt")?;
    Ok(report)
}

/// A recording as 16-bit PCM WAV, for an engine that does not read its format.
#[cfg(feature = "audio")]
pub fn to_wav(path: &Path) -> Result<Vec<u8>, String> {
    use rodio::Source;
    let file = std::fs::File::open(path).map_err(|e| format!("{}: {}", path.display(), e))?;
    let decoder = rodio::Decoder::new(std::io::BufReader::new(file)).map_err(|e| format!("{}: {}", path.display(), e))?;
    let (channels, rate) = (decoder.channels(), decoder.sample_rate());
    let samples: Vec<i16> = decoder.collect();
    let data_len = (samples.len() * 2) as u32;
    let mut out = Vec::with_capacity(44 + samples.len() * 2);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&channels.to_le_bytes());
    out.extend_from_slice(&rate.to_le_bytes());
    out.extend_from_slice(&(rate * channels as u32 * 2).to_le_bytes());
    out.extend_from_slice(&(channels * 2).to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for s in samples {
        out.extend_from_slice(&s.to_le_bytes());
    }
    Ok(out)
}

#[cfg(not(feature = "audio"))]
pub fn to_wav(_path: &Path) -> Result<Vec<u8>, String> {
    Err("this build has no audio decoder (it was built without the 'audio' feature)".into())
}

/// Everything in a folder as one zip file (stored, not compressed: the
/// pictures and recordings are compressed already), under `prefix/`.
/// Godot's own cache folder (.godot) is left out.
pub fn zip_dir(dir: &Path, prefix: &str) -> Result<Vec<u8>, String> {
    let mut files = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let mut entries: Vec<_> = std::fs::read_dir(&d).map_err(|e| format!("{}: {}", d.display(), e))?.filter_map(|e| e.ok()).collect();
        entries.sort_by_key(|e| e.file_name());
        for e in entries {
            let path = e.path();
            if e.file_name() == ".godot" {
                continue;
            }
            if path.is_dir() {
                stack.push(path);
            } else if path.is_file() {
                files.push(path);
            }
        }
    }
    files.sort();
    let mut out = Vec::new();
    let mut central = Vec::new();
    let (time, date) = (0u16, (0 << 9) | (1 << 5) | 1u16);   // 1980-01-01, the earliest a zip can say
    for path in &files {
        let rel = path.strip_prefix(dir).map_err(|e| e.to_string())?.to_string_lossy().replace('\\', "/");
        let name = format!("{}/{}", prefix, rel);
        let data = std::fs::read(path).map_err(|e| format!("{}: {}", path.display(), e))?;
        if data.len() > u32::MAX as usize || out.len() > u32::MAX as usize {
            return Err("the game is too large for one zip file (4 GB)".into());
        }
        let crc = crate::comfy::crc32(&data);
        let offset = out.len() as u32;
        let fixed = |buf: &mut Vec<u8>, sig: u32, central_entry: bool| {
            buf.extend_from_slice(&sig.to_le_bytes());
            if central_entry {
                buf.extend_from_slice(&20u16.to_le_bytes());   // made by
            }
            buf.extend_from_slice(&20u16.to_le_bytes());   // needed to extract
            buf.extend_from_slice(&0x0800u16.to_le_bytes());   // names in UTF-8
            buf.extend_from_slice(&0u16.to_le_bytes());   // stored
            buf.extend_from_slice(&time.to_le_bytes());
            buf.extend_from_slice(&date.to_le_bytes());
            buf.extend_from_slice(&crc.to_le_bytes());
            buf.extend_from_slice(&(data.len() as u32).to_le_bytes());
            buf.extend_from_slice(&(data.len() as u32).to_le_bytes());
            buf.extend_from_slice(&(name.len() as u16).to_le_bytes());
            buf.extend_from_slice(&0u16.to_le_bytes());   // extra
        };
        fixed(&mut out, 0x0403_4b50, false);
        out.extend_from_slice(name.as_bytes());
        out.extend_from_slice(&data);
        fixed(&mut central, 0x0201_4b50, true);
        central.extend_from_slice(&0u16.to_le_bytes());   // comment
        central.extend_from_slice(&0u16.to_le_bytes());   // disk
        central.extend_from_slice(&0u16.to_le_bytes());   // internal attributes
        central.extend_from_slice(&0u32.to_le_bytes());   // external attributes
        central.extend_from_slice(&offset.to_le_bytes());
        central.extend_from_slice(name.as_bytes());
    }
    if files.len() > u16::MAX as usize {
        return Err("too many files for one zip file".into());
    }
    let start = out.len() as u32;
    out.extend_from_slice(&central);
    out.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&(files.len() as u16).to_le_bytes());
    out.extend_from_slice(&(files.len() as u16).to_le_bytes());
    out.extend_from_slice(&(central.len() as u32).to_le_bytes());
    out.extend_from_slice(&start.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every `SubResource("x")` and `ExtResource("x")` a scene uses is declared in it, once.
    fn check_scene(text: &str) {
        assert!(text.starts_with("[gd_scene"), "no header");
        let ids = |tag: &str| -> Vec<String> {
            text.lines().filter(|l| l.starts_with(tag)).filter_map(|l| l.split(" id=\"").nth(1)).map(|r| r.split('"').next().unwrap().to_string()).collect()
        };
        let (subs, exts) = (ids("[sub_resource"), ids("[ext_resource"));
        for (kind, declared) in [("SubResource(\"", &subs), ("ExtResource(\"", &exts)] {
            let mut seen = std::collections::HashSet::new();
            for id in declared {
                assert!(seen.insert(id), "{} declared twice", id);
            }
            for used in text.split(kind).skip(1).map(|r| r.split('"').next().unwrap()) {
                assert!(declared.iter().any(|d| d == used), "{}{}\") is used but not declared", kind, used);
            }
        }
        // one root, every other node under a parent that exists, no two alike under one parent
        let mut paths = std::collections::HashSet::new();
        let mut roots = 0;
        for line in text.lines().filter(|l| l.starts_with("[node ")) {
            let name = line.split("name=\"").nth(1).unwrap().split('"').next().unwrap();
            assert!(!name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'), "bad node name '{}'", name);
            match line.split("parent=\"").nth(1).map(|r| r.split('"').next().unwrap()) {
                None => roots += 1,
                Some(parent) => {
                    assert!(parent == "." || paths.contains(parent), "parent '{}' of '{}' does not exist yet", parent, name);
                    let path = if parent == "." { name.to_string() } else { format!("{}/{}", parent, name) };
                    assert!(paths.insert(path.clone()), "two nodes at '{}'", path);
                }
            }
        }
        assert_eq!(roots, 1, "a scene has one root");
        // every value is something Godot's text format has, and no number is unprintable
        assert!(!text.contains("NaN") && !text.contains("inf"), "a number that is not one");
        for line in text.lines().filter(|l| l.contains("Transform3D(")) {
            let inside = line.split("Transform3D(").nth(1).unwrap().trim_end_matches(')');
            assert_eq!(inside.split(", ").count(), 12, "a transform has twelve numbers: {}", line);
            assert!(inside.split(", ").all(|n| n.parse::<f32>().is_ok()), "{}", line);
        }
    }

    #[test]
    fn every_place_becomes_a_well_formed_godot_scene() {
        for name in art::BACKGROUNDS {
            let place = places::builtin(name).unwrap();
            let scene = place_scene(name, &place, &[]);
            check_scene(&scene);
            for marker in ["Spawn", "CastLeft", "CastCentre", "CastRight", "Air", "Parts", "Lights", "Collision", "EdgeWest"] {
                assert!(scene.contains(&format!("[node name=\"{}\"", marker)), "{} has no {}", name, marker);
            }
            let meshes = scene.matches("type=\"MeshInstance3D\"").count();
            assert_eq!(meshes, place.parts.len(), "{}: every part is a mesh", name);
            assert_eq!(scene.matches("Light3D\" parent=\"Lights\"").count(), place.lights.len(), "{}: every light", name);
            assert_eq!(scene.matches("type=\"CollisionShape3D\"").count(), 4 + place.obstacles().len(), "{}: the edges and what is in the way", name);
        }
    }

    #[test]
    fn transforms_are_written_as_godot_reads_them() {
        // unturned: the scale down the diagonal, then where it is
        assert_eq!(transform(UPRIGHT, [2.0, 3.0, 4.0], [1.0, 0.5, -2.0]), "Transform3D(2, 0, 0, 0, 3, 0, 0, 0, 4, 1, 0.5, -2)");
        // a quarter turn about y sends x to -z and z to x (rows of the matrix whose columns are the turned axes)
        assert_eq!(transform(turn([0.0, 90.0, 0.0]), [1.0; 3], [0.0; 3]), "Transform3D(0, 0, 1, 0, 1, 0, -1, 0, 0, 0, 0, 0)");
        // a light high to one side looks at the middle: its -z points from where it is to the origin
        let b = looking_in([0.0, 10.0, 10.0]);
        let back = [b[0][2], b[1][2], b[2][2]];
        assert!((back[1] - 0.7071).abs() < 1e-3 && (back[2] - 0.7071).abs() < 1e-3 && back[0].abs() < 1e-6, "{:?}", back);
        // straight overhead is still a proper turn
        let down = looking_in([0.0, 5.0, 0.0]);
        assert!(down.iter().flatten().all(|v| v.is_finite()) && (down[1][2] - 1.0).abs() < 1e-5);
        assert_eq!(num(0.1 + 0.2), "0.3");
        assert_eq!(num(-0.0), "0");
    }

    #[test]
    fn a_story_exports_as_a_godot_project() {
        let dir = std::env::temp_dir().join(format!("below-stairs-godot-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let (doc, _) = Doc::parse(crate::BUILT_IN_STORY);
        let report = export(&dir, &doc, None, None, false, Mode::Story).unwrap();
        for file in ["project.godot", "main.tscn", "scripts/story_vm.gd", "story/story.json", "art/bg/pantry.png", "art/portraits/p_smile.png", "places/study.tscn"] {
            assert!(dir.join(file).is_file(), "{} was not written", file);
        }
        assert!(report.kept.is_empty() && report.written.len() > 60, "{:?}", report.kept);
        // the scenes that ship with the project hold together too
        for scene in ["main.tscn", "scenes/hud.tscn", "scenes/stage_2d.tscn", "scenes/player.tscn", "scenes/character_3d.tscn"] {
            check_scene(&std::fs::read_to_string(dir.join(scene)).unwrap());
        }
        // the story is the editor's document, with each look filled in
        let story: Value = serde_json::from_str(&std::fs::read_to_string(dir.join("story/story.json")).unwrap()).unwrap();
        assert_eq!(story["scenes"].as_array().unwrap().len(), doc.scenes.len());
        assert!(story["characters"][1]["look_all"]["skin"].as_str().unwrap().starts_with('#'));
        // what is there is the user's: a second export leaves a changed scene and picture alone, and refreshes the story
        std::fs::write(dir.join("places/study.tscn"), "mine").unwrap();
        std::fs::write(dir.join("story/story.json"), "old").unwrap();
        let again = export(&dir, &doc, None, None, false, Mode::Story).unwrap();
        assert_eq!(std::fs::read_to_string(dir.join("places/study.tscn")).unwrap(), "mine");
        assert!(std::fs::read_to_string(dir.join("story/story.json")).unwrap().len() > 1000);
        assert_eq!(again.written, vec!["story/story.json".to_string(), "README.txt".to_string()]);
        // ... unless told to
        export(&dir, &doc, None, None, true, Mode::Story).unwrap();
        assert!(std::fs::read_to_string(dir.join("places/study.tscn")).unwrap().starts_with("[gd_scene"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_game_can_be_all_2d_or_all_3d() {
        let (doc, _) = Doc::parse(crate::BUILT_IN_STORY);
        // all 2D: nothing is 3D, and no places are written
        let flat = story_json(&doc, Mode::Flat);
        assert_eq!((flat["render"].as_str(), flat["locales"].as_object().map(|m| m.len())), (Some(""), Some(0)));
        // all 3D: every place, and a figure for everyone with a portrait who has no model
        let mut with_model = doc.clone();
        with_model.characters[1].model = "own.glb".into();
        let full = story_json(&with_model, Mode::Full);
        assert_eq!(full["render"].as_str(), Some("3d"));
        for (c, def) in full["characters"].as_array().unwrap().iter().zip(&with_model.characters) {
            let want = if def.sprite.is_empty() { "" } else if def.model.is_empty() { "figure" } else { def.model.as_str() };
            assert_eq!(c["model"].as_str(), Some(want), "{}", def.id);
        }
        // as the story says: left as it is
        assert_eq!(story_json(&doc, Mode::Story)["render"], serde_json::to_value(&doc).unwrap()["render"]);
        let dir = std::env::temp_dir().join(format!("below-stairs-godot-2d-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        export(&dir, &doc, None, None, true, Mode::Flat).unwrap();
        assert!(!dir.join("places").exists() && dir.join("art/bg/pantry.png").is_file());
        assert!(std::fs::read_to_string(dir.join("README.txt")).unwrap().contains("everything 2D"));
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!((Mode::parse("2d"), Mode::parse("3d"), Mode::parse(""), Mode::parse("4d")), (Some(Mode::Flat), Some(Mode::Full), Some(Mode::Story), None));
    }

    #[test]
    fn a_folder_zips_into_something_a_zip_reader_takes() {
        let dir = std::env::temp_dir().join(format!("below-stairs-zip-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("art/bg")).unwrap();
        std::fs::create_dir_all(dir.join(".godot")).unwrap();
        std::fs::write(dir.join("project.godot"), "config_version=5\n").unwrap();
        std::fs::write(dir.join("art/bg/x.png"), [0u8, 1, 2, 3, 255]).unwrap();
        std::fs::write(dir.join(".godot/cache"), "not this").unwrap();
        let zip = zip_dir(&dir, "tale").unwrap();
        // read it back by its own directory: two entries, under the prefix, data where it says
        let end = zip.len() - 22;
        assert_eq!(&zip[end..end + 4], &0x0605_4b50u32.to_le_bytes());
        let count = u16::from_le_bytes([zip[end + 10], zip[end + 11]]);
        assert_eq!(count, 2);
        let mut at = u32::from_le_bytes(zip[end + 16..end + 20].try_into().unwrap()) as usize;
        let mut names = Vec::new();
        for _ in 0..count {
            assert_eq!(&zip[at..at + 4], &0x0201_4b50u32.to_le_bytes());
            let size = u32::from_le_bytes(zip[at + 24..at + 28].try_into().unwrap()) as usize;
            let len = u16::from_le_bytes([zip[at + 28], zip[at + 29]]) as usize;
            let local = u32::from_le_bytes(zip[at + 42..at + 46].try_into().unwrap()) as usize;
            let name = String::from_utf8(zip[at + 46..at + 46 + len].to_vec()).unwrap();
            let data = &zip[local + 30 + len..local + 30 + len + size];
            assert_eq!(crate::comfy::crc32(data), u32::from_le_bytes(zip[at + 16..at + 20].try_into().unwrap()));
            names.push(name);
            at += 46 + len;
        }
        assert_eq!(names, vec!["tale/art/bg/x.png".to_string(), "tale/project.godot".to_string()]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(feature = "audio")]
    #[test]
    fn recordings_godot_cannot_play_are_converted() {
        // (any recording the decoder reads: here a short WAV, as the converter cannot tell)
        let dir = std::env::temp_dir().join(format!("below-stairs-wav-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut wav = b"RIFF".to_vec();
        let pcm: Vec<u8> = (0..800i16).flat_map(|i| ((i % 50) * 400).to_le_bytes()).collect();
        wav.extend_from_slice(&(36 + pcm.len() as u32).to_le_bytes());
        wav.extend_from_slice(b"WAVEfmt ");
        wav.extend_from_slice(&[16, 0, 0, 0, 1, 0, 1, 0]);
        wav.extend_from_slice(&8000u32.to_le_bytes());
        wav.extend_from_slice(&16000u32.to_le_bytes());
        wav.extend_from_slice(&[2, 0, 16, 0]);
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&(pcm.len() as u32).to_le_bytes());
        wav.extend_from_slice(&pcm);
        std::fs::write(dir.join("in.wav"), &wav).unwrap();
        let out = to_wav(&dir.join("in.wav")).unwrap();
        assert_eq!(&out[..4], b"RIFF");
        assert_eq!(u32::from_le_bytes(out[24..28].try_into().unwrap()), 8000);
        assert_eq!(out.len(), wav.len());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
