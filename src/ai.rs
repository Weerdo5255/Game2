//! Asking a language model for something new: a character, or a place.
//!
//! Two kinds of model are spoken to, both over plain HTTP:
//! - Claude, through the Claude API. The key comes from the environment
//!   (`ANTHROPIC_API_KEY`, or `ANTHROPIC_AUTH_TOKEN`), and is never written
//!   anywhere, shown on the page, or logged. `ANTHROPIC_BASE_URL` points
//!   elsewhere (a gateway, or a test server).
//! - A model run by Ollama on this computer or another (`OLLAMA_HOST`,
//!   http://127.0.0.1:11434 when it is not set).
//!
//! Both are asked for JSON that fits a schema, and what comes back is
//! cleaned by the game's own rules before anything uses it.

use std::io::{BufRead, BufReader};
use std::time::Duration;

use serde_json::{json, Value};

/// The model asked when none is named.
pub const CLAUDE_MODEL: &str = "claude-opus-5-5";
const API_VERSION: &str = "2026-10-21";
/// Models that can hand a declined request on to another model by themselves.
const FALLBACK_MODELS: [&str; 5] = ["claude-fable-5-2", "claude-fable-5-1", "claude-opus-5-5", "claude-opus-5", "claude-sonnet-5-5"];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Provider {
    Claude,
    Ollama,
}

impl Provider {
    pub fn parse(s: &str) -> Option<Provider> {
        match s {
            "claude" | "" => Some(Provider::Claude),
            "ollama" => Some(Provider::Ollama),
            _ => None,
        }
    }
}

fn env(name: &str) -> Option<String> {
    std::env::var(name).ok().map(|v| v.trim().to_string()).filter(|v| !v.is_empty())
}

fn claude_base() -> String {
    env("ANTHROPIC_BASE_URL").unwrap_or_else(|| "https://api.anthropic.com".into()).trim_end_matches('/').to_string()
}

/// Where Ollama listens: `OLLAMA_HOST` may be "host:port", or "0.0.0.0" (listen everywhere, meaning here).
pub fn ollama_base() -> String {
    let given = env("OLLAMA_HOST").unwrap_or_else(|| "127.0.0.1:11434".into());
    let with_scheme = if given.contains("://") { given } else { format!("http://{}", given) };
    let fixed = with_scheme.replace("://0.0.0.0", "://127.0.0.1");
    let has_port = fixed.rsplit("://").next().map_or(false, |h| h.trim_end_matches('/').contains(':'));
    let fixed = fixed.trim_end_matches('/').to_string();
    if has_port {
        fixed
    } else {
        format!("{}:11434", fixed)
    }
}

/// The key, and how it is sent. (Only ever put in a request header.)
fn claude_key() -> Option<(&'static str, String)> {
    env("ANTHROPIC_API_KEY").map(|k| ("x-api-key", k)).or_else(|| env("ANTHROPIC_AUTH_TOKEN").map(|t| ("authorization", format!("Bearer {}", t))))
}

/// A client for one address: through the proxy the environment names
/// (`HTTPS_PROXY` / `HTTP_PROXY`), unless `NO_PROXY` or the address says
/// it is near (this computer is never reached through a proxy).
fn agent(url: &str, read: Duration) -> ureq::Agent {
    let mut builder = ureq::AgentBuilder::new().timeout_connect(Duration::from_secs(10)).timeout_read(read);
    if let Some(proxy) = proxy_for(url).and_then(|p| ureq::Proxy::new(p).ok()) {
        builder = builder.proxy(proxy);
    }
    builder.build()
}

