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
use crate::gfx::{rgb, Canvas, BLACK, H, W};
use crate::script::{CharDef, Doc};
use serde::Serialize;
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
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
    /// background, sprite (neutral portrait) or expression.
    pub kind: String,
    pub description: String,
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
                Some(WorkflowInfo {
                    title: t["title"].as_str().unwrap_or(&file).to_string(),
                    description: t["description"].as_str().unwrap_or("").to_string(),
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
}

impl Asset {
    pub fn output(&self) -> String {
        format!("{}.png", self.id)
    }
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
            });
        }
    }
    out
}

/// The game's own painting of an asset, as the workflows want it.
/// `role` is guide, silhouette or face.
fn painted(asset: &Asset, role: &str) -> Result<Vec<u8>, String> {
    match &asset.character {
        None => {
            let (mut c, animated) = art::background(&asset.name, None);
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

/// Feed the guide straight to the ControlNet, without edge detection.
fn drop_canny(prompt: &mut Map<String, Value>) {
    let ids: Vec<String> = prompt.iter().filter(|(_, n)| n["class_type"] == "Canny").map(|(k, _)| k.clone()).collect();
    for id in ids {
        let source = prompt[&id]["inputs"]["image"].clone();
        prompt.remove(&id);
        for node in prompt.values_mut() {
            if let Some(inputs) = node["inputs"].as_object_mut() {
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
    let workflow = ov.workflow.clone().or_else(|| ov.workflows.get(asset.kind).cloned()).or_else(|| s.get("workflow").and_then(Value::as_str).map(str::to_string));
    let workflow = workflow.ok_or_else(|| format!("config.json names no workflow for {}s", asset.kind))?;
    let template = suite.template(&workflow)?;
    if template["kind"].as_str().map_or(false, |k| k != asset.kind) {
        return Err(format!("'{}' is a workflow for {}s, not for this {}", workflow, template["kind"].as_str().unwrap_or("?"), asset.kind));
    }
    merge(&mut s, &template["defaults"]);
    merge(&mut s, &Value::Object(explicit));
    merge(&mut s, &Value::Object(ov.settings.clone()));

    let text = |key: &str| s.get(key).and_then(Value::as_str).unwrap_or("").to_string();
    let mut subject = ov.prompts.get(&asset.prompt_key).filter(|p| !p.trim().is_empty()).cloned().unwrap_or_else(|| asset.prompt.clone());
    if asset.character.is_some() {
        subject = format!("{}, {}", subject, art::expr_prompt(&asset.expr));
    }
    // one seed per character, so every expression shares its neutral's noise
    let seed_key = asset.character.as_ref().map_or(asset.id.as_str(), |c| c.id.as_str());
    let hash = seed_key.bytes().fold(2166136261u32, |h, b| (h ^ b as u32).wrapping_mul(16777619));
    let seed = s.get("seed").and_then(Value::as_u64).unwrap_or(0) + (hash % 100_000) as u64;
    s.insert("positive".into(), json!(format!("{}{}{}", text("style_prefix"), subject, text("style_suffix"))));
    s.insert("seed".into(), json!(seed));
    s.insert("prefix".into(), json!(format!("below_stairs/{}/{}", tag, asset.id)));

    let raw = serde_json::to_string(&template["prompt"]).unwrap_or_default();
    let mut uploads = Vec::new();
    let roles: &[&str] = match asset.kind {
        "background" => &["guide"],
        "sprite" => &["guide", "silhouette"],
        _ => &["guide", "silhouette", "face", "base"],
    };
    for role in roles {
        if !raw.contains(&format!("\"${}\"", role)) {
            continue; // this workflow does not use that image
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
    let errors = validate_graph(&prompt, &suite.specs()?);
    if !errors.is_empty() {
        return Err(format!("{}: {}", workflow, errors.join("; ")));
    }
    Ok(Built { prompt, output_node, uploads, workflow })
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

    fn queue(&self, prompt: &Value, client_id: &str) -> Result<String, String> {
        let resp = self.agent.post(&format!("{}/prompt", self.server)).send_json(json!({ "prompt": prompt, "client_id": client_id })).map_err(|e| explain(e, &self.server))?;
        let answer: Value = resp.into_json().map_err(|e| e.to_string())?;
        if answer["node_errors"].as_object().map_or(false, |o| !o.is_empty()) {
            return Err(format!("ComfyUI rejected the workflow: {}", describe_error(&answer.to_string())));
        }
        answer["prompt_id"].as_str().map(str::to_string).ok_or_else(|| "ComfyUI did not return a prompt id".to_string())
    }

    /// Wait for a queued prompt; returns its first output image record.
    fn wait(&self, prompt_id: &str, output_node: &str, timeout: Duration, cancel: &AtomicBool) -> Result<Value, String> {
        let deadline = Instant::now() + timeout;
        loop {
            if cancel.load(Ordering::Relaxed) {
                let _ = self.agent.post(&format!("{}/interrupt", self.server)).send_json(json!({}));
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
                if let Some(image) = entry["outputs"][output_node]["images"].get(0) {
                    return Ok(image.clone());
                }
                if status["completed"] == json!(true) {
                    return Err(format!("the workflow finished without an image from node {}", output_node));
                }
            }
            if Instant::now() > deadline {
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
    #[serde(skip)]
    story_path: PathBuf,
    #[serde(skip)]
    overrides: Overrides,
}

/// A queue of enhancement jobs, worked through one at a time.
pub struct Jobs {
    pub suite: Suite,
    list: Mutex<Vec<Job>>,
    wake: Condvar,
    next_id: AtomicU64,
    cancel: AtomicBool,
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
            cancel: AtomicBool::new(false),
            version: AtomicU64::new(1),
            timeout,
        });
        let worker = jobs.clone();
        let _ = std::thread::Builder::new().name("comfy-jobs".into()).spawn(move || worker.work());
        jobs
    }

    pub fn snapshot(&self) -> Vec<Job> {
        self.list.lock().unwrap().clone()
    }

    /// Queue assets of one story. Neutral portraits go ahead of the
    /// expressions built from them.
    pub fn enqueue(&self, story: &str, story_path: &Path, doc: &Doc, ids: &[String], ov: &Overrides) -> Result<usize, String> {
        let all = assets_for(doc);
        let mut chosen: Vec<&Asset> = Vec::new();
        for id in ids {
            chosen.push(all.iter().find(|a| &a.id == id).ok_or_else(|| format!("this story has no asset called '{}'", id))?);
        }
        if chosen.is_empty() {
            return Err("nothing was selected".into());
        }
        chosen.sort_by_key(|a| ["background", "sprite", "expression"].iter().position(|k| *k == a.kind));
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
        let mut list = self.list.lock().unwrap();
        list.retain(|j| j.state == "queued" || j.state == "running" || !(j.story == story && chosen.iter().any(|a| a.id == j.asset)));
        for (a, (this, workflow)) in chosen.iter().zip(built) {
            if list.iter().any(|j| j.story == story && j.asset == a.id && j.state == "queued") {
                continue;
            }
            let label = match &a.character {
                Some(c) => format!("{} ({})", if c.name.is_empty() { &c.id } else { &c.name }, a.expr),
                None => format!("{} (background)", a.name),
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
                story_path: story_path.to_path_buf(),
                overrides: this,
            });
        }
        if list.len() > 400 {
            let extra = list.len() - 400;
            let mut dropped = 0;
            list.retain(|j| j.state == "queued" || j.state == "running" || { dropped += 1; dropped > extra });
        }
        self.cancel.store(false, Ordering::Relaxed);
        self.wake.notify_all();
        Ok(chosen.len())
    }

    /// Drop everything queued and stop the job in hand.
    pub fn cancel_all(&self) {
        self.cancel.store(true, Ordering::Relaxed);
        for j in self.list.lock().unwrap().iter_mut().filter(|j| j.state == "queued") {
            j.state = "cancelled";
        }
    }

    fn work(&self) {
        loop {
            let job = {
                let mut list = self.list.lock().unwrap();
                loop {
                    if let Some(j) = list.iter_mut().find(|j| j.state == "queued") {
                        j.state = "running";
                        break j.clone();
                    }
                    list = self.wake.wait(list).unwrap();
                }
            };
            self.cancel.store(false, Ordering::Relaxed);
            let started = Instant::now();
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| self.run(&job))).unwrap_or_else(|_| Err("the editor hit an internal error while painting the guide".into()));
            let mut list = self.list.lock().unwrap();
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
        let asset = assets_for(&doc).into_iter().find(|a| a.id == job.asset).ok_or("this asset is no longer in the story")?;
        let built = build(&self.suite, &asset, &job.story, &job.overrides)?;
        let out_dir = enhanced_dir(&job.story_path);
        let target = out_dir.join(asset.output());
        let cfg = self.suite.config()?;
        let client = Client::new(cfg["server"].as_str().unwrap_or("http://127.0.0.1:8188"), Duration::from_secs(120));
        for (name, role) in &built.uploads {
            let png = match role.as_str() {
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
        let image = client.wait(&prompt_id, &built.output_node, self.timeout, &self.cancel)?;
        let bytes = client.download(&image)?;
        if !bytes.starts_with(PNG_MAGIC) || bytes.len() < 33 {
            return Err("ComfyUI returned something that is not a PNG".into());
        }
        if asset.character.is_some() && !matches!(bytes[25], 4 | 6) {
            return Err("the result has no transparency; a character workflow must end by cutting the figure out".into());
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
        assert_eq!(workflows.len(), 6);
        for w in &workflows {
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
        assert_eq!(nowhere.workflows().len(), 6);
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
    fn globs() {
        assert!(glob_match("spr_a_*", "spr_a_smile") && glob_match("*", "x") && glob_match("bg_*_night", "bg_vista_night"));
        assert!(!glob_match("spr_a_*", "spr_q_smile") && !glob_match("bg_axis", "bg_axis2"));
    }
}
