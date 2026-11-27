//! Presentation layer: title screen, dialogue box, choices, HUD, save/load.

use crate::art;
use crate::audio::Audio;
use crate::gfx::*;
use crate::script::{parse_color, Cmd, Script, Vm, NARRATOR};
use std::collections::HashMap;
use std::path::PathBuf;

const SAVE_FILE: &str = "below_stairs.sav";
const COLS: usize = 64;
const LINES: usize = 4;
const FADE_HALF: f32 = 0.3;
const NARRATION: u32 = rgb(214, 222, 255);

/// One frame of player input, independent of the windowing backend.
#[derive(Default, Clone)]
pub struct Input {
    pub advance: bool,
    pub up: bool,
    pub down: bool,
    pub skip: bool,
    pub save: bool,
    pub load: bool,
    pub mute: bool,
    pub back: bool,
    pub click: bool,
    pub mouse: (i32, i32),
    pub mouse_moved: bool,
}

struct Slot {
    who: String,
    expr: String,
    appear: f32,
    /// Here because their schedule has them here, not because the scene showed them.
    auto: bool,
}

enum Mode {
    Title { sel: usize },
    /// Waiting for a fade to finish before running more script.
    Run,
    Card { title: String, sub: String, t: f32 },
    Say { who: String, pages: Vec<Vec<String>>, page: usize, shown: f32 },
    Choice { opts: Vec<(Vec<String>, String)>, sel: usize },
    Ending { title: String, lines: Vec<String>, t: f32 },
}

pub struct Game {
    script: Script,
    vm: Vm,
    mode: Mode,
    bg: String,
    slots: [Option<Slot>; 3],
    fade: Option<(f32, Option<String>)>,
    time: f32,
    skip_timer: f32,
    toasts: Vec<(String, u32, f32)>,
    note: Option<(String, f32)>,
    sprites: HashMap<(String, String), Canvas>,
    /// Cached static painting of the current background: (name, canvas, animated).
    bg_cache: Option<(String, Canvas, bool)>,
    /// Directory of enhanced art that overrides the procedural paintings.
    art_dir: Option<PathBuf>,
    pub audio: Audio,
    cur_pc: usize,
    pub quit: bool,
    /// Set by a 3D presenter: where the place is a 3D world, `render` leaves
    /// out the painting and the portraits and fills with this colour
    /// instead, so that only what lies over the world is drawn.
    pub overlay: Option<u32>,
    /// How many lines of dialogue have been begun since the game started.
    pub lines_begun: usize,
    /// People the scene sent off although their schedule has them here.
    dismissed: std::collections::HashSet<String>,
    /// The place the cast was last brought into line with the schedules for.
    presence_place: String,
    /// A record of scheduled comings and goings ("09:30 q arrives in study").
    pub moves: Vec<String>,
}

/// What a 3D presenter shows at this moment of the story.
#[cfg_attr(not(feature = "world3d"), allow(dead_code))]
pub struct WorldView {
    pub place: String,
    /// Left, centre, right: who stands there, their expression, how far
    /// they have faded in (0..1) and whether someone else is speaking.
    pub cast: [Option<(String, String, f32, bool)>; 3],
}

fn wrap(text: &str, cols: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut cur = String::new();
    for word in text.split_whitespace() {
        if !cur.is_empty() && cur.chars().count() + 1 + word.chars().count() > cols {
            lines.push(std::mem::take(&mut cur));
        }
        if !cur.is_empty() {
            cur.push(' ');
        }
        cur.push_str(word);
    }
    if !cur.is_empty() {
        lines.push(cur);
    }
    lines
}

