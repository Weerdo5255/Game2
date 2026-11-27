//! The visual story editor: a small local web server. The page it serves
//! (editor.html) edits a story as a graph; this side owns the files (a
//! library of independent stories), the authoritative parser and
//! validator, paints previews of the art, and runs ComfyUI jobs.
//!
//! It listens on 127.0.0.1 only, and refuses requests that did not come
//! from its own page, so other web sites cannot reach it through a browser.

use crate::art;
use crate::audio;
use crate::comfy::{self, Jobs, Overrides};
use crate::gfx::Canvas;
use crate::script::{valid_id, CharDef, Doc, Item, Problem, Scene, VarDef, OPS, SLOTS};
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const PAGE: &str = include_str!("editor.html");
const MAX_BODY: usize = 8 << 20;
/// An imported story may carry its pictures and recordings.
const MAX_IMPORT: usize = 512 << 20;
/// What an exported story file calls itself.
const EXPORT_FORMAT: &str = "below-stairs-story";
const EXPORT_VERSION: u64 = 1;

/// The stories the editor can open: the main one (normally
/// assets/story.txt) and any number in `<dir>/<id>/story.txt`. Each keeps
/// its enhanced art and art preferences in its own folder.
struct Library {
    main: PathBuf,
    dir: PathBuf,
}

impl Library {
    fn path(&self, id: &str) -> Result<PathBuf, String> {
        if id == "main" || id.is_empty() {
            return Ok(self.main.clone());
        }
        let ok = id.len() <= 64 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
        let path = self.dir.join(id).join("story.txt");
        if ok && path.is_file() {
            Ok(path)
        } else {
            Err(format!("there is no story called '{}'", id))
        }
    }

    fn list(&self) -> Vec<Value> {
        let mut ids = vec!["main".to_string()];
        let mut others: Vec<String> = std::fs::read_dir(&self.dir)
            .into_iter()
            .flatten()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().to_string())
            // "main" is reserved, and the main story may itself live in this folder
            .filter(|id| !id.starts_with('.') && id != "main" && self.path(id).map_or(false, |p| !same_file(&p, &self.main)))
            .collect();
        others.sort();
        ids.extend(others);
        ids.into_iter()
            .filter_map(|id| {
                let path = self.path(&id).ok()?;
                let (doc, _) = Doc::parse(&std::fs::read_to_string(&path).ok()?);
                let enhanced = std::fs::read_dir(comfy::enhanced_dir(&path)).map_or(0, |d| d.filter_map(|e| e.ok()).filter(|e| e.file_name().to_string_lossy().ends_with(".png")).count());
                Some(json!({
                    "id": id, "title": if doc.title.is_empty() { "Untitled story".to_string() } else { doc.title },
                    "path": path.display().to_string(), "scenes": doc.scenes.len(), "characters": doc.characters.len(), "enhanced": enhanced,
                }))
            })
            .collect()
    }

    /// A folder name for a new story with this title, not yet taken.
    fn new_id(&self, title: &str) -> String {
        let slug = slug(title);
        let mut id = slug.clone();
        let mut n = 1;
        while self.dir.join(&id).exists() {
            n += 1;
            id = format!("{}-{}", slug, n);
        }
        id
    }

    /// Add a story that came from somewhere else (an imported file).
    fn adopt(&self, doc: &Doc, name: &str) -> Result<(String, PathBuf), String> {
        let id = self.new_id(name);
        let folder = self.dir.join(&id);
        std::fs::create_dir_all(&folder).map_err(|e| format!("{}: {}", folder.display(), e))?;
        std::fs::write(folder.join("story.txt"), doc.to_text()).map_err(|e| e.to_string())?;
        Ok((id, folder))
    }

    /// Make a new story: a copy of an existing one, or a small starter.
    fn create(&self, title: &str, copy: Option<&str>) -> Result<String, String> {
        let title = title.trim();
        if title.is_empty() {
            return Err("a story needs a title".into());
        }
        let id = self.new_id(title);
        let mut doc = match copy {
            Some(from) => Doc::parse(&std::fs::read_to_string(self.path(from)?).map_err(|e| e.to_string())?).0,
            None => {
                let (built_in, _) = Doc::parse(crate::BUILT_IN_STORY);
                Doc {
                    header: built_in.header,
                    characters: vec![CharDef { id: "hero".into(), name: "Hero".into(), color: "#ffd27f".into(), sprite: "p".into(), ..Default::default() }],
                    vars: vec![VarDef { id: "resolve".into(), label: "Resolve".into(), good: true, character: "hero".into(), ..Default::default() }],
                    scenes: vec![Scene {
                        label: "start".into(),
                        chapter: String::new(),
                        pos: Some((0.0, 0.0)),
                        items: vec![
                            Item::Card { title: "Chapter One".into(), sub: String::new() },
                            Item::Bg { name: "vista".into() },
                            Item::Show { who: "hero".into(), expr: "neutral".into(), slot: "center".into() },
                            Item::Say { who: "n".into(), text: "Your story begins here.".into() },
                            Item::End { title: "The End".into(), text: "Every story needs one.".into() },
                        ],
                    }],
                    ..Default::default()
                }
            }
        };
        doc.title = title.to_string();
        doc.sanitize();
        let folder = self.dir.join(&id);
        std::fs::create_dir_all(&folder).map_err(|e| format!("{}: {}", folder.display(), e))?;
        std::fs::write(folder.join("story.txt"), doc.to_text()).map_err(|e| e.to_string())?;
        if let Some(from) = copy {
            // a copy brings its art, its sound and its 3D places along
            let source = self.path(from)?;
            for sub in ["", "models"] {
                let (src, target) = (crate::places::world_dir(&source).join(sub), folder.join("world").join(sub));
                for e in std::fs::read_dir(&src).into_iter().flatten().filter_map(|e| e.ok()).filter(|e| e.path().is_file()) {
                    let _ = std::fs::create_dir_all(&target).and_then(|_| std::fs::copy(e.path(), target.join(e.file_name())).map(|_| ()));
                }
            }
            for sub in ["", "samples"] {
                let (src, target) = (audio::audio_dir(&source).join(sub), folder.join("audio").join(sub));
                for e in std::fs::read_dir(&src).into_iter().flatten().filter_map(|e| e.ok()).filter(|e| e.path().is_file()) {
                    let _ = std::fs::create_dir_all(&target).and_then(|_| std::fs::copy(e.path(), target.join(e.file_name())).map(|_| ()));
                }
            }
            let _ = std::fs::copy(source.with_file_name("art.json"), folder.join("art.json"));
            let (src, target) = (crate::painting::dir(&source), folder.join("paintings"));
            for e in std::fs::read_dir(&src).into_iter().flatten().filter_map(|e| e.ok()).filter(|e| e.path().is_file()) {
                let _ = std::fs::create_dir_all(&target).and_then(|_| std::fs::copy(e.path(), target.join(e.file_name())).map(|_| ()));
            }
            if let Ok(entries) = std::fs::read_dir(comfy::enhanced_dir(&source)) {
                let target = folder.join("enhanced");
                for e in entries.filter_map(|e| e.ok()).filter(|e| e.path().is_file()) {
                    let _ = std::fs::create_dir_all(&target).and_then(|_| std::fs::copy(e.path(), target.join(e.file_name())).map(|_| ()));
                }
            }
        }
        Ok(id)
    }

    /// Take a story out of the library. Nothing is destroyed: its folder
    /// moves to `<dir>/.removed/`.
    fn remove(&self, id: &str) -> Result<(), String> {
        if id == "main" || id.is_empty() {
            return Err("the main story cannot be removed from the editor".into());
        }
        self.path(id)?;
        let bin = self.dir.join(".removed");
        std::fs::create_dir_all(&bin).map_err(|e| e.to_string())?;
        let mut target = bin.join(id);
        let mut n = 1;
        while target.exists() {
            n += 1;
            target = bin.join(format!("{}-{}", id, n));
        }
        std::fs::rename(self.dir.join(id), &target).map_err(|e| format!("could not move the story: {}", e))?;
        // when it was removed (moving a folder does not change its own date)
        let _ = std::fs::write(target.join("removed.txt"), now().to_string());
        Ok(())
    }
}

impl Library {
    /// The stories that were removed and can still be brought back.
    fn trash(&self) -> Vec<Value> {
        let bin = self.dir.join(".removed");
        let mut names: Vec<String> = std::fs::read_dir(&bin).into_iter().flatten().filter_map(|e| e.ok()).map(|e| e.file_name().to_string_lossy().to_string()).filter(|n| trash_name(n) && bin.join(n).join("story.txt").is_file()).collect();
        names.sort();
        names
            .into_iter()
            .map(|name| {
                let folder = bin.join(&name);
                let (doc, _) = Doc::parse(&std::fs::read_to_string(folder.join("story.txt")).unwrap_or_default());
                json!({ "name": name, "title": if doc.title.is_empty() { "Untitled story".to_string() } else { doc.title }, "scenes": doc.scenes.len(), "removed": std::fs::read_to_string(folder.join("removed.txt")).ok().and_then(|t| t.trim().parse::<u64>().ok()).unwrap_or_else(|| modified(&folder)), "bytes": dir_bytes(&folder) })
            })
            .collect()
    }

    /// Bring a removed story back into the library, under a free name.
    fn restore(&self, name: &str) -> Result<String, String> {
        let from = self.dir.join(".removed").join(name);
        if !trash_name(name) || !from.join("story.txt").is_file() {
            return Err(format!("there is no removed story called '{}'", name));
        }
        // a name the library can open: story ids are at most 64 characters
        let base: String = name.chars().take(58).collect();
        let mut id = base.clone();
        let mut n = 1;
        while id == "main" || self.dir.join(&id).exists() {
            n += 1;
            id = format!("{}-{}", base, n);
        }
        std::fs::rename(&from, self.dir.join(&id)).map_err(|e| format!("could not bring the story back: {}", e))?;
        let _ = std::fs::remove_file(self.dir.join(&id).join("removed.txt"));
        Ok(id)
    }

    /// Delete a removed story for good.
    fn purge(&self, name: &str) -> Result<(), String> {
        let folder = self.dir.join(".removed").join(name);
        if !trash_name(name) || !folder.join("story.txt").is_file() {
            return Err(format!("there is no removed story called '{}'", name));
        }
        std::fs::remove_dir_all(&folder).map_err(|e| format!("could not delete the story: {}", e))
    }
}

