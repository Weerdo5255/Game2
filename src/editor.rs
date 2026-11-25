//! The visual story editor: a small local web server. The page it serves
//! (editor.html) edits the story as a graph; this side owns the file,
//! the authoritative parser and validator, and paints previews of the art.
//!
//! It listens on 127.0.0.1 only, and refuses requests that did not come
//! from its own page, so other web sites cannot reach it through a browser.

use crate::art;
use crate::gfx::Canvas;
use crate::script::{Doc, Problem, OPS, SLOTS};
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

const PAGE: &str = include_str!("editor.html");
const MAX_BODY: usize = 8 << 20;

struct State {
    story: PathBuf,
    port: u16,
    art_dir: Option<PathBuf>,
    /// Painted previews, keyed by request path.
    cache: Mutex<HashMap<String, Arc<Vec<u8>>>>,
    backed_up: Mutex<bool>,
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
    let head_end = loop {
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
        let hosts = [format!("127.0.0.1:{}", self.port), format!("localhost:{}", self.port)];
        if !req.headers.get("host").map_or(false, |h| hosts.contains(h)) {
            return false;
        }
        if req.method == "GET" {
            return true;
        }
        // writes must come from our own page, as JSON (which browsers will
        // not send cross-site without a preflight that we never answer)
        let own_origin = req.headers.get("origin").map_or(true, |o| hosts.iter().any(|h| *o == format!("http://{}", h)));
        own_origin && req.headers.get("content-type").map_or(false, |c| c.starts_with("application/json"))
    }

    fn load(&self) -> Result<(Doc, Vec<String>), String> {
        let text = std::fs::read_to_string(&self.story).map_err(|e| format!("{}: {}", self.story.display(), e))?;
        Ok(Doc::parse(&text))
    }

    fn save(&self, doc: &Doc) -> Result<(), String> {
        let err = |e: std::io::Error| format!("{}: {}", self.story.display(), e);
        // keep the file as it was before this editing session
        let mut backed_up = self.backed_up.lock().unwrap();
        if !*backed_up && self.story.is_file() {
            let mut bak = self.story.clone().into_os_string();
            bak.push(".bak");
            std::fs::copy(&self.story, PathBuf::from(bak)).map_err(err)?;
        }
        *backed_up = true;
        // write beside the file and rename, so a crash cannot leave half a story
        let mut tmp = self.story.clone().into_os_string();
        tmp.push(".tmp");
        std::fs::write(&tmp, doc.to_text()).map_err(err)?;
        std::fs::rename(&tmp, &self.story).map_err(err)
    }

