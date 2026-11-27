//! Sending the game's art through ComfyUI from the editor.
//!
//! This is the editor's counterpart of comfyui/enhance.py and shares its
//! files: workflow templates (comfyui/workflows/*.json), settings
//! (comfyui/config.json) and node signatures (comfyui/node_specs.json).
//! Copies of all three are built in, for when the binary runs elsewhere.
//!
//! A job paints the guide images for one asset of one story, uploads them,
//! queues the chosen workflow, waits, and saves the result into that
//! story's `enhanced` folder, where the game picks it up.

use crate::art;
use crate::audio;
use crate::gfx::{rgb, Canvas, BLACK, H, W};
use crate::script::{CharDef, Doc};
use serde::Serialize;
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

const BUILT_IN_CONFIG: &str = include_str!("../comfyui/config.json");
const BUILT_IN_SPECS: &str = include_str!("../comfyui/node_specs.json");
const BUILT_IN_WORKFLOWS: &[(&str, &str)] = &[
    ("background.json", include_str!("../comfyui/workflows/background.json")),
    ("background_reimagine.json", include_str!("../comfyui/workflows/background_reimagine.json")),
    ("background_refine.json", include_str!("../comfyui/workflows/background_refine.json")),
    ("sprite_base.json", include_str!("../comfyui/workflows/sprite_base.json")),
    ("sprite_reimagine.json", include_str!("../comfyui/workflows/sprite_reimagine.json")),
    ("sprite_expression.json", include_str!("../comfyui/workflows/sprite_expression.json")),
    ("voice_elevenlabs.json", include_str!("../comfyui/workflows/voice_elevenlabs.json")),
    ("voice_elevenlabs_clone.json", include_str!("../comfyui/workflows/voice_elevenlabs_clone.json")),
    ("voice_f5_clone.json", include_str!("../comfyui/workflows/voice_f5_clone.json")),
    ("music_stable_audio.json", include_str!("../comfyui/workflows/music_stable_audio.json")),
    ("music_ace_step.json", include_str!("../comfyui/workflows/music_ace_step.json")),
];
const PNG_MAGIC: &[u8] = b"\x89PNG\r\n\x1a\n";

fn read_json(path: &Path) -> Result<Value, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {}", path.display(), e))?;
    serde_json::from_str(&text).map_err(|e| format!("{} is not valid JSON: {}", path.display(), e))
}

/// The shared ComfyUI files, read from a folder with built-in fallbacks.
pub struct Suite {
    pub dir: PathBuf,
}

#[derive(Serialize, Clone)]
pub struct WorkflowInfo {
    pub file: String,
    pub title: String,
    /// background, sprite (neutral portrait), expression, voice or music.
    pub kind: String,
    pub description: String,
    /// For a voice workflow that speaks from a recording: it needs a sample.
    pub needs_sample: bool,
    /// ... and the words spoken in that sample.
    pub needs_sample_text: bool,
    /// Where the list of stock voices can be read on the server: [node, input].
    pub voice_options: Option<(String, String)>,
}

impl Suite {
    pub fn config(&self) -> Result<Value, String> {
        let path = self.dir.join("config.json");
        if path.is_file() {
            read_json(&path)
        } else {
            Ok(serde_json::from_str(BUILT_IN_CONFIG).expect("built-in config"))
        }
    }

    /// Change a few top-level settings, leaving the rest of the file alone.
    pub fn update_config(&self, changes: &Map<String, Value>) -> Result<(), String> {
        let mut cfg = self.config()?;
        let obj = cfg.as_object_mut().ok_or("config.json must be an object")?;
        for key in ["server", "checkpoint", "controlnet"] {
            if let Some(v) = changes.get(key).and_then(Value::as_str) {
                obj.insert(key.to_string(), json!(v.trim()));
            }
        }
        std::fs::create_dir_all(&self.dir).map_err(|e| format!("{}: {}", self.dir.display(), e))?;
        let text = serde_json::to_string_pretty(&cfg).map_err(|e| e.to_string())? + "\n";
        std::fs::write(self.dir.join("config.json"), text).map_err(|e| e.to_string())
    }

    fn specs(&self) -> Result<Value, String> {
        let path = self.dir.join("node_specs.json");
        let all = if path.is_file() { read_json(&path)? } else { serde_json::from_str(BUILT_IN_SPECS).expect("built-in specs") };
        Ok(all["nodes"].clone())
    }

    fn template(&self, file: &str) -> Result<Value, String> {
        if file.contains(['/', '\\']) || file.contains("..") || !file.ends_with(".json") {
            return Err(format!("'{}' is not a workflow file name", file));
        }
        let path = self.dir.join("workflows").join(file);
        if path.is_file() {
            return read_json(&path);
        }
        BUILT_IN_WORKFLOWS
            .iter()
            .find(|(name, _)| *name == file)
            .map(|(_, text)| serde_json::from_str(text).expect("built-in workflow"))
            .ok_or_else(|| format!("no workflow called '{}'", file))
    }

    /// Every workflow on offer: the files in the workflows folder, or the
    /// built-in set when that folder does not exist.
    pub fn workflows(&self) -> Vec<WorkflowInfo> {
        let folder = self.dir.join("workflows");
        let mut names: Vec<String> = match std::fs::read_dir(&folder) {
            Ok(entries) => entries.filter_map(|e| e.ok()).map(|e| e.file_name().to_string_lossy().to_string()).filter(|n| n.ends_with(".json")).collect(),
            Err(_) => BUILT_IN_WORKFLOWS.iter().map(|(n, _)| n.to_string()).collect(),
        };
        names.sort();
        names
            .into_iter()
            .filter_map(|file| {
                let t = self.template(&file).ok()?;
                let kind = t["kind"].as_str()?.to_string();
                let raw = t["prompt"].to_string();
                Some(WorkflowInfo {
                    title: t["title"].as_str().unwrap_or(&file).to_string(),
                    description: t["description"].as_str().unwrap_or("").to_string(),
                    needs_sample: raw.contains("\"$sample\""),
                    needs_sample_text: raw.contains("\"$sample_text\""),
                    voice_options: t["voice_options"]["node"].as_str().zip(t["voice_options"]["input"].as_str()).map(|(n, i)| (n.to_string(), i.to_string())),
                    kind,
                    file,
                })
            })
            .collect()
    }
}

// ------------------------------------------------------------- assets

/// One image the game uses: a background, or one expression of a character.
#[derive(Clone)]
pub struct Asset {
    pub id: String,
    /// background, sprite (a neutral portrait) or expression.
    pub kind: &'static str,
    pub name: String,
    pub character: Option<CharDef>,
    pub expr: String,
    /// What it depicts, for the text prompt.
    pub prompt: String,
    /// Key under which a story stores its own prompt for this subject.
    pub prompt_key: String,
    /// For a voice line: who says it ("n" for the narrator) and the words.
    pub who: String,
    pub text: String,
    /// For music: how long, in seconds (0 = the workflow's default).
    pub seconds: i32,
    /// For a place the story painted for itself: that painting.
    pub painting: Option<Box<crate::painting::PaintingData>>,
}

impl Asset {
    pub fn is_audio(&self) -> bool {
        self.kind == "voice" || self.kind == "music"
    }

    /// File name of an image asset. (Sound takes its extension from what ComfyUI returns.)
    pub fn output(&self) -> String {
        format!("{}.png", self.id)
    }
}