fn proxy_for(url: &str) -> Option<String> {
    let (scheme, rest) = url.split_once("://")?;
    let authority = rest.split('/').next().unwrap_or("");
    let host = authority.rsplit('@').next().unwrap_or("");
    let host = if host.starts_with('[') { host.split(']').next().unwrap_or("").trim_start_matches('[') } else { host.split(':').next().unwrap_or("") }.to_ascii_lowercase();
    if host == "localhost" || host.starts_with("127.") || host == "::1" || host.ends_with(".localhost") {
        return None;
    }
    let skip = env("NO_PROXY").or_else(|| env("no_proxy")).unwrap_or_default();
    for entry in skip.split(',').map(|e| e.trim().to_ascii_lowercase()).filter(|e| !e.is_empty()) {
        let entry = entry.split(':').next().unwrap_or("").to_string();
        let bare = entry.trim_start_matches("*.").trim_start_matches('.');
        if entry == "*" || host == bare || host.ends_with(&format!(".{}", bare)) {
            return None;
        }
    }
    if scheme == "https" {
        env("HTTPS_PROXY").or_else(|| env("https_proxy")).or_else(|| env("ALL_PROXY")).or_else(|| env("all_proxy"))
    } else {
        env("HTTP_PROXY").or_else(|| env("http_proxy")).or_else(|| env("ALL_PROXY")).or_else(|| env("all_proxy"))
    }
}

/// What the page needs to know to offer the choice: whether a Claude key is
/// there (not the key), and which models Ollama has.
pub fn status() -> Value {
    let tags = format!("{}/api/tags", ollama_base());
    let ollama = agent(&tags, Duration::from_secs(3)).get(&tags).call();
    let ollama = match ollama {
        Ok(r) => {
            let v: Value = r.into_json().unwrap_or(Value::Null);
            let models: Vec<String> = v["models"].as_array().into_iter().flatten().filter_map(|m| m["name"].as_str().map(str::to_string)).collect();
            json!({ "ok": true, "host": ollama_base(), "models": models })
        }
        Err(e) => json!({ "ok": false, "host": ollama_base(), "error": format!("Ollama is not answering at {} ({})", ollama_base(), short(&e.to_string())) }),
    };
    json!({
        "claude": { "ready": claude_key().is_some(), "model": CLAUDE_MODEL, "custom_url": env("ANTHROPIC_BASE_URL").is_some() },
        "ollama": ollama,
    })
}

fn short(s: &str) -> String {
    s.chars().take(300).collect()
}

/// One request: instructions, the ask, and the shape the answer must take.
pub struct Ask<'a> {
    pub system: &'a str,
    pub user: &'a str,
    pub schema: &'a Value,
    /// Room for the answer, in tokens.
    pub max_tokens: u32,
}

/// Ask, and get back the JSON that was asked for (not yet cleaned).
pub fn ask(provider: Provider, model: &str, req: &Ask) -> Result<Value, String> {
    let text = match provider {
        Provider::Claude => claude(model, req)?,
        Provider::Ollama => ollama(model, req)?,
    };
    json_in(&text).ok_or_else(|| format!("the model's answer was not the JSON asked for (it began: {})", short(text.trim()).chars().take(160).collect::<String>()))
}

/// The JSON object in a model's answer, even if it came wrapped in prose or a code fence.
pub fn json_in(text: &str) -> Option<Value> {
    if let Ok(v) = serde_json::from_str::<Value>(text.trim()) {
        return Some(v).filter(Value::is_object);
    }
    let (start, end) = (text.find('{')?, text.rfind('}')?);
    serde_json::from_str::<Value>(&text[start..=end]).ok().filter(Value::is_object)
}

