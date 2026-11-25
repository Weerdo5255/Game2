//! Presentation layer: title screen, dialogue box, choices, HUD, save/load.

use crate::art;
use crate::gfx::*;
use crate::script::{Cmd, Script, Vm};
use std::collections::HashMap;

pub const SPEAKERS: &[&str] = &["n", "m", "p", "v", "s", "q", "t", "c", "a"];
const SAVE_FILE: &str = "below_stairs.sav";
const COLS: usize = 44;
const LINES: usize = 4;
const FADE_HALF: f32 = 0.3;

fn speaker(id: &str) -> (&'static str, u32) {
    match id {
        "m" => ("Marlowe", rgb(150, 230, 160)),
        "p" => ("Peregrine", rgb(200, 180, 255)),
        "v" => ("Lord Varrow", rgb(255, 130, 130)),
        "s" => ("Lord Solberg", rgb(110, 230, 220)),
        "q" => ("Quince", rgb(170, 200, 230)),
        "t" => ("Tobe", rgb(255, 170, 110)),
        "c" => ("Cook", rgb(255, 220, 170)),
        "a" => ("The Admiral", WHITE),
        _ => ("", rgb(214, 222, 255)),
    }
}

/// One frame of player input, independent of the windowing backend.
#[derive(Default, Clone)]
pub struct Input {
    pub advance: bool,
    pub up: bool,
    pub down: bool,
    pub skip: bool,
    pub save: bool,
    pub load: bool,
    pub back: bool,
    pub click: bool,
    pub mouse: (i32, i32),
    pub mouse_moved: bool,
}

struct Slot {
    who: String,
    expr: String,
    appear: f32,
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
    cur_pc: usize,
    pub quit: bool,
}