/// Everything a story can be heard to do: its music tracks, and every
/// distinct line of dialogue (a line said twice is recorded once).
pub fn audio_assets(doc: &Doc) -> Vec<Asset> {
    let blank = |id: String, kind: &'static str| Asset {
        id,
        kind,
        name: String::new(),
        character: None,
        expr: String::new(),
        prompt: String::new(),
        prompt_key: String::new(),
        who: String::new(),
        text: String::new(),
        seconds: 0,
        painting: None,
    };
    let mut out = Vec::new();
    for t in doc.tracks.iter().filter(|t| crate::script::valid_id(&t.id)) {
        out.push(Asset { name: if t.title.is_empty() { t.id.clone() } else { t.title.clone() }, prompt: t.prompt.clone(), seconds: t.seconds, ..blank(audio::music_stem(&t.id), "music") });
    }
    let mut seen = std::collections::HashSet::new();
    for scene in &doc.scenes {
        for item in &scene.items {
            let crate::script::Item::Say { who, text } = item else { continue };
            let who_ok = who == crate::script::NARRATOR || doc.characters.iter().any(|c| &c.id == who && crate::script::valid_id(&c.id));
            let stem = audio::voice_stem(who, text);
            if !who_ok || text.trim().is_empty() || !seen.insert(stem.clone()) {
                continue;
            }
            let name = doc.characters.iter().find(|c| &c.id == who).map_or("Narration".to_string(), |c| if c.name.is_empty() { c.id.clone() } else { c.name.clone() });
            out.push(Asset { name, who: who.clone(), text: text.trim().to_string(), ..blank(stem, "voice") });
        }
    }
    out
}

/// Pictures and sound together.
#[cfg(test)]
pub fn all_assets(doc: &Doc) -> Vec<Asset> {
    let mut all = assets_for(doc);
    all.extend(audio_assets(doc));
    all
}

/// Every asset a story can show: all backgrounds, and six expressions of
/// each character that has a portrait.
pub fn assets_for(doc: &Doc) -> Vec<Asset> {
    let mut out = Vec::new();
    for name in art::BACKGROUNDS {
        out.push(Asset {
            id: format!("bg_{}", name),
            kind: "background",
            name: name.to_string(),
            character: None,
            expr: String::new(),
            prompt: art::bg_prompt(name).to_string(),
            prompt_key: format!("bg_{}", name),
            who: String::new(),
            text: String::new(),
            seconds: 0,
            painting: None,
        });
    }
    for ch in doc.characters.iter().filter(|c| art::SPRITES.contains(&c.sprite.as_str()) && crate::script::valid_id(&c.id)) {
        for expr in art::EXPRS {
            out.push(Asset {
                id: format!("spr_{}_{}", ch.id, expr),
                kind: if *expr == "neutral" { "sprite" } else { "expression" },
                name: ch.name.clone(),
                character: Some(ch.clone()),
                expr: expr.to_string(),
                prompt: art::sprite_prompt(&ch.sprite).to_string(),
                prompt_key: format!("spr_{}", ch.id),
                who: String::new(),
                text: String::new(),
                seconds: 0,
                painting: None,
            });
        }
    }
    out
}

/// Every picture asset of a story, with the places it painted for itself.
pub fn assets_of(doc: &Doc, story: &Path) -> Vec<Asset> {
    let mut out = assets_for(doc);
    let at = out.iter().position(|a| a.character.is_some()).unwrap_or(out.len());
    let own: Vec<Asset> = crate::painting::names(story)
        .into_iter()
        .filter_map(|name| crate::painting::load(story, &name).map(|p| (name, p)))
        .map(|(name, p)| Asset {
            id: format!("bg_{}", name),
            kind: "background",
            prompt: if p.prompt.is_empty() { format!("{}, painted background, no people", if p.title.is_empty() { name.replace('_', " ") } else { p.title.clone() }) } else { p.prompt.clone() },
            prompt_key: format!("bg_{}", name),
            name,
            character: None,
            expr: String::new(),
            who: String::new(),
            text: String::new(),
            seconds: 0,
            painting: Some(Box::new(p)),
        })
        .collect();
    out.splice(at..at, own);
    out
}

/// Pictures and sound together, with the story's own places.
pub fn all_assets_of(doc: &Doc, story: &Path) -> Vec<Asset> {
    let mut all = assets_of(doc, story);
    all.extend(audio_assets(doc));
    all
}

/// The game's own painting of an asset, as the workflows want it.
/// `role` is guide, silhouette or face.
fn painted(asset: &Asset, role: &str) -> Result<Vec<u8>, String> {
    match &asset.character {
        None => {
            let (mut c, animated) = match &asset.painting {
                Some(p) => (p.render(), false),
                None => art::background(&asset.name, None),
            };
            if animated {
                art::animate(&mut c, &asset.name, 0.5);
            }
            debug_assert!(c.w == W && c.h == H);
            c.png_bytes(false)
        }
        Some(ch) => {
            let sprite = art::procedural_sprite(&ch.sprite, &ch.look, &asset.expr);
            let mut c = Canvas::new(sprite.w, sprite.h);
            match role {
                "silhouette" => {
                    for (m, p) in c.px.iter_mut().zip(sprite.px.iter()) {
                        let a = (p >> 24) as u8;
                        *m = rgb(a, a, a);
                    }
                }
                "face" => {
                    let (x, y, w, h) = art::face_rect(&ch.sprite);
                    c.clear(BLACK);
                    c.ellipse(x + w / 2, y + h / 2, w / 2, h / 2, rgb(255, 255, 255));
                }
                _ => {
                    // the sprite on neutral grey: the img2img starting image
                    c.clear(rgb(128, 128, 128));
                    c.blit(&sprite, 0, 0, false, 256);
                }
            }
            c.png_bytes(false)
        }
    }
}

// ---------------------------------------------------------- workflows

fn glob_match(pattern: &str, text: &str) -> bool {
    match pattern.split_once('*') {
        None => pattern == text,
        Some((head, tail)) => text.len() >= head.len() && text.starts_with(head) && {
            let rest = &text[head.len()..];
            (0..=rest.len()).filter(|i| rest.is_char_boundary(*i)).any(|i| glob_match(tail, &rest[i..]))
        },
    }
}

fn fill(node: &Value, values: &Map<String, Value>) -> Result<Value, String> {
    Ok(match node {
        Value::Object(o) => Value::Object(o.iter().map(|(k, v)| Ok((k.clone(), fill(v, values)?))).collect::<Result<_, String>>()?),
        Value::Array(a) => Value::Array(a.iter().map(|v| fill(v, values)).collect::<Result<_, String>>()?),
        Value::String(s) if s.starts_with('$') => values.get(&s[1..]).cloned().ok_or_else(|| format!("the workflow needs a value for '{}' (set it in comfyui/config.json)", &s[1..]))?,
        other => other.clone(),
    })
}

/// CRC-32 as zlib computes it, so seeds match comfyui/enhance.py.
pub(crate) fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &byte in data {
        crc ^= byte as u32;
        for _ in 0..8 {
            crc = if crc & 1 != 0 { (crc >> 1) ^ 0xEDB8_8320 } else { crc >> 1 };
        }
    }
    !crc
}

/// Feed the guide straight to the ControlNet, without edge detection.
fn drop_canny(prompt: &mut Map<String, Value>) {
    let ids: Vec<String> = prompt.iter().filter(|(_, n)| n["class_type"] == "Canny").map(|(k, _)| k.clone()).collect();
    for id in ids {
        let source = prompt[&id]["inputs"]["image"].clone();
        prompt.remove(&id);
        for node in prompt.values_mut() {
            if let Some(inputs) = node.get_mut("inputs").and_then(Value::as_object_mut) {
                for v in inputs.values_mut() {
                    if v.as_array().map_or(false, |a| a.len() == 2 && a[0] == json!(id)) {
                        *v = source.clone();
                    }
                }
            }
        }
    }
}