fn claude(model: &str, req: &Ask) -> Result<String, String> {
    let (header, key) = claude_key().ok_or("no Claude API key: start the editor with ANTHROPIC_API_KEY set in its environment")?;
    let model = if model.trim().is_empty() { CLAUDE_MODEL } else { model.trim() };
    let mut body = json!({
        "model": model,
        "max_tokens": req.max_tokens,
        "stream": true,
        "system": req.system,
        "messages": [{ "role": "user", "content": req.user }],
        "output_config": { "effort": "medium", "format": { "type": "json_schema", "schema": req.schema } },
    });
    let mut betas: Vec<&str> = Vec::new();
    if FALLBACK_MODELS.contains(&model) {
        // if this model declines, the API hands the request to another model rather than failing
        body["fallbacks"] = json!("default");
        betas.push("server-side-fallback-2026-07-01");
    }
    if header == "authorization" {
        betas.push("oauth-2025-04-20");
    }
    let url = format!("{}/v1/messages", claude_base());
    let mut wait = Duration::from_secs(2);
    for attempt in 0..4 {
        let mut r = agent(&url, Duration::from_secs(180)).post(&url).set("anthropic-version", API_VERSION).set("content-type", "application/json").set(header, &key);
        if !betas.is_empty() {
            r = r.set("anthropic-beta", &betas.join(","));
        }
        match r.send_json(body.clone()) {
            Ok(resp) => return read_stream(BufReader::new(resp.into_reader())),
            Err(ureq::Error::Status(code, resp)) => {
                let retry_after = resp.header("retry-after").and_then(|s| s.trim().parse::<u64>().ok());
                let detail: Value = resp.into_json().unwrap_or(Value::Null);
                let message = detail["error"]["message"].as_str().map(short).unwrap_or_default();
                let busy = matches!(code, 429 | 500 | 502 | 503 | 504 | 529);
                if busy && attempt < 3 {
                    std::thread::sleep(retry_after.map(|s| Duration::from_secs(s.min(30))).unwrap_or(wait));
                    wait *= 2;
                    continue;
                }
                return Err(match code {
                    401 | 403 => format!("the Claude API did not accept the key ({})", message),
                    429 | 529 => format!("the Claude API is busy just now; try again in a little while ({})", message),
                    _ => format!("the Claude API answered {}: {}", code, message),
                });
            }
            Err(e) if attempt < 3 => {
                eprintln!("ai: cannot reach the Claude API, trying again ({})", short(&e.to_string()));
                std::thread::sleep(wait);
                wait *= 2;
            }
            Err(e) => return Err(format!("cannot reach the Claude API at {} ({})", claude_base(), short(&e.to_string()))),
        }
    }
    Err("the Claude API could not be reached".into())
}

/// Read a streamed answer: the text, from every text block in order.
pub fn read_stream(reader: impl BufRead) -> Result<String, String> {
    let mut text = String::new();
    let mut stop = String::new();
    for line in reader.lines() {
        let line = line.map_err(|e| format!("the answer was cut off ({})", e))?;
        let Some(data) = line.strip_prefix("data:") else { continue };
        let Ok(event) = serde_json::from_str::<Value>(data.trim()) else { continue };
        match event["type"].as_str().unwrap_or("") {
            "content_block_delta" if event["delta"]["type"] == "text_delta" => text.push_str(event["delta"]["text"].as_str().unwrap_or("")),
            "message_delta" => {
                if let Some(s) = event["delta"]["stop_reason"].as_str() {
                    stop = s.to_string();
                }
            }
            "error" => return Err(format!("the Claude API stopped with an error: {}", event["error"]["message"].as_str().map(short).unwrap_or_default())),
            "message_stop" => break,
            _ => {}
        }
    }
    match stop.as_str() {
        "refusal" => Err("Claude declined to make this; try describing it differently".into()),
        "max_tokens" => Err("the answer ran out of room before it was finished; ask for something simpler".into()),
        _ if text.trim().is_empty() => Err("the Claude API sent back nothing".into()),
        _ => Ok(text),
    }
}

fn ollama(model: &str, req: &Ask) -> Result<String, String> {
    if model.trim().is_empty() {
        return Err("choose which of Ollama's models to use".into());
    }
    let body = json!({
        "model": model.trim(),
        "stream": false,
        "format": req.schema,
        "messages": [{ "role": "system", "content": req.system }, { "role": "user", "content": req.user }],
        "options": { "num_predict": req.max_tokens, "num_ctx": 16384 },
    });
    let url = format!("{}/api/chat", ollama_base());
    match agent(&url, Duration::from_secs(900)).post(&url).send_json(body) {
        Ok(resp) => {
            let v: Value = resp.into_json().map_err(|e| format!("Ollama's answer could not be read ({})", e))?;
            v["message"]["content"].as_str().map(str::to_string).filter(|t| !t.trim().is_empty()).ok_or_else(|| "Ollama sent back nothing".to_string())
        }
        Err(ureq::Error::Status(code, resp)) => {
            let v: Value = resp.into_json().unwrap_or(Value::Null);
            Err(format!("Ollama answered {}: {}", code, v["error"].as_str().map(short).unwrap_or_default()))
        }
        Err(e) => Err(format!("cannot reach Ollama at {} ({})", ollama_base(), short(&e.to_string()))),
    }
}

