//! The story script: a small line-based language (see assets/story.txt for
//! the syntax) plus the virtual machine that walks it.

use std::collections::HashMap;

#[derive(Clone, Debug)]
pub struct Cond {
    pub var: String,
    pub op: String,
    pub val: i32,
}

#[derive(Clone, Debug)]
pub struct Opt {
    pub cond: Option<Cond>,
    pub text: String,
    pub target: String,
}

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
    pub cmds: Vec<Cmd>,
    pub labels: HashMap<String, usize>,
}

fn slot_index(s: &str) -> Option<usize> {
    match s {
        "left" => Some(0),
        "center" => Some(1),
        "right" => Some(2),
        _ => None,
    }
}

fn parse_cond(toks: &[&str]) -> Result<Cond, String> {
    if toks.len() != 3 || !["==", "!=", ">=", "<=", ">", "<"].contains(&toks[1]) {
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

impl Script {
    pub fn parse(src: &str) -> Result<Script, String> {
        let mut cmds: Vec<Cmd> = Vec::new();
        let mut labels = HashMap::new();
        for (n, raw) in src.lines().enumerate() {
            let line = raw.trim();
            let err = |m: String| format!("story line {}: {}", n + 1, m);
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some(name) = line.strip_prefix('*') {
                if labels.insert(name.trim().to_string(), cmds.len()).is_some() {
                    return Err(err(format!("duplicate label '{}'", name)));
                }
                continue;
            }
            if let Some(opt) = line.strip_prefix("- ") {
                let (mut text, target) = opt
                    .rsplit_once("=>")
                    .map(|(a, b)| (a.trim(), b.trim().to_string()))
                    .ok_or_else(|| err("choice option needs '=> label'".into()))?;
                let mut cond = None;
                if let Some(rest) = text.strip_prefix('[') {
                    let (c, t) = rest.split_once(']').ok_or_else(|| err("missing ']'".into()))?;
                    cond = Some(parse_cond(&c.split_whitespace().collect::<Vec<_>>()).map_err(err)?);
                    text = t.trim();
                }
                match cmds.last_mut() {
                    Some(Cmd::Choice(opts)) => opts.push(Opt { cond, text: text.into(), target }),
                    _ => return Err(err("option outside of a 'choice' block".into())),
                }
                continue;
            }
            let toks: Vec<&str> = line.split_whitespace().collect();
            let rest = line[toks[0].len()..].trim();
            let cmd = match toks[0] {
                "bg" if toks.len() == 2 => Cmd::Bg(toks[1].into()),
                "show" if toks.len() == 4 => Cmd::Show {
                    who: toks[1].into(),
                    expr: toks[2].into(),
                    slot: slot_index(toks[3]).ok_or_else(|| err(format!("bad slot '{}'", toks[3])))?,
                },
                "hide" if toks.len() == 2 => match toks[1] {
                    "all" => Cmd::Hide(None),
                    s => Cmd::Hide(Some(slot_index(s).ok_or_else(|| err(format!("bad slot '{}'", s)))?)),
                },
                "add" | "set" if toks.len() == 3 => {
                    let v = toks[2].parse().map_err(|_| err(format!("bad number '{}'", toks[2])))?;
                    if toks[0] == "add" {
                        Cmd::Add(toks[1].into(), v)
                    } else {
                        Cmd::Set(toks[1].into(), v)
                    }
                }
                "if" if toks.len() == 6 && toks[4] == "goto" => {
                    Cmd::If(parse_cond(&toks[1..4]).map_err(err)?, toks[5].into())
                }
                "goto" if toks.len() == 2 => Cmd::Goto(toks[1].into()),
                "choice" if toks.len() == 1 => Cmd::Choice(Vec::new()),
                "card" => {
                    let (a, b) = split_pair(rest);
                    Cmd::Card(a, b)
                }
                "end" => {
                    let (a, b) = split_pair(rest);
                    Cmd::End(a, b)
                }
                _ => match line.split_once(": ") {
                    Some((who, text)) if !who.contains(' ') => {
                        Cmd::Say { who: who.into(), text: text.trim().into() }
                    }
                    _ => return Err(err(format!("cannot parse '{}'", line))),
                },
            };
            cmds.push(cmd);
        }
        Ok(Script { cmds, labels })
    }

    /// Check every reference in the script. Returns a list of problems.
    pub fn validate(&self, speakers: &[&str], sprites: &[&str], exprs: &[&str], bgs: &[&str]) -> Vec<String> {
        let mut errs = Vec::new();
        let label = |l: &String, errs: &mut Vec<String>| {
            if !self.labels.contains_key(l) {
                errs.push(format!("unknown label '{}'", l));
            }
        };
        for cmd in &self.cmds {
            match cmd {
                Cmd::Bg(b) if !bgs.contains(&b.as_str()) => errs.push(format!("unknown background '{}'", b)),
                Cmd::Show { who, expr, .. } => {
                    if !sprites.contains(&who.as_str()) {
                        errs.push(format!("unknown character '{}'", who));
                    }
                    if !exprs.contains(&expr.as_str()) {
                        errs.push(format!("unknown expression '{}'", expr));
                    }
                }
                Cmd::Say { who, .. } if !speakers.contains(&who.as_str()) => {
                    errs.push(format!("unknown speaker '{}'", who))
                }
                Cmd::Choice(opts) => {
                    if opts.is_empty() || opts.iter().all(|o| o.cond.is_some()) {
                        errs.push("choice needs at least one unconditional option".into());
                    }
                    for o in opts {
                        label(&o.target, &mut errs);
                    }
                }
                Cmd::If(_, l) | Cmd::Goto(l) => label(l, &mut errs),
                _ => {}
            }
        }
        if !self.labels.contains_key("start") {
            errs.push("missing 'start' label".into());
        }
        errs
    }
}

/// Program counter plus story variables.
#[derive(Clone, Default)]
pub struct Vm {
    pub pc: usize,
    pub vars: HashMap<String, i32>,
}

impl Vm {
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
        loop {
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
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    fn story() -> Script {
        Script::parse(include_str!("../assets/story.txt")).expect("story parses")
    }

    #[test]
    fn story_is_valid() {
        let errs = story().validate(
            crate::game::SPEAKERS,
            crate::art::SPRITES,
            crate::art::EXPRS,
            crate::art::BACKGROUNDS,
        );
        assert!(errs.is_empty(), "{:#?}", errs);
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
        let s = story();
        let mut vm = Vm::default();
        vm.jump(&s, "start");
        let (mut endings, mut paths) = (BTreeSet::new(), 0);
        explore(&s, vm, &mut endings, &mut paths);
        println!("{} paths, endings: {:?}", paths, endings);
        assert_eq!(endings.len(), 5, "{:?}", endings);
    }

    #[test]
    fn parser_rejects_garbage() {
        assert!(Script::parse("this is not a command").is_err());
        assert!(Script::parse("- orphan option => nowhere").is_err());
        assert!(Script::parse("*a\n*a").is_err());
    }
}