/// A workflow ready to queue.
#[derive(Debug)]
pub struct Built {
    /// Node packs or services the workflow relies on beyond plain ComfyUI.
    pub requires: Vec<(String, String)>,
    pub prompt: Value,
    pub output_node: String,
    /// (name in ComfyUI's input folder, role) for each image to upload.
    pub uploads: Vec<(String, String)>,
    pub workflow: String,
}

/// Choices made in the editor for one run.
#[derive(Clone, Default)]
pub struct Overrides {
    pub workflow: Option<String>,
    /// Settings such as denoise that should beat the config for this run.
    pub settings: Map<String, Value>,
    /// The story's own prompts, by `Asset::prompt_key`.
    pub prompts: BTreeMap<String, String>,
    /// The story's preferred workflow per kind.
    pub workflows: BTreeMap<String, String>,
    /// How each speaker sounds, by character id ("n" for the narrator).
    pub voices: BTreeMap<String, Voice>,
}

/// Formats a voice sample may be in.
pub const SAMPLE_EXTENSIONS: [&str; 5] = ["wav", "mp3", "flac", "ogg", "m4a"];

/// A plain file name (no folders, not hidden) with one of these extensions.
pub fn plain_file(name: &str, extensions: &[&str]) -> bool {
    let ok = |c: char| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.';
    // names that Windows takes for devices, whatever follows the first dot
    let device = name.split('.').next().map_or(false, |first| {
        let first = first.to_ascii_lowercase();
        ["con", "prn", "aux", "nul"].contains(&first.as_str()) || (first.len() == 4 && (first.starts_with("com") || first.starts_with("lpt")) && first.ends_with(|c: char| c.is_ascii_digit()))
    });
    !device && !name.starts_with('.') && !name.ends_with('.') && name.chars().all(ok) && name.rsplit_once('.').map_or(false, |(stem, ext)| !stem.is_empty() && extensions.contains(&ext))
}

/// The voices saved with a story, read entry by entry so that one odd
/// entry costs only itself. File names are checked here: a story folder
/// from somewhere else must not be able to point a sample at another file.
pub fn voices_from(saved: &Value) -> BTreeMap<String, Voice> {
    let text = |v: &Value| v.as_str().unwrap_or("").trim().to_string();
    saved
        .as_object()
        .into_iter()
        .flatten()
        .filter(|(_, v)| v.is_object())
        .map(|(id, v)| {
            let voice = Voice {
                workflow: Some(text(&v["workflow"])).filter(|w| plain_file(w, &["json"])).unwrap_or_default(),
                voice: text(&v["voice"]),
                sample: Some(text(&v["sample"])).filter(|f| plain_file(f, &SAMPLE_EXTENSIONS)).unwrap_or_default(),
                sample_text: text(&v["sample_text"]),
                speed: v["speed"].as_f64().or_else(|| v["speed"].as_str().and_then(|t| t.trim().parse().ok())).filter(|n| n.is_finite()).map_or(0.0, |n| if n > 0.0 { n.clamp(0.5, 2.0) } else { 0.0 }),
            };
            (id.clone(), voice)
        })
        .collect()
}

/// One speaker's voice: a stock voice for ordinary text to speech, or a
/// sample recording to clone, depending on the workflow.
#[derive(Clone, Default, Debug, serde::Deserialize, Serialize)]
pub struct Voice {
    #[serde(default)]
    pub workflow: String,
    /// Name of a stock voice, for workflows that take one.
    #[serde(default)]
    pub voice: String,
    /// File name of the sample recording, in the story's audio/samples folder.
    #[serde(default)]
    pub sample: String,
    /// The words spoken in the sample (some cloning models need them).
    #[serde(default)]
    pub sample_text: String,
    #[serde(default)]
    pub speed: f64,
}

/// Fill in the workflow for an asset. `tag` keeps uploads and outputs of
/// different stories apart on the ComfyUI side.
pub fn build(suite: &Suite, asset: &Asset, tag: &str, ov: &Overrides) -> Result<Built, String> {
    let cfg = suite.config()?;
    let mut s = Map::new();
    let merge = |s: &mut Map<String, Value>, from: &Value| {
        if let Some(o) = from.as_object() {
            for (k, v) in o {
                s.insert(k.clone(), v.clone());
            }
        }
    };
    for (k, v) in cfg.as_object().ok_or("config.json must be an object")? {
        if k != "kinds" && k != "overrides" {
            s.insert(k.clone(), v.clone());
        }
    }
    if asset.kind == "expression" {
        merge(&mut s, &cfg["kinds"]["sprite"]);
    }
    merge(&mut s, &cfg["kinds"][asset.kind]);
    let mut explicit = Map::new();
    if let Some(overrides) = cfg["overrides"].as_object() {
        for (pattern, extra) in overrides {
            if glob_match(pattern, &asset.id) {
                merge(&mut explicit, extra);
            }
        }
    }
    // most specific wins: this send, then the story's choice, then a per-asset
    // override in config.json, then the kind's default
    let voice = ov.voices.get(&asset.who).cloned().unwrap_or_default();
    let workflow = ov
        .workflow
        .clone()
        .or_else(|| Some(voice.workflow.clone()).filter(|w| asset.kind == "voice" && !w.is_empty()))
        .or_else(|| ov.workflows.get(asset.kind).cloned())
        .or_else(|| explicit.get("workflow").and_then(Value::as_str).map(str::to_string))
        .or_else(|| s.get("workflow").and_then(Value::as_str).map(str::to_string));
    let workflow = workflow.ok_or_else(|| format!("config.json names no workflow for {}s", asset.kind))?;
    let template = suite.template(&workflow)?;
    if template["kind"].as_str().map_or(false, |k| k != asset.kind) {
        return Err(format!("'{}' is a workflow for {}s, not for this {}", workflow, template["kind"].as_str().unwrap_or("?"), asset.kind));
    }
    merge(&mut s, &template["defaults"]);
    merge(&mut s, &Value::Object(explicit));
    merge(&mut s, &Value::Object(ov.settings.clone()));

    let text = |key: &str| s.get(key).and_then(Value::as_str).unwrap_or("").to_string();
    let (style_prefix, style_suffix) = (text("style_prefix"), text("style_suffix"));
    let mut subject = ov.prompts.get(&asset.prompt_key).filter(|p| !p.trim().is_empty()).cloned().unwrap_or_else(|| asset.prompt.clone());
    if asset.character.is_some() {
        subject = format!("{}, {}", subject, art::expr_prompt(&asset.expr));
    }
    if asset.kind == "voice" {
        // the line itself, and how this speaker sounds
        s.insert("text".into(), json!(asset.text));
        if !voice.voice.is_empty() {
            s.insert("voice".into(), json!(voice.voice));
        }
        if voice.speed > 0.0 {
            s.insert("speed".into(), json!(voice.speed));
        }
        s.insert("sample_text".into(), json!(voice.sample_text));
    }
    if asset.kind == "music" && asset.seconds > 0 {
        s.insert("seconds".into(), json!(asset.seconds));
    }
    // one seed per character, so every expression shares its neutral's noise (and every line its voice)
    let seed_key = if asset.kind == "voice" { asset.who.as_str() } else { asset.character.as_ref().map_or(asset.id.as_str(), |c| c.id.as_str()) };
    // "take" asks for a different attempt at the same thing (a line or a track being redone)
    let take = if asset.is_audio() { s.get("take").and_then(Value::as_u64).unwrap_or(0) } else { 0 };
    let seed = (s.get("seed").and_then(Value::as_u64).unwrap_or(0) + (crc32(seed_key.as_bytes()) % 100_000) as u64 + take) % (1 << 63);
    s.insert("positive".into(), json!(format!("{}{}{}", style_prefix, subject, style_suffix)));
    s.insert("seed".into(), json!(seed));
    s.insert("prefix".into(), json!(format!("below_stairs/{}/{}", tag, asset.id)));

    let raw = serde_json::to_string(&template["prompt"]).unwrap_or_default();
    let mut uploads = Vec::new();
    let roles: &[&str] = match asset.kind {
        "background" => &["guide"],
        "sprite" => &["guide", "silhouette"],
        "voice" => &["sample"],
        "music" => &[],
        _ => &["guide", "silhouette", "face", "base"],
    };
    for role in roles {
        if !raw.contains(&format!("\"${}\"", role)) {
            continue; // this workflow does not use that file
        }
        if *role == "sample" {
            // a recording of the speaker, uploaded once per speaker under its own name
            if !plain_file(&voice.sample, &SAMPLE_EXTENSIONS) {
                return Err(format!("'{}' speaks from a sample recording: add one for {} first", workflow, asset.name));
            }
            if raw.contains("\"$sample_text\"") && voice.sample_text.trim().is_empty() {
                return Err(format!("'{}' also needs the words spoken in {}'s sample", workflow, asset.name));
            }
            let ext = voice.sample.rsplit('.').next().unwrap_or("wav");
            let name = format!("bsas_{}_sample_{}.{}", tag, asset.who, ext);
            s.insert("sample".into(), json!(name));
            uploads.push((name, "sample".to_string()));
            continue;
        }
        let role = if *role == "guide" && template["guide"] == "enhanced" { "current" } else { role };
        let name = format!("bsas_{}_{}_{}.png", tag, asset.id, role);
        s.insert(if role == "current" { "guide".to_string() } else { role.to_string() }, json!(name));
        uploads.push((name, role.to_string()));
    }
    let mut prompt = fill(&template["prompt"], &s)?;
    match s.get("control_preprocess").and_then(Value::as_str).unwrap_or("canny") {
        "canny" => {}
        "none" => drop_canny(prompt.as_object_mut().ok_or("workflow prompt must be an object")?),
        other => return Err(format!("control_preprocess must be 'canny' or 'none', not '{}'", other)),
    }
    let output_node = template["output_node"].as_str().ok_or("workflow has no output_node")?.to_string();
    let out_class = prompt[&output_node]["class_type"].as_str().unwrap_or("");
    let savers: &[&str] = if asset.is_audio() { &["SaveAudio", "SaveAudioMP3"] } else { &["SaveImage"] };
    if !savers.contains(&out_class) {
        return Err(format!("{}: output_node '{}' must be the id of its {} node", workflow, output_node, savers[0]));
    }
    let specs = suite.specs()?;
    let errors = validate_graph(&prompt, &specs);
    if !errors.is_empty() {
        return Err(format!("{}: {}", workflow, errors.join("; ")));
    }
    let mut requires: Vec<(String, String)> = Vec::new();
    for node in prompt.as_object().into_iter().flat_map(|o| o.values()) {
        let cls = node["class_type"].as_str().unwrap_or("");
        if let Some(what) = specs[cls]["requires"].as_str() {
            if !requires.iter().any(|(c, _)| c == cls) {
                requires.push((cls.to_string(), what.to_string()));
            }
        }
    }
    Ok(Built { requires, prompt, output_node, uploads, workflow })
}