// ------------------------------------------------------------ what is asked for

fn obj(props: Value, required: &[&str]) -> Value {
    json!({ "type": "object", "properties": props, "required": required, "additionalProperties": false })
}

fn num() -> Value {
    json!({ "type": "number" })
}

fn hex() -> Value {
    json!({ "type": "string", "description": "a colour as #rrggbb" })
}

fn s(description: &str) -> Value {
    json!({ "type": "string", "description": description })
}

/// The shape of a new character.
pub fn character_schema() -> Value {
    let look = obj(
        json!({
            "skin": hex(), "hair": hex(), "brow": hex(), "iris": hex(), "coat": hex(),
            "width": num(), "jaw": num(), "age": num(), "blush": num(), "shoulders": num(), "lashes": num(), "lips": num(),
        }),
        &["skin", "hair", "brow", "iris", "coat", "width", "jaw", "age", "blush", "shoulders", "lashes", "lips"],
    );
    obj(
        json!({
            "id": s("a short lowercase id: letters, digits and _"),
            "name": s("the name shown when they speak"),
            "color": hex(),
            "sprite": { "type": "string", "enum": ["p", "v", "s", "q", "t", "c"] },
            "look": look,
            "bio": s("two or three sentences: who they are, what they want, how they speak"),
            "art_prompt": s("a description of their portrait for an image model: appearance, clothes, expression, period"),
        }),
        &["id", "name", "color", "sprite", "look", "bio", "art_prompt"],
    )
}

/// The shape of a new place: a painting, and (if asked for) a room to walk in.
pub fn place_schema(want3d: bool) -> Value {
    let alpha = num();
    let shape = |name: &str, mut props: Value, required: &[&str]| {
        props["shape"] = json!({ "type": "string", "enum": [name] });
        let mut req: Vec<&str> = vec!["shape"];
        req.extend_from_slice(required);
        obj(props, &req)
    };
    let pt = obj(json!({ "x": num(), "y": num() }), &["x", "y"]);
    let layer = json!({ "anyOf": [
        shape("gradient", json!({ "top": hex(), "bottom": hex(), "y0": num(), "y1": num() }), &["top", "bottom", "y0", "y1"]),
        shape("rect", json!({ "x": num(), "y": num(), "w": num(), "h": num(), "color": hex(), "alpha": alpha }), &["x", "y", "w", "h", "color", "alpha"]),
        shape("soft", json!({ "x": num(), "y": num(), "w": num(), "h": num(), "feather": num(), "color": hex(), "alpha": num() }), &["x", "y", "w", "h", "feather", "color", "alpha"]),
        shape("ellipse", json!({ "x": num(), "y": num(), "rx": num(), "ry": num(), "color": hex(), "alpha": num() }), &["x", "y", "rx", "ry", "color", "alpha"]),
        shape("poly", json!({ "points": { "type": "array", "items": pt }, "color": hex(), "alpha": num() }), &["points", "color", "alpha"]),
        shape("line", json!({ "x0": num(), "y0": num(), "x1": num(), "y1": num(), "width": num(), "color": hex(), "alpha": num() }), &["x0", "y0", "x1", "y1", "width", "color", "alpha"]),
        shape("glow", json!({ "x": num(), "y": num(), "r": num(), "color": hex(), "strength": num() }), &["x", "y", "r", "color", "strength"]),
        shape("stars", json!({ "count": { "type": "integer" }, "y0": num(), "y1": num(), "color": hex(), "seed": { "type": "integer" } }), &["count", "y0", "y1", "color", "seed"]),
    ]});
    let mut props = json!({
        "name": s("a short lowercase id for the place: letters, digits and _"),
        "title": s("what the place is called, in a few words"),
        "prompt": s("a description of the scene for an image model to paint it from, without people"),
        "vignette": num(),
        "layers": { "type": "array", "items": layer },
    });
    let mut required = vec!["name", "title", "prompt", "vignette", "layers"];
    if want3d {
        let v3 = || obj(json!({ "x": num(), "y": num(), "z": num() }), &["x", "y", "z"]);
        let part = obj(
            json!({ "shape": { "type": "string", "enum": ["box", "cylinder", "sphere", "capsule"] }, "at": v3(), "size": v3(), "turn": num(), "color": hex(), "glow": num(), "solid": { "type": "boolean" }, "name": s("what it is") }),
            &["shape", "at", "size", "turn", "color", "glow", "solid", "name"],
        );
        let light = obj(json!({ "kind": { "type": "string", "enum": ["point", "sun"] }, "at": v3(), "color": hex(), "power": num() }), &["kind", "at", "color", "power"]);
        props["room"] = obj(
            json!({ "sky": hex(), "ambient": hex(), "brightness": num(), "half_width": num(), "half_depth": num(), "parts": { "type": "array", "items": part }, "lights": { "type": "array", "items": light } }),
            &["sky", "ambient", "brightness", "half_width", "half_depth", "parts", "lights"],
        );
        required.push("room");
    }
    obj(props, &required)
}

