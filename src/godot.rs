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
pub fn place_scene(name: &str, place: &PlaceData) -> String {
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
        "[sub_resource type=\"Environment\" id=\"air\"]\nbackground_mode = 1\nbackground_color = {sky}\nambient_light_source = 2\nambient_light_color = {amb}\nambient_light_energy = {energy}\nfog_enabled = true\nfog_mode = 1\nfog_light_color = {sky}\nfog_density = 1.0\nfog_depth_begin = {f0}\nfog_depth_end = {f1}\n",
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
                "[node name=\"{}\" type=\"OmniLight3D\" parent=\"Lights\"]\ntransform = {}\nlight_color = {}\nlight_energy = {}\nomni_range = {}\nshadow_enabled = {}\n",
                node,
                transform(UPRIGHT, [1.0; 3], l.at),
                color(&l.color, 1.0),
                num((l.power / 250_000.0).clamp(0.05, 16.0)),
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
pub fn story_json(doc: &Doc) -> Value {
    let mut v = serde_json::to_value(doc).unwrap_or_else(|_| json!({}));
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
pub fn export(dir: &Path, doc: &Doc, story: Option<&Path>, art_dir: Option<&Path>, force: bool) -> Result<Report, String> {
    let mut report = Report::default();
    for (name, text) in TEMPLATE {
        put(&dir.join(name), text.as_bytes(), force, false, &mut report, name)?;
    }
    // the story: always as it now stands
    let story_text = serde_json::to_string_pretty(&story_json(doc)).map_err(|e| e.to_string())? + "\n";
    put(&dir.join("story/story.json"), story_text.as_bytes(), force, true, &mut report, "story/story.json")?;

    // pictures: every place the game paints, and every expression of everyone with a portrait
    for name in art::BACKGROUNDS {
        let label = format!("art/bg/{}.png", name);
        let path = dir.join(&label);
        if path.exists() && !force {
            report.kept.push(label);
            continue;
        }
        let (canvas, _) = art::background(name, art_dir);
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

    // places: the story's own where it has one, else the game's
    let world = story.map(places::world_dir).filter(|d| d.is_dir());
    for name in art::BACKGROUNDS {
        if let Some(place) = places::load(world.as_deref(), name) {
            let label = format!("places/{}.tscn", name);
            put(&dir.join(&label), place_scene(name, &place).as_bytes(), force, false, &mut report, &label)?;
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
        copy_all(world.join("models"), "models", &|f| places::model_file(f), &mut report)?;
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
        if flac > 0 {
            report.notes.push(format!("{} recording(s) are FLAC, which Godot does not play: convert them to Ogg Vorbis, MP3 or WAV (the names stay the same)", flac));
        }
    }
    // a character's model that is not there shows as a cut-out
    for c in doc.characters.iter().filter(|c| places::model_file(&c.model)) {
        if !dir.join("models").join(&c.model).is_file() {
            report.notes.push(format!("{}'s model '{}' is not in the story's world/models folder: they will appear as a cut-out", c.id, c.model));
        }
    }
    Ok(report)
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
            let scene = place_scene(name, &place);
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
        let report = export(&dir, &doc, None, None, false).unwrap();
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
        let again = export(&dir, &doc, None, None, false).unwrap();
        assert_eq!(std::fs::read_to_string(dir.join("places/study.tscn")).unwrap(), "mine");
        assert!(std::fs::read_to_string(dir.join("story/story.json")).unwrap().len() > 1000);
        assert_eq!(again.written, vec!["story/story.json".to_string()]);
        // ... unless told to
        export(&dir, &doc, None, None, true).unwrap();
        assert!(std::fs::read_to_string(dir.join("places/study.tscn")).unwrap().starts_with("[gd_scene"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