/// Vertical layout of the choice menu: (top of box, [(y, height) per option]).
fn choice_layout(opts: &[(Vec<String>, String)]) -> (i32, i32, Vec<(i32, i32)>) {
    let total: i32 = opts.iter().map(|o| o.0.len() as i32 * 24 + 22).sum::<i32>() + 24;
    let top = (H - total) / 2;
    let mut y = top + 14;
    let rows = opts
        .iter()
        .map(|o| {
            let h = o.0.len() as i32 * 24 + 22;
            y += h;
            (y - h, h)
        })
        .collect();
    (top, total, rows)
}

const TITLE_ITEMS: [&str; 3] = ["New Game", "Continue", "Quit"];

fn title_item_y(i: usize) -> i32 {
    392 + i as i32 * 44
}

impl Game {
    pub fn new(script: Script, art_dir: Option<PathBuf>, audio: Audio) -> Game {
        Game {
            vm: Vm::new(&script),
            script,
            mode: Mode::Title { sel: 0 },
            bg: String::new(),
            slots: [None, None, None],
            fade: None,
            time: 0.0,
            skip_timer: 0.0,
            toasts: Vec::new(),
            note: None,
            sprites: HashMap::new(),
            bg_cache: None,
            art_dir,
            audio,
            overlay: None,
            lines_begun: 0,
            dismissed: Default::default(),
            presence_place: String::new(),
            moves: Vec::new(),
            cur_pc: 0,
            quit: false,
        }
    }

    fn has_save() -> bool {
        std::path::Path::new(SAVE_FILE).exists()
    }

    /// Bring the cast on stage into line with the characters' schedules:
    /// whoever is due in this place walks in (to a free spot), whoever is
    /// due elsewhere walks out. People the scene itself shows are left to it.
    fn sync_presence(&mut self) {
        if !self.script.characters.iter().any(|c| !c.schedule.is_empty() || !c.interrupts.is_empty()) {
            return;
        }
        let place = self.bg.clone();
        // on arriving somewhere, whoever is there is simply there: no announcements
        let quiet = self.presence_place != place;
        self.presence_place = place.clone();
        let stamp = crate::script::time_text(self.vm.clock);
        let here: Vec<String> = self.vm.present(&self.script, &place).into_iter().filter(|who| self.script.character(who).map_or(false, |c| !c.sprite.is_empty())).map(str::to_string).collect();
        for i in 0..3 {
            let leaving = self.slots[i].as_ref().map_or(false, |s| s.auto && !here.contains(&s.who));
            if leaving {
                let who = self.slots[i].take().unwrap().who;
                let (name, col) = self.speaker(&who);
                self.moves.push(format!("{} {} leaves {}", stamp, who, place));
                if !quiet {
                    self.toasts.push((format!("{} leaves", name), col, 3.0));
                }
            }
        }
        self.dismissed.retain(|who| here.contains(who));
        for who in here {
            if self.dismissed.contains(&who) || self.slots.iter().flatten().any(|s| s.who == who) {
                continue;
            }
            // the sides first, the middle last
            let Some(free) = [0, 2, 1].into_iter().find(|i| self.slots[*i].is_none()) else { break };
            self.slots[free] = Some(Slot { who: who.clone(), expr: "neutral".into(), appear: if quiet { 1.0 } else { 0.0 }, auto: true });
            let (name, col) = self.speaker(&who);
            self.moves.push(format!("{} {} {} {}", stamp, who, if quiet { "is in" } else { "arrives in" }, place));
            if !quiet {
                self.toasts.push((format!("{} arrives", name), col, 3.0));
            }
        }
    }

    fn reset_scene(&mut self) {
        self.bg.clear();
        self.dismissed.clear();
        self.presence_place.clear();
        self.slots = [None, None, None];
        self.fade = None;
        self.toasts.clear();
        self.audio.stop_voice();
        self.audio.play_music("");
    }

    pub fn new_game(&mut self) {
        self.reset_scene();
        self.start_at("start");
    }

    /// Begin a fresh run at any scene (the editor's "play from here").
    pub fn start_at(&mut self, label: &str) {
        self.reset_scene();
        self.vm = Vm::new(&self.script);
        self.vm.jump(&self.script, label);
        self.run();
    }