/// What a story is about, told briefly, so that what is made belongs in it.
pub fn story_context(doc: &crate::script::Doc) -> String {
    let mut out = String::new();
    if !doc.title.trim().is_empty() {
        out += &format!("The story is called \"{}\".\n", doc.title.trim());
    }
    let cast: Vec<String> = doc.characters.iter().take(24).map(|c| if c.name.is_empty() { c.id.clone() } else { format!("{} ({})", c.name, c.id) }).collect();
    if !cast.is_empty() {
        out += &format!("Its characters so far: {}.\n", cast.join(", "));
    }
    let lines: Vec<String> = doc
        .scenes
        .iter()
        .flat_map(|s| s.items.iter())
        .filter_map(|i| match i {
            crate::script::Item::Say { text, .. } => Some(text.clone()),
            _ => None,
        })
        .take(12)
        .collect();
    if !lines.is_empty() {
        out += &format!("Some of its lines, for the tone: {}\n", lines.iter().map(|l| format!("\"{}\"", l.chars().take(160).collect::<String>())).collect::<Vec<_>>().join(" "));
    }
    out
}

pub const CHARACTER_SYSTEM: &str = "You design characters for a visual novel. The game draws each portrait itself, from one of six base figures, recoloured and reshaped by a few numbers:
- sprite: p a young gentleman, v an elder lord, s a bewigged lord, q a steward, t a footman, c a cook. Choose the base that fits best; the numbers below make it someone else.
- look colours (#rrggbb): skin, hair, brow, iris, coat.
- look numbers: width 46 to 64 (face width), jaw 0.62 to 0.98 (narrow to broad), age 0 to 1 (young to old), blush 0 to 1, shoulders 96 to 132, lashes 0 to 1, lips 0 to 1.
- color is the colour their name is written in: readable on a dark panel.
Make someone who belongs in the story you are told about and is unlike the characters it already has. The id must be new.";

pub const PLACE_SYSTEM: &str = "You paint backgrounds for a visual novel, as layers of simple shapes drawn in order, first at the back, on a canvas 1152 wide and 648 tall (x to the right, y down).
Shapes: gradient (a band across the whole width from y0 to y1, top colour to bottom colour), rect, soft (a rectangle with edges blurred `feather` pixels wide: haze, light shafts, distant masses), ellipse (centre x, y and radii), poly (a filled polygon of up to 96 points), line (a stroke `width` thick), glow (light that brightens what is under it; strength 0.2 to 1.5), stars (scattered points between y0 and y1).
alpha is 0 to 1. Colours are #rrggbb. vignette 0 to 0.6 darkens the corners.
Paint like a skilled matte painter: sky or ceiling first, then far shapes in hazy colours, then middle distance, then near detail with strong values; light sources with glows; 80 to 250 layers. Characters' portraits stand in the lower middle of the frame, so keep the centre from about y 250 down uncluttered, and give the floor or ground a clear plane there.
Use a new id for the place, unlike the places the story already has.";

pub const ROOM_HELP: &str = "Also build the place as a simple 3D room to walk in (room): metres, y up, the floor at y 0, the room centred on x 0 z 0, half_width and half_depth 3 to 20. Parts are boxes, cylinders, spheres and capsules: at is a part's middle, size its width, height and depth, turn degrees about the upright; solid parts cannot be walked through. Give it a floor, walls (or ground and distant shapes outdoors), the main furniture, and 1 to 6 lights (point lamps of 200 to 2000, or a sun of 2000 to 20000). Brightness is the ambient light, 100 to 2000. The cast stands near z 0 facing +z; leave from x -2 to 2, z -1 to 3 clear.";

/// A character from what the model made: cleaned to what the game will draw.
pub fn character_from(v: &Value, taken: &[String]) -> Result<(crate::script::CharDef, String, String), String> {
    let mut id: String = v["id"].as_str().unwrap_or("").to_lowercase().chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '_' }).take(32).collect();
    id = id.trim_matches('_').to_string();
    if id.is_empty() || id.chars().next().map_or(true, |c| c.is_ascii_digit()) || id == "n" {
        id = format!("c_{}", id);
    }
    let base = id.clone();
    let mut n = 2;
    while taken.iter().any(|t| *t == id) {
        id = format!("{}_{}", base, n);
        n += 1;
    }
    let name: String = v["name"].as_str().unwrap_or("").chars().filter(|c| !c.is_control()).take(40).collect::<String>().trim().to_string();
    let sprite = v["sprite"].as_str().filter(|s| ["p", "v", "s", "q", "t", "c"].contains(s)).unwrap_or("p").to_string();
    let mut look = std::collections::BTreeMap::new();
    let defaults = crate::art::look_defaults(&sprite);
    for (k, spec) in &defaults {
        let given = &v["look"][k.as_str()];
        if let Some(text) = given.as_str() {
            let h = text.trim().trim_start_matches('#');
            if h.len() == 6 && h.chars().all(|c| c.is_ascii_hexdigit()) && spec.starts_with('#') {
                look.insert(k.clone(), format!("#{}", h.to_lowercase()));
            }
        } else if let Some(x) = given.as_f64().filter(|x| x.is_finite()) {
            let mut parts = spec.split(':').skip(1).filter_map(|p| p.parse::<f64>().ok());
            if let (Some(lo), Some(hi)) = (parts.next(), parts.next()) {
                let x = x.clamp(lo, hi);
                look.insert(k.clone(), format!("{}", (x * 100.0).round() / 100.0));
            }
        }
    }
    let color = v["color"].as_str().map(|c| c.trim().trim_start_matches('#').to_lowercase()).filter(|h| h.len() == 6 && h.chars().all(|c| c.is_ascii_hexdigit())).map_or("#e8d8b0".to_string(), |h| format!("#{}", h));
    let bio = v["bio"].as_str().unwrap_or("").chars().filter(|c| !c.is_control()).take(800).collect::<String>().trim().to_string();
    let prompt = v["art_prompt"].as_str().unwrap_or("").chars().filter(|c| !c.is_control()).take(600).collect::<String>().trim().to_string();
    let def = crate::script::CharDef { name: if name.is_empty() { id.clone() } else { name }, id, color, sprite, look, ..Default::default() };
    Ok((def, bio, prompt))
}