/// A folder name the library itself would have made.
fn trash_name(name: &str) -> bool {
    !name.is_empty() && name.len() <= 80 && !name.starts_with('.') && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// Size of a file or of everything in a folder (links are not followed).
fn dir_bytes(path: &Path) -> u64 {
    match std::fs::symlink_metadata(path) {
        Ok(m) if m.is_file() => m.len(),
        Ok(m) if m.is_dir() => std::fs::read_dir(path).into_iter().flatten().filter_map(|e| e.ok()).map(|e| dir_bytes(&e.path())).sum(),
        _ => 0,
    }
}

/// When a file or folder last changed, in seconds since 1970 (0 if unknown).
fn modified(path: &Path) -> u64 {
    std::fs::metadata(path).and_then(|m| m.modified()).ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_secs())
}

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// The marker beside a story that says it must not be changed.
fn lock_file(story: &Path) -> PathBuf {
    let mut p = story.to_path_buf().into_os_string();
    p.push(".locked");
    PathBuf::from(p)
}

/// Earlier versions of a story are kept beside it, in ".history" (or
/// ".history-<name>" for a story file not called story.txt, so that story
/// files sharing a folder each keep their own).
fn history_dir(story: &Path) -> PathBuf {
    let stem = story.file_stem().unwrap_or_default().to_string_lossy();
    let name = if stem == "story" { ".history".to_string() } else { format!(".history-{}", stem) };
    story.parent().unwrap_or(Path::new(".")).join(name)
}

/// The names given to snapshots, as typed (file names keep only a plain form).
fn snapshot_labels(story: &Path) -> serde_json::Map<String, Value> {
    std::fs::read_to_string(history_dir(story).join("labels.json")).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

/// How many automatic snapshots are kept, and how far apart they are taken.
const AUTO_SNAPSHOTS: usize = 30;
const AUTO_SNAPSHOT_GAP: u64 = 300;

/// A snapshot's file name: `<seconds>_<auto|named>[_<label>].txt`.
fn snapshot_parts(name: &str) -> Option<(u64, &str, String)> {
    if !comfy::plain_file(name, &["txt"]) {
        return None;
    }
    let mut parts = name.strip_suffix(".txt")?.splitn(3, '_');
    let time = parts.next()?.parse().ok()?;
    let kind = parts.next().filter(|k| *k == "auto" || *k == "named")?;
    Some((time, kind, parts.next().unwrap_or("").replace('-', " ")))
}

/// Keep the story file as it is now in its history. Returns the snapshot's name.
fn take_snapshot(story: &Path, kind: &str, label: &str) -> Result<String, String> {
    let dir = history_dir(story);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let mut slug: String = label.trim().chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect();
    slug = slug.split('-').filter(|p| !p.is_empty()).collect::<Vec<_>>().join("-");
    slug.truncate(40);
    let slug = slug.trim_end_matches('-').to_string();
    let mut time = now();
    let name = loop {
        let name = if slug.is_empty() { format!("{}_{}.txt", time, kind) } else { format!("{}_{}_{}.txt", time, kind, slug) };
        if !dir.join(&name).exists() {
            break name;
        }
        time += 1;
    };
    std::fs::copy(story, dir.join(&name)).map_err(|e| e.to_string())?;
    if !label.trim().is_empty() {
        let mut labels = snapshot_labels(story);
        labels.insert(name.clone(), json!(label.trim().chars().filter(|c| !c.is_control()).take(80).collect::<String>()));
        let _ = std::fs::write(dir.join("labels.json"), serde_json::to_string_pretty(&labels).unwrap_or_default());
    }
    Ok(name)
}

fn snapshots(story: &Path) -> Vec<(u64, String, String, String)> {
    let mut all: Vec<(u64, String, String, String)> = std::fs::read_dir(history_dir(story))
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            let (time, kind, label) = snapshot_parts(&name)?;
            Some((time, kind.to_string(), label, name))
        })
        .collect();
    all.sort();
    all.reverse();
    all
}

/// A title as a plain name for a folder or a file: lower case, letters, digits and dashes.
fn slug(title: &str) -> String {
    let mut slug: String = title.to_lowercase().chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect();
    slug = slug.split('-').filter(|p| !p.is_empty()).collect::<Vec<_>>().join("-");
    slug.truncate(40);
    if slug.is_empty() || slug == "main" {
        slug = "story".into();
    }
    slug
}

/// Move a file into a `.removed` folder beside it, under a name not yet taken.
fn set_aside(file: &Path) -> bool {
    let Some(dir) = file.parent() else { return false };
    let bin = dir.join(".removed");
    let (stem, ext) = (file.file_stem().unwrap_or_default().to_string_lossy().to_string(), file.extension().unwrap_or_default().to_string_lossy().to_string());
    let mut target = bin.join(format!("{}.{}", stem, ext));
    let mut n = 1;
    while target.exists() {
        n += 1;
        target = bin.join(format!("{}-{}.{}", stem, n, ext));
    }
    std::fs::create_dir_all(&bin).and_then(|_| std::fs::rename(file, &target)).is_ok()
}

fn base64_encode(bytes: &[u8]) -> String {
    const ABC: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = (chunk[0] as u32) << 16 | (*chunk.get(1).unwrap_or(&0) as u32) << 8 | *chunk.get(2).unwrap_or(&0) as u32;
        for i in 0..4 {
            out.push(if i <= chunk.len() { ABC[(n >> (18 - 6 * i) & 63) as usize] as char } else { '=' });
        }
    }
    out
}

/// Where a file carried inside an exported story belongs, if it is one of
/// the kinds a story keeps: enhanced pictures, recordings, voice samples.
/// The name comes from the file being imported, so it is checked strictly.
fn carried_file(name: &str) -> Option<(&'static str, &str)> {
    let (folder, file, extensions): (&'static str, &str, &[&str]) = if let Some(f) = name.strip_prefix("enhanced/") {
        ("enhanced", f, &["png"])
    } else if let Some(f) = name.strip_prefix("audio/samples/") {
        ("audio/samples", f, &comfy::SAMPLE_EXTENSIONS)
    } else if let Some(f) = name.strip_prefix("audio/") {
        ("audio", f, &audio::EXTENSIONS)
    } else if let Some(f) = name.strip_prefix("world/models/") {
        // 3D models: checked by their own rule (and their content, on import)
        return Some(("world/models", f)).filter(|_| crate::places::model_file(f));
    } else if let Some(f) = name.strip_prefix("world/") {
        ("world", f, &["json"])
    } else if let Some(f) = name.strip_prefix("paintings/") {
        ("paintings", f, &["json"])
    } else {
        return None;
    };
    Some((folder, file)).filter(|_| file.len() <= 120 && comfy::plain_file(file, extensions))
}

/// Does this look like a model file of the kind its name says: binary glTF, or glTF as JSON?
fn model_bytes_ok(name: &str, bytes: &[u8]) -> bool {
    if name.to_ascii_lowercase().ends_with(".glb") {
        bytes.len() > 20 && bytes.starts_with(b"glTF")
    } else {
        serde_json::from_slice::<Value>(bytes).map_or(false, |v| v["asset"].is_object() && gltf_outside_files(&v).is_empty())
    }
}

/// Files a glTF (JSON) model expects to find beside it. Only one file can be
/// kept per model here, so such a model would not load.
fn gltf_outside_files(v: &Value) -> Vec<String> {
    ["buffers", "images"]
        .iter()
        .flat_map(|key| v[*key].as_array().into_iter().flatten())
        .filter_map(|e| e["uri"].as_str())
        .filter(|uri| !uri.starts_with("data:"))
        .map(str::to_string)
        .collect()
}

/// One place is written at a time.
static WORLD_WRITES: Mutex<()> = Mutex::new(());

/// A place description from a file, cleaned; `None` if it is not one.
fn place_from(bytes: &[u8]) -> Option<crate::places::PlaceData> {
    let mut place: crate::places::PlaceData = serde_json::from_slice(bytes).ok()?;
    place.clean();
    Some(place)
}

/// Is this a PNG of a size the game will open? Read from its header alone.
fn png_size_ok(bytes: &[u8]) -> bool {
    let side = |at: usize| bytes.get(at..at + 4).map_or(0, |b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]));
    bytes.starts_with(b"\x89PNG\r\n\x1a\n") && bytes.get(12..16) == Some(b"IHDR") && (1..=crate::gfx::MAX_PNG_SIDE).contains(&side(16)) && (1..=crate::gfx::MAX_PNG_SIDE).contains(&side(20))
}

fn base64_decode(text: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(text.len() * 3 / 4);
    let (mut acc, mut bits) = (0u32, 0u32);
    for c in text.bytes().filter(|c| !c.is_ascii_whitespace() && *c != b'=') {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            _ => return None,
        };
        acc = (acc << 6) | v as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    Some(out)
}

fn same_file(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}

/// A fingerprint of a file's contents, so a save can tell whether the file
/// changed since the page loaded it.
fn revision(path: &Path) -> String {
    let bytes = std::fs::read(path).unwrap_or_default();
    let hash = bytes.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ *b as u64).wrapping_mul(0x0100_0000_01b3));
    format!("{:016x}-{}", hash, bytes.len())
}

struct State {
    library: Library,
    port: u16,
    /// Preview the game's own paintings even where enhanced art exists.
    procedural: bool,
    jobs: Arc<Jobs>,
    /// Painted previews, keyed by request.
    cache: Mutex<HashMap<String, Arc<Vec<u8>>>>,
    backed_up: Mutex<HashSet<PathBuf>>,
}

/// Only one large upload is taken in at a time.
static IMPORTING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
struct ImportSlot;
impl Drop for ImportSlot {
    fn drop(&mut self) {
        IMPORTING.store(false, Ordering::Release);
    }
}

/// May this request be served at all? Decided from its first lines alone,
/// before any body is read.
fn request_allowed(method: &str, headers: &HashMap<String, String>) -> bool {
    // The page must be addressed as this machine (any port, so a forwarded
    // port works): a hostile site that points its own name at 127.0.0.1
    // still sends its own name here.
    let Some(host) = headers.get("host") else { return false };
    let name = host.rsplit_once(':').filter(|(_, p)| p.chars().all(|c| c.is_ascii_digit())).map_or(host.as_str(), |(n, _)| n);
    if name != "127.0.0.1" && name != "localhost" {
        return false;
    }
    if method == "GET" {
        return true;
    }
    // writes must come from our own page, as JSON (which browsers will
    // not send cross-site without a preflight that we never answer)
    let own_origin = headers.get("origin").map_or(true, |o| *o == format!("http://{}", host));
    own_origin && headers.get("content-type").map_or(false, |c| c.starts_with("application/json"))
}

struct Request {
    /// Held while a large import is being handled.
    _slot: Option<ImportSlot>,
    method: String,
    path: String,
    query: String,
    headers: HashMap<String, String>,
    body: Vec<u8>,
}

fn problems(doc: &Doc, story: &Path) -> Vec<Problem> {
    let bgs = crate::painting::backgrounds_for(Some(story));
    let names: Vec<&str> = bgs.iter().map(String::as_str).collect();
    doc.problems(art::EXPRS, &names, art::SPRITES)
}

