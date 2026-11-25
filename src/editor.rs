//! The visual story editor: a small local web server. The page it serves
//! (editor.html) edits a story as a graph; this side owns the files (a
//! library of independent stories), the authoritative parser and
//! validator, paints previews of the art, and runs ComfyUI jobs.
//!
//! It listens on 127.0.0.1 only, and refuses requests that did not come
//! from its own page, so other web sites cannot reach it through a browser.

use crate::art;
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
            .filter(|id| !id.starts_with('.') && self.path(id).is_ok())
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

    /// Make a new story: a copy of an existing one, or a small starter.
    fn create(&self, title: &str, copy: Option<&str>) -> Result<String, String> {
        let title = title.trim();
        if title.is_empty() {
            return Err("a story needs a title".into());
        }
        let mut slug: String = title.to_lowercase().chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect();
        slug = slug.split('-').filter(|p| !p.is_empty()).collect::<Vec<_>>().join("-");
        slug.truncate(40);
        if slug.is_empty() || slug == "main" {
            slug = "story".into();
        }
        let mut id = slug.clone();
        let mut n = 1;
        while self.dir.join(&id).exists() {
            n += 1;
            id = format!("{}-{}", slug, n);
        }
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
            // a copy brings its art along
            let source = self.path(from)?;
            let _ = std::fs::copy(source.with_file_name("art.json"), folder.join("art.json"));
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
        std::fs::rename(self.dir.join(id), &target).map_err(|e| format!("could not move the story: {}", e))
    }
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

struct Request {
    method: String,
    path: String,
    query: String,
    headers: HashMap<String, String>,
    body: Vec<u8>,
}

fn problems(doc: &Doc) -> Vec<Problem> {
    doc.problems(art::EXPRS, art::BACKGROUNDS, art::SPRITES)
}