/// Check a prompt graph against the node signatures; returns error strings.
pub fn validate_graph(prompt: &Value, specs: &Value) -> Vec<String> {
    let mut errors = Vec::new();
    let Some(nodes) = prompt.as_object() else { return vec!["the workflow is not an object of nodes".into()] };
    for (id, node) in nodes {
        let cls = node["class_type"].as_str().unwrap_or("?");
        let at = format!("node {} ({})", id, cls);
        let Some(spec) = specs[cls]["inputs"].as_object() else {
            errors.push(format!("{}: unknown node class", at));
            continue;
        };
        let empty = Map::new();
        let given = node["inputs"].as_object().unwrap_or(&empty);
        for name in given.keys().filter(|n| !spec.contains_key(*n)) {
            errors.push(format!("{}: unexpected input '{}'", at, name));
        }
        for (name, info) in spec {
            let Some(value) = given.get(name) else {
                if info["optional"] != json!(true) {
                    errors.push(format!("{}: missing input '{}'", at, name));
                }
                continue;
            };
            let kind = info["type"].as_str().unwrap_or("");
            if let Some(link) = value.as_array() {
                let source = link.first().and_then(Value::as_str).and_then(|src| nodes.get(src));
                match (source, link.get(1).and_then(Value::as_u64)) {
                    (Some(src), Some(index)) if link.len() == 2 => {
                        let outs = &specs[src["class_type"].as_str().unwrap_or("")]["outputs"];
                        match outs.get(index as usize).and_then(Value::as_str) {
                            Some(t) if t == kind => {}
                            Some(t) => errors.push(format!("{}: input '{}' wants {} but is linked to a {}", at, name, kind, t)),
                            None if outs.is_array() => errors.push(format!("{}: input '{}' uses an output that does not exist", at, name)),
                            None => {}
                        }
                    }
                    _ => errors.push(format!("{}: input '{}' links to a missing node", at, name)),
                }
                continue;
            }
            let in_range = |v: f64| info["min"].as_f64().map_or(true, |m| v >= m) && info["max"].as_f64().map_or(true, |m| v <= m);
            let ok = match kind {
                "INT" => value.as_i64().map_or(value.as_u64().is_some(), |v| in_range(v as f64)),
                "FLOAT" => value.as_f64().map_or(false, in_range),
                "BOOLEAN" => value.is_boolean(),
                "STRING" => value.is_string(),
                "COMBO" => value.as_str().map_or(false, |v| info["options"].as_array().map_or(true, |opts| opts.iter().any(|o| o == v))),
                _ => false, // a link was required
            };
            if !ok {
                errors.push(format!("{}: '{}' = {} is not a valid {}", at, name, value, kind));
            }
        }
    }
    errors
}

// ------------------------------------------------------------- client

/// Talks to a ComfyUI server over its HTTP API.
pub struct Client {
    server: String,
    agent: ureq::Agent,
}

fn explain(e: ureq::Error, server: &str) -> String {
    match e {
        ureq::Error::Status(code, resp) => {
            let body = resp.into_string().unwrap_or_default();
            format!("ComfyUI answered {}: {}", code, describe_error(&body))
        }
        ureq::Error::Transport(t) => format!("cannot reach ComfyUI at {} ({})", server, t),
    }
}

/// Turn ComfyUI's error JSON into one readable line.
fn describe_error(body: &str) -> String {
    let Ok(data) = serde_json::from_str::<Value>(body) else { return body.trim().chars().take(300).collect() };
    let mut parts = Vec::new();
    match &data["error"] {
        Value::Object(e) => parts.push(format!("{} {}", e.get("message").and_then(Value::as_str).unwrap_or(""), e.get("details").and_then(Value::as_str).unwrap_or("")).trim().to_string()),
        Value::String(e) => parts.push(e.clone()),
        _ => {}
    }
    if let Some(nodes) = data["node_errors"].as_object() {
        for (id, info) in nodes {
            for e in info["errors"].as_array().into_iter().flatten() {
                parts.push(format!("node {} ({}): {} {}", id, info["class_type"].as_str().unwrap_or("?"), e["message"].as_str().unwrap_or(""), e["details"].as_str().unwrap_or("")).trim().to_string());
            }
        }
    }
    parts.retain(|p| !p.is_empty());
    if parts.is_empty() {
        body.trim().chars().take(300).collect()
    } else {
        parts.join("; ")
    }
}