/// Can a story name this place (the game's own, or one it painted)?
fn known_place(story: &Path, name: &str) -> bool {
    art::BACKGROUNDS.contains(&name) || (crate::painting::usable_name(name) && crate::painting::dir(story).join(format!("{}.json", name)).is_file())
}

/// A painting as this build understands it, or nothing.
fn painting_from(bytes: &[u8]) -> Option<crate::painting::PaintingData> {
    let mut p: crate::painting::PaintingData = serde_json::from_slice(bytes).ok()?;
    p.clean();
    Some(p)
}

fn read_request(stream: &mut TcpStream) -> Option<Request> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    // one deadline for the whole request, however slowly it trickles in
    let mut deadline = Instant::now() + Duration::from_secs(20);
    let head_end = loop {
        if Instant::now() > deadline {
            return None;
        }
        if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break i;
        }
        if buf.len() > 64 << 10 {
            return None;
        }
        let n = stream.read(&mut chunk).ok()?;
        if n == 0 {
            return None;
        }
        buf.extend_from_slice(&chunk[..n]);
    };
    let head = String::from_utf8_lossy(&buf[..head_end]).to_string();
    let mut lines = head.split("\r\n");
    let mut first = lines.next()?.split(' ');
    let (method, target) = (first.next()?.to_string(), first.next()?.to_string());
    let headers: HashMap<String, String> =
        lines.filter_map(|l| l.split_once(':')).map(|(k, v)| (k.trim().to_lowercase(), v.trim().to_string())).collect();
    let len: usize = headers.get("content-length").and_then(|v| v.parse().ok()).unwrap_or(0);
    let mut slot = None;
    if len > MAX_BODY {
        // only a story being imported by our own page may be larger, one at a time
        let import = method == "POST" && target.split('?').next() == Some("/api/import") && request_allowed(&method, &headers);
        let refuse = |stream: &mut TcpStream, status: &str, msg: &str| {
            respond_json(stream, status, &json!({ "error": msg }));
            None
        };
        if !import {
            return refuse(stream, "413 Payload Too Large", "that is more than the editor accepts");
        }
        if len > MAX_IMPORT {
            return refuse(stream, "413 Payload Too Large", &format!("this file is larger than the editor can import ({} MB at most)", MAX_IMPORT >> 20));
        }
        if IMPORTING.swap(true, Ordering::AcqRel) {
            return refuse(stream, "503 Service Unavailable", "another story is being imported; try again in a moment");
        }
        slot = Some(ImportSlot);
        deadline = Instant::now() + Duration::from_secs(900);
    }
    let mut body = buf[head_end + 4..].to_vec();
    while body.len() < len {
        if Instant::now() > deadline {
            return None;
        }
        let n = stream.read(&mut chunk).ok()?;
        if n == 0 {
            return None;
        }
        body.extend_from_slice(&chunk[..n]);
    }
    body.truncate(len);
    let (path, query) = target.split_once('?').map(|(p, q)| (p.to_string(), q.to_string())).unwrap_or((target.clone(), String::new()));
    Some(Request { _slot: slot, method, path, query, headers, body })
}

fn respond(stream: &mut TcpStream, status: &str, content_type: &str, cache: bool, body: &[u8]) {
    let head = format!(
        "HTTP/1.1 {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nCache-Control: {}\r\nX-Content-Type-Options: nosniff\r\nConnection: close\r\n\r\n",
        status,
        content_type,
        body.len(),
        if cache { "max-age=3600" } else { "no-store" }
    );
    let _ = stream.write_all(head.as_bytes()).and_then(|_| stream.write_all(body));
}

/// Send a preview image. The browser may keep it but must ask each time;
/// if it still has the version named by `tag`, it is told so instead.
fn respond_image(stream: &mut TcpStream, req: &Request, tag: &str, body: impl FnOnce() -> Option<Arc<Vec<u8>>>) {
    let etag = format!("\"{:016x}\"", tag.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ b as u64).wrapping_mul(0x0100_0000_01b3)));
    if req.headers.get("if-none-match") == Some(&etag) {
        let head = format!("HTTP/1.1 304 Not Modified\r\nETag: {}\r\nCache-Control: no-cache\r\nConnection: close\r\n\r\n", etag);
        let _ = stream.write_all(head.as_bytes());
        return;
    }
    match body() {
        Some(png) => {
            let head = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: image/png\r\nContent-Length: {}\r\nETag: {}\r\nCache-Control: no-cache\r\nX-Content-Type-Options: nosniff\r\nConnection: close\r\n\r\n",
                png.len(),
                etag
            );
            let _ = stream.write_all(head.as_bytes()).and_then(|_| stream.write_all(&png));
        }
        None => respond(stream, "500 Internal Server Error", "text/plain", false, b"could not paint"),
    }
}

fn respond_json(stream: &mut TcpStream, status: &str, value: &Value) {
    respond(stream, status, "application/json", false, value.to_string().as_bytes());
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() && s.is_char_boundary(i + 1) && s.is_char_boundary(i + 3) => match u8::from_str_radix(&s[i + 1..i + 3], 16) {
                Ok(b) => {
                    out.push(b);
                    i += 3;
                    continue;
                }
                Err(_) => out.push(b'%'),
            },
            b'+' => out.push(b' '),
            b => out.push(b),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}

fn query_map(query: &str) -> HashMap<String, String> {
    query.split('&').filter_map(|kv| kv.split_once('=')).map(|(k, v)| (percent_decode(k), percent_decode(v))).collect()
}

impl State {
    fn origin_ok(&self, req: &Request) -> bool {
        request_allowed(&req.method, &req.headers)
    }

    fn load(&self, story: &Path) -> Result<(Doc, Vec<String>), String> {
        let text = std::fs::read_to_string(story).map_err(|e| format!("{}: {}", story.display(), e))?;
        Ok(Doc::parse(&text))
    }

    /// `keep`: the version on disk is about to be replaced wholesale, so it is kept in the history whatever its age.
    fn save(&self, story: &Path, doc: &Doc, keep: bool) -> Result<(), String> {
        let err = |e: std::io::Error| format!("{}: {}", story.display(), e);
        // keep each file as it was before this editing session
        let mut backed_up = self.backed_up.lock().unwrap();
        if !backed_up.contains(story) && story.is_file() {
            let mut bak = story.to_path_buf().into_os_string();
            bak.push(".bak");
            std::fs::copy(story, PathBuf::from(bak)).map_err(err)?;
        }
        backed_up.insert(story.to_path_buf());
        // (the guard is held to the end: saves of one story never interleave)
        // and keep how it was, now and then, so an earlier version can be brought back;
        // always when the whole story is being replaced by another version
        let text = doc.to_text();
        if std::fs::read_to_string(story).map_or(false, |old| old != text) {
            let autos: Vec<_> = snapshots(story).into_iter().filter(|s| s.1 == "auto").collect();
            // a snapshot stamped ahead of the clock (a corrected clock, a copied folder) must not hold others back
            let due = autos.first().map_or(true, |latest| latest.0 > now() || now() - latest.0 >= AUTO_SNAPSHOT_GAP);
            if (keep || due) && take_snapshot(story, "auto", "").is_ok() {
                for old in autos.iter().skip(AUTO_SNAPSHOTS - 1) {
                    let _ = std::fs::remove_file(history_dir(story).join(&old.3));
                }
            }
        }
        // write beside the file and rename, so a crash cannot leave half a story
        let mut tmp = story.to_path_buf().into_os_string();
        tmp.push(".tmp");
        std::fs::write(&tmp, text).map_err(err)?;
        std::fs::rename(&tmp, story).map_err(err)
    }

    /// The folder of enhanced art to preview for a story, if it has one.
    fn art_dir(&self, story: &Path) -> Option<PathBuf> {
        Some(comfy::enhanced_dir(story)).filter(|d| !self.procedural && d.is_dir())
    }

    fn prefs(&self, story: &Path) -> Value {
        std::fs::read_to_string(story.with_file_name("art.json")).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_else(|| json!({}))
    }