    /// Display name and name-tag colour of a speaker.
    fn speaker(&self, id: &str) -> (String, u32) {
        match self.script.character(id) {
            Some(c) if id != NARRATOR => (c.name.clone(), parse_color(&c.color).unwrap_or(WHITE)),
            _ => (String::new(), NARRATION),
        }
    }

    fn to_title(&mut self) {
        self.reset_scene();
        self.mode = Mode::Title { sel: 0 };
    }

    /// What the game is currently showing; used by the headless driver.
    /// Does the story show any of its places as a 3D world?
    pub fn has_world(&self) -> bool {
        !self.script.world.is_empty()
    }

    /// The place and cast to show in 3D now, if this place is a 3D world
    /// (and the story is not on its title screen or a chapter card).
    #[cfg_attr(not(feature = "world3d"), allow(dead_code))]
    pub fn world_view(&self) -> Option<WorldView> {
        if matches!(self.mode, Mode::Title { .. } | Mode::Card { .. }) || !self.script.world.contains(&self.bg) {
            return None;
        }
        let speaking = self.speaking();
        let cast = [0, 1, 2].map(|i| self.slots[i].as_ref().map(|s| (s.who.clone(), s.expr.clone(), s.appear, speaking.as_ref().map_or(false, |w| w != &s.who))));
        Some(WorldView { place: self.bg.clone(), cast })
    }

    /// Whoever of the cast on stage is speaking.
    fn speaking(&self) -> Option<String> {
        match &self.mode {
            Mode::Say { who, .. } if self.slots.iter().flatten().any(|s| &s.who == who) => Some(who.clone()),
            _ => None,
        }
    }

    /// A character's portrait, painted (or loaded) once.
    #[cfg_attr(not(feature = "world3d"), allow(dead_code))]
    pub fn portrait(&mut self, who: &str, expr: &str) -> Option<&Canvas> {
        let def = self.script.character(who)?;
        let dir = self.art_dir.as_deref();
        Some(self.sprites.entry((who.to_string(), expr.to_string())).or_insert_with(|| art::sprite(def, expr, dir)))
    }