impl Client {
    pub fn new(server: &str, timeout: Duration) -> Client {
        let agent = ureq::AgentBuilder::new().timeout_connect(Duration::from_secs(3)).timeout(timeout).build();
        Client { server: server.trim_end_matches('/').to_string(), agent }
    }

    fn get(&self, path: &str) -> Result<Value, String> {
        let resp = self.agent.get(&format!("{}{}", self.server, path)).call().map_err(|e| explain(e, &self.server))?;
        resp.into_json().map_err(|e| format!("ComfyUI gave an unreadable answer for {}: {}", path, e))
    }

    /// The names a loader node offers (checkpoints, ControlNets).
    pub fn model_list(&self, node: &str, input: &str) -> Result<Vec<String>, String> {
        let info = self.get(&format!("/object_info/{}", node))?;
        let entry = &info[node]["input"]["required"][input];
        // old servers send [[names...], {...}], newer ones ["COMBO", {"options": [...]}]
        let list = entry[0].as_array().or_else(|| entry[1]["options"].as_array());
        Ok(list.into_iter().flatten().filter_map(|v| v.as_str().map(str::to_string)).collect())
    }

    /// Put an image in ComfyUI's input folder, replacing any old copy.
    fn upload(&self, name: &str, png: &[u8]) -> Result<(), String> {
        let boundary = format!("----belowstairs{:016x}", png.len() as u64 * 2654435761 + name.len() as u64);
        let mut body = Vec::with_capacity(png.len() + 512);
        for (field, value) in [("overwrite", "true"), ("type", "input")] {
            body.extend_from_slice(format!("--{}\r\nContent-Disposition: form-data; name=\"{}\"\r\n\r\n{}\r\n", boundary, field, value).as_bytes());
        }
        body.extend_from_slice(format!("--{}\r\nContent-Disposition: form-data; name=\"image\"; filename=\"{}\"\r\nContent-Type: image/png\r\n\r\n", boundary, name).as_bytes());
        body.extend_from_slice(png);
        body.extend_from_slice(format!("\r\n--{}--\r\n", boundary).as_bytes());
        let resp = self
            .agent
            .post(&format!("{}/upload/image", self.server))
            .set("Content-Type", &format!("multipart/form-data; boundary={}", boundary))
            .send_bytes(&body)
            .map_err(|e| explain(e, &self.server))?;
        let stored: Value = resp.into_json().map_err(|e| e.to_string())?;
        if stored["name"] != json!(name) {
            return Err(format!("ComfyUI stored the upload as {} instead of '{}'", stored["name"], name));
        }
        Ok(())
    }

    /// Whether the server has a node class (for workflows that need extras).
    fn has_node(&self, class: &str) -> Result<bool, String> {
        Ok(self.get(&format!("/object_info/{}", class))?.get(class).is_some())
    }

    /// The choices a combo input offers on this server (stock voices, say).
    pub fn options(&self, node: &str, input: &str) -> Result<Vec<String>, String> {
        self.model_list(node, input)
    }

    fn queue(&self, prompt: &Value, client_id: &str) -> Result<String, String> {
        let mut payload = json!({ "prompt": prompt, "client_id": client_id });
        // partner nodes (paid services) are billed to a Comfy account; the key is read
        // from the environment only and never written anywhere
        if let Ok(key) = std::env::var("COMFY_ORG_API_KEY") {
            if !key.trim().is_empty() {
                payload["extra_data"] = json!({ "api_key_comfy_org": key.trim() });
            }
        }
        let resp = self.agent.post(&format!("{}/prompt", self.server)).send_json(payload).map_err(|e| explain(e, &self.server))?;
        let answer: Value = resp.into_json().map_err(|e| e.to_string())?;
        if answer["node_errors"].as_object().map_or(false, |o| !o.is_empty()) {
            return Err(format!("ComfyUI rejected the workflow: {}", describe_error(&answer.to_string())));
        }
        answer["prompt_id"].as_str().map(str::to_string).ok_or_else(|| "ComfyUI did not return a prompt id".to_string())
    }

    /// Take our prompt off ComfyUI's queue, and stop it if it is the one running.
    fn abandon(&self, prompt_id: &str) {
        let _ = self.agent.post(&format!("{}/queue", self.server)).send_json(json!({ "delete": [prompt_id] }));
        let _ = self.agent.post(&format!("{}/interrupt", self.server)).send_json(json!({ "prompt_id": prompt_id }));
    }

    /// Wait for a queued prompt; returns its first output image record.
    fn wait(&self, prompt_id: &str, output_node: &str, timeout: Duration, cancelled: &dyn Fn() -> bool) -> Result<Value, String> {
        let deadline = Instant::now() + timeout;
        loop {
            if cancelled() {
                self.abandon(prompt_id);
                return Err("cancelled".into());
            }
            let history = self.get(&format!("/history/{}", prompt_id))?;
            if let Some(entry) = history.get(prompt_id) {
                let status = &entry["status"];
                if status["status_str"] == "error" {
                    let detail = status["messages"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .find(|m| m[0] == "execution_error")
                        .map(|m| format!("node {} ({}): {}", m[1]["node_id"], m[1]["node_type"].as_str().unwrap_or("?"), m[1]["exception_message"].as_str().unwrap_or("").trim()))
                        .unwrap_or_else(|| "no details given".into());
                    return Err(format!("ComfyUI failed while running the workflow: {}", detail));
                }
                let produced = &entry["outputs"][output_node];
                if let Some(file) = produced["images"].get(0).or_else(|| produced["audio"].get(0)) {
                    return Ok(file.clone());
                }
                if status["completed"] == json!(true) {
                    return Err(format!("the workflow finished without a result from node {}", output_node));
                }
            }
            if Instant::now() > deadline {
                // do not leave it running for a result nobody will collect
                self.abandon(prompt_id);
                return Err(format!("timed out after {} minutes waiting for ComfyUI", timeout.as_secs() / 60));
            }
            std::thread::sleep(Duration::from_millis(if timeout < Duration::from_secs(60) { 50 } else { 1000 }));
        }
    }

    fn download(&self, image: &Value) -> Result<Vec<u8>, String> {
        let resp = self
            .agent
            .get(&format!("{}/view", self.server))
            .query("filename", image["filename"].as_str().unwrap_or(""))
            .query("subfolder", image["subfolder"].as_str().unwrap_or(""))
            .query("type", image["type"].as_str().unwrap_or("output"))
            .call()
            .map_err(|e| explain(e, &self.server))?;
        let mut bytes = Vec::new();
        resp.into_reader().take(256 << 20).read_to_end(&mut bytes).map_err(|e| e.to_string())?;
        Ok(bytes)
    }
}

// --------------------------------------------------------------- jobs

#[derive(Serialize, Clone)]
pub struct Job {
    pub id: u64,
    pub story: String,
    pub asset: String,
    pub label: String,
    pub workflow: String,
    /// queued, running, done, failed or cancelled.
    pub state: &'static str,
    pub message: String,
    pub seconds: u64,
    /// background, sprite or expression.
    pub kind: &'static str,
    /// Cancelling bumps the queue's generation; a job from an older one stops.
    #[serde(skip)]
    generation: u64,
    #[serde(skip)]
    story_path: PathBuf,
    #[serde(skip)]
    overrides: Overrides,
}

/// Every kind of asset, in the order jobs run.
const KINDS: [&str; 5] = ["background", "sprite", "expression", "music", "voice"];

/// A queue of enhancement jobs, worked through one at a time.
pub struct Jobs {
    pub suite: Suite,
    list: Mutex<Vec<Job>>,
    wake: Condvar,
    next_id: AtomicU64,
    generation: AtomicU64,
    /// Bumped whenever an image on disk changes, so pages can refresh.
    pub version: AtomicU64,
    /// Seconds to wait for one image.
    pub timeout: Duration,
}

/// Where a story keeps its enhanced art: beside the story file.
pub fn enhanced_dir(story: &Path) -> PathBuf {
    story.parent().unwrap_or(Path::new(".")).join("enhanced")
}

impl Jobs {
    pub fn start(suite: Suite, timeout: Duration) -> Arc<Jobs> {
        let jobs = Arc::new(Jobs {
            suite,
            list: Mutex::new(Vec::new()),
            wake: Condvar::new(),
            next_id: AtomicU64::new(1),
            generation: AtomicU64::new(1),
            version: AtomicU64::new(1),
            timeout,
        });
        let worker = jobs.clone();
        let _ = std::thread::Builder::new().name("comfy-jobs".into()).spawn(move || worker.work());
        jobs
    }