    /// Recordings are filed under the ids of tracks and speakers. When a save
    /// changes such an id (the entry in the same place has a new id, and
    /// neither id exists on the other side), the recordings, the voice
    /// sample and the voice settings move with it.
    fn follow_renames(&self, story: &Path, before: &Doc, after: &Doc) {
        fn renamed<'a>(old: Vec<&'a str>, new: Vec<&'a str>) -> Vec<(&'a str, &'a str)> {
            old.iter().zip(new.iter()).filter(|(o, n)| o != n && !new.contains(o) && !old.contains(n)).map(|(o, n)| (*o, *n)).collect()
        }
        let dir = audio::audio_dir(story);
        let rename = |from: PathBuf, to_stem: String| {
            let to = from.with_file_name(format!("{}.{}", to_stem, from.extension().unwrap_or_default().to_string_lossy()));
            if !to.exists() {
                let _ = std::fs::rename(from, to);
            }
        };
        for (old, new) in renamed(before.tracks.iter().map(|t| t.id.as_str()).collect(), after.tracks.iter().map(|t| t.id.as_str()).collect()) {
            if let Some(file) = audio::find(&dir, &audio::music_stem(old)) {
                rename(file, audio::music_stem(new));
            }
        }
        let people = renamed(before.characters.iter().map(|c| c.id.as_str()).collect(), after.characters.iter().map(|c| c.id.as_str()).collect());
        if people.is_empty() {
            return;
        }
        let mut prefs = self.prefs(story);
        let mut prefs_changed = false;
        for (old, new) in people {
            for scene in &after.scenes {
                for item in &scene.items {
                    if let Item::Say { who, text } = item {
                        if who == new {
                            if let Some(file) = audio::find(&dir, &audio::voice_stem(old, text)) {
                                rename(file, audio::voice_stem(new, text));
                            }
                        }
                    }
                }
            }
            let mut sample = None;
            for ext in comfy::SAMPLE_EXTENSIONS {
                let from = dir.join("samples").join(format!("{}.{}", old, ext));
                if from.is_file() {
                    sample = Some(format!("{}.{}", new, ext));
                    rename(from, new.to_string());
                }
            }
            if let Some(mut voice) = prefs["voices"].as_object_mut().and_then(|v| v.remove(old)) {
                if let (Some(name), true) = (sample, voice.is_object()) {
                    voice["sample"] = json!(name);
                }
                prefs["voices"][new] = voice;
                prefs_changed = true;
            }
        }
        if prefs_changed {
            let _ = std::fs::write(story.with_file_name("art.json"), serde_json::to_string_pretty(&prefs).unwrap_or_default() + "\n");
        }
        self.jobs.version.fetch_add(1, Ordering::Relaxed);
    }

    /// Everything the Manage view shows: each story with its size and state,
    /// and the stories that were removed.
    fn admin_overview(&self) -> Value {
        let stories: Vec<Value> = self
            .library
            .list()
            .into_iter()
            .filter_map(|mut entry| {
                let path = self.library.path(entry["id"].as_str()?).ok()?;
                let folder = path.parent().unwrap_or(Path::new(".")).to_path_buf();
                let (doc, _) = self.load(&path).ok()?;
                let (mut lines, mut words, mut endings, mut choices) = (0, 0, 0, 0);
                for item in doc.scenes.iter().flat_map(|s| &s.items) {
                    match item {
                        Item::Say { text, .. } => {
                            lines += 1;
                            words += text.split_whitespace().count();
                        }
                        Item::Choice { options } => {
                            choices += 1;
                            words += options.iter().map(|o| o.text.split_whitespace().count()).sum::<usize>();
                        }
                        Item::End { .. } => endings += 1,
                        _ => {}
                    }
                }
                let world_aside = dir_bytes(&folder.join("world").join(".removed")) + dir_bytes(&folder.join("world").join("models").join(".removed"));
                let aside = dir_bytes(&folder.join("enhanced").join(".removed")) + dir_bytes(&folder.join("audio").join(".removed")) + dir_bytes(&PathBuf::from(format!("{}.bak", path.display()))) + world_aside;
                // (3D places and models are counted with the art)
                let (art, sound) = (dir_bytes(&folder.join("enhanced")) + dir_bytes(&folder.join("world")).saturating_sub(world_aside), dir_bytes(&folder.join("audio")));
                entry["lines"] = json!(lines);
                entry["words"] = json!(words);
                entry["endings"] = json!(endings);
                entry["choices"] = json!(choices);
                entry["modified"] = json!(modified(&path));
                entry["locked"] = json!(lock_file(&path).exists());
                entry["snapshots"] = json!(snapshots(&path).len());
                entry["bytes"] = json!({ "story": dir_bytes(&path), "art": art.saturating_sub(dir_bytes(&folder.join("enhanced").join(".removed"))), "sound": sound.saturating_sub(dir_bytes(&folder.join("audio").join(".removed"))), "aside": aside, "history": dir_bytes(&history_dir(&path)) });
                Some(entry)
            })
            .collect();
        json!({ "stories": stories, "trash": self.library.trash(), "folder": self.library.dir.display().to_string() })
    }

    fn history(&self, story: &Path) -> Value {
        let labels = snapshot_labels(story);
        let list: Vec<Value> = snapshots(story)
            .into_iter()
            .map(|(time, kind, label, name)| {
                let label = labels.get(&name).and_then(Value::as_str).map_or(label, str::to_string);
                let file = history_dir(story).join(&name);
                let scenes = std::fs::read_to_string(&file).map_or(0, |t| Doc::parse(&t).0.scenes.len());
                json!({ "name": name, "time": time, "kind": kind, "label": label, "scenes": scenes, "bytes": dir_bytes(&file) })
            })
            .collect();
        json!({ "snapshots": list })
    }

    /// The art and sound choices of a story, with anything that is not one
    /// of ours dropped (they may come from an imported file).
    fn clean_prefs(given: &Value) -> Value {
        let strings = |v: &Value, file: bool| -> serde_json::Map<String, Value> {
            v.as_object().into_iter().flatten().filter(|(k, v)| k.len() <= 200 && v.as_str().map_or(false, |t| !file || comfy::plain_file(t, &["json"]))).map(|(k, v)| (k.clone(), v.clone())).collect()
        };
        let voices: BTreeMap<String, comfy::Voice> = comfy::voices_from(&given["voices"]).into_iter().filter(|(id, _)| valid_id(id)).collect();
        json!({ "prompts": strings(&given["prompts"], false), "workflows": strings(&given["workflows"], true), "voices": voices })
    }

    /// A story as one file: the story itself, its art and sound choices and,
    /// if asked, its enhanced pictures, recordings and voice samples.
    fn export(&self, story: &Path, with_files: bool) -> Result<Value, String> {
        let (mut doc, _) = self.load(story)?;
        doc.sanitize();
        let mut out = json!({ "format": EXPORT_FORMAT, "version": EXPORT_VERSION, "story": doc, "prefs": Self::clean_prefs(&self.prefs(story)) });
        let folder = story.parent().unwrap_or(Path::new("."));
        let mut files = serde_json::Map::new();
        // the places a story painted for itself are part of it: they always travel
        for name in crate::painting::names(story) {
            if let Ok(bytes) = std::fs::read(crate::painting::dir(story).join(format!("{}.json", name))) {
                files.insert(format!("paintings/{}.json", name), json!(base64_encode(&bytes)));
            }
        }
        if !with_files && !files.is_empty() {
            out["files"] = Value::Object(files.clone());
        }
        if with_files {
            for sub in ["enhanced", "audio", "audio/samples", "world", "world/models"] {
                let mut names: Vec<String> = std::fs::read_dir(folder.join(sub)).into_iter().flatten().filter_map(|e| e.ok()).filter(|e| e.path().is_file()).map(|e| e.file_name().to_string_lossy().to_string()).collect();
                names.sort();
                for name in names {
                    let key = format!("{}/{}", sub, name);
                    if carried_file(&key).is_some() {
                        files.insert(key, json!(base64_encode(&std::fs::read(folder.join(sub).join(&name)).map_err(|e| format!("{}: {}", name, e))?)));
                    }
                }
            }
            out["files"] = Value::Object(files);
        }
        Ok(out)
    }

    /// Add a story from a file: one of our exports, a bare story in JSON, or
    /// the text of a story file (sent as `{"text": ...}`). It always becomes
    /// a new story; nothing already in the library is touched.
    fn import(&self, body: &[u8]) -> Result<Value, String> {
        // Read straight into what a story file may hold. Anything else in
        // the file is skipped as it goes by and costs no memory.
        #[derive(serde::Deserialize, Default)]
        #[serde(default)]
        struct VoiceIn {
            workflow: Option<String>,
            voice: Option<String>,
            sample: Option<String>,
            sample_text: Option<String>,
            speed: Option<f64>,
        }
        #[derive(serde::Deserialize, Default)]
        #[serde(default)]
        struct PrefsIn {
            prompts: BTreeMap<String, String>,
            workflows: BTreeMap<String, String>,
            voices: BTreeMap<String, VoiceIn>,
        }
        #[derive(serde::Deserialize, Default)]
        #[serde(default)]
        struct FileIn {
            format: Option<String>,
            version: Option<u64>,
            name: Option<String>,
            text: Option<String>,
            story: Option<Doc>,
            scenes: Option<serde::de::IgnoredAny>,
            prefs: Option<PrefsIn>,
            files: BTreeMap<String, String>,
        }
        let no_story = "this file holds no story: expected an exported story (.json) or a story text file";
        let given: FileIn = serde_json::from_slice(body).map_err(|e| if e.is_data() { format!("the story in this file cannot be read: {}", e) } else { no_story.to_string() })?;
        if given.format.as_deref().map_or(false, |f| f != EXPORT_FORMAT) {
            return Err("this is not a story file from this editor".into());
        }
        if given.version.map_or(false, |v| v > EXPORT_VERSION) {
            return Err("this story file was made by a newer version of the editor".into());
        }
        let (mut doc, unread): (Doc, Vec<String>) = if let Some(story) = given.story {
            (story, Vec::new())
        } else if given.scenes.is_some() {
            // a bare story, as the editor itself holds it
            (serde_json::from_slice(body).map_err(|e| format!("the story in this file cannot be read: {}", e))?, Vec::new())
        } else if let Some(text) = &given.text {
            if text.trim_start().starts_with(['{', '[']) {
                return Err("this looks like a damaged or incomplete .json file, not a story".into());
            }
            Doc::parse(text)
        } else {
            return Err(no_story.into());
        };
        doc.sanitize();
        if doc.scenes.is_empty() {
            return Err("this file holds no scenes".into());
        }
        doc.survives_text()?;
        // decode and check every carried file before anything is written
        let mut files = Vec::new();
        let mut skipped = Vec::new();
        for (name, data) in &given.files {
            let bytes = carried_file(name).and_then(|place| Some((place, base64_decode(data)?)));
            match bytes {
                Some(((folder, file), bytes))
                    if !bytes.is_empty()
                        && (folder != "enhanced" || png_size_ok(&bytes))
                        && (folder != "world/models" || model_bytes_ok(file, &bytes))
                        && (folder != "world" || ((art::BACKGROUNDS.contains(&file.trim_end_matches(".json")) || crate::painting::usable_name(file.trim_end_matches(".json"))) && place_from(&bytes).is_some()))
                        && (folder != "paintings" || (crate::painting::usable_name(file.trim_end_matches(".json")) && painting_from(&bytes).is_some())) =>
                {
                    // a place or painting is written as this build understands it, not as it came
                    let bytes = match folder {
                        "world" => serde_json::to_vec(&place_from(&bytes).unwrap()).unwrap_or(bytes),
                        "paintings" => serde_json::to_vec_pretty(&painting_from(&bytes).unwrap()).unwrap_or(bytes),
                        _ => bytes,
                    };
                    files.push((folder, file, bytes))
                }
                _ => skipped.push(name.clone()),
            }
        }
        // an untitled story stays untitled; its folder is named after the file it came from
        let file_name = given.name.as_deref().unwrap_or("");
        let stem = file_name.rsplit(['/', '\\']).next().unwrap_or("").split('.').next().unwrap_or("");
        let folder_name = if !doc.title.trim().is_empty() { doc.title.clone() } else if !stem.trim().is_empty() { stem.to_string() } else { "imported story".to_string() };
        let (id, folder) = self.library.adopt(&doc, &folder_name)?;
        let written = (|| -> std::io::Result<()> {
            if let Some(prefs) = &given.prefs {
                let voices: serde_json::Map<String, Value> = prefs
                    .voices
                    .iter()
                    .map(|(id, v)| (id.clone(), json!({ "workflow": v.workflow, "voice": v.voice, "sample": v.sample, "sample_text": v.sample_text, "speed": v.speed })))
                    .collect();
                let clean = Self::clean_prefs(&json!({ "prompts": prefs.prompts, "workflows": prefs.workflows, "voices": voices }));
                std::fs::write(folder.join("art.json"), serde_json::to_string_pretty(&clean).unwrap_or_default() + "\n")?;
            }
            for (sub, file, bytes) in &files {
                std::fs::create_dir_all(folder.join(sub))?;
                std::fs::write(folder.join(sub).join(file), bytes)?;
            }
            Ok(())
        })();
        if let Err(e) = written {
            // no half-imported story is left in the library
            let _ = std::fs::remove_dir_all(&folder);
            return Err(format!("the story could not be written: {}", e));
        }
        let notes = problems(&doc, &folder.join("story.txt"));
        Ok(json!({
            "id": id, "title": if doc.title.is_empty() { "Untitled story".to_string() } else { doc.title.clone() }, "scenes": doc.scenes.len(), "files": files.len(), "skipped": skipped, "unread": unread,
            "errors": notes.iter().filter(|p| p.level == "error").count(), "stories": self.library.list(),
        }))
    }

    /// What the story has chosen for its art: its own prompts and workflows.
    fn overrides(&self, story: &Path) -> Overrides {
        let prefs = self.prefs(story);
        let strings = |v: &Value| -> BTreeMap<String, String> { v.as_object().into_iter().flatten().filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_string()))).collect() };
        let voices = comfy::voices_from(&prefs["voices"]);
        Overrides { prompts: strings(&prefs["prompts"]), workflows: strings(&prefs["workflows"]), voices, ..Default::default() }
    }

    /// Everything the Sound view shows for a story: who speaks and how much
    /// of it is recorded, and the music tracks.
    fn sound_overview(&self, story: &Path) -> Result<Value, String> {
        let (doc, _) = self.load(story)?;
        let dir = audio::audio_dir(story);
        let assets = comfy::audio_assets(&doc);
        let file_of = |stem: &str| audio::find(&dir, stem).and_then(|p| p.file_name().map(|f| f.to_string_lossy().to_string()));
        let mut speakers: Vec<Value> = Vec::new();
        let ids = std::iter::once(crate::script::NARRATOR.to_string()).chain(doc.characters.iter().map(|c| c.id.clone()));
        for id in ids {
            let lines: Vec<&comfy::Asset> = assets.iter().filter(|a| a.kind == "voice" && a.who == id).collect();
            if lines.is_empty() {
                continue;
            }
            let ch = doc.characters.iter().find(|c| c.id == id);
            speakers.push(json!({
                "id": id, "name": lines[0].name, "color": ch.map_or("#d6deff", |c| c.color.as_str()),
                "lines": lines.len(), "voiced": lines.iter().filter(|a| file_of(&a.id).is_some()).count(),
            }));
        }
        let tracks: Vec<Value> = doc.tracks.iter().map(|t| json!({ "id": t.id, "asset": audio::music_stem(&t.id), "file": file_of(&audio::music_stem(&t.id)) })).collect();
        let cfg = self.jobs.suite.config()?;
        let workflows: Vec<comfy::WorkflowInfo> = self.jobs.suite.workflows().into_iter().filter(|w| w.kind == "voice" || w.kind == "music").collect();
        Ok(json!({
            "speakers": speakers, "tracks": tracks, "workflows": workflows, "prefs": self.prefs(story),
            "defaults": { "voice": cfg["kinds"]["voice"]["workflow"], "music": cfg["kinds"]["music"]["workflow"] },
            "api_key": std::env::var("COMFY_ORG_API_KEY").map_or(false, |k| !k.trim().is_empty()),
            "folder": dir.display().to_string(), "playback": cfg!(feature = "audio"),
        }))
    }

    /// Everything the Art view shows for a story.
    fn art_overview(&self, story: &Path) -> Result<Value, String> {
        let (doc, _) = self.load(story)?;
        let dir = comfy::enhanced_dir(story);
        let assets: Vec<Value> = comfy::assets_of(&doc, story)
            .iter()
            .map(|a| {
                let meta = std::fs::metadata(dir.join(a.output())).ok();
                let mtime = meta.as_ref().and_then(|m| m.modified().ok()).and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_secs());
                json!({
                    "id": a.id, "kind": a.kind, "name": a.name, "expr": a.expr, "prompt": a.prompt, "prompt_key": a.prompt_key,
                    "character": a.character.as_ref().map(|c| c.id.clone()), "enhanced": meta.is_some(), "mtime": mtime,
                })
            })
            .collect();
        let cfg = self.jobs.suite.config()?;
        Ok(json!({
            "config": { "server": cfg["server"], "checkpoint": cfg["checkpoint"], "controlnet": cfg["controlnet"], "denoise": { "background": cfg["kinds"]["background"]["denoise"], "sprite": cfg["kinds"]["sprite"]["denoise"], "expression": cfg["kinds"]["expression"]["denoise"] } },
            "workflows": self.jobs.suite.workflows(), "assets": assets, "prefs": self.prefs(story),
            "folder": dir.display().to_string(), "suite": self.jobs.suite.dir.display().to_string(),
        }))
    }

    fn meta(&self) -> Value {
        let looks: BTreeMap<&str, BTreeMap<String, String>> = art::SPRITES.iter().map(|s| (*s, art::look_defaults(s))).collect();
        json!({
            "exprs": art::EXPRS, "bgs": art::BACKGROUNDS, "sprites": art::SPRITES,
            "ops": OPS, "slots": SLOTS, "looks": looks,
            // whether this build can show places as a 3D world
            "world3d": cfg!(feature = "world3d"),
        })
    }

    fn doc_from(&self, body: &[u8]) -> Result<Doc, String> {
        let mut doc: Doc = serde_json::from_slice(body).map_err(|e| format!("bad story data: {}", e))?;
        doc.sanitize();
        Ok(doc)
    }

    /// Paint (or fetch from cache) a preview image.
    fn art(&self, key: &str, paint: impl FnOnce() -> Option<(Canvas, bool)>) -> Option<Arc<Vec<u8>>> {
        if let Some(hit) = self.cache.lock().unwrap().get(key) {
            return Some(hit.clone());
        }
        let (canvas, alpha) = paint()?;
        let bytes = Arc::new(canvas.png_bytes(alpha).ok()?);
        let mut cache = self.cache.lock().unwrap();
        if cache.len() > 400 {
            cache.clear();
        }
        cache.insert(key.to_string(), bytes.clone());
        Some(bytes)
    }

    fn handle(&self, req: Request, stream: &mut TcpStream) {
        if !self.origin_ok(&req) {
            let msg = format!("Forbidden. Open the story editor at http://127.0.0.1:{}/ in a browser on this machine.", self.port);
            return respond(stream, "403 Forbidden", "text/plain", false, msg.as_bytes());
        }
        let fail = |stream: &mut TcpStream, msg: String| respond_json(stream, "400 Bad Request", &json!({ "error": msg }));
        let q = query_map(&req.query);
        // an imported file can be large and is read by its own types, not as loose JSON
        let body: Value = if req.method == "GET" || req.path == "/api/import" { Value::Null } else { serde_json::from_slice(&req.body).unwrap_or(Value::Null) };
        // which story the request is about: ?story= or a "story" field
        let story_id = q.get("story").cloned().or_else(|| body["story"].as_str().map(str::to_string)).unwrap_or_else(|| "main".into());
        let story = match self.library.path(&story_id) {
            Ok(p) => p,
            Err(e) => return respond_json(stream, "404 Not Found", &json!({ "error": e })),
        };
        // a locked story is not changed by any route: not its text, its art, its sound or its history
        const CHANGES: [&str; 13] = ["/api/ai/place/save", "/api/world", "/api/world/reset", "/api/world/model", "/api/story", "/api/stories/remove", "/api/admin/clean", "/api/history/remove", "/api/art/prefs", "/api/art/run", "/api/art/revert", "/api/sound/sample", "/api/sound/prune"];
        if req.method != "GET" && CHANGES.contains(&req.path.as_str()) && lock_file(&story).exists() {
            return respond_json(stream, "423 Locked", &json!({ "error": "this story is locked: unlock it under Manage to change it", "locked": true }));
        }
        let answer = |stream: &mut TcpStream, result: Result<Value, String>| match result {
            Ok(v) => respond_json(stream, "200 OK", &v),
            Err(e) => respond_json(stream, "400 Bad Request", &json!({ "error": e })),
        };
        let ids = |v: &Value| -> Vec<String> { v.as_array().into_iter().flatten().filter_map(|x| x.as_str().map(str::to_string)).collect() };
        match (req.method.as_str(), req.path.as_str()) {
            ("GET", "/") | ("GET", "/index.html") => respond(stream, "200 OK", "text/html; charset=utf-8", false, PAGE.as_bytes()),
            ("GET", "/api/stories") => respond_json(stream, "200 OK", &json!({ "stories": self.library.list() })),
            ("POST", "/api/stories") => {
                let made = self.library.create(body["title"].as_str().unwrap_or(""), body["copy"].as_str());
                answer(stream, made.map(|id| json!({ "id": id, "stories": self.library.list() })))
            }
            ("GET", "/api/export") => {
                let format = q.get("format").map_or("json", |f| f.as_str());
                let made: Result<(Vec<u8>, &str, &str), String> = match format {
                    "txt" => self.load(&story).map(|(mut doc, _)| {
                        doc.sanitize();
                        (doc.to_text().into_bytes(), "text/plain; charset=utf-8", "story.txt")
                    }),
                    "json" | "bundle" => self.export(&story, format == "bundle").and_then(|v| {
                        let bytes = (serde_json::to_string_pretty(&v).unwrap_or_default() + "\n").into_bytes();
                        if bytes.len() > MAX_IMPORT - (1 << 20) {
                            return Err(format!("this story's pictures and recordings come to {} MB, more than one file can carry ({} MB): export the story alone and copy its 'enhanced' and 'audio' folders by hand", bytes.len() >> 20, MAX_IMPORT >> 20));
                        }
                        Ok((bytes, "application/json", if format == "bundle" { "bundle.json" } else { "story.json" }))
                    }),
                    "godot" | "godot2d" | "godot3d" => self.load(&story).and_then(|(mut doc, _)| {
                        // the whole game, as a Godot project in one zip: built in a folder of its own, then packed
                        doc.sanitize();
                        let mode = match format {
                            "godot2d" => crate::godot::Mode::Flat,
                            "godot3d" => crate::godot::Mode::Full,
                            _ => crate::godot::Mode::Story,
                        };
                        static N: AtomicUsize = AtomicUsize::new(0);
                        let work = std::env::temp_dir().join(format!("below-stairs-godot-{}-{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
                        let _ = std::fs::remove_dir_all(&work);
                        let built = crate::godot::export(&work, &doc, Some(&story), self.art_dir(&story).as_deref(), true, mode).and_then(|_| {
                            crate::godot::zip_dir(&work, &slug(&doc.title))
                        });
                        let _ = std::fs::remove_dir_all(&work);
                        built.map(|bytes| (bytes, "application/zip", match mode {
                            crate::godot::Mode::Flat => "godot-2d.zip",
                            crate::godot::Mode::Full => "godot-3d.zip",
                            crate::godot::Mode::Story => "godot.zip",
                        }))
                    }),
                    _ => Err("export as json, bundle, txt, godot, godot2d or godot3d".into()),
                };
                match made {
                    Ok((bytes, kind, ending)) => {
                        // a download, named after the story
                        let name = if story_id.is_empty() || story_id == "main" { self.load(&story).map(|(d, _)| slug(&d.title)).unwrap_or_else(|_| "story".into()) } else { story_id.clone() };
                        let name: String = name.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_').collect();
                        let head = format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: {}\r\nContent-Length: {}\r\nContent-Disposition: attachment; filename=\"{}.{}\"\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nConnection: close\r\n\r\n",
                            kind,
                            bytes.len(),
                            if name.is_empty() { "story" } else { &name },
                            ending
                        );
                        let _ = stream.write_all(head.as_bytes()).and_then(|_| stream.write_all(&bytes));
                    }
                    Err(e) => fail(stream, e),
                }
            }
            ("POST", "/api/import") => answer(stream, self.import(&req.body)),
            // ---- managing the library
            ("GET", "/api/admin") => respond_json(stream, "200 OK", &self.admin_overview()),
            ("POST", "/api/admin/restore") => {
                let back = self.library.restore(body["name"].as_str().unwrap_or(""));
                answer(stream, back.map(|id| json!({ "id": id, "stories": self.library.list() })))
            }
            ("POST", "/api/admin/purge") => answer(stream, self.library.purge(body["name"].as_str().unwrap_or("")).map(|_| json!({ "ok": true }))),
            ("POST", "/api/admin/clean") => {
                // delete what was only set aside: replaced pictures and recordings, and the session's backup copy
                let folder = story.parent().unwrap_or(Path::new(".")).to_path_buf();
                let targets = [
                    folder.join("enhanced").join(".removed"),
                    folder.join("audio").join(".removed"),
                    folder.join("world").join(".removed"),
                    folder.join("world").join("models").join(".removed"),
                    PathBuf::from(format!("{}.bak", story.display())),
                ];
                let freed: u64 = targets.iter().map(|t| dir_bytes(t)).sum();
                for t in &targets {
                    let _ = if t.is_dir() { std::fs::remove_dir_all(t) } else { std::fs::remove_file(t) };
                }
                self.backed_up.lock().unwrap().remove(&story);
                respond_json(stream, "200 OK", &json!({ "freed": freed }))
            }
            ("POST", "/api/admin/lock") => {
                let file = lock_file(&story);
                let done = if body["locked"].as_bool().unwrap_or(false) { std::fs::write(&file, "This story is locked in the story editor. Delete this file to unlock it.\n") } else if file.exists() { std::fs::remove_file(&file) } else { Ok(()) };
                answer(stream, done.map(|_| json!({ "locked": file.exists() })).map_err(|e| e.to_string()))
            }
            ("POST", "/api/godot") => {
                // write (or bring up to date) the Godot project for this story
                let folder = story.parent().unwrap_or(Path::new("."));
                // (the main story's project sits beside its folder: assets/story.txt and godot/)
                let dir = if story_id == "main" || story_id.is_empty() { folder.parent().unwrap_or(Path::new(".")).join("godot") } else { folder.join("godot") };
                let done = self.load(&story).and_then(|(mut doc, _)| {
                    doc.sanitize();
                    let mode = crate::godot::Mode::parse(body["mode"].as_str().unwrap_or("")).ok_or("the mode is 2d, 3d or story")?;
                    crate::godot::export(&dir, &doc, Some(&story), self.art_dir(&story).as_deref(), body["force"].as_bool().unwrap_or(false), mode)
                });
                answer(stream, done.map(|r| json!({ "folder": dir.display().to_string(), "written": r.written.len(), "kept": r.kept.len(), "notes": r.notes })))
            }
            ("GET", "/api/history") => respond_json(stream, "200 OK", &self.history(&story)),
            ("POST", "/api/history/snapshot") => answer(stream, take_snapshot(&story, "named", body["label"].as_str().unwrap_or("")).map(|name| json!({ "name": name }))),
            ("GET", "/api/history/get") | ("POST", "/api/history/remove") => {
                let name = q.get("name").cloned().or_else(|| body["name"].as_str().map(str::to_string)).unwrap_or_default();
                let file = history_dir(&story).join(&name);
                if snapshot_parts(&name).is_none() || !file.is_file() {
                    return respond_json(stream, "404 Not Found", &json!({ "error": "there is no such snapshot" }));
                }
                if req.method == "POST" {
                    return answer(stream, std::fs::remove_file(&file).map(|_| json!({ "ok": true })).map_err(|e| e.to_string()));
                }
                let read = std::fs::read_to_string(&file).map_err(|e| e.to_string()).map(|text| {
                    let (mut doc, _) = Doc::parse(&text);
                    doc.sanitize();
                    json!({ "doc": doc })
                });
                answer(stream, read)
            }
            ("POST", "/api/stories/remove") => {
                if lock_file(&story).exists() {
                    return respond_json(stream, "423 Locked", &json!({ "error": "this story is locked: unlock it under Manage first", "locked": true }));
                }
                let removed = self.library.remove(&story_id).map(|_| {
                    self.jobs.forget_story(&story_id);
                    json!({ "stories": self.library.list() })
                });
                answer(stream, removed)
            }
            ("GET", "/api/story") => match self.load(&story) {
                Ok((mut doc, parse_errors)) => {
                    // tidy what a hand-edited file may leave loose (a scene in a chapter that was never declared, say)
                    doc.sanitize();
                    let p = problems(&doc, &story);
                    let mut meta = self.meta();
                    meta["path"] = json!(story.display().to_string());
                    meta["bgs"] = json!(crate::painting::backgrounds_for(Some(&story)));
                    meta["painted"] = json!(crate::painting::names(&story));
                    respond_json(stream, "200 OK", &json!({ "doc": doc, "problems": p, "parse_errors": parse_errors, "meta": meta, "revision": revision(&story), "locked": lock_file(&story).exists() }))
                }
                Err(e) => fail(stream, e),
            },
            ("POST", "/api/validate") => match self.doc_from(&req.body) {
                Ok(doc) => respond_json(stream, "200 OK", &json!({ "problems": problems(&doc, &story) })),
                Err(e) => fail(stream, e),
            },
            ("PUT", "/api/story") => {
                // refuse to overwrite a file that changed since this page loaded it (another tab, a hand edit)
                if lock_file(&story).exists() {
                    return respond_json(stream, "423 Locked", &json!({ "error": "this story is locked: unlock it under Manage to change it", "locked": true }));
                }
                let on_disk = revision(&story);
                if q.get("base").map_or(false, |base| *base != on_disk) && !q.contains_key("force") {
                    return respond_json(stream, "409 Conflict", &json!({ "error": "the story file has changed since you opened it", "conflict": true }));
                }
                let before = self.load(&story).ok().map(|(doc, _)| doc);
                match self.doc_from(&req.body).and_then(|doc| doc.survives_text().and_then(|_| self.save(&story, &doc, q.contains_key("force") || q.contains_key("keep"))).map(|_| doc)) {
                    Ok(doc) => {
                        if let Some(before) = &before {
                            self.follow_renames(&story, before, &doc);
                        }
                        respond_json(stream, "200 OK", &json!({ "ok": true, "doc": doc, "problems": problems(&doc, &story), "revision": revision(&story) }))
                    }
                    Err(e) => fail(stream, e),
                }
            }
            ("POST", "/api/play") => answer(stream, self.play(&story, body["label"].as_str().unwrap_or("")).map(|_| json!({ "ok": true }))),

            // ---- art: previews
            ("GET", "/art/sprite") => {
                let base = q.get("base").cloned().unwrap_or_default();
                let expr = q.get("expr").cloned().unwrap_or_else(|| "neutral".into());
                let id = q.get("id").cloned().unwrap_or_default();
                let look: BTreeMap<String, String> =
                    q.get("look").map(|l| l.split(',').filter_map(|kv| kv.split_once('=')).map(|(k, v)| (k.to_string(), v.to_string())).collect()).unwrap_or_default();
                // the id becomes part of a file name: story ids only
                if !art::SPRITES.contains(&base.as_str()) || !art::EXPRS.contains(&expr.as_str()) || !(id.is_empty() || valid_id(&id)) {
                    return respond(stream, "404 Not Found", "text/plain", false, b"no such portrait");
                }
                let dir = self.art_dir(&story).filter(|_| !q.contains_key("plain"));
                let def = CharDef { id, sprite: base, look, ..Default::default() };
                let key = self.art_key(&req, dir.as_deref().map(|d| d.join(format!("spr_{}_{}.png", def.id, expr))));
                respond_image(stream, &req, &key, || self.art(&key, || Some((art::sprite(&def, &expr, dir.as_deref()), true))))
            }
            ("GET", path) if path.starts_with("/art/bg/") => {
                let name = path.trim_start_matches("/art/bg/").trim_end_matches(".png").to_string();
                if !known_place(&story, &name) {
                    return respond(stream, "404 Not Found", "text/plain", false, b"no such background");
                }
                let dir = self.art_dir(&story).filter(|_| !q.contains_key("plain"));
                let full = q.contains_key("full");
                // (a painting the story made: its file's age tells a stale picture from a fresh one)
                let enhanced = dir.as_deref().map(|d| d.join(format!("bg_{}.png", name))).filter(|f| f.is_file() || art::BACKGROUNDS.contains(&name.as_str()));
                let key = self.art_key(&req, enhanced.or_else(|| Some(crate::painting::dir(&story).join(format!("{}.json", name))).filter(|_| !art::BACKGROUNDS.contains(&name.as_str()))));
                respond_image(stream, &req, &key, || {
                    self.art(&key, || {
                        let (mut c, animated) = art::background_of(&name, dir.as_deref(), Some(&story));
                        if animated {
                            art::animate(&mut c, &name, 0.5);
                        }
                        Some((if full { c } else { c.resized(384, 216) }, false))
                    })
                })
            }

            // ---- art: ComfyUI
            ("GET", "/api/art") => answer(stream, self.art_overview(&story)),
            ("GET", "/api/art/jobs") => respond_json(stream, "200 OK", &json!({ "jobs": self.jobs.snapshot(), "version": self.jobs.version.load(Ordering::Relaxed) })),
            ("GET", "/api/art/server") => {
                // is ComfyUI there, and which models does it have?
                let cfg = self.jobs.suite.config().unwrap_or(Value::Null);
                let url = cfg["server"].as_str().unwrap_or("http://127.0.0.1:8188").to_string();
                let client = comfy::Client::new(&url, Duration::from_secs(8));
                let found = client.model_list("CheckpointLoaderSimple", "ckpt_name").and_then(|c| Ok((c, client.model_list("ControlNetLoader", "control_net_name")?)));
                let reply = match found {
                    Ok((checkpoints, controlnets)) => json!({ "ok": true, "server": url, "checkpoints": checkpoints, "controlnets": controlnets }),
                    Err(e) => json!({ "ok": false, "server": url, "error": e }),
                };
                respond_json(stream, "200 OK", &reply)
            }
            ("PUT", "/api/art/config") => {
                let saved = body.as_object().ok_or("expected settings".to_string()).and_then(|o| self.jobs.suite.update_config(o));
                answer(stream, saved.map(|_| json!({ "ok": true })))
            }
            ("PUT", "/api/art/prefs") => {
                // the story's own prompts, preferred workflows and voices, kept beside it
                let voices: BTreeMap<String, comfy::Voice> = comfy::voices_from(&body["voices"]).into_iter().filter(|(id, _)| valid_id(id)).collect();
                // a page that does not send a section leaves it as it was
                let old = self.prefs(&story);
                let section = |key: &str| body[key].as_object().cloned().or_else(|| old[key].as_object().cloned()).unwrap_or_default();
                let voices = if body["voices"].is_object() { json!(voices) } else { old["voices"].clone() };
                let clean = json!({ "prompts": section("prompts"), "workflows": section("workflows"), "voices": if voices.is_object() { voices } else { json!({}) } });
                let text = serde_json::to_string_pretty(&clean).unwrap_or_default() + "\n";
                answer(stream, std::fs::write(story.with_file_name("art.json"), text).map(|_| json!({ "ok": true })).map_err(|e| e.to_string()))
            }
            ("POST", "/api/art/run") => {
                let queued = self.load(&story).and_then(|(doc, _)| {
                    let mut ov = self.overrides(&story);
                    ov.workflow = body["workflow"].as_str().filter(|w| !w.is_empty()).map(str::to_string);
                    if let Some(take) = body["settings"]["take"].as_u64() {
                        ov.settings.insert("take".into(), json!(take % 1_000_000));
                    }
                    for key in ["denoise", "control_strength", "refine_denoise"] {
                        if let Some(n) = body["settings"][key].as_f64() {
                            ov.settings.insert(key.to_string(), json!(n.clamp(0.0, if key == "control_strength" { 2.0 } else { 1.0 })));
                        }
                    }
                    self.jobs.enqueue(&story_id, &story, &doc, &ids(&body["assets"]), &ov)
                });
                answer(stream, queued.map(|n| json!({ "queued": n })))
            }
            ("POST", "/api/art/cancel") => {
                self.jobs.cancel_all();
                respond_json(stream, "200 OK", &json!({ "ok": true }))
            }
            ("POST", "/api/art/revert") => {
                // go back to the game's own painting; the files are set aside, not deleted
                let dir = comfy::enhanced_dir(&story);
                let mut moved = 0;
                for id in ids(&body["assets"]).iter().filter(|id| valid_id(id)) {
                    if id.starts_with("voice_") || id.starts_with("music_") {
                        if let Some(file) = audio::find(&audio::audio_dir(&story), id) {
                            moved += set_aside(&file) as usize;
                        }
                        continue;
                    }
                    let file = dir.join(format!("{}.png", id));
                    // never on top of an image set aside earlier
                    let mut target = dir.join(".removed").join(format!("{}.png", id));
                    let mut n = 1;
                    while target.exists() {
                        n += 1;
                        target = dir.join(".removed").join(format!("{}-{}.png", id, n));
                    }
                    if file.is_file() && std::fs::create_dir_all(dir.join(".removed")).and_then(|_| std::fs::rename(&file, &target)).is_ok() {
                        moved += 1;
                    }
                }
                self.jobs.version.fetch_add(1, Ordering::Relaxed);
                respond_json(stream, "200 OK", &json!({ "reverted": moved }))
            }
            // ---- sound
            ("GET", "/api/sound") => answer(stream, self.sound_overview(&story)),
            ("GET", "/api/sound/lines") => {
                // every distinct line one speaker says, and whether it is recorded
                let who = q.get("who").cloned().unwrap_or_default();
                let listed = self.load(&story).map(|(doc, _)| {
                    let dir = audio::audio_dir(&story);
                    let mut scene_of: HashMap<String, String> = HashMap::new();
                    for sc in &doc.scenes {
                        for it in &sc.items {
                            if let Item::Say { who: w, text } = it {
                                scene_of.entry(audio::voice_stem(w, text)).or_insert_with(|| sc.label.clone());
                            }
                        }
                    }
                    let lines: Vec<Value> = comfy::audio_assets(&doc)
                        .iter()
                        .filter(|a| a.kind == "voice" && a.who == who)
                        .map(|a| json!({ "id": a.id, "text": a.text, "scene": scene_of.get(&a.id), "file": audio::find(&dir, &a.id).and_then(|p| p.file_name().map(|f| f.to_string_lossy().to_string())) }))
                        .collect();
                    json!({ "lines": lines })
                });
                answer(stream, listed)
            }
            ("GET", "/api/sound/options") => {
                // the stock voices a workflow can use, read from the ComfyUI server itself
                let file = q.get("workflow").cloned().unwrap_or_default();
                let found = self.jobs.suite.workflows().into_iter().find(|w| w.file == file).and_then(|w| w.voice_options);
                let reply = match found {
                    None => json!({ "options": [] }),
                    Some((node, input)) => {
                        let cfg = self.jobs.suite.config().unwrap_or(Value::Null);
                        let client = comfy::Client::new(cfg["server"].as_str().unwrap_or("http://127.0.0.1:8188"), Duration::from_secs(8));
                        match client.options(&node, &input) {
                            Ok(options) => json!({ "options": options }),
                            Err(e) => json!({ "options": [], "error": e }),
                        }
                    }
                };
                respond_json(stream, "200 OK", &reply)
            }
            // ---- making new characters and places with a language model
            ("GET", "/api/ai") => respond_json(stream, "200 OK", &crate::ai::status()),
            ("POST", "/api/ai/character") | ("POST", "/api/ai/place") => {
                let made = (|| -> Result<Value, String> {
                    let provider = crate::ai::Provider::parse(body["provider"].as_str().unwrap_or("")).ok_or("ask Claude or Ollama")?;
                    let model = body["model"].as_str().unwrap_or("");
                    let brief: String = body["brief"].as_str().unwrap_or("").chars().filter(|c| !c.is_control() || *c == '\n').take(2000).collect();
                    if brief.trim().is_empty() {
                        return Err("say what to make".into());
                    }
                    let (doc, _) = self.load(&story)?;
                    let context = crate::ai::story_context(&doc);
                    if req.path == "/api/ai/character" {
                        let taken: Vec<String> = doc.characters.iter().map(|c| c.id.clone()).chain(["n".to_string()]).collect();
                        let user = format!("{}\nMake this character: {}", context, brief.trim());
                        let schema = crate::ai::character_schema();
                        let v = crate::ai::ask(provider, model, &crate::ai::Ask { system: crate::ai::CHARACTER_SYSTEM, user: &user, schema: &schema, max_tokens: 8000 })?;
                        let (def, bio, art_prompt) = crate::ai::character_from(&v, &taken)?;
                        Ok(json!({ "character": def, "bio": bio, "art_prompt": art_prompt }))
                    } else {
                        let want3d = body["want3d"].as_bool().unwrap_or(false);
                        let taken = crate::painting::backgrounds_for(Some(&story));
                        let user = format!("{}The story's places so far: {}.\nPaint this place: {}", context, taken.join(", "), brief.trim());
                        let system = if want3d { format!("{}\n{}", crate::ai::PLACE_SYSTEM, crate::ai::ROOM_HELP) } else { crate::ai::PLACE_SYSTEM.to_string() };
                        let schema = crate::ai::place_schema(want3d);
                        let v = crate::ai::ask(provider, model, &crate::ai::Ask { system: &system, user: &user, schema: &schema, max_tokens: 32000 })?;
                        let (name, painting, room) = crate::ai::place_from(&v, &taken)?;
                        let preview = painting.render().resized(576, 324).png_bytes(false)?;
                        Ok(json!({ "name": name, "painting": painting, "room": room, "preview": format!("data:image/png;base64,{}", base64_encode(&preview)) }))
                    }
                })();
                answer(stream, made)
            }
            ("POST", "/api/ai/place/save") => {
                // a place that was made (and looked at): its painting, and its room if it has one
                let saved = (|| -> Result<Value, String> {
                    let name = body["name"].as_str().unwrap_or("").to_string();
                    if !crate::painting::usable_name(&name) {
                        return Err(format!("'{}' cannot name a new place: lowercase letters, digits and _, and not one of the game's own", name));
                    }
                    let replace = body["replace"].as_bool().unwrap_or(false);
                    if known_place(&story, &name) && !replace {
                        return Err(format!("the story already has a place called '{}'", name));
                    }
                    let painting = serde_json::to_vec(&body["painting"]).ok().and_then(|b| painting_from(&b)).ok_or("this is not a painting")?;
                    crate::painting::save(&story, &name, &painting)?;
                    let mut room_saved = false;
                    if body["room"].is_object() {
                        if let Some(place) = serde_json::to_vec(&body["room"]).ok().and_then(|b| place_from(&b)) {
                            let dir = crate::places::world_dir(&story);
                            std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
                            let _one = WORLD_WRITES.lock().unwrap_or_else(|e| e.into_inner());
                            let (tmp, file) = (dir.join(format!("{}.json.tmp", name)), dir.join(format!("{}.json", name)));
                            std::fs::write(&tmp, serde_json::to_string(&place).unwrap_or_default() + "\n").and_then(|_| std::fs::rename(&tmp, &file)).map_err(|e| e.to_string())?;
                            room_saved = true;
                        }
                    }
                    Ok(json!({ "ok": true, "name": name, "room": room_saved, "bgs": crate::painting::backgrounds_for(Some(&story)), "painted": crate::painting::names(&story) }))
                })();
                answer(stream, saved)
            }
            // ---- 3D places and models
            ("GET", "/api/world") | ("PUT", "/api/world") | ("POST", "/api/world/reset") => {
                use crate::places;
                let name = q.get("place").cloned().or_else(|| body["place"].as_str().map(str::to_string)).unwrap_or_default();
                if !known_place(&story, &name) {
                    return respond_json(stream, "404 Not Found", &json!({ "error": format!("there is no place called '{}'", name) }));
                }
                let dir = places::world_dir(&story);
                let file = dir.join(format!("{}.json", name));
                match req.method.as_str() {
                    "PUT" => {
                        let saved = place_from(&req.body).ok_or("this is not a description of a place".to_string()).and_then(|place| {
                            std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
                            // beside the file, then renamed: a crash cannot leave half a place (and one save at a time)
                            let _one = WORLD_WRITES.lock().unwrap_or_else(|e| e.into_inner());
                            let tmp = dir.join(format!("{}.json.tmp", name));
                            std::fs::write(&tmp, serde_json::to_string(&place).unwrap_or_default() + "\n").and_then(|_| std::fs::rename(&tmp, &file)).map_err(|e| e.to_string())?;
                            Ok(json!({ "ok": true, "place": place }))
                        });
                        answer(stream, saved)
                    }
                    "POST" => {
                        // back to the game's own build; the story's is set aside, not deleted
                        let moved = file.is_file() && set_aside(&file);
                        respond_json(stream, "200 OK", &json!({ "reset": moved, "place": places::builtin(&name) }))
                    }
                    _ => {
                        let own = places::custom(&dir, &name);
                        let error = own.as_ref().and_then(|r| r.as_ref().err().cloned());
                        let place = own.and_then(|r| r.ok());
                        let custom = place.is_some();
                        let mut models: Vec<String> = std::fs::read_dir(dir.join("models")).into_iter().flatten().filter_map(|e| e.ok()).map(|e| e.file_name().to_string_lossy().to_string()).filter(|f| places::model_file(f)).collect();
                        models.sort();
                        let listed: Vec<Value> = crate::painting::backgrounds_for(Some(&story)).iter().map(|b| json!({ "name": b, "custom": dir.join(format!("{}.json", b)).is_file() })).collect();
                        respond_json(stream, "200 OK", &json!({
                            "place": place.or_else(|| places::builtin(&name)).unwrap_or_else(places::PlaceData::blank),
                            "custom": custom, "error": error, "models": models, "places": listed, "blank": places::PlaceData::blank(),
                            "folder": dir.display().to_string(), "engine": cfg!(feature = "world3d"),
                        }))
                    }
                }
            }
            ("POST", "/api/world/model") => {
                // a glTF model for a place or a character, sent as base64
                let name = body["name"].as_str().unwrap_or("").to_string();
                let saved = (|| {
                    if !crate::places::model_file(&name) {
                        return Err("a model is a .glb or .gltf file with a plain name (letters, digits, - and _)".to_string());
                    }
                    let bytes = base64_decode(body["data"].as_str().unwrap_or("")).ok_or("the model did not arrive intact")?;
                    if let Some(outside) = serde_json::from_slice::<Value>(&bytes).ok().map(|v| gltf_outside_files(&v)).filter(|f| !f.is_empty()) {
                        return Err(format!("this model keeps part of itself in other files ({}): save it as a single .glb instead", outside.join(", ")));
                    }
                    if !model_bytes_ok(&name, &bytes) {
                        return Err("this does not look like a glTF model".to_string());
                    }
                    let dir = crate::places::world_dir(&story).join("models");
                    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
                    // a model of that name already there is set aside, not written over
                    if dir.join(&name).is_file() && std::fs::read(dir.join(&name)).map_or(true, |old| old != bytes) {
                        set_aside(&dir.join(&name));
                    }
                    std::fs::write(dir.join(&name), bytes).map_err(|e| e.to_string())?;
                    Ok(json!({ "model": name }))
                })();
                answer(stream, saved)
            }
            ("POST", "/api/sound/sample") => {
                // a recording of a speaker to clone the voice from, sent as base64
                let who = body["who"].as_str().unwrap_or("").to_string();
                let ext = body["name"].as_str().unwrap_or("").rsplit('.').next().unwrap_or("").to_lowercase();
                let saved = (|| {
                    if !valid_id(&who) {
                        return Err("unknown speaker".to_string());
                    }
                    if !comfy::SAMPLE_EXTENSIONS.contains(&ext.as_str()) {
                        return Err("a voice sample must be a WAV, MP3, FLAC, OGG or M4A file".to_string());
                    }
                    let bytes = base64_decode(body["data"].as_str().unwrap_or("")).filter(|b| b.len() > 64).ok_or("the sample did not arrive intact")?;
                    let dir = audio::audio_dir(&story).join("samples");
                    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
                    // the new one is safely written before any older one goes
                    let name = format!("{}.{}", who, ext);
                    std::fs::write(dir.join(&name), bytes).map_err(|e| e.to_string())?;
                    for old in comfy::SAMPLE_EXTENSIONS.iter().filter(|old| **old != ext) {
                        let _ = std::fs::remove_file(dir.join(format!("{}.{}", who, old)));
                    }
                    // and the story remembers it at once
                    let mut prefs = self.prefs(&story);
                    if !prefs.is_object() {
                        prefs = json!({});
                    }
                    if !prefs["voices"].is_object() {
                        prefs["voices"] = json!({});
                    }
                    if !prefs["voices"][&who].is_object() {
                        prefs["voices"][&who] = json!({});
                    }
                    prefs["voices"][&who]["sample"] = json!(name);
                    std::fs::write(story.with_file_name("art.json"), serde_json::to_string_pretty(&prefs).unwrap_or_default() + "\n").map_err(|e| e.to_string())?;
                    Ok(json!({ "sample": name }))
                })();
                answer(stream, saved)
            }
            ("POST", "/api/sound/prune") => {
                // recordings of lines that have since been reworded or cut
                let pruned = self.load(&story).map(|(doc, _)| {
                    let wanted: HashSet<String> = comfy::audio_assets(&doc).into_iter().map(|a| a.id).collect();
                    let mut moved = 0;
                    for entry in std::fs::read_dir(audio::audio_dir(&story)).into_iter().flatten().filter_map(|e| e.ok()) {
                        let path = entry.path();
                        let stem = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
                        if path.is_file() && (stem.starts_with("voice_") || stem.starts_with("music_")) && !wanted.contains(&stem) {
                            moved += set_aside(&path) as usize;
                        }
                    }
                    self.jobs.version.fetch_add(1, Ordering::Relaxed);
                    json!({ "removed": moved })
                });
                answer(stream, pruned)
            }
            ("GET", "/audio") => {
                // play back a recording, a track or a sample in the page
                let file = q.get("file").cloned().unwrap_or_default();
                let parts: Vec<&str> = file.split('/').collect();
                let safe = |p: &str| !p.is_empty() && !p.starts_with('.') && p.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.');
                let ok = match parts.as_slice() {
                    [name] => safe(name),
                    ["samples", name] => safe(name),
                    _ => false,
                };
                let kind = match file.rsplit('.').next().unwrap_or("") {
                    "wav" => "audio/wav",
                    "mp3" => "audio/mpeg",
                    "flac" => "audio/flac",
                    "ogg" => "audio/ogg",
                    "m4a" => "audio/mp4",
                    _ => "",
                };
                // checked before anything is opened: the name comes from the request
                let path = audio::audio_dir(&story).join(&file);
                match Some(path).filter(|p| ok && !kind.is_empty() && p.is_file()).and_then(|p| std::fs::read(p).ok()) {
                    Some(bytes) => respond(stream, "200 OK", kind, false, &bytes),
                    None => respond(stream, "404 Not Found", "text/plain", false, b"no such recording"),
                }
            }
            _ => respond(stream, "404 Not Found", "text/plain", false, b"not found"),
        }
    }

    /// Cache key for a preview: the request without its cache-busting
    /// `v`, plus the timestamp of the enhanced file it would show, so a
    /// preview is repainted only when its own image changes.
    fn art_key(&self, req: &Request, enhanced: Option<PathBuf>) -> String {
        let query: Vec<&str> = req.query.split('&').filter(|kv| !kv.starts_with("v=")).collect();
        let stamp = enhanced.and_then(|f| std::fs::metadata(f).ok()).and_then(|m| m.modified().ok()).and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_nanos());
        format!("{}?{}#{}", req.path, query.join("&"), stamp)
    }

    /// Launch the game on the saved story, starting at a scene.
    fn play(&self, story: &Path, label: &str) -> Result<(), String> {
        let (doc, errors) = self.load(story)?;
        if let Some(e) = errors.first() {
            return Err(e.clone());
        }
        if let Some(p) = problems(&doc, story).iter().find(|p| p.level == "error") {
            return Err(format!("fix the story's errors first ({})", p.msg));
        }
        if !doc.scenes.iter().any(|s| s.label == label) {
            return Err(format!("no scene called '{}'", label));
        }
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        let mut cmd = std::process::Command::new(exe);
        cmd.arg("--story").arg(story).arg("--start").arg(label);
        if self.procedural {
            cmd.arg("--procedural");
        }
        let mut child = cmd.spawn().map_err(|e| format!("could not start the game: {}", e))?;
        std::thread::spawn(move || {
            let _ = child.wait();
        });
        Ok(())
    }
}

