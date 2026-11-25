//! The story: a structured document (`Doc`) that round-trips to the
//! line-based text format in assets/story.txt and to JSON for the visual
//! editor, the compiled `Script` the game runs, and the `Vm` that walks it.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};

/// Speaker id for narration; always available, never declared.
pub const NARRATOR: &str = "n";
pub const SLOTS: [&str; 3] = ["left", "center", "right"];
pub const OPS: [&str; 6] = ["==", "!=", ">=", "<=", ">", "<"];

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Cond {
    pub var: String,
    pub op: String,
    pub val: i32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Opt {
    #[serde(default)]
    pub cond: Option<Cond>,
    pub text: String,
    pub target: String,
}

/// A character: who can speak, how their name is shown, and how they look.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct CharDef {
    pub id: String,
    pub name: String,
    /// Name-tag colour, "#rrggbb".
    #[serde(default)]
    pub color: String,
    /// Which built-in portrait to paint ("" = never shown on screen).
    #[serde(default)]
    pub sprite: String,
    /// Overrides for that portrait: colours and proportions.
    #[serde(default)]
    pub look: BTreeMap<String, String>,
}

/// A story property (variable): something choices change and conditions test.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct VarDef {
    pub id: String,
    /// Shown to the player when it changes, and on its meter.
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub initial: i32,
    /// Size of the HUD meter; 0 for none.
    #[serde(default)]
    pub meter: i32,
    /// Whether a rise is good news for the player.
    #[serde(default = "yes")]
    pub good: bool,
    /// The character this property belongs to, if any.
    #[serde(default)]
    pub character: String,
    /// Hidden properties change silently.
    #[serde(default)]
    pub hidden: bool,
}

fn yes() -> bool {
    true
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "lowercase")]
pub enum Item {
    Say { who: String, text: String },
    Bg { name: String },
    Show { who: String, expr: String, slot: String },
    /// `slot` is left, center, right or all.
    Hide { slot: String },
    Add { var: String, n: i32 },
    Set { var: String, n: i32 },
    If { var: String, op: String, val: i32, target: String },
    Goto { target: String },
    Card { title: String, sub: String },
    End { title: String, text: String },
    Choice { options: Vec<Opt> },
    Comment { text: String },
}

/// A chapter: a named group of scenes that the editor shows as one panel
/// on the main graph and opens as a graph of its own. Chapters are for
/// organising a large story; the game itself does not look at them.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ChapterDef {
    pub name: String,
    /// Position of the chapter's panel on the main graph.
    #[serde(default)]
    pub pos: Option<(f32, f32)>,
    #[serde(default)]
    pub note: String,
}

/// A labelled run of the story. Flow falls through to the next scene in
/// the document unless the scene ends in a jump, a choice or an ending.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Scene {
    pub label: String,
    /// The chapter this scene belongs to ("" for the main graph).
    #[serde(default)]
    pub chapter: String,
    /// Position on the editor's canvas (the chapter's own, if it is in one).
    #[serde(default)]
    pub pos: Option<(f32, f32)>,
    #[serde(default)]
    pub items: Vec<Item>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Doc {
    /// Shown on the game's title screen and in the editor's story list.
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub subtitle: String,
    #[serde(default)]
    pub header: Vec<String>,
    #[serde(default)]
    pub characters: Vec<CharDef>,
    #[serde(default)]
    pub vars: Vec<VarDef>,
    #[serde(default)]
    pub chapters: Vec<ChapterDef>,
    #[serde(default)]
    pub scenes: Vec<Scene>,
}

/// Something wrong with a story, located for the editor.
#[derive(Clone, Debug, Serialize)]
pub struct Problem {
    /// "error" stops the game from running; "warning" does not.
    pub level: &'static str,
    pub scene: String,
    pub item: Option<usize>,
    pub msg: String,
}

pub fn parse_color(s: &str) -> Option<u32> {
    let hex = s.strip_prefix('#')?;
    if hex.len() != 6 {
        return None;
    }
    u32::from_str_radix(hex, 16).ok().map(|v| 0xFF00_0000 | v)
}