    pub fn kind(&self) -> &'static str {
        match self.mode {
            Mode::Title { .. } => "title",
            Mode::Run => "run",
            Mode::Card { .. } => "card",
            Mode::Say { .. } => "say",
            Mode::Choice { .. } => "choice",
            Mode::Ending { .. } => "ending",
        }
    }

    pub fn ending_title(&self) -> Option<&str> {
        match &self.mode {
            Mode::Ending { title, .. } => Some(title),
            _ => None,
        }
    }

    pub fn var(&self, name: &str) -> i32 {
        self.vm.var(name)
    }

    /// Choose option `i` of the current choice (clamped).
    pub fn pick(&mut self, i: usize) {
        if let Mode::Choice { opts, .. } = &self.mode {
            let target = opts[i.min(opts.len() - 1)].1.clone();
            self.vm.jump(&self.script, &target);
            self.run();
        }
    }

    /// Execute script commands until something needs to be shown.
    fn run(&mut self) {
        self.mode = Mode::Run;
        // bounded, so a story that only shuffles the cast in a loop cannot freeze the game
        for _ in 0..100_000 {
            let Some((i, cmd)) = self.vm.next(&self.script) else {
                break;
            };
            match cmd {
                Cmd::Bg(name) => {
                    if name != self.bg {
                        self.fade = Some((0.0, Some(name)));
                        return;
                    }
                }
                Cmd::Show { who, expr, slot } => {
                    let same = self.slots[slot].as_ref().map_or(false, |s| s.who == who);
                    self.slots[slot] = Some(Slot { who: who.clone(), expr, appear: if same { 1.0 } else { 0.0 }, auto: false });
                    // one of each person: if their schedule had already brought them in elsewhere, they step over
                    for other in 0..3 {
                        if other != slot && self.slots[other].as_ref().map_or(false, |s| s.who == who) {
                            self.slots[other] = None;
                        }
                    }
                    self.dismissed.remove(&who);
                }
                Cmd::Hide(which) => {
                    // someone the scene sends off stays off, whatever their schedule says, until the place changes
                    for i in (0..3).filter(|i| which.map_or(true, |w| w == *i)) {
                        if let Some(gone) = self.slots[i].take() {
                            self.dismissed.insert(gone.who);
                        }
                    }
                }
                Cmd::Music(track) => self.audio.play_music(&track),
                Cmd::Say { who, text } => {
                    self.sync_presence();
                    self.cur_pc = i;
                    self.lines_begun += 1;
                    self.audio.say(&who, &text);
                    let lines = wrap(&text, COLS);
                    let pages = lines.chunks(LINES).map(|c| c.to_vec()).collect();
                    self.mode = Mode::Say { who, pages, page: 0, shown: 0.0 };
                    return;
                }
                Cmd::Choice(opts) => {
                    self.sync_presence();
                    self.cur_pc = i;
                    let opts = opts
                        .iter()
                        .filter(|o| o.cond.as_ref().map_or(true, |c| self.vm.test(&self.script, c)))
                        .map(|o| (wrap(&o.text, 54), o.target.clone()))
                        .collect();
                    self.mode = Mode::Choice { opts, sel: 0 };
                    return;
                }
                Cmd::Add(var, n) => {
                    // announce the consequence, unless the property is a hidden one
                    let Some(def) = self.script.var_def(&var).filter(|d| !d.hidden && !d.label.is_empty()) else { continue };
                    let col = if (n > 0) == def.good { rgb(150, 240, 160) } else { rgb(255, 130, 120) };
                    self.toasts.push((format!("{} {}{}", def.label, if n > 0 { "+" } else { "" }, n), col, 3.0));
                }
                Cmd::Card(title, sub) => {
                    self.reset_scene();
                    self.mode = Mode::Card { title, sub, t: 0.0 };
                    return;
                }
                Cmd::End(title, text) => {
                    self.toasts.clear();
                    self.mode = Mode::Ending { title, lines: wrap(&text, COLS), t: 0.0 };
                    return;
                }
                _ => {}
            }
        }
        self.to_title();
    }

    fn save(&mut self) {
        if !matches!(self.mode, Mode::Say { .. } | Mode::Choice { .. }) {
            return;
        }
        let mut s = format!("pc={}\nbg={}\nmusic={}\n", self.cur_pc, self.bg, self.audio.current_music());
        for (i, slot) in self.slots.iter().enumerate() {
            if let Some(sl) = slot {
                s += &format!("slot{}={},{}{}\n", i, sl.who, sl.expr, if sl.auto { ",auto" } else { "" });
            }
        }
        for (k, v) in &self.vm.vars {
            s += &format!("var.{}={}\n", k, v);
        }
        s += &format!("clock={}\n", self.vm.clock);
        for e in &self.vm.events {
            s += &format!("event={}\n", e);
        }
        let msg = match std::fs::write(SAVE_FILE, s) {
            Ok(()) => "Game saved".to_string(),
            Err(e) => format!("Save failed: {}", e),
        };
        self.note = Some((msg, 2.5));
    }

    fn load(&mut self) {
        let Ok(text) = std::fs::read_to_string(SAVE_FILE) else {
            self.note = Some(("No saved game".into(), 2.5));
            return;
        };
        let mut vm = Vm::new(&self.script);
        let mut bg = String::new();
        let mut music = String::new();
        let mut slots: [Option<Slot>; 3] = [None, None, None];
        let mut pc = None;
        for line in text.lines() {
            let Some((k, v)) = line.split_once('=') else { continue };
            if k == "pc" {
                pc = v.parse::<usize>().ok();
            } else if k == "bg" {
                bg = v.to_string();
            } else if k == "music" {
                music = v.to_string();
            } else if k == "clock" {
                vm.clock = v.parse().unwrap_or(vm.clock);
            } else if k == "event" {
                vm.events.insert(v.to_string());
            } else if let Some(name) = k.strip_prefix("var.") {
                vm.vars.insert(name.to_string(), v.parse().unwrap_or(0));
            } else if let Some(i) = k.strip_prefix("slot").and_then(|i| i.parse::<usize>().ok()) {
                if let (Some((who, expr)), true) = (v.split_once(','), i < 3) {
                    let (expr, auto) = expr.split_once(',').map_or((expr, false), |(e, a)| (e, a == "auto"));
                    slots[i] = Some(Slot { who: who.into(), expr: expr.into(), appear: 1.0, auto });
                }
            }
        }
        match pc {
            Some(pc) if pc < self.script.cmds.len() => {
                vm.pc = pc;
                vm.place = bg.clone();
                self.reset_scene();
                self.vm = vm;
                self.presence_place = bg.clone();
                self.bg = bg;
                self.slots = slots;
                self.audio.play_music(&music);
                self.run();
                self.note = Some(("Game loaded".into(), 2.5));
            }
            _ => self.note = Some(("Save file is damaged".into(), 2.5)),
        }
    }

    pub fn update(&mut self, inp: &Input, dt: f32) {
        self.time += dt;
        for t in self.toasts.iter_mut() {
            t.2 -= dt;
        }
        self.toasts.retain(|t| t.2 > 0.0);
        if let Some(n) = &mut self.note {
            n.1 -= dt;
            if n.1 <= 0.0 {
                self.note = None;
            }
        }
        for s in self.slots.iter_mut().flatten() {
            s.appear = (s.appear + dt * 4.0).min(1.0);
        }

        if inp.mute && self.audio.available() {
            self.audio.toggle_mute();
            self.note = Some((if self.audio.muted { "Sound off".into() } else { "Sound on".into() }, 1.5));
        }
        if inp.load {
            self.load();
            return;
        }
        if inp.save {
            self.save();
        }
        if inp.back {
            if matches!(self.mode, Mode::Title { .. }) {
                self.quit = true;
            } else {
                self.to_title();
            }
            return;
        }

        if let Some((mut t, mut pending)) = self.fade.take() {
            t += dt * if inp.skip { 3.0 } else { 1.0 };
            if t >= FADE_HALF {
                if let Some(b) = pending.take() {
                    self.bg = b;
                    self.slots = [None, None, None];
                    self.dismissed.clear();
                }
            }
            if t >= FADE_HALF * 2.0 {
                self.run();
            } else {
                self.fade = Some((t, pending));
            }
            return;
        }

        // holding the skip key advances rapidly
        let mut go = inp.advance || inp.click;
        if inp.skip {
            self.skip_timer += dt;
            if self.skip_timer > 0.07 {
                self.skip_timer = 0.0;
                go = true;
            }
        }

        match &mut self.mode {
            Mode::Title { sel } => {
                let avail = |i: usize| i != 1 || Game::has_save();
                let n = TITLE_ITEMS.len();
                if inp.down {
                    *sel = (*sel + 1) % n;
                    if !avail(*sel) {
                        *sel = (*sel + 1) % n;
                    }
                }
                if inp.up {
                    *sel = (*sel + n - 1) % n;
                    if !avail(*sel) {
                        *sel = (*sel + n - 1) % n;
                    }
                }
                let mut hit = None;
                for i in 0..n {
                    let y = title_item_y(i);
                    if (inp.mouse.1 >= y - 10 && inp.mouse.1 < y + 34) && (inp.mouse.0 - W / 2).abs() < 170 && avail(i) {
                        hit = Some(i);
                    }
                }
                if let (Some(i), true) = (hit, inp.mouse_moved || inp.click) {
                    *sel = i;
                }
                if inp.advance || (inp.click && hit.is_some()) {
                    match *sel {
                        0 => self.new_game(),
                        1 => self.load(),
                        _ => self.quit = true,
                    }
                }
            }
            Mode::Run => {}
            Mode::Card { t, .. } => {
                *t += dt;
                if go && *t > 0.3 {
                    self.run();
                }
            }
            Mode::Say { pages, page, shown, .. } => {
                let total: usize = pages[*page].iter().map(|l| l.chars().count()).sum();
                *shown += dt * if inp.skip { 2000.0 } else { 48.0 };
                if go {
                    if (*shown as usize) < total {
                        *shown = total as f32;
                    } else if *page + 1 < pages.len() {
                        *page += 1;
                        *shown = 0.0;
                    } else {
                        self.run();
                    }
                }
            }
            Mode::Choice { opts, sel } => {
                let n = opts.len();
                if inp.down {
                    *sel = (*sel + 1) % n;
                }
                if inp.up {
                    *sel = (*sel + n - 1) % n;
                }
                let (_, _, rows) = choice_layout(opts);
                let hit = rows
                    .iter()
                    .position(|(y, h)| inp.mouse.1 >= *y && inp.mouse.1 < y + h && inp.mouse.0 >= 96 && inp.mouse.0 < W - 96);
                if let (Some(i), true) = (hit, inp.mouse_moved || inp.click) {
                    *sel = i;
                }
                // choices are never auto-skipped
                if inp.advance || (inp.click && hit.is_some()) {
                    let i = *sel;
                    self.pick(i);
                }
            }
            Mode::Ending { t, .. } => {
                *t += dt;
                if (inp.advance || inp.click) && *t > 1.0 {
                    self.to_title();
                }
            }
        }
    }

    /// Draw the current background (cached static painting plus overlay).
    fn draw_bg(&mut self, c: &mut Canvas, name: &str) {
        if name.is_empty() {
            c.clear(BLACK);
            return;
        }
        if self.bg_cache.as_ref().map_or(true, |b| b.0 != name) {
            let (img, animated) = art::background(name, self.art_dir.as_deref());
            self.bg_cache = Some((name.to_string(), img, animated));
        }
        let (_, img, animated) = self.bg_cache.as_ref().unwrap();
        c.px.copy_from_slice(&img.px);
        if *animated {
            art::animate(c, name, self.time);
        }
    }

    pub fn render(&mut self, c: &mut Canvas) {
        match &self.mode {
            Mode::Title { sel } => {
                let sel = *sel;
                self.draw_bg(c, "vista_night");
                c.rect_blend(0, 0, W, 250, rgba(8, 8, 28, 120));
                // the story's own title, wrapped and sized to fit
                let title = if self.script.title.is_empty() { "Untitled Story".to_string() } else { self.script.title.to_uppercase() };
                let mut lines = wrap(&title, 14);
                let scale = if lines.len() <= 2 && lines.iter().all(|l| l.chars().count() <= 22) { 6 } else { 4 };
                if scale == 4 {
                    lines = wrap(&title, 30);
                    lines.truncate(3);
                }
                let mut y = if lines.len() == 1 { 86 } else { 54 };
                for line in &lines {
                    c.text_c(W / 2, y, line, GOLD, scale);
                    y += scale * 8 + 16;
                }
                for (i, line) in wrap(&self.script.subtitle, 66).iter().take(2).enumerate() {
                    c.text_c(W / 2, y.max(196) + i as i32 * 22, line, rgb(200, 210, 255), 2);
                }
                c.panel(W / 2 - 170, 372, 340, 150);
                for (i, item) in TITLE_ITEMS.iter().enumerate() {
                    let enabled = i != 1 || Game::has_save();
                    let col = if !enabled {
                        rgb(90, 96, 140)
                    } else if i == sel {
                        GOLD
                    } else {
                        WHITE
                    };
                    c.text_c(W / 2, title_item_y(i), item, col, 3);
                    if i == sel {
                        c.text_sh(W / 2 - 150, title_item_y(i), ">", GOLD, 3);
                    }
                }
                c.rect_blend(0, 556, W, 92, rgba(8, 8, 28, 150));
                c.text_c(W / 2, 572, "Enter / Space / Click: advance    Arrows: choose", rgb(200, 210, 255), 2);
                c.text_c(W / 2, 604, "Ctrl: skip   F5: save   F9: load   M: sound   Esc: title", rgb(200, 210, 255), 2);
            }
            Mode::Card { title, sub, t } => {
                c.clear(rgb(8, 8, 24));
                for i in 0..260 {
                    let (x, y) = ((hash(i, 0, 12) % W as u32) as i32, (hash(i, 1, 12) % H as u32) as i32);
                    c.cover(x, y, rgb(150, 160, 220), 0.3 + hashf(i, 2, 12) * 0.7);
                }
                c.text_c(W / 2, 248, title, GOLD, 4);
                c.rect(W / 2 - 240, 300, 480, 2, rgb(112, 128, 208));
                c.text_c(W / 2, 322, sub, rgb(214, 222, 255), 2);
                c.fade(256u32.saturating_sub((*t * 640.0) as u32));
            }
            _ => self.render_scene(c),
        }
        if let Some((msg, _)) = &self.note {
            let w = msg.len() as i32 * 16 + 36;
            c.panel(W / 2 - w / 2, 12, w, 40);
            c.text_sh(W / 2 - w / 2 + 18, 24, msg, WHITE, 2);
        }
        if let Some((t, _)) = &self.fade {
            let k = 1.0 - (t - FADE_HALF).abs() / FADE_HALF;
            c.fade((k.clamp(0.0, 1.0) * 256.0) as u32);
        }
    }

    fn render_scene(&mut self, c: &mut Canvas) {
        let bg = self.bg.clone();
        let world = self.overlay.filter(|_| self.script.world.contains(&bg));
        match world {
            Some(backdrop) => c.clear(backdrop),
            None => self.draw_bg(c, &bg),
        }

        // cast; whoever is not speaking is dimmed (in a 3D world they stand in it instead)
        let speaking = self.speaking();
        for (i, slot) in self.slots.iter().enumerate().filter(|_| world.is_none()) {
            let Some(s) = slot else { continue };
            let key = (s.who.clone(), s.expr.clone());
            let dir = self.art_dir.as_deref();
            let Some(def) = self.script.character(&s.who) else { continue };
            let spr = self.sprites.entry(key).or_insert_with(|| art::sprite(def, &s.expr, dir));
            let x = 72 + i as i32 * art::SPR_W;
            let mut y = 30 + ((1.0 - s.appear) * 18.0) as i32;
            if def.sprite == "a" || self.bg == "axis" {
                // the Admiral bobs; at the axis, so does everyone
                y += ((self.time * 1.6 + i as f32).sin() * 7.0) as i32;
            }
            let dim = speaking.as_ref().map_or(false, |w| w != &s.who);
            c.blit(spr, x, y, dim, (s.appear * 256.0) as u32);
        }

        match &self.mode {
            Mode::Say { who, pages, page, shown } => {
                let (name, col) = self.speaker(who);
                c.panel(24, 456, W - 48, 174);
                if !name.is_empty() {
                    let w = name.len() as i32 * 16 + 36;
                    c.panel(44, 424, w, 40);
                    c.text_sh(62, 436, &name, col, 2);
                }
                let text_col = if who == NARRATOR { col } else { WHITE };
                let mut left = *shown as usize;
                let mut done = true;
                for (i, line) in pages[*page].iter().enumerate() {
                    let n = line.chars().count();
                    let part: String = line.chars().take(left).collect();
                    c.text_sh(64, 484 + i as i32 * 34, &part, text_col, 2);
                    if left < n {
                        done = false;
                        break;
                    }
                    left -= n;
                }
                if done && (self.time * 3.0) as i32 % 2 == 0 {
                    c.tri(W - 76, 602, W - 52, 602, W - 64, 614, GOLD);
                }
                self.render_hud(c);
            }
            Mode::Choice { opts, sel } => {
                c.fade(96);
                let (top, total, rows) = choice_layout(opts);
                c.panel(96, top, W - 192, total);
                for (i, ((lines, _), (y, h))) in opts.iter().zip(rows.iter()).enumerate() {
                    let on = i == *sel;
                    if on {
                        c.rect_blend(104, *y, W - 208, *h - 4, rgba(70, 92, 200, 150));
                        c.text_sh(114, y + 10, ">", GOLD, 2);
                    }
                    for (j, line) in lines.iter().enumerate() {
                        c.text_sh(144, y + 10 + j as i32 * 24, line, if on { GOLD } else { rgb(214, 222, 255) }, 2);
                    }
                }
                self.render_hud(c);
            }
            Mode::Ending { title, lines, t } => {
                c.fade(200);
                let h = lines.len() as i32 * 30;
                let top = (H - h - 190) / 2;
                c.text_c(W / 2, top, "- AN ENDING -", rgb(150, 160, 220), 2);
                c.text_c(W / 2, top + 40, title, GOLD, 4);
                c.rect(W / 2 - 300, top + 92, 600, 2, rgb(112, 128, 208));
                for (i, line) in lines.iter().enumerate() {
                    c.text_c(W / 2, top + 116 + i as i32 * 30, line, WHITE, 2);
                }
                if *t > 1.0 && (self.time * 2.0) as i32 % 2 == 0 {
                    c.text_c(W / 2, top + 140 + h, "Press Enter", rgb(150, 160, 220), 2);
                }
            }
            _ => {}
        }
    }

    fn render_hud(&self, c: &mut Canvas) {
        // one meter per property that declares one
        let meters: Vec<_> = self.script.vars.iter().filter(|v| v.meter > 0).take(4).collect();
        if !meters.is_empty() {
            let pips = meters.iter().map(|v| v.meter.min(12)).max().unwrap_or(0);
            let widest = meters.iter().map(|v| v.label.chars().count() as i32).max().unwrap_or(0);
            let px = W - 16 - pips * 20;
            let left = px - 12 - widest * 16;
            c.rect_blend(left - 12, 8, W - left + 4, 8 + meters.len() as i32 * 26, rgba(8, 8, 28, 130));
            for (i, v) in meters.iter().enumerate() {
                let y = 16 + i as i32 * 26;
                let label = v.label.to_uppercase();
                let col = if v.good { rgb(120, 230, 150) } else { rgb(255, 110, 100) };
                c.text_sh(px - 12 - label.chars().count() as i32 * 16, y, &label, WHITE, 2);
                for k in 0..v.meter.min(12) {
                    c.rect(px + k * 20, y, 17, 16, OUTLINE);
                    c.rect(px + k * 20 + 2, y + 2, 13, 12, if k < self.vm.var(&v.id) { col } else { rgb(60, 64, 100) });
                }
            }
        }
        // the time, for a story that keeps a clock
        if self.script.clock.is_some() {
            let day = self.vm.clock.div_euclid(crate::script::DAY) + 1;
            let label = format!("DAY {}  {}", day, crate::script::time_text(self.vm.clock));
            let w = label.len() as i32 * 16 + 24;
            c.rect_blend(W / 2 - w / 2, 8, w, 32, rgba(8, 8, 28, 130));
            c.text_c(W / 2, 16, &label, rgb(214, 222, 255), 2);
        }
        for (i, (msg, col, _)) in self.toasts.iter().rev().take(4).enumerate() {
            c.text_sh(20, 16 + i as i32 * 26, msg, *col, 2);
        }
    }
}