fn open_browser(url: &str) {
    let attempt = if cfg!(target_os = "windows") {
        std::process::Command::new("cmd").args(["/C", "start", "", url]).spawn()
    } else if cfg!(target_os = "macos") {
        std::process::Command::new("open").arg(url).spawn()
    } else {
        std::process::Command::new("xdg-open").arg(url).stderr(std::process::Stdio::null()).spawn()
    };
    if let Ok(mut child) = attempt {
        std::thread::spawn(move || {
            let _ = child.wait();
        });
    }
}

/// How the editor is started.
pub struct Options {
    pub story: PathBuf,
    pub stories: PathBuf,
    pub comfy: PathBuf,
    pub port: u16,
    pub procedural: bool,
    pub browser: bool,
    /// Seconds to wait for ComfyUI to finish one image.
    pub job_timeout: u64,
}

/// Run the editor until the process is stopped.
pub fn serve(o: Options) -> Result<(), String> {
    let listener = TcpListener::bind(("127.0.0.1", o.port)).map_err(|e| format!("cannot listen on 127.0.0.1:{}: {} (try --port N)", o.port, e))?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    let url = format!("http://127.0.0.1:{}/", port);
    println!("story editor: {}\nmain story:   {}\nmore stories: {}\npress Ctrl+C to stop", url, o.story.display(), o.stories.display());
    if o.browser {
        open_browser(&url);
    }
    let jobs = Jobs::start(comfy::Suite { dir: o.comfy }, Duration::from_secs(o.job_timeout));
    let state = Arc::new(State {
        library: Library { main: o.story, dir: o.stories },
        port,
        procedural: o.procedural,
        jobs,
        cache: Mutex::new(HashMap::new()),
        backed_up: Mutex::new(HashSet::new()),
    });
    let active = Arc::new(AtomicUsize::new(0));
    for stream in listener.incoming() {
        let Ok(mut stream) = stream else {
            std::thread::sleep(Duration::from_millis(50));
            continue;
        };
        // a browser opens a handful of connections; far more than that is not a browser
        if active.load(Ordering::Relaxed) >= 64 {
            continue;
        }
        let (state, active) = (state.clone(), active.clone());
        active.fetch_add(1, Ordering::Relaxed);
        let counter = active.clone();
        let spawned = std::thread::Builder::new().spawn(move || {
            let _ = stream.set_read_timeout(Some(Duration::from_secs(10)));
            let _ = stream.set_write_timeout(Some(Duration::from_secs(20)));
            if let Some(req) = read_request(&mut stream) {
                state.handle(req, &mut stream);
            }
            active.fetch_sub(1, Ordering::Relaxed);
        });
        if spawned.is_err() {
            counter.fetch_sub(1, Ordering::Relaxed);
        }
    }
    Ok(())
}