/// A place from what the model made: its name, its painting, and its room if one was asked for.
pub fn place_from(v: &Value, taken: &[String]) -> Result<(String, crate::painting::PaintingData, Option<crate::places::PlaceData>), String> {
    let mut name: String = v["name"].as_str().unwrap_or("").to_lowercase().chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '_' }).take(32).collect();
    name = name.trim_matches('_').to_string();
    if !crate::painting::usable_name(&name) {
        name = format!("place_{}", name).trim_end_matches('_').to_string();
    }
    let base = name.clone();
    let mut n = 2;
    while taken.iter().any(|t| *t == name) || !crate::painting::usable_name(&name) {
        name = format!("{}_{}", base, n);
        n += 1;
    }
    let mut painting = crate::painting::PaintingData {
        title: v["title"].as_str().unwrap_or("").to_string(),
        prompt: v["prompt"].as_str().unwrap_or("").to_string(),
        vignette: v["vignette"].as_f64().unwrap_or(0.25) as f32,
        layers: Vec::new(),
    };
    // one bad layer does not spoil the rest
    let mut dropped = 0;
    for l in v["layers"].as_array().into_iter().flatten() {
        match serde_json::from_value::<crate::painting::Layer>(l.clone()) {
            Ok(l) => painting.layers.push(l),
            Err(_) => dropped += 1,
        }
    }
    if painting.layers.is_empty() {
        return Err(format!("the model painted nothing usable{}", if dropped > 0 { format!(" ({} layer(s) were not shapes the game knows)", dropped) } else { String::new() }));
    }
    painting.clean();
    let room = v.get("room").filter(|r| r.is_object()).map(room_from);
    Ok((name, painting, room))
}