fn read_request(stream: &mut TcpStream) -> Option<Request> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    // one deadline for the whole request, however slowly it trickles in
    let deadline = Instant::now() + Duration::from_secs(20);
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
    if len > MAX_BODY {
        return None;
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
    Some(Request { method, path, query, headers, body })
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
        // The page must be addressed as this machine (any port, so a forwarded
        // port works): a hostile site that points its own name at 127.0.0.1
        // still sends its own name here.
        let Some(host) = req.headers.get("host") else { return false };
        let name = host.rsplit_once(':').filter(|(_, p)| p.chars().all(|c| c.is_ascii_digit())).map_or(host.as_str(), |(n, _)| n);
        if name != "127.0.0.1" && name != "localhost" {
            return false;
        }
        if req.method == "GET" {
            return true;
        }
        // writes must come from our own page, as JSON (which browsers will
        // not send cross-site without a preflight that we never answer)
        let own_origin = req.headers.get("origin").map_or(true, |o| *o == format!("http://{}", host));
        own_origin && req.headers.get("content-type").map_or(false, |c| c.starts_with("application/json"))
    }

    fn load(&self, story: &Path) -> Result<(Doc, Vec<String>), String> {
        let text = std::fs::read_to_string(story).map_err(|e| format!("{}: {}", story.display(), e))?;
        Ok(Doc::parse(&text))
    }

    fn save(&self, story: &Path, doc: &Doc) -> Result<(), String> {
        let err = |e: std::io::Error| format!("{}: {}", story.display(), e);
        // keep each file as it was before this editing session
        let mut backed_up = self.backed_up.lock().unwrap();
        if !backed_up.contains(story) && story.is_file() {
            let mut bak = story.to_path_buf().into_os_string();
            bak.push(".bak");
            std::fs::copy(story, PathBuf::from(bak)).map_err(err)?;
        }
        backed_up.insert(story.to_path_buf());
        // write beside the file and rename, so a crash cannot leave half a story
        let mut tmp = story.to_path_buf().into_os_string();
        tmp.push(".tmp");
        std::fs::write(&tmp, doc.to_text()).map_err(err)?;
        std::fs::rename(&tmp, story).map_err(err)
    }

    /// The folder of enhanced art to preview for a story, if it has one.
    fn art_dir(&self, story: &Path) -> Option<PathBuf> {
        Some(comfy::enhanced_dir(story)).filter(|d| !self.procedural && d.is_dir())
    }

    fn prefs(&self, story: &Path) -> Value {
        std::fs::read_to_string(story.with_file_name("art.json")).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_else(|| json!({}))
    }

    /// What the story has chosen for its art: its own prompts and workflows.
    fn overrides(&self, story: &Path) -> Overrides {
        let prefs = self.prefs(story);
        let strings = |v: &Value| -> BTreeMap<String, String> { v.as_object().into_iter().flatten().filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_string()))).collect() };
        Overrides { prompts: strings(&prefs["prompts"]), workflows: strings(&prefs["workflows"]), ..Default::default() }
    }

    /// Everything the Art view shows for a story.
    fn art_overview(&self, story: &Path) -> Result<Value, String> {
        let (doc, _) = self.load(story)?;
        let dir = comfy::enhanced_dir(story);
        let assets: Vec<Value> = comfy::assets_for(&doc)
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
        let body: Value = if req.method == "GET" { Value::Null } else { serde_json::from_slice(&req.body).unwrap_or(Value::Null) };
        // which story the request is about: ?story= or a "story" field
        let story_id = q.get("story").cloned().or_else(|| body["story"].as_str().map(str::to_string)).unwrap_or_else(|| "main".into());
        let story = match self.library.path(&story_id) {
            Ok(p) => p,
            Err(e) => return respond_json(stream, "404 Not Found", &json!({ "error": e })),
        };
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
            ("POST", "/api/stories/remove") => answer(stream, self.library.remove(&story_id).map(|_| json!({ "stories": self.library.list() }))),
            ("GET", "/api/story") => match self.load(&story) {
                Ok((doc, parse_errors)) => {
                    let p = problems(&doc);
                    let mut meta = self.meta();
                    meta["path"] = json!(story.display().to_string());
                    respond_json(stream, "200 OK", &json!({ "doc": doc, "problems": p, "parse_errors": parse_errors, "meta": meta }))
                }
                Err(e) => fail(stream, e),
            },
            ("POST", "/api/validate") => match self.doc_from(&req.body) {
                Ok(doc) => respond_json(stream, "200 OK", &json!({ "problems": problems(&doc) })),
                Err(e) => fail(stream, e),
            },
            ("PUT", "/api/story") => match self.doc_from(&req.body).and_then(|doc| doc.survives_text().and_then(|_| self.save(&story, &doc)).map(|_| doc)) {
                Ok(doc) => {
                    respond_json(stream, "200 OK", &json!({ "ok": true, "doc": doc, "problems": problems(&doc) }))
                }
                Err(e) => fail(stream, e),
            },
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
                self.send_art(stream, &key, || Some((art::sprite(&def, &expr, dir.as_deref()), true)))
            }
            ("GET", path) if path.starts_with("/art/bg/") => {
                let name = path.trim_start_matches("/art/bg/").trim_end_matches(".png").to_string();
                if !art::BACKGROUNDS.contains(&name.as_str()) {
                    return respond(stream, "404 Not Found", "text/plain", false, b"no such background");
                }
                let dir = self.art_dir(&story).filter(|_| !q.contains_key("plain"));
                let full = q.contains_key("full");
                let key = self.art_key(&req, dir.as_deref().map(|d| d.join(format!("bg_{}.png", name))));
                self.send_art(stream, &key, || {
                    let (mut c, animated) = art::background(&name, dir.as_deref());
                    if animated {
                        art::animate(&mut c, &name, 0.5);
                    }
                    Some((if full { c } else { c.resized(384, 216) }, false))
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
                // the story's own prompts and preferred workflows, kept beside it
                let clean = json!({ "prompts": body["prompts"].as_object().cloned().unwrap_or_default(), "workflows": body["workflows"].as_object().cloned().unwrap_or_default() });
                let text = serde_json::to_string_pretty(&clean).unwrap_or_default() + "\n";
                answer(stream, std::fs::write(story.with_file_name("art.json"), text).map(|_| json!({ "ok": true })).map_err(|e| e.to_string()))
            }
            ("POST", "/api/art/run") => {
                let queued = self.load(&story).and_then(|(doc, _)| {
                    let mut ov = self.overrides(&story);
                    ov.workflow = body["workflow"].as_str().filter(|w| !w.is_empty()).map(str::to_string);
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
                    let file = dir.join(format!("{}.png", id));
                    if file.is_file() && std::fs::create_dir_all(dir.join(".removed")).and_then(|_| std::fs::rename(&file, dir.join(".removed").join(format!("{}.png", id)))).is_ok() {
                        moved += 1;
                    }
                }
                self.jobs.version.fetch_add(1, Ordering::Relaxed);
                respond_json(stream, "200 OK", &json!({ "reverted": moved }))
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

    fn send_art(&self, stream: &mut TcpStream, key: &str, paint: impl FnOnce() -> Option<(Canvas, bool)>) {
        match self.art(key, paint) {
            Some(png) => respond(stream, "200 OK", "image/png", true, &png),
            None => respond(stream, "500 Internal Server Error", "text/plain", false, b"could not paint"),
        }
    }

    /// Launch the game on the saved story, starting at a scene.
    fn play(&self, story: &Path, label: &str) -> Result<(), String> {
        let (doc, errors) = self.load(story)?;
        if let Some(e) = errors.first() {
            return Err(e.clone());
        }
        if let Some(p) = problems(&doc).iter().find(|p| p.level == "error") {
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