fn wrap(text: &str, cols: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut cur = String::new();
    for word in text.split_whitespace() {
        if !cur.is_empty() && cur.len() + 1 + word.len() > cols {
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
    let total: i32 = opts.iter().map(|o| o.0.len() as i32 * 10 + 8).sum::<i32>() + 8;
    let top = (H - total) / 2;
    let mut y = top + 6;
    let rows = opts
        .iter()
        .map(|o| {
            let h = o.0.len() as i32 * 10 + 8;
            y += h;
            (y - h, h)
        })
        .collect();
    (top, total, rows)
}

const TITLE_ITEMS: [&str; 3] = ["New Game", "Continue", "Quit"];

fn title_item_y(i: usize) -> i32 {
    132 + i as i32 * 15
}

impl Game {
    pub fn new(script: Script) -> Game {
        Game {
            script,
            vm: Vm::default(),
            mode: Mode::Title { sel: 0 },
            bg: String::new(),
            slots: [None, None, None],
            fade: None,
            time: 0.0,
            skip_timer: 0.0,
            toasts: Vec::new(),
            note: None,
            sprites: HashMap::new(),
            cur_pc: 0,
            quit: false,
        }
    }

    fn has_save() -> bool {
        std::path::Path::new(SAVE_FILE).exists()
    }

    fn reset_scene(&mut self) {
        self.bg.clear();
        self.slots = [None, None, None];
        self.fade = None;
        self.toasts.clear();
    }

    pub fn new_game(&mut self) {
        self.reset_scene();
        self.vm = Vm::default();
        self.vm.jump(&self.script, "start");
        self.run();
    }

    fn to_title(&mut self) {
        self.reset_scene();
        self.mode = Mode::Title { sel: 0 };
    }

    /// What the game is currently showing; used by the headless driver.
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
        loop {
            let Some((i, cmd)) = self.vm.next(&self.script) else {
                self.to_title();
                return;
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
                    self.slots[slot] = Some(Slot { who, expr, appear: if same { 1.0 } else { 0.0 } });
                }
                Cmd::Hide(None) => self.slots = [None, None, None],
                Cmd::Hide(Some(s)) => self.slots[s] = None,
                Cmd::Say { who, text } => {
                    self.cur_pc = i;
                    let lines = wrap(&text, COLS);
                    let pages = lines.chunks(LINES).map(|c| c.to_vec()).collect();
                    self.mode = Mode::Say { who, pages, page: 0, shown: 0.0 };
                    return;
                }
                Cmd::Choice(opts) => {
                    self.cur_pc = i;
                    let opts = opts
                        .iter()
                        .filter(|o| o.cond.as_ref().map_or(true, |c| self.vm.test(c)))
                        .map(|o| (wrap(&o.text, 41), o.target.clone()))
                        .collect();
                    self.mode = Mode::Choice { opts, sel: 0 };
                    return;
                }
                Cmd::Add(var, n) => {
                    let label = match var.as_str() {
                        "peace" => "Detente",
                        "heat" => "Suspicion",
                        "favor" => "Peregrine's regard",
                        "trust" => "Quince's trust",
                        _ => continue,
                    };
                    // rising suspicion is bad news; everything else rising is good
                    let good = (n > 0) != (var == "heat");
                    let col = if good { rgb(150, 240, 160) } else { rgb(255, 130, 120) };
                    self.toasts.push((format!("{} {}{}", label, if n > 0 { "+" } else { "" }, n), col, 3.0));
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
    }

    fn save(&mut self) {
        if !matches!(self.mode, Mode::Say { .. } | Mode::Choice { .. }) {
            return;
        }
        let mut s = format!("pc={}\nbg={}\n", self.cur_pc, self.bg);
        for (i, slot) in self.slots.iter().enumerate() {
            if let Some(sl) = slot {
                s += &format!("slot{}={},{}\n", i, sl.who, sl.expr);
            }
        }
        for (k, v) in &self.vm.vars {
            s += &format!("var.{}={}\n", k, v);
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
        let mut vm = Vm::default();
        let mut bg = String::new();
        let mut slots: [Option<Slot>; 3] = [None, None, None];
        let mut pc = None;
        for line in text.lines() {
            let Some((k, v)) = line.split_once('=') else { continue };
            if k == "pc" {
                pc = v.parse::<usize>().ok();
            } else if k == "bg" {
                bg = v.to_string();
            } else if let Some(name) = k.strip_prefix("var.") {
                vm.vars.insert(name.to_string(), v.parse().unwrap_or(0));
            } else if let Some(i) = k.strip_prefix("slot").and_then(|i| i.parse::<usize>().ok()) {
                if let (Some((who, expr)), true) = (v.split_once(','), i < 3) {
                    slots[i] = Some(Slot { who: who.into(), expr: expr.into(), appear: 1.0 });
                }
            }
        }
        match pc {
            Some(pc) if pc < self.script.cmds.len() => {
                vm.pc = pc;
                self.reset_scene();
                self.vm = vm;
                self.bg = bg;
                self.slots = slots;
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
                    if (inp.mouse.1 >= y - 3 && inp.mouse.1 < y + 11) && (inp.mouse.0 - W / 2).abs() < 60 && avail(i) {
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
                    .position(|(y, h)| inp.mouse.1 >= *y && inp.mouse.1 < y + h && inp.mouse.0 >= 16 && inp.mouse.0 < W - 16);
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

    pub fn render(&mut self, c: &mut Canvas) {
        match &self.mode {
            Mode::Title { sel } => {
                art::draw_bg(c, "vista_night", self.time);
                c.text_c(W / 2, 22, "BELOW STAIRS,", GOLD, 2);
                c.text_c(W / 2, 44, "ABOVE STARS", GOLD, 2);
                c.text_c(W / 2, 68, "A tale of Halcyon Reach", rgb(200, 210, 255), 1);
                c.panel(W / 2 - 64, 124, 128, 52);
                for (i, item) in TITLE_ITEMS.iter().enumerate() {
                    let enabled = i != 1 || Game::has_save();
                    let col = if !enabled {
                        rgb(90, 96, 140)
                    } else if i == *sel {
                        GOLD
                    } else {
                        WHITE
                    };
                    c.text_c(W / 2, title_item_y(i), item, col, 1);
                    if i == *sel {
                        c.text_sh(W / 2 - 56, title_item_y(i), ">", GOLD, 1);
                    }
                }
                c.text_c(W / 2, 186, "Enter/Space/Click: advance   Arrows: choose", rgb(200, 210, 255), 1);
                c.text_c(W / 2, 198, "Ctrl: skip  F5: save  F9: load  Esc: title", rgb(200, 210, 255), 1);
            }
            Mode::Card { title, sub, t } => {
                c.clear(rgb(8, 8, 24));
                for x in 0..W {
                    if hash(x, 0, 12) % 9 == 0 {
                        let y = (hash(x, 1, 12) % H as u32) as i32;
                        c.put(x, y, rgb(120, 130, 190));
                    }
                }
                c.text_c(W / 2, 84, title, GOLD, 2);
                c.hline(W / 2 - 80, 106, 160, rgb(112, 128, 208));
                c.text_c(W / 2, 114, sub, rgb(214, 222, 255), 1);
                c.fade(256u32.saturating_sub((*t * 640.0) as u32));
            }
            _ => self.render_scene(c),
        }
        if let Some((msg, _)) = &self.note {
            let w = msg.len() as i32 * 8 + 12;
            c.panel(W / 2 - w / 2, 4, w, 16);
            c.text(W / 2 - w / 2 + 6, 8, msg, WHITE);
        }
        if let Some((t, _)) = &self.fade {
            let k = 1.0 - (t - FADE_HALF).abs() / FADE_HALF;
            c.fade((k.clamp(0.0, 1.0) * 256.0) as u32);
        }
        c.quantize();
    }

    fn render_scene(&mut self, c: &mut Canvas) {
        art::draw_bg(c, &self.bg, self.time);

        // cast; whoever is not speaking is dimmed
        let speaking = match &self.mode {
            Mode::Say { who, .. } if self.slots.iter().flatten().any(|s| &s.who == who) => Some(who.clone()),
            _ => None,
        };
        for (i, slot) in self.slots.iter().enumerate() {
            let Some(s) = slot else { continue };
            let key = (s.who.clone(), s.expr.clone());
            let spr = self.sprites.entry(key).or_insert_with(|| art::sprite(&s.who, &s.expr));
            let x = 24 + i as i32 * 112;
            let mut y = 10 + ((1.0 - s.appear) * 6.0) as i32;
            if s.who == "a" || self.bg == "axis" {
                // the Admiral bobs; at the axis, so does everyone
                y += ((self.time * 1.6 + i as f32).sin() * 2.5) as i32;
            }
            let dim = speaking.as_ref().map_or(false, |w| w != &s.who);
            c.blit(spr, x, y, dim, (s.appear * 16.0) as u32);
        }

        match &self.mode {
            Mode::Say { who, pages, page, shown } => {
                let (name, col) = speaker(who);
                c.panel(8, 152, W - 16, 58);
                if !name.is_empty() {
                    let w = name.len() as i32 * 8 + 12;
                    c.panel(14, 141, w, 15);
                    c.text(20, 145, name, col);
                }
                let text_col = if who == "n" { col } else { WHITE };
                let mut left = *shown as usize;
                let mut done = true;
                for (i, line) in pages[*page].iter().enumerate() {
                    let n = line.chars().count();
                    let part: String = line.chars().take(left).collect();
                    c.text_sh(16, 161 + i as i32 * 11, &part, text_col, 1);
                    if left < n {
                        done = false;
                        break;
                    }
                    left -= n;
                }
                if done && (self.time * 3.0) as i32 % 2 == 0 {
                    c.tri(W - 24, 201, W - 16, 201, W - 20, 205, GOLD);
                }
                self.render_hud(c);
            }
            Mode::Choice { opts, sel } => {
                c.fade(96);
                let (top, total, rows) = choice_layout(opts);
                c.panel(16, top, W - 32, total);
                for (i, ((lines, _), (y, h))) in opts.iter().zip(rows.iter()).enumerate() {
                    let on = i == *sel;
                    if on {
                        c.rect(19, *y, W - 38, *h - 2, rgb(48, 64, 150));
                        c.text(24, y + 4, ">", GOLD);
                    }
                    for (j, line) in lines.iter().enumerate() {
                        c.text_sh(36, y + 4 + j as i32 * 10, line, if on { GOLD } else { rgb(214, 222, 255) }, 1);
                    }
                }
                self.render_hud(c);
            }
            Mode::Ending { title, lines, t } => {
                c.fade(192);
                let h = lines.len() as i32 * 11;
                let top = (H - h - 70) / 2;
                c.text_c(W / 2, top, "- AN ENDING -", rgb(150, 160, 220), 1);
                c.text_c(W / 2, top + 16, title, GOLD, 2);
                c.hline(W / 2 - 100, top + 38, 200, rgb(112, 128, 208));
                for (i, line) in lines.iter().enumerate() {
                    c.text_sh(16, top + 46 + i as i32 * 11, line, WHITE, 1);
                }
                if *t > 1.0 && (self.time * 2.0) as i32 % 2 == 0 {
                    c.text_c(W / 2, top + 54 + h, "Press Enter", rgb(150, 160, 220), 1);
                }
            }
            _ => {}
        }
    }

    fn render_hud(&self, c: &mut Canvas) {
        let rows = [
            ("DETENTE", self.vm.var("peace"), 7, rgb(120, 230, 150)),
            ("SUSPICION", self.vm.var("heat"), 4, rgb(255, 110, 100)),
        ];
        for (i, (label, val, max, col)) in rows.iter().enumerate() {
            let y = 5 + i as i32 * 11;
            let px = W - 8 - 49;
            c.text_sh(px - 4 - label.len() as i32 * 8, y, label, WHITE, 1);
            for k in 0..*max {
                c.rect(px + k * 7, y, 6, 7, OUTLINE);
                c.rect(px + k * 7 + 1, y + 1, 4, 5, if k < *val { *col } else { rgb(60, 64, 100) });
            }
        }
        for (i, (msg, col, _)) in self.toasts.iter().rev().take(4).enumerate() {
            c.text_sh(8, 5 + i as i32 * 11, msg, *col, 1);
        }
    }
}