fn room_from(r: &Value) -> crate::places::PlaceData {
    use crate::places::{Light, Part, PlaceData};
    let f = |v: &Value, d: f32| v.as_f64().map(|x| x as f32).filter(|x| x.is_finite()).unwrap_or(d);
    let v3 = |v: &Value, d: [f32; 3]| [f(&v["x"], d[0]), f(&v["y"], d[1]), f(&v["z"], d[2])];
    let colour = |v: &Value, d: &str| v.as_str().unwrap_or(d).to_string();
    let mut place = PlaceData::blank();
    let (hw, hd) = (f(&r["half_width"], 6.0).clamp(2.0, 40.0), f(&r["half_depth"], 6.0).clamp(2.0, 40.0));
    place.floor = [[-hw + 0.4, -hd + 0.4], [hw - 0.4, hd - 0.4]];
    place.spawn = [0.0, 0.0, (hd - 1.0).min(4.0).max(1.0)];
    place.air.sky = colour(&r["sky"], "#8fb4e6");
    place.air.ambient = colour(&r["ambient"], "#ffffff");
    place.air.brightness = f(&r["brightness"], 400.0);
    let parts: Vec<Part> = r["parts"]
        .as_array()
        .into_iter()
        .flatten()
        .take(400)
        .map(|p| Part {
            shape: p["shape"].as_str().unwrap_or("box").to_string(),
            at: v3(&p["at"], [0.0, 0.5, 0.0]),
            rot: [0.0, f(&p["turn"], 0.0), 0.0],
            size: v3(&p["size"], [1.0, 1.0, 1.0]),
            color: colour(&p["color"], "#c0b8a8"),
            glow: f(&p["glow"], 0.0),
            rough: 0.8,
            metal: 0.0,
            shadow: true,
            solid: p["solid"].as_bool().unwrap_or(false),
            file: String::new(),
            name: p["name"].as_str().unwrap_or("").chars().take(40).collect(),
        })
        .collect();
    if !parts.is_empty() {
        place.parts = parts;
    }
    let lights: Vec<Light> = r["lights"]
        .as_array()
        .into_iter()
        .flatten()
        .take(16)
        .map(|l| Light { kind: if l["kind"] == "sun" { "sun".into() } else { "point".into() }, at: v3(&l["at"], [0.0, 3.0, 0.0]), color: colour(&l["color"], "#fff4e0"), power: f(&l["power"], 800.0), range: 0.0, shadows: l["kind"] == "sun" })
        .collect();
    if !lights.is_empty() {
        place.lights = lights;
    }
    place.clean();
    place
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_streamed_answer_is_put_back_together() {
        let sse = "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{}}\n\n\
            event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n\
            event: ping\ndata: {\"type\":\"ping\"}\n\n\
            data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"{\\\"a\\\":\"}}\n\n\
            data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"1}\"}}\n\n\
            data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"}}\n\n\
            data: {\"type\":\"message_stop\"}\n\n";
        assert_eq!(read_stream(sse.as_bytes()).unwrap(), "{\"a\":1}");
        let refused = "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"refusal\"}}\n";
        assert!(read_stream(refused.as_bytes()).unwrap_err().contains("declined"));
        let cut = "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"{\\\"a\\\"\"}}\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"max_tokens\"}}\n";
        assert!(read_stream(cut.as_bytes()).unwrap_err().contains("room"));
        let err = "data: {\"type\":\"error\",\"error\":{\"type\":\"overloaded_error\",\"message\":\"Overloaded\"}}\n";
        assert!(read_stream(err.as_bytes()).unwrap_err().contains("Overloaded"));
        assert_eq!(json_in("Here it is:\n```json\n{\"x\": 2}\n```"), Some(json!({"x": 2})));
        assert_eq!(json_in("no"), None);
    }

    #[test]
    fn what_a_model_makes_is_cleaned_before_use() {
        let made = json!({ "id": "Mrs Hobb!", "name": "Mrs\u{7} Hobb", "color": "zz", "sprite": "x",
            "look": { "skin": "#AABBCC", "hair": "red", "width": 500, "jaw": 0.7, "age": "old", "brow": "#112233", "iris": "#000000", "coat": "#222222", "blush": 0.2, "shoulders": 100, "lashes": 0.5, "lips": 0.5 },
            "bio": "A housekeeper.", "art_prompt": "stern woman" });
        let (def, bio, prompt) = character_from(&made, &["mrs_hobb".to_string()]).unwrap();
        assert_eq!(def.id, "mrs_hobb_2");
        assert_eq!(def.name, "Mrs Hobb");
        assert_eq!((def.sprite.as_str(), def.color.as_str()), ("p", "#e8d8b0"));
        assert_eq!(def.look.get("skin").map(String::as_str), Some("#aabbcc"));
        assert!(!def.look.contains_key("hair") && !def.look.contains_key("age"));
        assert_eq!(def.look.get("width").map(String::as_str), Some("64"));
        assert_eq!((bio.as_str(), prompt.as_str()), ("A housekeeper.", "stern woman"));

        let place = json!({ "name": "pantry", "title": "Roof", "prompt": "a roof", "vignette": 0.3,
            "layers": [ { "shape": "gradient", "top": "#000010", "bottom": "#202040", "y0": 0, "y1": 648 }, { "shape": "spiral" }, { "shape": "rect", "x": 0, "y": 500, "w": 1152, "h": 148, "color": "#303030", "alpha": 1 } ],
            "room": { "sky": "#101020", "ambient": "#8080a0", "brightness": 300, "half_width": 8, "half_depth": 6,
                "parts": [ { "shape": "box", "at": {"x":0,"y":-0.1,"z":0}, "size": {"x":16,"y":0.2,"z":12}, "turn": 0, "color": "#404040", "glow": 0, "solid": false, "name": "roof" } ],
                "lights": [ { "kind": "point", "at": {"x":0,"y":3,"z":0}, "color": "#ffd080", "power": 900 } ] } });
        let (name, painting, room) = place_from(&place, &[]).unwrap();
        assert_eq!(name, "place_pantry", "the game's own names are not taken over");
        assert_eq!(painting.layers.len(), 2, "the shape it does not know is left out");
        let room = room.unwrap();
        assert_eq!(room.parts.len(), 1);
        assert_eq!(room.air.sky, "#101020");
        assert!(place_from(&json!({ "name": "x", "layers": [] }), &[]).is_err());
        // what is asked for can be asked for: every object closed, everything required
        fn closed(v: &Value) {
            if v["type"] == "object" {
                assert_eq!(v["additionalProperties"], false, "{}", v);
                let props: Vec<&String> = v["properties"].as_object().unwrap().keys().collect();
                let req: Vec<&str> = v["required"].as_array().unwrap().iter().map(|r| r.as_str().unwrap()).collect();
                assert_eq!(props.len(), req.len());
            }
            match v {
                Value::Object(m) => m.values().for_each(closed),
                Value::Array(a) => a.iter().for_each(closed),
                _ => {}
            }
        }
        closed(&character_schema());
        closed(&place_schema(true));
        closed(&place_schema(false));
    }

    #[test]
    fn ollama_is_found_where_it_says() {
        // (only the parsing of OLLAMA_HOST, which is read when asked)
        std::env::set_var("OLLAMA_HOST", "0.0.0.0");
        assert_eq!(ollama_base(), "http://127.0.0.1:11434");
        std::env::set_var("OLLAMA_HOST", "http://gpu-box:9000/");
        assert_eq!(ollama_base(), "http://gpu-box:9000");
        std::env::remove_var("OLLAMA_HOST");
        assert_eq!(ollama_base(), "http://127.0.0.1:11434");
        // and nothing on this computer goes through a proxy
        assert_eq!(proxy_for("http://127.0.0.1:11434/api/tags"), None);
        assert_eq!(proxy_for("http://localhost:8080/v1/messages"), None);
    }
}