    fn meta(&self) -> Value {
        let looks: BTreeMap<&str, BTreeMap<String, String>> = art::SPRITES.iter().map(|s| (*s, art::look_defaults(s))).collect();
        json!({
            "exprs": art::EXPRS, "bgs": art::BACKGROUNDS, "sprites": art::SPRITES,
            "ops": OPS, "slots": SLOTS, "looks": looks, "path": self.story.display().to_string(),
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
            return respond(stream, "403 Forbidden", "text/plain", false, b"forbidden");
        }
        let fail = |stream: &mut TcpStream, msg: String| respond_json(stream, "400 Bad Request", &json!({ "error": msg }));
        match (req.method.as_str(), req.path.as_str()) {
            ("GET", "/") | ("GET", "/index.html") => respond(stream, "200 OK", "text/html; charset=utf-8", false, PAGE.as_bytes()),
            ("GET", "/api/story") => match self.load() {
                Ok((doc, parse_errors)) => {
                    let p = problems(&doc);
                    respond_json(stream, "200 OK", &json!({ "doc": doc, "problems": p, "parse_errors": parse_errors, "meta": self.meta() }))
                }
                Err(e) => fail(stream, e),
            },
            ("POST", "/api/validate") => match self.doc_from(&req.body) {
                Ok(doc) => respond_json(stream, "200 OK", &json!({ "problems": problems(&doc) })),
                Err(e) => fail(stream, e),
            },
            ("PUT", "/api/story") => match self.doc_from(&req.body).and_then(|doc| self.save(&doc).map(|_| doc)) {
                Ok(doc) => respond_json(stream, "200 OK", &json!({ "ok": true, "doc": doc, "problems": problems(&doc) })),
                Err(e) => fail(stream, e),
            },
            ("POST", "/api/play") => {
                let label = serde_json::from_slice::<Value>(&req.body).ok().and_then(|v| v["label"].as_str().map(str::to_string)).unwrap_or_default();
                match self.play(&label) {
                    Ok(()) => respond_json(stream, "200 OK", &json!({ "ok": true })),
                    Err(e) => fail(stream, e),
                }
            }
            ("GET", "/art/sprite") => {
                let q = query_map(&req.query);
                let base = q.get("base").cloned().unwrap_or_default();
                let expr = q.get("expr").cloned().unwrap_or_else(|| "neutral".into());
                let id = q.get("id").cloned().unwrap_or_default();
                let look: BTreeMap<String, String> =
                    q.get("look").map(|l| l.split(',').filter_map(|kv| kv.split_once('=')).map(|(k, v)| (k.to_string(), v.to_string())).collect()).unwrap_or_default();
                if !art::SPRITES.contains(&base.as_str()) || !art::EXPRS.contains(&expr.as_str()) {
                    return respond(stream, "404 Not Found", "text/plain", false, b"no such portrait");
                }
                let key = format!("{}?{}", req.path, req.query);
                let def = crate::script::CharDef { id, sprite: base, look, ..Default::default() };
                match self.art(&key, || Some((art::sprite(&def, &expr, self.art_dir.as_deref()), true))) {
                    Some(png) => respond(stream, "200 OK", "image/png", true, &png),
                    None => respond(stream, "500 Internal Server Error", "text/plain", false, b"could not paint"),
                }
            }
            ("GET", path) if path.starts_with("/art/bg/") => {
                let name = path.trim_start_matches("/art/bg/").trim_end_matches(".png").to_string();
                if !art::BACKGROUNDS.contains(&name.as_str()) {
                    return respond(stream, "404 Not Found", "text/plain", false, b"no such background");
                }
                let paint = || {
                    let (mut c, animated) = art::background(&name, self.art_dir.as_deref());
                    if animated {
                        art::animate(&mut c, &name, 0.5);
                    }
                    Some((c.resized(384, 216), false))
                };
                match self.art(path, paint) {
                    Some(png) => respond(stream, "200 OK", "image/png", true, &png),
                    None => respond(stream, "500 Internal Server Error", "text/plain", false, b"could not paint"),
                }
            }
            _ => respond(stream, "404 Not Found", "text/plain", false, b"not found"),
        }
    }

    /// Launch the game on the saved story, starting at a scene.
    fn play(&self, label: &str) -> Result<(), String> {
        let (doc, errors) = self.load()?;
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
        cmd.arg("--story").arg(&self.story).arg("--start").arg(label);
        match &self.art_dir {
            Some(dir) => cmd.arg("--art-dir").arg(dir),
            None => cmd.arg("--procedural"),
        };
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

/// Run the editor until the process is stopped.
pub fn serve(story: PathBuf, port: u16, art_dir: Option<PathBuf>, browser: bool) -> Result<(), String> {
    let listener = TcpListener::bind(("127.0.0.1", port)).map_err(|e| format!("cannot listen on 127.0.0.1:{}: {} (try --port N)", port, e))?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    let url = format!("http://127.0.0.1:{}/", port);
    println!("story editor: {}\nediting:      {}\npress Ctrl+C to stop", url, story.display());
    if browser {
        open_browser(&url);
    }
    let state = Arc::new(State { story, port, art_dir, cache: Mutex::new(HashMap::new()), backed_up: Mutex::new(false) });
    for stream in listener.incoming() {
        let Ok(mut stream) = stream else { continue };
        let state = state.clone();
        std::thread::spawn(move || {
            let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(10)));
            if let Some(req) = read_request(&mut stream) {
                state.handle(req, &mut stream);
            }
        });
    }
    Ok(())
}