    pub fn snapshot(&self) -> Vec<Job> {
        self.list.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// Queue assets of one story. Neutral portraits go ahead of the
    /// expressions built from them.
    pub fn enqueue(&self, story: &str, story_path: &Path, doc: &Doc, ids: &[String], ov: &Overrides) -> Result<usize, String> {
        let all = all_assets_of(doc, story_path);
        let mut chosen: Vec<&Asset> = Vec::new();
        for id in ids {
            chosen.push(all.iter().find(|a| &a.id == id).ok_or_else(|| format!("this story has no asset called '{}'", id))?);
        }
        if chosen.is_empty() {
            return Err("nothing was selected".into());
        }
        chosen.sort_by_key(|a| KINDS.iter().position(|k| *k == a.kind));
        // fail now, not an hour into the queue, if a workflow cannot be built
        let mut built = Vec::new();
        for a in &chosen {
            let mut this = ov.clone();
            // a workflow picked for one kind must not be forced on the others
            if let Some(w) = &ov.workflow {
                if self.suite.template(w).map_or(true, |t| t["kind"].as_str().map_or(false, |k| k != a.kind)) {
                    this.workflow = None;
                }
            }
            let b = build(&self.suite, a, story, &this).map_err(|e| format!("{}: {}", a.id, e))?;
            built.push((this, b.workflow));
        }
        let mut list = self.list.lock().unwrap_or_else(|e| e.into_inner());
        // a new request for an asset replaces its finished entries and any still waiting
        list.retain(|j| j.state == "running" || !(j.story == story && chosen.iter().any(|a| a.id == j.asset)));
        for (a, (this, workflow)) in chosen.iter().zip(built) {
            let label = match (a.kind, &a.character) {
                ("voice", _) => format!("{}: \u{201c}{}\u{201d}", a.name, a.text.chars().take(48).collect::<String>()),
                ("music", _) => format!("{} (music)", a.name),
                (_, Some(c)) => format!("{} ({})", if c.name.is_empty() { &c.id } else { &c.name }, a.expr),
                (_, None) => format!("{} (background)", a.name),
            };
            list.push(Job {
                id: self.next_id.fetch_add(1, Ordering::Relaxed),
                story: story.to_string(),
                asset: a.id.clone(),
                label,
                workflow,
                state: "queued",
                message: String::new(),
                seconds: 0,
                kind: a.kind,
                generation: self.generation.load(Ordering::Relaxed),
                story_path: story_path.to_path_buf(),
                overrides: this,
            });
        }
        if list.len() > 400 {
            let extra = list.len() - 400;
            let mut dropped = 0;
            list.retain(|j| j.state == "queued" || j.state == "running" || { dropped += 1; dropped > extra });
        }
        // whatever order requests came in, a neutral portrait runs before the expressions built from it
        let rank = |j: &Job| KINDS.iter().position(|k| *k == j.kind).unwrap_or(KINDS.len());
        let mut waiting: Vec<Job> = list.iter().filter(|j| j.state == "queued").cloned().collect();
        waiting.sort_by_key(rank);
        let mut waiting = waiting.into_iter();
        for j in list.iter_mut().filter(|j| j.state == "queued") {
            *j = waiting.next().expect("same number of queued jobs");
        }
        self.wake.notify_all();
        Ok(chosen.len())
    }

    /// Drop everything queued and stop the job in hand.
    pub fn cancel_all(&self) {
        self.generation.fetch_add(1, Ordering::Relaxed);
        for j in self.list.lock().unwrap_or_else(|e| e.into_inner()).iter_mut().filter(|j| j.state == "queued") {
            j.state = "cancelled";
        }
    }

    /// Drop the jobs of one story (it is being removed).
    pub fn forget_story(&self, story: &str) {
        self.list.lock().unwrap_or_else(|e| e.into_inner()).retain(|j| j.story != story || j.state == "running");
    }

    fn work(&self) {
        loop {
            let job = {
                let mut list = self.list.lock().unwrap_or_else(|e| e.into_inner());
                loop {
                    if let Some(j) = list.iter_mut().find(|j| j.state == "queued") {
                        j.state = "running";
                        break j.clone();
                    }
                    list = self.wake.wait(list).unwrap_or_else(|e| e.into_inner());
                }
            };
            let started = Instant::now();
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| self.run(&job))).unwrap_or_else(|_| Err("the editor hit an internal error while painting the guide".into()));
            let mut list = self.list.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(j) = list.iter_mut().find(|j| j.id == job.id) {
                j.seconds = started.elapsed().as_secs();
                match result {
                    Ok(path) => {
                        j.state = "done";
                        j.message = format!("saved {}", path.display());
                    }
                    Err(e) if e == "cancelled" => j.state = "cancelled",
                    Err(e) => {
                        j.state = "failed";
                        j.message = e;
                    }
                }
            }
            self.version.fetch_add(1, Ordering::Relaxed);
        }
    }