pub fn valid_id(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Story text is one line per element: fold any line breaks into spaces.
fn one_line(s: &str) -> String {
    s.replace(['\r', '\n'], " ").trim().to_string()
}

/// The same for comments, which may be indented.
fn comment_line(s: &str) -> String {
    s.replace(['\r', '\n'], " ").trim_end().to_string()
}

fn parse_cond(toks: &[&str]) -> Result<Cond, String> {
    if toks.len() != 3 || !OPS.contains(&toks[1]) {
        return Err(format!("bad condition '{}'", toks.join(" ")));
    }
    let val = toks[2].parse().map_err(|_| format!("bad number '{}'", toks[2]))?;
    Ok(Cond { var: toks[0].into(), op: toks[1].into(), val })
}

fn split_pair(rest: &str) -> (String, String) {
    match rest.split_once('|') {
        Some((a, b)) => (a.trim().into(), b.trim().into()),
        None => (rest.trim().into(), String::new()),
    }
}

enum Line {
    Title(String, String),
    Character(CharDef),
    Var(VarDef),
    Chapter(ChapterDef),
    InChapter(String),
    Label(String),
    Pos(f32, f32),
    Option(Opt),
    Item(Item),
}

fn parse_line(line: &str) -> Result<Line, String> {
    if let Some(text) = line.strip_prefix('#') {
        let text = text.strip_prefix(' ').unwrap_or(text);
        if let Some(rest) = text.strip_prefix("@pos ") {
            let nums: Vec<f32> = rest.split_whitespace().filter_map(|v| v.parse().ok()).collect();
            if nums.len() == 2 {
                return Ok(Line::Pos(nums[0], nums[1]));
            }
        }
        if let Some(name) = text.strip_prefix("@chapter ") {
            return Ok(Line::InChapter(name.trim().to_string()));
        }
        return Ok(Line::Item(Item::Comment { text: text.to_string() }));
    }
    if let Some(name) = line.strip_prefix('*') {
        return Ok(Line::Label(name.trim().to_string()));
    }
    if let Some(opt) = line.strip_prefix("- ") {
        let (mut text, target) = opt
            .rsplit_once("=>")
            .map(|(a, b)| (a.trim(), b.trim().to_string()))
            .ok_or("choice option needs '=> label'")?;
        let mut cond = None;
        if let Some(rest) = text.strip_prefix('[') {
            let (c, t) = rest.split_once(']').ok_or("missing ']'")?;
            cond = Some(parse_cond(&c.split_whitespace().collect::<Vec<_>>())?);
            text = t.trim();
        }
        return Ok(Line::Option(Opt { cond, text: text.into(), target }));
    }
    let toks: Vec<&str> = line.split_whitespace().collect();
    let rest = line[toks[0].len()..].trim();
    let number = |s: &str| s.parse::<i32>().map_err(|_| format!("bad number '{}'", s));
    let item = match toks[0] {
        "title" => {
            let (title, subtitle) = split_pair(rest);
            return Ok(Line::Title(title, subtitle));
        }
        "character" => {
            let parts: Vec<&str> = rest.split('|').map(str::trim).collect();
            if parts.len() < 2 {
                return Err("character needs: id | name | #colour | portrait | look".into());
            }
            let mut look = BTreeMap::new();
            for kv in parts.get(4).unwrap_or(&"").split_whitespace() {
                let (k, v) = kv.split_once('=').ok_or(format!("bad look setting '{}'", kv))?;
                look.insert(k.to_string(), v.to_string());
            }
            return Ok(Line::Character(CharDef {
                id: parts[0].into(),
                name: parts[1].into(),
                color: parts.get(2).unwrap_or(&"").to_string(),
                sprite: parts.get(3).map(|s| if *s == "-" { "" } else { s }).unwrap_or("").to_string(),
                look,
            }));
        }
        "var" => {
            let parts: Vec<&str> = rest.split('|').map(str::trim).collect();
            let mut v = VarDef { id: parts[0].into(), good: true, ..VarDef::default() };
            v.label = parts.get(1).unwrap_or(&"").to_string();
            if let Some(n) = parts.get(2).filter(|s| !s.is_empty()) {
                v.initial = number(n)?;
            }
            for flag in parts.get(3).unwrap_or(&"").split_whitespace() {
                match flag.split_once('=') {
                    Some(("meter", n)) => v.meter = number(n)?,
                    Some(("character", c)) => v.character = c.to_string(),
                    None if flag == "good" => v.good = true,
                    None if flag == "bad" => v.good = false,
                    None if flag == "hidden" => v.hidden = true,
                    _ => return Err(format!("unknown property flag '{}'", flag)),
                }
            }
            return Ok(Line::Var(v));
        }
        "chapter" => {
            // chapter Name | x y | note - written by hand, the position is often left out
            let parts: Vec<&str> = rest.splitn(3, '|').map(str::trim).collect();
            let second = parts.get(1).copied().unwrap_or("");
            let words: Vec<&str> = second.split_whitespace().collect();
            let xy: Vec<f32> = words.iter().filter_map(|v| v.parse().ok()).collect();
            let is_pos = words.len() == 2 && xy.len() == 2;
            let note = match (is_pos || second.is_empty(), parts.get(2)) {
                (true, third) => third.unwrap_or(&"").to_string(),
                (false, None) => second.to_string(),
                (false, Some(third)) => format!("{} | {}", second, third),
            };
            return Ok(Line::Chapter(ChapterDef { name: parts[0].to_string(), pos: if is_pos { Some((xy[0], xy[1])) } else { None }, note }));
        }
        "bg" if toks.len() == 2 => Item::Bg { name: toks[1].into() },
        "show" if toks.len() == 4 => Item::Show { who: toks[1].into(), expr: toks[2].into(), slot: toks[3].into() },
        "hide" if toks.len() == 2 => Item::Hide { slot: toks[1].into() },
        "add" if toks.len() == 3 => Item::Add { var: toks[1].into(), n: number(toks[2])? },
        "set" if toks.len() == 3 => Item::Set { var: toks[1].into(), n: number(toks[2])? },
        "if" if toks.len() == 6 && toks[4] == "goto" => {
            let c = parse_cond(&toks[1..4])?;
            Item::If { var: c.var, op: c.op, val: c.val, target: toks[5].into() }
        }
        "goto" if toks.len() == 2 => Item::Goto { target: toks[1].into() },
        "choice" if toks.len() == 1 => Item::Choice { options: Vec::new() },
        "card" => {
            let (title, sub) = split_pair(rest);
            Item::Card { title, sub }
        }
        "end" => {
            let (title, text) = split_pair(rest);
            Item::End { title, text }
        }
        _ => match line.split_once(": ") {
            Some((who, text)) if !who.contains(' ') => Item::Say { who: who.into(), text: text.trim().into() },
            // a line of dialogue not yet written: "who:" and nothing else
            None if toks.len() == 1 && line.ends_with(':') && line.len() > 1 => Item::Say { who: line[..line.len() - 1].into(), text: String::new() },
            _ => return Err(format!("cannot parse '{}'", line)),
        },
    };
    Ok(Line::Item(item))
}

impl Doc {
    /// Parse story text. Lines that cannot be understood are kept as
    /// comments (so nothing is lost) and reported in the error list.
    pub fn parse(src: &str) -> (Doc, Vec<String>) {
        let mut doc = Doc::default();
        let mut errors = Vec::new();
        for (n, raw) in src.lines().enumerate() {
            let line = raw.trim();
            if line.is_empty() {
                continue;
            }
            let parsed = parse_line(line).unwrap_or_else(|e| {
                errors.push(format!("story line {}: {}", n + 1, e));
                Line::Item(Item::Comment { text: format!("UNPARSED: {}", line) })
            });
            match parsed {
                Line::Title(title, subtitle) => {
                    doc.title = title;
                    doc.subtitle = subtitle;
                }
                Line::Character(c) => doc.characters.push(c),
                Line::Var(v) => doc.vars.push(v),
                Line::Chapter(c) => doc.chapters.push(c),
                Line::InChapter(name) => match doc.scenes.last_mut() {
                    // like the position marker, it sits straight under the label
                    Some(scene) if scene.items.is_empty() && scene.chapter.is_empty() && !scene.label.is_empty() => scene.chapter = name,
                    Some(scene) => scene.items.push(Item::Comment { text: format!("(@chapter {})", name) }),
                    None => doc.header.push(format!("(@chapter {})", name)),
                },
                Line::Label(label) => doc.scenes.push(Scene { label, ..Scene::default() }),
                Line::Pos(x, y) => match doc.scenes.last_mut() {
                    // the marker is the line straight after the label; anything else is a note
                    Some(scene) if scene.items.is_empty() && scene.pos.is_none() && !scene.label.is_empty() => scene.pos = Some((x, y)),
                    Some(scene) => scene.items.push(Item::Comment { text: format!("(@pos {} {})", x, y) }),
                    None => doc.header.push(format!("(@pos {} {})", x, y)),
                },
                Line::Option(opt) => {
                    // an option belongs to the choice above it, even past notes in between
                    let items = doc.scenes.last_mut().map(|s| &mut s.items);
                    let choice = items.and_then(|items| items.iter_mut().rev().find(|i| !matches!(i, Item::Comment { .. })));
                    match choice {
                        Some(Item::Choice { options }) => options.push(opt),
                        _ => {
                            errors.push(format!("story line {}: option outside of a 'choice' block", n + 1));
                            if doc.scenes.is_empty() {
                                doc.scenes.push(Scene::default());
                            }
                            doc.scenes.last_mut().unwrap().items.push(Item::Comment { text: format!("UNPARSED: {}", line) });
                        }
                    }
                }
                Line::Item(Item::Comment { text }) if doc.scenes.is_empty() => doc.header.push(text),
                Line::Item(item) => {
                    if doc.scenes.is_empty() {
                        doc.scenes.push(Scene::default());
                    }
                    doc.scenes.last_mut().unwrap().items.push(item);
                }
            }
        }
        (doc, errors)
    }

    /// Tidy a document that came from the editor: single-line text, trimmed ids.
    pub fn sanitize(&mut self) {
        let id = |s: &mut String| *s = s.trim().to_string();
        let pipe_free = |s: &mut String| *s = one_line(s).replace('|', "/");
        // text that ends a line must not end in the field separator
        let last_field = |s: &str| one_line(s).trim_end_matches([' ', '|']).to_string();
        pipe_free(&mut self.title);
        self.subtitle = last_field(&self.subtitle);
        for h in self.header.iter_mut() {
            *h = comment_line(h);
            if h.starts_with("@pos ") || h.starts_with("@chapter ") {
                h.insert(0, '(');
            }
        }
        // items of an unlabelled scene anywhere but first would be absorbed by the scene before it
        let mut i = 1;
        while i < self.scenes.len() {
            if self.scenes[i].label.trim().is_empty() {
                let stray = self.scenes.remove(i);
                self.scenes[i - 1].items.extend(stray.items);
            } else {
                i += 1;
            }
        }
        for c in &mut self.characters {
            id(&mut c.id);
            pipe_free(&mut c.name);
            c.color = c.color.trim().to_lowercase();
            id(&mut c.sprite);
            c.look = c.look.iter().map(|(k, v)| (k.trim().to_string(), v.trim().to_string())).filter(|(k, v)| valid_id(k) && !v.is_empty() && !v.contains([' ', '|'])).collect();
        }
        for v in &mut self.vars {
            id(&mut v.id);
            pipe_free(&mut v.label);
            id(&mut v.character);
        }
        // every chapter a scene names exists, once
        for c in &mut self.chapters {
            pipe_free(&mut c.name);
            c.note = last_field(&c.note).replace('|', "/");
        }
        for scene in &mut self.scenes {
            scene.chapter = one_line(&scene.chapter).replace('|', "/");
            if !scene.chapter.is_empty() && !self.chapters.iter().any(|c| c.name == scene.chapter) {
                self.chapters.push(ChapterDef { name: scene.chapter.clone(), ..Default::default() });
            }
        }
        let mut seen = HashSet::new();
        self.chapters.retain(|c| !c.name.is_empty() && seen.insert(c.name.clone()));
        for scene in &mut self.scenes {
            id(&mut scene.label);
            for item in &mut scene.items {
                match item {
                    Item::Say { who, text } => {
                        id(who);
                        *text = one_line(text);
                    }
                    Item::Bg { name } => id(name),
                    Item::Show { who, expr, slot } => {
                        id(who);
                        id(expr);
                        id(slot);
                    }
                    Item::Hide { slot } => id(slot),
                    Item::Add { var, .. } | Item::Set { var, .. } => id(var),
                    Item::If { var, op, target, .. } => {
                        id(var);
                        id(op);
                        id(target);
                    }
                    Item::Goto { target } => id(target),
                    Item::Card { title, sub } => {
                        pipe_free(title);
                        *sub = one_line(sub);
                    }
                    Item::End { title, text } => {
                        pipe_free(title);
                        *text = one_line(text);
                    }
                    Item::Choice { options } => {
                        for o in options {
                            // "=>" separates text from target, and a leading "[" starts a condition
                            o.text = one_line(&o.text).replace("=>", "->");
                            if o.text.starts_with('[') {
                                o.text.replace_range(0..1, "(");
                            }
                            id(&mut o.target);
                            if let Some(c) = &mut o.cond {
                                id(&mut c.var);
                                id(&mut c.op);
                            }
                        }
                    }
                    Item::Comment { text } => {
                        *text = comment_line(text);
                        if text.starts_with("@pos ") || text.starts_with("@chapter ") {
                            text.insert(0, '(');
                        }
                    }
                }
            }
        }
    }

    /// Write the document in the story text format.
    pub fn to_text(&self) -> String {
        let mut out = String::new();
        let comment = |out: &mut String, text: &str| {
            out.push_str(if text.is_empty() { "#" } else { "# " });
            out.push_str(text);
            out.push('\n');
        };
        for h in &self.header {
            comment(&mut out, h);
        }
        if !self.title.is_empty() || !self.subtitle.is_empty() {
            out.push_str(format!("\ntitle {} | {}", self.title, self.subtitle).trim_end_matches([' ', '|']));
            out.push('\n');
        }
        if !self.characters.is_empty() {
            out.push('\n');
        }
        for c in &self.characters {
            let look: Vec<String> = c.look.iter().map(|(k, v)| format!("{}={}", k, v)).collect();
            let sprite = if c.sprite.is_empty() { "-" } else { &c.sprite };
            out.push_str(format!("character {} | {} | {} | {} | {}", c.id, c.name, c.color, sprite, look.join(" ")).trim_end_matches([' ', '|']));
            out.push('\n');
        }
        if !self.vars.is_empty() {
            out.push('\n');
        }
        for v in &self.vars {
            let mut flags = Vec::new();
            if v.meter > 0 {
                flags.push(format!("meter={}", v.meter));
            }
            flags.push(if v.good { "good".to_string() } else { "bad".to_string() });
            if !v.character.is_empty() {
                flags.push(format!("character={}", v.character));
            }
            if v.hidden {
                flags.push("hidden".into());
            }
            out.push_str(&format!("var {} | {} | {} | {}\n", v.id, v.label, v.initial, flags.join(" ")));
        }
        if !self.chapters.is_empty() {
            out.push('\n');
        }
        for c in &self.chapters {
            let pos = c.pos.map(|(x, y)| format!("{} {}", x.round(), y.round())).unwrap_or_default();
            out.push_str(format!("chapter {} | {} | {}", c.name, pos, c.note).trim_end_matches([' ', '|']));
            out.push('\n');
        }
        for scene in &self.scenes {
            out.push('\n');
            if !scene.label.is_empty() {
                out.push_str(&format!("*{}\n", scene.label));
                if !scene.chapter.is_empty() {
                    out.push_str(&format!("# @chapter {}\n", scene.chapter));
                }
                if let Some((x, y)) = scene.pos {
                    out.push_str(&format!("# @pos {} {}\n", x.round(), y.round()));
                }
            }
            for item in &scene.items {
                match item {
                    Item::Say { who, text } => out.push_str(&format!("{}: {}\n", who, text)),
                    Item::Bg { name } => out.push_str(&format!("bg {}\n", name)),
                    Item::Show { who, expr, slot } => out.push_str(&format!("show {} {} {}\n", who, expr, slot)),
                    Item::Hide { slot } => out.push_str(&format!("hide {}\n", slot)),
                    Item::Add { var, n } => out.push_str(&format!("add {} {}\n", var, n)),
                    Item::Set { var, n } => out.push_str(&format!("set {} {}\n", var, n)),
                    Item::If { var, op, val, target } => out.push_str(&format!("if {} {} {} goto {}\n", var, op, val, target)),
                    Item::Goto { target } => out.push_str(&format!("goto {}\n", target)),
                    Item::Card { title, sub } => out.push_str(&format!("card {} | {}\n", title, sub)),
                    Item::End { title, text } => out.push_str(&format!("end {} | {}\n", title, text)),
                    Item::Choice { options } => {
                        out.push_str("choice\n");
                        for o in options {
                            let cond = o.cond.as_ref().map(|c| format!("[{} {} {}] ", c.var, c.op, c.val)).unwrap_or_default();
                            out.push_str(&format!("- {}{} => {}\n", cond, o.text, o.target));
                        }
                    }
                    Item::Comment { text } => comment(&mut out, text),
                }
            }
        }
        out
    }

    /// Where a scene's flow can go next: (target label, falls-through?).
    fn exits(&self, i: usize) -> Vec<String> {
        let mut out = Vec::new();
        let mut open = true; // can flow run off the end of this scene?
        for item in &self.scenes[i].items {
            match item {
                Item::If { target, .. } => out.push(target.clone()),
                Item::Goto { target } => {
                    out.push(target.clone());
                    open = false;
                    break;
                }
                Item::Choice { options } => {
                    out.extend(options.iter().map(|o| o.target.clone()));
                    open = false;
                    break;
                }
                Item::End { .. } => {
                    open = false;
                    break;
                }
                _ => {}
            }
        }
        if open {
            out.push(self.scenes.get(i + 1).map(|s| s.label.clone()).unwrap_or_default());
        }
        out
    }

    /// Scenes flow can reach from the top of scene `i` before anything is
    /// put in front of the player (a line, a choice, a card or an ending).
    fn silent_exits(&self, i: usize) -> Vec<String> {
        let mut out = Vec::new();
        for item in &self.scenes[i].items {
            match item {
                Item::If { target, .. } => out.push(target.clone()),
                Item::Goto { target } => {
                    out.push(target.clone());
                    return out;
                }
                Item::Say { .. } | Item::Choice { .. } | Item::Card { .. } | Item::End { .. } => return out,
                _ => {}
            }
        }
        out.push(self.scenes.get(i + 1).map(|s| s.label.clone()).unwrap_or_default());
        out
    }

    /// Whether writing this document as text and reading it back gives the
    /// same document. The editor refuses to save one that would not.
    pub fn survives_text(&self) -> Result<(), String> {
        let (again, errors) = Doc::parse(&self.to_text());
        if let Some(e) = errors.first() {
            return Err(format!("something is incomplete and cannot be written to the story file ({})", e.replacen("story line", "it would be line", 1)));
        }
        if &again != self {
            let scene = self.scenes.iter().zip(again.scenes.iter()).find(|(a, b)| a != b).map(|(a, _)| a.label.clone());
            return Err(match scene {
                Some(s) => format!("scene '{}' holds something that cannot be written to the story file", s),
                None => "the cast, properties or scene list hold something that cannot be written to the story file".into(),
            });
        }
        Ok(())
    }

    /// Everything wrong with the story. `exprs`, `bgs` and `sprites` are the
    /// expressions, backgrounds and portraits the game can draw.
    pub fn problems(&self, exprs: &[&str], bgs: &[&str], sprites: &[&str]) -> Vec<Problem> {
        let out = std::cell::RefCell::new(Vec::new());
        let push = |level: &'static str, scene: &str, item: Option<usize>, msg: String| out.borrow_mut().push(Problem { level, scene: scene.to_string(), item, msg });
        let chars: HashMap<&str, &CharDef> = self.characters.iter().map(|c| (c.id.as_str(), c)).collect();
        let vars: HashSet<&str> = self.vars.iter().map(|v| v.id.as_str()).collect();
        let labels: HashSet<&str> = self.scenes.iter().map(|s| s.label.as_str()).collect();

        let mut seen = HashSet::new();
        for c in &self.characters {
            if !valid_id(&c.id) || c.id == NARRATOR {
                push("error", "", None, format!("character id '{}' is not usable (letters, digits and _ only; '{}' is the narrator)", c.id, NARRATOR));
            }
            if !seen.insert(c.id.as_str()) {
                push("error", "", None, format!("two characters share the id '{}'", c.id));
            }
            if c.name.is_empty() {
                push("warning", "", None, format!("character '{}' has no name", c.id));
            }
            if !c.color.is_empty() && parse_color(&c.color).is_none() {
                push("error", "", None, format!("character '{}': colour '{}' should look like #c8b4ff", c.id, c.color));
            }
            if !c.sprite.is_empty() && !sprites.contains(&c.sprite.as_str()) {
                push("error", "", None, format!("character '{}': unknown portrait '{}'", c.id, c.sprite));
            }
        }
        seen.clear();
        for v in &self.vars {
            if !valid_id(&v.id) {
                push("error", "", None, format!("property id '{}' is not usable (letters, digits and _ only)", v.id));
            }
            if !seen.insert(v.id.as_str()) {
                push("error", "", None, format!("two properties share the id '{}'", v.id));
            }
            if !v.character.is_empty() && !chars.contains_key(v.character.as_str()) {
                push("error", "", None, format!("property '{}' belongs to unknown character '{}'", v.id, v.character));
            }
        }
        seen.clear();
        if !labels.contains("start") {
            push("error", "", None, "there is no scene called 'start'".into());
        }
        for (si, scene) in self.scenes.iter().enumerate() {
            let s = scene.label.as_str();
            if !valid_id(s) {
                push("error", s, None, if s.is_empty() { "content before the first scene label".to_string() } else { format!("scene name '{}' is not usable (letters, digits and _ only)", s) });
            } else if !seen.insert(s) {
                push("error", s, None, format!("two scenes are called '{}'", s));
            }
            let target = |t: &String, i: usize| {
                if !labels.contains(t.as_str()) || t.is_empty() {
                    push("error", s, Some(i), format!("jumps to '{}', which does not exist", t));
                }
            };
            let mut ended = false;
            for (i, item) in scene.items.iter().enumerate() {
                let mut err = |msg: String| push("error", s, Some(i), msg);
                let var = |v: &String, err: &mut dyn FnMut(String)| {
                    if !vars.contains(v.as_str()) {
                        err(format!("unknown property '{}'", v));
                    }
                };
                if ended && !matches!(item, Item::Comment { .. }) {
                    push("warning", s, Some(i), "never reached: it comes after a jump, choice or ending".into());
                    break;
                }
                match item {
                    Item::Say { who, text } => {
                        if who != NARRATOR && !chars.contains_key(who.as_str()) {
                            err(format!("unknown speaker '{}'", who));
                        }
                        if text.is_empty() {
                            err("empty line of dialogue".into());
                        }
                    }
                    Item::Bg { name } => {
                        if !bgs.contains(&name.as_str()) {
                            err(format!("unknown background '{}'", name));
                        }
                    }
                    Item::Show { who, expr, slot } => {
                        match chars.get(who.as_str()) {
                            None => err(format!("unknown character '{}'", who)),
                            Some(c) if c.sprite.is_empty() => err(format!("'{}' has no portrait to show", who)),
                            _ => {}
                        }
                        if !exprs.contains(&expr.as_str()) {
                            err(format!("unknown expression '{}'", expr));
                        }
                        if !SLOTS.contains(&slot.as_str()) {
                            err(format!("unknown position '{}'", slot));
                        }
                    }
                    Item::Hide { slot } => {
                        if slot != "all" && !SLOTS.contains(&slot.as_str()) {
                            err(format!("unknown position '{}'", slot));
                        }
                    }
                    Item::Add { var: v, .. } | Item::Set { var: v, .. } => var(v, &mut err),
                    Item::If { var: v, op, target: t, .. } => {
                        var(v, &mut err);
                        if !OPS.contains(&op.as_str()) {
                            err(format!("unknown comparison '{}'", op));
                        }
                        target(t, i);
                    }
                    Item::Goto { target: t } => {
                        target(t, i);
                        ended = true;
                    }
                    Item::Choice { options } => {
                        if options.is_empty() || options.iter().all(|o| o.cond.is_some()) {
                            err("a choice needs at least one option without a condition".into());
                        }
                        for o in options {
                            if o.text.is_empty() {
                                err("a choice option has no text".into());
                            }
                            if let Some(c) = &o.cond {
                                var(&c.var, &mut err);
                                if !OPS.contains(&c.op.as_str()) {
                                    err(format!("unknown comparison '{}'", c.op));
                                }
                            }
                            target(&o.target, i);
                        }
                        ended = true;
                    }
                    Item::Card { title, .. } | Item::End { title, .. } => {
                        if title.is_empty() {
                            err("needs a title".into());
                        }
                        ended = matches!(item, Item::End { .. });
                    }
                    Item::Comment { text } => {
                        if text.starts_with("UNPARSED: ") {
                            err("this line could not be understood".into());
                        }
                    }
                }
            }
            if !ended && si + 1 == self.scenes.len() {
                push("warning", s, None, "the story runs off the end here; finish with an ending or a jump".into());
            }
        }
        let index: HashMap<&str, usize> = self.scenes.iter().enumerate().map(|(i, s)| (s.label.as_str(), i)).collect();
        // jumps that can chase each other for ever without anything reaching the screen
        let silent: Vec<Vec<usize>> = (0..self.scenes.len()).map(|i| self.silent_exits(i).iter().filter_map(|t| index.get(t.as_str()).copied()).collect()).collect();
        let mut state = vec![0u8; self.scenes.len()];
        fn cycles(i: usize, g: &[Vec<usize>], state: &mut [u8], hits: &mut Vec<usize>) {
            state[i] = 1;
            for &t in &g[i] {
                if state[t] == 1 {
                    hits.push(t);
                } else if state[t] == 0 {
                    cycles(t, g, state, hits);
                }
            }
            state[i] = 2;
        }
        let mut hits = Vec::new();
        for i in 0..self.scenes.len() {
            if state[i] == 0 {
                cycles(i, &silent, &mut state, &mut hits);
            }
        }
        hits.sort();
        hits.dedup();
        for i in hits {
            push("error", &self.scenes[i].label, None, "jumps can circle back here without a line, choice or ending in between, which would hang the game".into());
        }
        // scenes nothing leads to
        let mut reached = vec![false; self.scenes.len()];
        let mut stack: Vec<usize> = index.get("start").copied().into_iter().collect();
        while let Some(i) = stack.pop() {
            if std::mem::replace(&mut reached[i], true) {
                continue;
            }
            stack.extend(self.exits(i).iter().filter_map(|t| index.get(t.as_str()).copied()));
        }
        for (i, scene) in self.scenes.iter().enumerate() {
            if !reached[i] && index.contains_key("start") {
                push("warning", &scene.label, None, "nothing leads to this scene".into());
            }
        }
        out.into_inner()
    }
}

// ----------------------------------------------------------- the runtime

#[derive(Clone, Debug)]
pub enum Cmd {
    Bg(String),
    Show { who: String, expr: String, slot: usize },
    /// `None` hides everyone.
    Hide(Option<usize>),
    Say { who: String, text: String },
    Choice(Vec<Opt>),
    Add(String, i32),
    Set(String, i32),
    If(Cond, String),
    Goto(String),
    Card(String, String),
    End(String, String),
}

pub struct Script {
    pub title: String,
    pub subtitle: String,
    pub cmds: Vec<Cmd>,
    pub labels: HashMap<String, usize>,
    pub characters: Vec<CharDef>,
    pub vars: Vec<VarDef>,
}

impl Script {
    /// Compile a document. Call `Doc::problems` first; unknown slots and
    /// the like are skipped here rather than reported.
    pub fn compile(doc: &Doc) -> Script {
        let mut cmds = Vec::new();
        let mut labels = HashMap::new();
        let slot = |s: &str| SLOTS.iter().position(|x| *x == s);
        for scene in &doc.scenes {
            labels.entry(scene.label.clone()).or_insert(cmds.len());
            for item in &scene.items {
                cmds.push(match item.clone() {
                    Item::Say { who, text } => Cmd::Say { who, text },
                    Item::Bg { name } => Cmd::Bg(name),
                    Item::Show { who, expr, slot: s } => match slot(&s) {
                        Some(slot) => Cmd::Show { who, expr, slot },
                        None => continue,
                    },
                    Item::Hide { slot: s } => Cmd::Hide(slot(&s)),
                    Item::Add { var, n } => Cmd::Add(var, n),
                    Item::Set { var, n } => Cmd::Set(var, n),
                    Item::If { var, op, val, target } => Cmd::If(Cond { var, op, val }, target),
                    Item::Goto { target } => Cmd::Goto(target),
                    Item::Card { title, sub } => Cmd::Card(title, sub),
                    Item::End { title, text } => Cmd::End(title, text),
                    Item::Choice { options } => Cmd::Choice(options),
                    Item::Comment { .. } => continue,
                });
            }
        }
        Script { title: doc.title.clone(), subtitle: doc.subtitle.clone(), cmds, labels, characters: doc.characters.clone(), vars: doc.vars.clone() }
    }

    pub fn character(&self, id: &str) -> Option<&CharDef> {
        self.characters.iter().find(|c| c.id == id)
    }

    pub fn var_def(&self, id: &str) -> Option<&VarDef> {
        self.vars.iter().find(|v| v.id == id)
    }
}

/// Program counter plus story variables.
#[derive(Clone, Default)]
pub struct Vm {
    pub pc: usize,
    pub vars: HashMap<String, i32>,
}

impl Vm {
    /// A fresh run: every property at its declared starting value.
    pub fn new(s: &Script) -> Vm {
        Vm { pc: 0, vars: s.vars.iter().map(|v| (v.id.clone(), v.initial)).collect() }
    }

    pub fn var(&self, name: &str) -> i32 {
        self.vars.get(name).copied().unwrap_or(0)
    }

    pub fn test(&self, c: &Cond) -> bool {
        let v = self.var(&c.var);
        match c.op.as_str() {
            "==" => v == c.val,
            "!=" => v != c.val,
            ">=" => v >= c.val,
            "<=" => v <= c.val,
            ">" => v > c.val,
            _ => v < c.val,
        }
    }

    pub fn jump(&mut self, s: &Script, label: &str) {
        self.pc = s.labels.get(label).copied().unwrap_or(s.cmds.len());
    }

    /// Run control flow and return the next command the presentation layer
    /// must handle, with its index. `Add` is applied here and also returned
    /// so the game can announce it.
    pub fn next(&mut self, s: &Script) -> Option<(usize, Cmd)> {
        // the validator rejects jump loops, but never trust a story file with the CPU
        for _ in 0..100_000 {
            let i = self.pc;
            let cmd = s.cmds.get(i)?.clone();
            self.pc += 1;
            match &cmd {
                Cmd::Set(v, n) => {
                    self.vars.insert(v.clone(), *n);
                }
                Cmd::Add(v, n) => {
                    *self.vars.entry(v.clone()).or_insert(0) += n;
                    return Some((i, cmd));
                }
                Cmd::If(c, l) => {
                    if self.test(c) {
                        self.jump(s, l);
                    }
                }
                Cmd::Goto(l) => self.jump(s, l),
                _ => return Some((i, cmd)),
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    const STORY: &str = include_str!("../assets/story.txt");

    fn problems(doc: &Doc) -> Vec<Problem> {
        doc.problems(crate::art::EXPRS, crate::art::BACKGROUNDS, crate::art::SPRITES)
    }

    fn story() -> Doc {
        let (doc, errors) = Doc::parse(STORY);
        assert!(errors.is_empty(), "{:#?}", errors);
        doc
    }

    #[test]
    fn story_is_valid() {
        let p = problems(&story());
        assert!(p.is_empty(), "{:#?}", p);
    }

    fn explore(s: &Script, mut vm: Vm, endings: &mut BTreeSet<String>, paths: &mut usize) {
        for _ in 0..5000 {
            match vm.next(s) {
                None => panic!("script ran off the end without an ending"),
                Some((_, Cmd::End(title, _))) => {
                    endings.insert(title);
                    *paths += 1;
                    return;
                }
                Some((_, Cmd::Choice(opts))) => {
                    for o in opts.iter().filter(|o| o.cond.as_ref().map_or(true, |c| vm.test(c))) {
                        let mut branch = vm.clone();
                        branch.jump(s, &o.target);
                        explore(s, branch, endings, paths);
                    }
                    return;
                }
                _ => {}
            }
        }
        panic!("path did not terminate (loop in the story?)");
    }

    /// Play every possible combination of choices: each must reach an
    /// ending, and all five endings must be reachable.
    #[test]
    fn every_path_reaches_an_ending() {
        let s = Script::compile(&story());
        let mut vm = Vm::new(&s);
        vm.jump(&s, "start");
        let (mut endings, mut paths) = (BTreeSet::new(), 0);
        explore(&s, vm, &mut endings, &mut paths);
        println!("{} paths, endings: {:?}", paths, endings);
        assert_eq!(endings.len(), 5, "{:?}", endings);
    }

    #[test]
    fn text_round_trips() {
        let doc = story();
        let (again, errors) = Doc::parse(&doc.to_text());
        assert!(errors.is_empty(), "{:#?}", errors);
        assert_eq!(doc, again);
        // the file on disk is already in the editor's canonical form, so
        // saving from the editor does not reshuffle it
        assert_eq!(doc.to_text(), STORY);
    }

    #[test]
    fn json_round_trips() {
        let doc = story();
        let json = serde_json::to_string(&doc).unwrap();
        let mut back: Doc = serde_json::from_str(&json).unwrap();
        back.sanitize();
        assert_eq!(doc, back);
    }

    #[test]
    fn unparseable_lines_are_kept_and_reported() {
        let (doc, errors) = Doc::parse("*start\nthis is not a command\nn: Hello.\n- orphan => start\n");
        assert_eq!(errors.len(), 2);
        assert_eq!(doc.scenes[0].items.len(), 3);
        assert!(doc.to_text().contains("# UNPARSED: - orphan => start"));
        assert!(doc.to_text().contains("# UNPARSED: this is not a command"));
        assert!(problems(&doc).iter().any(|p| p.msg.contains("could not be understood")));
    }

    #[test]
    fn options_survive_notes_and_bad_lines_between_them() {
        let (doc, errors) = Doc::parse("*start\nchoice\n- A => start\n# why B\n- B => start\n-C => start\n- D => start\n");
        assert_eq!(errors.len(), 1);
        match &doc.scenes[0].items[0] {
            Item::Choice { options } => assert_eq!(options.iter().map(|o| o.text.as_str()).collect::<Vec<_>>(), ["A", "B", "D"]),
            other => panic!("{:?}", other),
        }
        let (orphan, errors) = Doc::parse("*start\nn: Hi.\n- lost => start\n");
        assert_eq!(errors.len(), 1);
        assert!(orphan.to_text().contains("# UNPARSED: - lost => start"));
    }

    #[test]
    fn incomplete_blocks_are_caught_before_saving() {
        let mut doc = story();
        doc.sanitize();
        assert!(doc.survives_text().is_ok());
        // an unwritten line of dialogue is fine
        doc.scenes[0].items.push(Item::Say { who: "p".into(), text: String::new() });
        assert!(doc.survives_text().is_ok());
        for bad in [
            Item::Add { var: String::new(), n: 1 },
            Item::Goto { target: String::new() },
            Item::Show { who: String::new(), expr: "neutral".into(), slot: "left".into() },
            Item::Choice { options: vec![Opt { cond: Some(Cond { var: String::new(), op: ">=".into(), val: 1 }), text: "A".into(), target: "start".into() }] },
        ] {
            let mut broken = doc.clone();
            broken.scenes[0].items.push(bad.clone());
            assert!(broken.survives_text().is_err(), "{:?}", bad);
        }
        // a note that looks like a position marker must not move the scene
        let mut tricky = doc.clone();
        tricky.scenes[0].items.insert(0, Item::Comment { text: "@pos 1 2".into() });
        tricky.sanitize();
        assert!(tricky.survives_text().is_ok());
    }

    #[test]
    fn silent_jump_loops_are_errors_and_cannot_hang() {
        let (doc, _) = Doc::parse("*start\nn: Hi.\ngoto hub\n*hub\nshow p smile left\ngoto hub\n");
        assert!(problems(&doc).iter().any(|p| p.level == "error" && p.msg.contains("hang")));
        let s = Script::compile(&doc);
        let mut vm = Vm::new(&s);
        vm.jump(&s, "hub");
        vm.pc += 1; // past the show, into the bare loop
        let (looping, _) = Doc::parse("*start\ngoto start\n");
        let s2 = Script::compile(&looping);
        assert!(Vm::new(&s2).next(&s2).is_none());
        let _ = vm;
        // a loop through a line of dialogue is a legitimate story
        let (ok, _) = Doc::parse("*start\nn: Round again.\ngoto start\n");
        assert!(!problems(&ok).iter().any(|p| p.msg.contains("hang")));
    }

    #[test]
    fn chapters_round_trip_and_do_not_change_the_game() {
        let src = "chapter One | 10 20 | the opening\nchapter Empty\n*start\n# @chapter One\n# @pos 1 2\nn: Hi.\n*two\n# @chapter Two\nn: Bye.\nend Done | .\n*three\nn: x\n# @chapter Late\n";
        let (mut doc, errors) = Doc::parse(src);
        assert!(errors.is_empty());
        assert_eq!((doc.scenes[0].chapter.as_str(), doc.scenes[0].pos), ("One", Some((1.0, 2.0))));
        assert_eq!(doc.chapters[0], ChapterDef { name: "One".into(), pos: Some((10.0, 20.0)), note: "the opening".into() });
        // a marker anywhere else is only a note
        assert_eq!(doc.scenes[2].chapter, "");
        doc.sanitize();
        assert_eq!(doc.chapters.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), ["One", "Empty", "Two"]);
        assert!(doc.survives_text().is_ok());
        let plain = Doc::parse("*start\nn: Hi.\n*two\nn: Bye.\nend Done | .\n*three\nn: x\n").0;
        assert_eq!(Script::compile(&doc).cmds.len(), Script::compile(&plain).cmds.len() + 0);
        assert_eq!(Script::compile(&doc).labels, Script::compile(&plain).labels);
    }

    #[test]
    fn awkward_titles_and_chapter_lines_survive() {
        let (doc, errors) = Doc::parse("chapter Act One | the long night\nchapter Two | 5 6\nchapter Three | 1 2 | notes | more\n*start\nn: Hi.\nend X | y\n");
        assert!(errors.is_empty());
        assert_eq!((doc.chapters[0].note.as_str(), doc.chapters[0].pos), ("the long night", None));
        assert_eq!((doc.chapters[1].note.as_str(), doc.chapters[1].pos), ("", Some((5.0, 6.0))));
        assert_eq!(doc.chapters[2].note, "notes | more");
        let mut doc = doc;
        doc.subtitle = "What now |".into();
        doc.title = "T".into();
        doc.chapters[0].note = "ends with a pipe |".into();
        doc.header.push("@pos 3 4".into());
        doc.sanitize();
        assert!(doc.survives_text().is_ok(), "{:?}", doc.survives_text());
        assert_eq!(doc.subtitle, "What now");
    }

    #[test]
    fn problems_are_found() {
        let src = "character p | Perry | #ffffff | p\nvar peace | Detente | 0 | meter=7 good\n*start\nx: Who am I?\nshow p grin left\nadd nope 1\nbg nowhere\nchoice\n- [peace >= 1] Only this => gone\n*start\n*orphan\nn: Alone.\n";
        let (doc, errors) = Doc::parse(src);
        assert!(errors.is_empty());
        let msgs: Vec<String> = problems(&doc).into_iter().map(|p| p.msg).collect();
        for want in ["unknown speaker 'x'", "unknown expression 'grin'", "unknown property 'nope'", "unknown background 'nowhere'", "without a condition", "'gone', which does not exist", "two scenes are called 'start'", "nothing leads to this scene", "runs off the end"] {
            assert!(msgs.iter().any(|m| m.contains(want)), "missing: {} in {:#?}", want, msgs);
        }
    }

    #[test]
    fn sanitize_keeps_text_on_one_line_and_parseable() {
        let mut doc = story();
        doc.scenes[0].items.push(Item::Say { who: "n".into(), text: "two\nlines".into() });
        doc.scenes[0].items.push(Item::Choice { options: vec![Opt { cond: None, text: "[odd] a => b".into(), target: "start".into() }] });
        doc.characters[0].name = "A | B".into();
        doc.sanitize();
        let (again, errors) = Doc::parse(&doc.to_text());
        assert!(errors.is_empty(), "{:#?}", errors);
        assert_eq!(doc, again);
    }

    #[test]
    fn properties_start_at_their_declared_values() {
        let (doc, _) = Doc::parse("var gold | Gold | 5 | good\n*start\nadd gold 2\nn: Hi.\n");
        let s = Script::compile(&doc);
        let mut vm = Vm::new(&s);
        vm.jump(&s, "start");
        vm.next(&s);
        assert_eq!(vm.var("gold"), 7);
    }
}
