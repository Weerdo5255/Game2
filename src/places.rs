//! 3D places as data: what a place is made of, where you may walk in it
//! and where the cast stands. The game's own places are files in
//! `assets/places/`; a story can carry its own version of any of them in a
//! `world` folder beside it (`world/<place>.json`), made in the editor's
//! World view. Nothing here needs the 3D engine: the editor serves and
//! checks these files in any build.

#![cfg_attr(not(feature = "world3d"), allow(dead_code))]

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// The most parts and lights a place may have (a file from elsewhere is cut down to this).
pub const MAX_PARTS: usize = 6000;
pub const MAX_LIGHTS: usize = 64;
pub const SHAPES: [&str; 5] = ["box", "cylinder", "sphere", "capsule", "model"];
/// How wide a walker is, for keeping out of walls and furniture.
pub const BODY: f32 = 0.32;
pub const EYE: f32 = 1.62;

fn one() -> f32 {
    1.0
}
fn yes() -> bool {
    true
}
fn white() -> String {
    "#ffffff".into()
}

/// One solid piece of a place.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Part {
    /// "box", "cylinder", "sphere", "capsule" (upright unless turned), or
    /// "model" (a glTF file from the story's `world/models` folder).
    pub shape: String,
    /// Its middle.
    pub at: [f32; 3],
    /// Turned by this many degrees about x, y and z (y first, then x, then z).
    #[serde(default, skip_serializing_if = "is_zero3")]
    pub rot: [f32; 3],
    /// Width, height and depth of the box it fits in.
    pub size: [f32; 3],
    #[serde(default = "white")]
    pub color: String,
    /// How strongly it shines by itself (0: not at all).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub glow: f32,
    #[serde(default = "one", skip_serializing_if = "is_one")]
    pub rough: f32,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub metal: f32,
    /// Casts a shadow.
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    pub shadow: bool,
    /// You cannot walk through it.
    #[serde(default, skip_serializing_if = "is_false")]
    pub solid: bool,
    /// For a model: its file name.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub file: String,
    /// A name for the editor's list.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
}