    fn run(&self, job: &Job) -> Result<PathBuf, String> {
        let text = std::fs::read_to_string(&job.story_path).map_err(|e| format!("{}: {}", job.story_path.display(), e))?;
        let (doc, _) = Doc::parse(&text);
        let asset = all_assets_of(&doc, &job.story_path).into_iter().find(|a| a.id == job.asset).ok_or("this is no longer in the story (was the line reworded?)")?;
        let built = build(&self.suite, &asset, &job.story, &job.overrides)?;
        let out_dir = if asset.is_audio() { audio::audio_dir(&job.story_path) } else { enhanced_dir(&job.story_path) };
        let target = out_dir.join(asset.output());
        let cfg = self.suite.config()?;
        let client = Client::new(cfg["server"].as_str().unwrap_or("http://127.0.0.1:8188"), Duration::from_secs(120));
        let cancelled = || self.generation.load(Ordering::Relaxed) != job.generation;
        // a workflow that relies on extra nodes: say so plainly if this server lacks them
        for (class, what) in &built.requires {
            if !client.has_node(class)? {
                return Err(format!("this ComfyUI has no '{}' node; the workflow needs {}", class, what));
            }
        }
        for (name, role) in &built.uploads {
            if cancelled() {
                return Err("cancelled".into());
            }
            let png = match role.as_str() {
                "sample" => {
                    let file = job.overrides.voices.get(&asset.who).map(|v| v.sample.clone()).filter(|f| plain_file(f, &SAMPLE_EXTENSIONS)).unwrap_or_default();
                    std::fs::read(audio::audio_dir(&job.story_path).join("samples").join(&file)).map_err(|_| format!("the voice sample '{}' is missing", file))?
                }
                "base" => {
                    let neutral = out_dir.join(format!("spr_{}_neutral.png", asset.character.as_ref().map_or("", |c| c.id.as_str())));
                    std::fs::read(&neutral).map_err(|_| "needs this character's enhanced neutral portrait first; send the neutral, then this".to_string())?
                }
                // a refine pass starts from the enhanced image when there is one
                "current" => std::fs::read(&target).or_else(|_| painted(&asset, "guide"))?,
                other => painted(&asset, other)?,
            };
            client.upload(name, &png)?;
        }
        let prompt_id = client.queue(&built.prompt, &format!("below-stairs-editor-{}", job.id))?;
        let image = client.wait(&prompt_id, &built.output_node, self.timeout, &cancelled)?;
        let bytes = client.download(&image)?;
        if asset.is_audio() {
            // keep whatever format the workflow saved, if the game can play it
            let produced = image["filename"].as_str().unwrap_or("");
            let ext = produced.rsplit('.').next().unwrap_or("").to_lowercase();
            if !audio::EXTENSIONS.contains(&ext.as_str()) {
                return Err(format!("the workflow saved '{}', which the game cannot play; save as FLAC, MP3, OGG or WAV", produced));
            }
            if bytes.len() < 64 {
                return Err("ComfyUI returned an empty audio file".into());
            }
            if !job.story_path.is_file() {
                return Err("the story was removed while this was being made; nothing was saved".into());
            }
            std::fs::create_dir_all(&out_dir).map_err(|e| format!("{}: {}", out_dir.display(), e))?;
            let target = out_dir.join(format!("{}.{}", asset.id, ext));
            let tmp = out_dir.join(format!("{}.part", asset.id));
            std::fs::write(&tmp, &bytes).and_then(|_| std::fs::rename(&tmp, &target)).map_err(|e| format!("{}: {}", target.display(), e))?;
            // an older take in another format must not win the lookup
            for other in audio::EXTENSIONS.iter().filter(|e| **e != ext) {
                let _ = std::fs::remove_file(out_dir.join(format!("{}.{}", asset.id, other)));
            }
            return Ok(target);
        }
        if !bytes.starts_with(PNG_MAGIC) || bytes.len() < 33 {
            return Err("ComfyUI returned something that is not a PNG".into());
        }
        if asset.character.is_some() && !matches!(bytes[25], 4 | 6) {
            return Err("the result has no transparency; a character workflow must end by cutting the figure out".into());
        }
        if !job.story_path.is_file() {
            return Err("the story was removed while this image was being made; nothing was saved".into());
        }
        std::fs::create_dir_all(&out_dir).map_err(|e| format!("{}: {}", out_dir.display(), e))?;
        let tmp = out_dir.join(format!("{}.part", asset.output()));
        std::fs::write(&tmp, &bytes).and_then(|_| std::fs::rename(&tmp, &target)).map_err(|e| format!("{}: {}", target.display(), e))?;
        Ok(target)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn suite() -> Suite {
        Suite { dir: PathBuf::from("comfyui") }
    }

    fn story() -> Doc {
        Doc::parse(include_str!("../assets/story.txt")).0
    }

    #[test]
    fn every_workflow_builds_for_every_asset_of_its_kind() {
        let (suite, assets) = (suite(), assets_for(&story()));
        assert_eq!(assets.len(), 7 + 7 * 6);
        let workflows = suite.workflows();
        assert_eq!(workflows.len(), 11);
        for w in workflows.iter().filter(|w| w.kind != "voice" && w.kind != "music") {
            let ov = Overrides { workflow: Some(w.file.clone()), ..Default::default() };
            let mut used = 0;
            for a in assets.iter().filter(|a| a.kind == w.kind) {
                let b = build(&suite, a, "main", &ov).unwrap_or_else(|e| panic!("{} / {}: {}", w.file, a.id, e));
                assert!(!b.prompt.to_string().contains("\"$"), "{} left a placeholder", w.file);
                assert!(!b.uploads.is_empty());
                used += 1;
            }
            assert!(used > 0, "{}", w.file);
        }
    }

    #[test]
    fn built_in_copies_match_the_files() {
        assert_eq!(BUILT_IN_WORKFLOWS.len(), suite().workflows().len());
        let nowhere = Suite { dir: PathBuf::from("/nonexistent/comfy") };
        assert_eq!(nowhere.workflows().len(), 11);
        assert!(build(&nowhere, &assets_for(&story())[0], "main", &Overrides::default()).is_ok());
    }

    #[test]
    fn a_workflow_is_refused_for_the_wrong_kind_of_asset() {
        let assets = assets_for(&story());
        let ov = Overrides { workflow: Some("sprite_base.json".into()), ..Default::default() };
        assert!(build(&suite(), &assets[0], "main", &ov).unwrap_err().contains("workflow for sprites"));
        let ov = Overrides { workflow: Some("../config.json".into()), ..Default::default() };
        assert!(build(&suite(), &assets[0], "main", &ov).is_err());
    }

    #[test]
    fn prompts_settings_and_seeds_follow_the_story() {
        let mut doc = story();
        doc.characters.push(CharDef { id: "zed".into(), name: "Zed".into(), sprite: "q".into(), ..Default::default() });
        let assets = assets_for(&doc);
        let find = |id: &str| assets.iter().find(|a| a.id == id).unwrap().clone();
        let mut ov = Overrides::default();
        ov.prompts.insert("spr_zed".into(), "a tall astronaut with silver hair".into());
        ov.settings.insert("denoise".into(), json!(0.42));
        let neutral = build(&suite(), &find("spr_zed_neutral"), "tale", &ov).unwrap();
        let text = neutral.prompt.to_string();
        assert!(text.contains("a tall astronaut with silver hair, calm neutral expression"));
        assert!(text.contains("0.42") && text.contains("below_stairs/tale/spr_zed_neutral"));
        let seed = |b: &Built| b.prompt.as_object().unwrap().values().find(|n| n["class_type"] == "KSampler").unwrap()["inputs"]["seed"].clone();
        let smile = build(&suite(), &find("spr_zed_smile"), "tale", &ov).unwrap();
        assert_eq!(seed(&neutral), seed(&smile));
        assert!(smile.uploads.iter().any(|(_, role)| role == "base"));
        assert_ne!(seed(&neutral), seed(&build(&suite(), &find("spr_q_neutral"), "tale", &ov).unwrap()));
        // the refine workflow starts from the current image and uses no ControlNet
        let ov = Overrides { workflow: Some("background_refine.json".into()), ..Default::default() };
        let refine = build(&suite(), &find("bg_study"), "tale", &ov).unwrap();
        assert_eq!(refine.uploads, vec![("bsas_tale_bg_study_current.png".to_string(), "current".to_string())]);
        assert!(!refine.prompt.to_string().contains("ControlNet"));
    }

    #[test]
    fn validator_catches_mistakes() {
        let specs = suite().specs().unwrap();
        let good = build(&suite(), &assets_for(&story())[0], "main", &Overrides::default()).unwrap().prompt;
        assert!(validate_graph(&good, &specs).is_empty());
        for (path, value) in [("/10/inputs/sampler_name", json!("eulerr")), ("/10/inputs/model", json!(["1", 1])), ("/11/inputs/samples", json!(["99", 0])), ("/3/inputs/width", json!("1024")), ("/1/class_type", json!("Nope"))] {
            let mut bad = good.clone();
            *bad.pointer_mut(path).unwrap() = value;
            assert!(!validate_graph(&bad, &specs).is_empty(), "{}", path);
        }
        let mut bad = good.clone();
        bad["10"]["inputs"].as_object_mut().unwrap().remove("denoise");
        assert!(!validate_graph(&bad, &specs).is_empty());
    }

    #[test]
    fn seeds_match_the_python_runner() {
        // zlib.crc32(b"p") == 0x8dbdc8e9 and zlib.crc32(b"bg_axis") == 0x1c4e2b0b... checked against Python below
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
        assert_eq!(crc32(b""), 0);
    }

    #[test]
    fn a_resend_replaces_the_waiting_job_and_neutrals_go_first() {
        let jobs = Jobs { suite: suite(), list: Mutex::new(Vec::new()), wake: Condvar::new(), next_id: AtomicU64::new(1), generation: AtomicU64::new(1), version: AtomicU64::new(1), timeout: Duration::from_secs(1) };
        let doc = story();
        let path = Path::new("assets/story.txt");
        let ids = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        jobs.enqueue("main", path, &doc, &ids(&["spr_p_smile", "bg_axis"]), &Overrides::default()).unwrap();
        let mut ov = Overrides::default();
        ov.workflow = Some("background_reimagine.json".into());
        jobs.enqueue("main", path, &doc, &ids(&["bg_axis", "spr_p_neutral"]), &ov).unwrap();
        let list = jobs.snapshot();
        assert_eq!(list.iter().map(|j| j.asset.as_str()).collect::<Vec<_>>(), ["bg_axis", "spr_p_neutral", "spr_p_smile"]);
        assert_eq!(list[0].workflow, "background_reimagine.json");
        assert_eq!(list[1].workflow, "sprite_base.json");
        // cancelling ends this generation; later sends are a new one
        jobs.cancel_all();
        assert!(jobs.snapshot().iter().all(|j| j.state == "cancelled"));
        jobs.enqueue("main", path, &doc, &ids(&["bg_study"]), &Overrides::default()).unwrap();
        let fresh = jobs.snapshot().into_iter().find(|j| j.asset == "bg_study").unwrap();
        assert_eq!(fresh.generation, jobs.generation.load(Ordering::Relaxed));
        assert_ne!(list[0].generation, fresh.generation);
        jobs.forget_story("main");
        assert!(jobs.snapshot().is_empty());
    }

    #[test]
    fn a_wrong_output_node_or_per_asset_workflow_is_handled() {
        let dir = std::env::temp_dir().join(format!("bsas-suite-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("workflows")).unwrap();
        let mut cfg: Value = serde_json::from_str(BUILT_IN_CONFIG).unwrap();
        cfg["overrides"]["bg_study"] = json!({ "workflow": "background_reimagine.json" });
        std::fs::write(dir.join("config.json"), cfg.to_string()).unwrap();
        for (name, text) in BUILT_IN_WORKFLOWS {
            std::fs::write(dir.join("workflows").join(name), text).unwrap();
        }
        let mut bad: Value = serde_json::from_str(BUILT_IN_WORKFLOWS[0].1).unwrap();
        bad["output_node"] = json!("11");
        std::fs::write(dir.join("workflows").join("bad.json"), bad.to_string()).unwrap();
        let s = Suite { dir: dir.clone() };
        let assets = assets_for(&story());
        let study = assets.iter().find(|a| a.id == "bg_study").unwrap();
        assert_eq!(build(&s, study, "main", &Overrides::default()).unwrap().workflow, "background_reimagine.json");
        assert_eq!(build(&s, &assets[0], "main", &Overrides::default()).unwrap().workflow, "background.json");
        let ov = Overrides { workflow: Some("bad.json".into()), ..Default::default() };
        assert!(build(&s, &assets[0], "main", &ov).unwrap_err().contains("SaveImage"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn every_line_and_track_is_an_audio_asset() {
        let (mut doc, _) = Doc::parse("character p | Perry | #fff | p\ntrack calm | Calm | 20 | harp\n*start\nmusic calm\np: Hello.\nn: He says hello.\np: Hello.\np:\nx: Nobody.\nend E | .\n");
        doc.sanitize();
        let a = audio_assets(&doc);
        assert_eq!(a.iter().map(|x| x.kind).collect::<Vec<_>>(), ["music", "voice", "voice"]);
        assert_eq!((a[0].id.as_str(), a[0].seconds, a[0].prompt.as_str()), ("music_calm", 20, "harp"));
        assert_eq!((a[1].who.as_str(), a[1].name.as_str(), a[2].who.as_str(), a[2].name.as_str()), ("p", "Perry", "n", "Narration"));
        assert_eq!(a[1].id, audio::voice_stem("p", "Hello."));
        assert_eq!(all_assets(&doc).len(), 7 + 6 + 3);
    }

    #[test]
    fn audio_workflows_build_with_the_right_voice_and_length() {
        let (mut doc, _) = Doc::parse("character p | Perry | #fff | p\ntrack calm | Calm | 20 | harp and strings\n*start\np: Hello there.\nend E | .\n");
        doc.sanitize();
        let assets = audio_assets(&doc);
        let (music, line) = (&assets[0], &assets[1]);
        // music: both local workflows, with the track's own length and description
        for file in ["music_stable_audio.json", "music_ace_step.json"] {
            let b = build(&suite(), music, "tale", &Overrides { workflow: Some(file.into()), ..Default::default() }).unwrap_or_else(|e| panic!("{}: {}", file, e));
            let text = b.prompt.to_string();
            assert!(text.contains("harp and strings") && text.contains("\"seconds\":20") && text.contains("below_stairs/tale/music_calm"), "{}", file);
            assert!(b.uploads.is_empty() && b.requires.is_empty());
        }
        // a stock voice
        let mut ov = Overrides::default();
        ov.voices.insert("p".into(), Voice { voice: "Sarah (female, american)".into(), speed: 1.1, ..Default::default() });
        let b = build(&suite(), line, "tale", &ov).unwrap();
        let text = b.prompt.to_string();
        assert_eq!(b.workflow, "voice_elevenlabs.json");
        assert!(text.contains("Hello there.") && text.contains("Sarah (female, american)") && text.contains("\"model.speed\":1.1"));
        assert!(b.requires.iter().any(|(c, w)| c == "ElevenLabsTextToSpeech" && w.contains("API key")));
        // cloning needs a sample, and F5 the words in it
        ov.voices.get_mut("p").unwrap().workflow = "voice_f5_clone.json".into();
        assert!(build(&suite(), line, "tale", &ov).unwrap_err().contains("add one for Perry"));
        ov.voices.get_mut("p").unwrap().sample = "p.wav".into();
        assert!(build(&suite(), line, "tale", &ov).unwrap_err().contains("words spoken"));
        ov.voices.get_mut("p").unwrap().sample_text = "The quick brown fox.".into();
        let b = build(&suite(), line, "tale", &ov).unwrap();
        assert_eq!(b.uploads, vec![("bsas_tale_sample_p.wav".to_string(), "sample".to_string())]);
        assert!(b.prompt.to_string().contains("The quick brown fox.") && b.requires[0].1.contains("F5-TTS"));
        ov.voices.get_mut("p").unwrap().workflow = "voice_elevenlabs_clone.json".into();
        assert!(build(&suite(), line, "tale", &ov).unwrap().prompt.to_string().contains("files.audio0"));
        // an image workflow cannot be used for sound, nor the reverse
        assert!(build(&suite(), line, "tale", &Overrides { workflow: Some("background.json".into()), ..Default::default() }).is_err());
    }

    #[test]
    fn globs() {
        assert!(glob_match("spr_a_*", "spr_a_smile") && glob_match("*", "x") && glob_match("bg_*_night", "bg_vista_night"));
        assert!(!glob_match("spr_a_*", "spr_q_smile") && !glob_match("bg_axis", "bg_axis2"));
    }
}