fn is_zero(v: &f32) -> bool {
    *v == 0.0
}
fn is_one(v: &f32) -> bool {
    *v == 1.0
}
fn is_zero3(v: &[f32; 3]) -> bool {
    *v == [0.0; 3]
}
fn is_true(v: &bool) -> bool {
    *v
}
fn is_false(v: &bool) -> bool {
    !*v
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Light {
    /// "point" (a lamp at `at`) or "sun" (light from far off, arriving from `at` towards the middle).
    pub kind: String,
    pub at: [f32; 3],
    #[serde(default = "white")]
    pub color: String,
    /// Lumens for a lamp, lux for a sun.
    pub power: f32,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub range: f32,
    #[serde(default, skip_serializing_if = "is_false")]
    pub shadows: bool,
}

/// Something you walk around that is not one of the parts: a rectangle on the floor.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Solid {
    pub at: [f32; 2],
    pub half: [f32; 2],
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Air {
    /// The colour of the sky, and of the distance.
    pub sky: String,
    pub ambient: String,
    pub brightness: f32,
    /// Where the distance begins to fade into the sky, and where it is gone.
    pub fog: [f32; 2],
}

impl Default for Air {
    fn default() -> Self {
        Air { sky: "#8fb4e6".into(), ambient: "#ffffff".into(), brightness: 400.0, fog: [60.0, 400.0] }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PlaceData {
    #[serde(default = "version")]
    pub version: u32,
    #[serde(default)]
    pub air: Air,
    /// Where you start: your feet.
    pub spawn: [f32; 3],
    /// Which way you face then, in degrees (0 looks along -z, positive turns left).
    #[serde(default)]
    pub yaw: f32,
    /// The walkable rectangle: least x and z, greatest x and z.
    pub floor: [[f32; 2]; 2],
    /// People drift here rather than stand.
    #[serde(default)]
    pub floating: bool,
    /// Where the left, centre and right of the cast stand (their feet).
    pub anchors: [[f32; 3]; 3],
    #[serde(default)]
    pub solids: Vec<Solid>,
    #[serde(default)]
    pub parts: Vec<Part>,
    #[serde(default)]
    pub lights: Vec<Light>,
}

fn version() -> u32 {
    1
}

/// "#rrggbb" as red, green and blue from 0 to 1; white if it is not one.
pub fn rgb(hex: &str) -> [f32; 3] {
    let h = hex.trim().trim_start_matches('#');
    match (h.len(), u32::from_str_radix(h, 16)) {
        (6, Ok(n)) => [((n >> 16) & 255) as f32 / 255.0, ((n >> 8) & 255) as f32 / 255.0, (n & 255) as f32 / 255.0],
        _ => [1.0, 1.0, 1.0],
    }
}

fn clean_color(c: &mut String) {
    let h = c.trim().trim_start_matches('#').to_lowercase();
    *c = if h.len() == 6 && h.chars().all(|c| c.is_ascii_hexdigit()) { format!("#{}", h) } else { "#ffffff".into() };
}

fn fin(v: f32, lo: f32, hi: f32, or: f32) -> f32 {
    if v.is_finite() {
        v.clamp(lo, hi)
    } else {
        or
    }
}

/// A plain file name for a model: letters, digits, `_`, `-` and one of the endings we load.
pub fn model_file(name: &str) -> bool {
    let ok = |c: char| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.';
    !name.starts_with('.') && name.len() <= 80 && name.chars().all(ok) && [".glb", ".gltf"].iter().any(|e| name.to_ascii_lowercase().ends_with(e) && name.len() > e.len())
}

impl PlaceData {
    /// An empty room to start from: a floor, a light, and room to walk.
    pub fn blank() -> PlaceData {
        PlaceData {
            version: 1,
            air: Air::default(),
            spawn: [0.0, 0.0, 3.0],
            yaw: 0.0,
            floor: [[-6.0, -6.0], [6.0, 6.0]],
            floating: false,
            anchors: [[-1.25, 0.0, 0.0], [0.0, 0.0, -0.25], [1.25, 0.0, 0.0]],
            solids: Vec::new(),
            parts: vec![Part { shape: "box".into(), at: [0.0, -0.1, 0.0], rot: [0.0; 3], size: [12.0, 0.2, 12.0], color: "#8a8478".into(), glow: 0.0, rough: 0.9, metal: 0.0, shadow: true, solid: false, file: String::new(), name: "floor".into() }],
            lights: vec![Light { kind: "sun".into(), at: [4.0, 9.0, 6.0], color: "#fff4e0".into(), power: 9000.0, range: 0.0, shadows: true }],
        }
    }

    /// Bring a description from anywhere within what the game will build:
    /// finite numbers, sizes and counts within reason, known shapes, plain file names.
    pub fn clean(&mut self) {
        const FAR: f32 = 5000.0;
        self.version = 1;
        let point = |p: &mut [f32; 3]| p.iter_mut().for_each(|v| *v = fin(*v, -FAR, FAR, 0.0));
        clean_color(&mut self.air.sky);
        clean_color(&mut self.air.ambient);
        self.air.brightness = fin(self.air.brightness, 0.0, 20_000.0, 400.0);
        self.air.fog[0] = fin(self.air.fog[0], 0.0, 2.0 * FAR, 60.0);
        self.air.fog[1] = fin(self.air.fog[1], self.air.fog[0] + 1.0, 4.0 * FAR, self.air.fog[0] + 300.0);
        point(&mut self.spawn);
        self.yaw = fin(self.yaw, -360.0, 360.0, 0.0);
        for corner in &mut self.floor {
            corner.iter_mut().for_each(|v| *v = fin(*v, -FAR, FAR, 0.0));
        }
        // least first, and room to stand
        for axis in 0..2 {
            if self.floor[0][axis] > self.floor[1][axis] {
                let (a, b) = (self.floor[0][axis], self.floor[1][axis]);
                self.floor[0][axis] = b;
                self.floor[1][axis] = a;
            }
            if self.floor[1][axis] - self.floor[0][axis] < 4.0 * BODY {
                let mid = (self.floor[0][axis] + self.floor[1][axis]) / 2.0;
                self.floor[0][axis] = mid - 2.0 * BODY;
                self.floor[1][axis] = mid + 2.0 * BODY;
            }
        }
        self.anchors.iter_mut().for_each(point);
        self.solids.truncate(MAX_PARTS);
        for s in &mut self.solids {
            s.at.iter_mut().for_each(|v| *v = fin(*v, -FAR, FAR, 0.0));
            s.half.iter_mut().for_each(|v| *v = fin(*v, 0.01, FAR, 0.5));
        }
        self.parts.truncate(MAX_PARTS);
        for p in &mut self.parts {
            if !SHAPES.contains(&p.shape.as_str()) {
                p.shape = "box".into();
            }
            point(&mut p.at);
            p.rot.iter_mut().for_each(|v| *v = fin(*v, -360.0, 360.0, 0.0));
            p.size.iter_mut().for_each(|v| *v = fin(*v, 0.005, 2.0 * FAR, 1.0));
            clean_color(&mut p.color);
            p.glow = fin(p.glow, 0.0, 100.0, 0.0);
            p.rough = fin(p.rough, 0.0, 1.0, 1.0);
            p.metal = fin(p.metal, 0.0, 1.0, 0.0);
            if p.shape != "model" || !model_file(&p.file) {
                p.file.clear();
                if p.shape == "model" {
                    p.shape = "box".into();
                }
            }
            p.name = p.name.chars().filter(|c| !c.is_control()).take(60).collect();
        }
        self.lights.truncate(MAX_LIGHTS);
        for l in &mut self.lights {
            if l.kind != "sun" {
                l.kind = "point".into();
            }
            point(&mut l.at);
            clean_color(&mut l.color);
            l.power = fin(l.power, 0.0, 1.0e9, 100_000.0);
            l.range = fin(l.range, 0.0, FAR, 0.0);
        }
    }

    /// Every rectangle you walk around: the listed ones, and the footprint of each part marked solid.
    pub fn obstacles(&self) -> Vec<([f32; 2], [f32; 2])> {
        let mut out: Vec<([f32; 2], [f32; 2])> = self.solids.iter().map(|s| (s.at, s.half)).collect();
        for p in self.parts.iter().filter(|p| p.solid) {
            // the box the part fits in once turned (about y, then x, then z, as it is drawn)
            let [x, y, z] = p.rot.map(f32::to_radians);
            let (sx, cx, sy, cy, sz, cz) = (x.sin(), x.cos(), y.sin(), y.cos(), z.sin(), z.cos());
            let rows = [[cy * cz + sy * sx * sz, -cy * sz + sy * sx * cz, sy * cx], [cx * sz, cx * cz, -sx], [-sy * cz + cy * sx * sz, sy * sz + cy * sx * cz, cy * cx]];
            // (a capsule is as deep as it is wide, whatever its third size says)
            let size = if p.shape == "capsule" { [p.size[0], p.size[1], p.size[0]] } else { p.size };
            let half = rows.map(|r| (0..3).map(|k| r[k].abs() * size[k] / 2.0).sum::<f32>());
            // only what a walker would meet: something overhead, or under the floor, is not in the way
            if p.at[1] + half[1] <= self.spawn[1] + 0.05 || p.at[1] - half[1] >= self.spawn[1] + EYE + 0.2 {
                continue;
            }
            out.push(([p.at[0], p.at[2]], [half[0], half[2]]));
        }
        out
    }

    /// Move within the place: stay on its floor and slide around solid things.
    pub fn walk(&self, obstacles: &[([f32; 2], [f32; 2])], from: [f32; 2], step: [f32; 2]) -> [f32; 2] {
        let clamp = |p: [f32; 2]| [p[0].clamp(self.floor[0][0] + BODY, self.floor[1][0] - BODY), p[1].clamp(self.floor[0][1] + BODY, self.floor[1][1] - BODY)];
        let mut to = clamp([from[0] + step[0], from[1] + step[1]]);
        for _ in 0..2 {
            for (centre, half) in obstacles {
                let d = [to[0] - centre[0], to[1] - centre[1]];
                let reach = [half[0] + BODY, half[1] + BODY];
                if d[0].abs() < reach[0] && d[1].abs() < reach[1] {
                    // leave by the nearer side
                    if reach[0] - d[0].abs() < reach[1] - d[1].abs() {
                        to[0] = centre[0] + reach[0] * if d[0] < 0.0 { -1.0 } else { 1.0 };
                    } else {
                        to[1] = centre[1] + reach[1] * if d[1] < 0.0 { -1.0 } else { 1.0 };
                    }
                }
            }
        }
        let to = clamp(to);
        // pushed back into something by a wall (a thing standing close to one): stay put rather than pass through
        let inside = |p: [f32; 2]| obstacles.iter().any(|(centre, half)| (p[0] - centre[0]).abs() < half[0] + BODY - 1e-3 && (p[1] - centre[1]).abs() < half[1] + BODY - 1e-3);
        if inside(to) && !inside(from) {
            return from;
        }
        to
    }
}

/// The game's own build of a place, if it has one.
pub fn builtin(name: &str) -> Option<PlaceData> {
    let text = match name {
        "pantry" => include_str!("../assets/places/pantry.json"),
        "axis" => include_str!("../assets/places/axis.json"),
        "dock" => include_str!("../assets/places/dock.json"),
        "vista" => include_str!("../assets/places/vista.json"),
        "vista_night" => include_str!("../assets/places/vista_night.json"),
        "ballroom" => include_str!("../assets/places/ballroom.json"),
        "study" => include_str!("../assets/places/study.json"),
        _ => return None,
    };
    let mut place: PlaceData = serde_json::from_str(text).ok()?;
    place.clean();
    Some(place)
}

/// Where a story keeps its own places and models: `world` beside the story file.
pub fn world_dir(story: &Path) -> PathBuf {
    story.parent().unwrap_or(Path::new(".")).join("world")
}

/// The story's own version of a place, if it has made one.
pub fn custom(world: &Path, name: &str) -> Option<Result<PlaceData, String>> {
    let file = world.join(format!("{}.json", name));
    let text = std::fs::read_to_string(&file).ok()?;
    Some(serde_json::from_str::<PlaceData>(&text).map_err(|e| format!("{}: {}", file.display(), e)).map(|mut p| {
        p.clean();
        p
    }))
}

/// The place to build: the story's own if there is one (and it can be read), else the game's.
pub fn load(world: Option<&Path>, name: &str) -> Option<PlaceData> {
    match world.and_then(|w| custom(w, name)) {
        Some(Ok(place)) => Some(place),
        Some(Err(e)) => {
            eprintln!("{} (using the game's own {})", e, name);
            builtin(name)
        }
        None => builtin(name),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_place_the_game_paints_has_a_build() {
        for name in crate::art::BACKGROUNDS {
            let place = builtin(name).unwrap_or_else(|| panic!("{} has no 3D build", name));
            assert!(!place.parts.is_empty() && !place.lights.is_empty(), "{} is empty", name);
            // you start on the floor, clear of the furniture, and so do the cast
            let obstacles = place.obstacles();
            let start = [place.spawn[0], place.spawn[2]];
            assert_eq!(place.walk(&obstacles, start, [0.0, 0.0]), start, "{}: the start is inside something", name);
            // cleaning a good description changes nothing
            let mut again = place.clone();
            again.clean();
            assert_eq!(again, place, "{}", name);
        }
    }

    #[test]
    fn walking_stays_inside_and_goes_round_things() {
        let mut place = PlaceData::blank();
        place.floor = [[-4.0, -4.0], [4.0, 4.0]];
        place.solids.push(Solid { at: [0.0, 0.0], half: [1.0, 0.5] });
        let obstacles = place.obstacles();
        // walls
        let at = place.walk(&obstacles, [3.5, 0.0], [5.0, 0.0]);
        assert!((at[0] - (4.0 - BODY)).abs() < 1e-4);
        // straight at the table from the front: stopped at its edge, not inside it
        let mut at = [0.0, 3.0];
        for _ in 0..200 {
            at = place.walk(&obstacles, at, [0.0, -0.05]);
        }
        assert!((at[1] - (0.5 + BODY)).abs() < 1e-3, "{:?}", at);
        // at an angle: slides along it and gets past
        let mut at = [0.9, 3.0];
        for _ in 0..400 {
            at = place.walk(&obstacles, at, [0.03, -0.05]);
        }
        assert!(at[1] < -0.5, "{:?}", at);
        // a part marked solid is walked round too, by its footprint as turned
        let mut turned = PlaceData::blank();
        turned.parts.push(Part { shape: "box".into(), at: [0.0, 0.5, 0.0], rot: [0.0, 90.0, 0.0], size: [4.0, 1.0, 0.4], color: "#ffffff".into(), glow: 0.0, rough: 1.0, metal: 0.0, shadow: true, solid: true, file: String::new(), name: String::new() });
        let o = turned.obstacles();
        assert!((o[0].1[0] - 0.2).abs() < 1e-3 && (o[0].1[1] - 2.0).abs() < 1e-3, "{:?}", o);
        // a post laid on its side is in the way along its whole length
        turned.parts[1].rot = [0.0, 0.0, 90.0];
        turned.parts[1].size = [0.4, 4.0, 0.4];
        let o = turned.obstacles();
        assert!((o[0].1[0] - 2.0).abs() < 1e-3 && (o[0].1[1] - 0.2).abs() < 1e-3, "{:?}", o);
        // and something well overhead is not in the way at all
        turned.parts[1].at[1] = 40.0;
        assert!(turned.obstacles().is_empty());
    }

    #[test]
    fn a_description_from_anywhere_is_brought_within_bounds() {
        let odd = r##"{"spawn":[1e30,0,"x"],"floor":[[5,5],[-5,-5]],"anchors":[[0,0,0],[0,0,0],[0,0,0]]}"##;
        assert!(serde_json::from_str::<PlaceData>(odd).is_err(), "a wrong type is refused");
        let loose = r##"{"spawn":[1e30,0,0],"floor":[[5,5],[-5,-5]],"anchors":[[0,0,0],[0,0,0],[0,0,0]],
            "air":{"sky":"red","ambient":"#FFF","brightness":-5,"fog":[10,2]},
            "parts":[{"shape":"teapot","at":[0,0,0],"size":[0,-1,1e9],"color":"javascript:x","glow":1e9},
                     {"shape":"model","at":[0,0,0],"size":[1,1,1],"file":"../../etc/passwd"},
                     {"shape":"model","at":[0,0,0],"size":[1,1,1],"file":"urn.glb"}],
            "lights":[{"kind":"laser","at":[0,1,0],"power":-1}]}"##;
        let mut p: PlaceData = serde_json::from_str(loose).unwrap();
        p.clean();
        assert_eq!(p.spawn[0], 5000.0);
        assert_eq!(p.floor, [[-5.0, -5.0], [5.0, 5.0]]);
        assert_eq!((p.air.sky.as_str(), p.air.brightness, p.air.fog), ("#ffffff", 0.0, [10.0, 11.0]));
        assert_eq!((p.parts[0].shape.as_str(), p.parts[0].color.as_str(), p.parts[0].glow), ("box", "#ffffff", 100.0));
        assert!(p.parts[0].size.iter().all(|s| *s >= 0.005 && *s <= 10_000.0));
        assert_eq!((p.parts[1].shape.as_str(), p.parts[1].file.as_str()), ("box", ""), "a model outside the models folder is not a model");
        assert_eq!((p.parts[2].shape.as_str(), p.parts[2].file.as_str()), ("model", "urn.glb"));
        assert_eq!((p.lights[0].kind.as_str(), p.lights[0].power), ("point", 0.0));
        assert!(model_file("urn-2.glb") && !model_file("urn.exe") && !model_file(".glb") && !model_file("a/b.glb"));
    }
}
