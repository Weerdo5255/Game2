//! Below Stairs, Above Stars - a sci-fi visual novel.
//!
//!   cargo run --release                    play
//!   cargo run --release -- --editor        open the visual story editor
//!       (--port N, --no-browser, --stories DIR, --comfy-dir DIR)
//!   cargo run --release -- --story FILE    use another story file
//!   cargo run --release -- --start LABEL   begin at a scene
//!   cargo run --release -- --silent        no music or voices
//!   cargo run --release --features world3d   places marked 3D become a world to walk through
//!   cargo run --release -- --2d            show paintings even where the story asks for 3D
//!   cargo run --release --features vr -- --vr   the same world in a VR headset (OpenXR)
//!   cargo run --release -- --format-story  tidy the story file's layout
//!   cargo run --release -- --procedural    ignore assets/enhanced
//!   cargo run --release -- --art-dir DIR   load enhanced art from DIR
//!   cargo run --release -- --window 800x450  initial window size (resizable)
//!   cargo run --release -- --export-guides DIR
//!       write every painting as a PNG guide for the ComfyUI suite
//!   cargo run -- --autoplay 0,1,2 --shots shots/
//!       headless playthrough, picking the listed option at each choice
//!       and saving screenshots

mod art;
mod audio;
mod comfy;
mod editor;
mod game;
mod gfx;
mod script;
#[cfg(feature = "world3d")]
mod world3d;

use game::{Game, Input};
use gfx::{Canvas, H, W};
use minifb::{Key, KeyRepeat, MouseButton, MouseMode, Window, WindowOptions};
use std::path::{Path, PathBuf};
use std::time::Instant;

const BUILT_IN_STORY: &str = include_str!("../assets/story.txt");

/// The story to play: the file named, else assets/story.txt (in the working
/// directory, then beside the executable), else the copy built in.
fn load_story(explicit: Option<PathBuf>) -> (String, String) {
    let mut candidates: Vec<PathBuf> = explicit.iter().cloned().collect();
    if explicit.is_none() {
        candidates.push(PathBuf::from("assets/story.txt"));
        if let Some(dir) = std::env::current_exe().ok().and_then(|e| e.parent().map(Path::to_path_buf)) {
            candidates.push(dir.join("assets/story.txt"));
        }
    }
    for path in candidates {
        match std::fs::read_to_string(&path) {
            Ok(text) => return (text, path.display().to_string()),
            Err(e) if explicit.is_some() => {
                eprintln!("cannot read {}: {}", path.display(), e);
                std::process::exit(1);
            }
            Err(_) => {}
        }
    }
    (BUILT_IN_STORY.to_string(), "the built-in story".to_string())
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let arg = |name: &str| args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned();
    let flag = |name: &str| args.iter().any(|a| a == name);

    if let Some(dir) = arg("--export-guides") {
        if let Err(e) = art::export_guides(Path::new(&dir)) {
            eprintln!("export failed: {}", e);
            std::process::exit(1);
        }
        return;
    }

    // a story's enhanced art lives beside it, in "enhanced"
    let beside_story = arg("--story").map(|s| comfy::enhanced_dir(Path::new(&s))).filter(|d| d.is_dir());
    let art_dir = if flag("--procedural") {
        None
    } else {
        arg("--art-dir").map(PathBuf::from).or(beside_story).or_else(|| if arg("--story").is_some() { None } else { art::find_enhanced_dir() })
    };

    if flag("--editor") {
        // the editor always works on a real file, created from the built-in story if need be
        let path = arg("--story").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("assets/story.txt"));
        if !path.exists() {
            let made = path.parent().map_or(Ok(()), std::fs::create_dir_all).and_then(|_| std::fs::write(&path, BUILT_IN_STORY));
            if let Err(e) = made {
                eprintln!("cannot create {}: {}", path.display(), e);
                std::process::exit(1);
            }
        }
        let options = editor::Options {
            story: path,
            stories: arg("--stories").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("stories")),
            comfy: arg("--comfy-dir").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("comfyui")),
            port: arg("--port").and_then(|p| p.parse().ok()).unwrap_or(7878),
            procedural: flag("--procedural"),
            browser: !flag("--no-browser"),
            job_timeout: arg("--job-timeout").and_then(|p| p.parse().ok()).unwrap_or(1800),
        };
        if let Err(e) = editor::serve(options) {
            eprintln!("{}", e);
            std::process::exit(1);
        }
        return;
    }

    let (text, source) = load_story(arg("--story").map(PathBuf::from));
    let (doc, errors) = script::Doc::parse(&text);
    if flag("--format-story") {
        // rewrite the story file in the editor's canonical layout
        let path = arg("--story").unwrap_or_else(|| "assets/story.txt".into());
        if !errors.is_empty() {
            eprintln!("not rewriting {}: it has lines that cannot be understood\n  {}", path, errors.join("\n  "));
            std::process::exit(1);
        }
        if let Err(e) = std::fs::write(&path, doc.to_text()) {
            eprintln!("cannot write {}: {}", path, e);
            std::process::exit(1);
        }
        println!("rewrote {}", path);
        return;
    }
    let mut errors: Vec<String> = errors;
    errors.extend(
        doc.problems(art::EXPRS, art::BACKGROUNDS, art::SPRITES)
            .into_iter()
            .filter(|p| p.level == "error")
            .map(|p| if p.scene.is_empty() { p.msg } else { format!("scene '{}': {}", p.scene, p.msg) }),
    );
    if !errors.is_empty() {
        eprintln!("problems in {}:\n  {}", source, errors.join("\n  "));
        eprintln!("open the editor to fix them:  --editor");
        std::process::exit(1);
    }
    let script = script::Script::compile(&doc);
    let start = arg("--start");
    if let Some(label) = &start {
        if !script.labels.contains_key(label) {
            eprintln!("no scene called '{}'", label);
            std::process::exit(1);
        }
    }

    if let Some(dir) = &art_dir {
        println!("enhanced art: {}", dir.display());
    }
    // sound lives beside the story, in "audio"; headless runs stay silent but log what would play
    let headless_run = arg("--autoplay").is_some();
    let story_file = arg("--story").map(PathBuf::from).or_else(|| Some(PathBuf::from("assets/story.txt")).filter(|p| p.is_file()));
    let sound = audio::Audio::new(story_file.map(|s| audio::audio_dir(&s)).filter(|_| !flag("--silent")), !headless_run);
    let mut game = Game::new(script, art_dir, sound);

    if let Some(list) = arg("--autoplay") {
        let picks = list.split(',').filter_map(|s| s.trim().parse().ok()).collect();
        headless(game, picks, arg("--shots"), start.as_deref().unwrap_or("start"));
        return;
    }
    let size = arg("--window")
        .and_then(|s| s.split_once('x').and_then(|(w, h)| Some((w.parse().ok()?, h.parse().ok()?))))
        .filter(|&(w, h): &(usize, usize)| w >= 160 && h >= 90)
        .unwrap_or((W as usize, H as usize));
    if let Some(label) = &start {
        game.start_at(label);
    }
    // a story with places marked 3D is shown by the 3D engine, if this build has one
    if flag("--vr") {
        if !cfg!(feature = "vr") {
            eprintln!("--vr needs a build with the VR feature: cargo run --release --features vr -- --vr");
        } else if flag("--2d") {
            eprintln!("--2d and --vr were both given: showing the ordinary 2D window (leave out --2d for the headset)");
        }
    }
    // (in a headset every story is shown by the 3D engine: paintings hang on a panel)
    if (game.has_world() || (flag("--vr") && cfg!(feature = "vr"))) && !flag("--2d") {
        #[cfg(feature = "world3d")]
        {
            let shot = arg("--world-shot").map(|file| (file, arg("--world-shot-lines").and_then(|n| n.parse().ok()).unwrap_or(1)));
            if shot.is_some() && start.is_none() {
                game.new_game();
            }
            world3d::run(game, world3d::Options { title: "Below Stairs, Above Stars".into(), size, shot, vr: flag("--vr") });
            return;
        }
        #[cfg(not(feature = "world3d"))]
        eprintln!("this story has places marked 3D, but this build has no 3D engine: showing their paintings (build with --features world3d)");
    }
    if arg("--world-shot").is_some() {
        // asked for a picture of the 3D world, which this run cannot show
        eprintln!("--world-shot needs a story with a place marked 3D, a build with --features world3d, and no --2d");
        std::process::exit(2);
    }
    if let Err(e) = windowed(game, size) {
        eprintln!("could not open a window: {}", e);
        std::process::exit(1);
    }
}

/// Where the frame sits inside a window of the given size: (scale, x offset, y offset).
fn letterbox(ww: usize, wh: usize) -> (f32, f32, f32) {
    let (fw, fh) = (W as f32, H as f32);
    let s = (ww as f32 / fw).min(wh as f32 / fh).max(0.01);
    (s, (ww as f32 - fw * s) / 2.0, (wh as f32 - fh * s) / 2.0)
}

/// Scale the frame into a window-sized buffer, keeping its aspect ratio.
/// (Done here rather than by minifb, whose aspect-preserving mode crashes.)
fn present(canvas: &Canvas, out: &mut Vec<u32>, ww: usize, wh: usize) {
    out.clear();
    out.resize(ww * wh, 0);
    let (s, ox, oy) = letterbox(ww, wh);
    let src_x: Vec<Option<usize>> = (0..ww)
        .map(|x| Some(((x as f32 + 0.5 - ox) / s).floor()).filter(|v| *v >= 0.0 && *v < W as f32).map(|v| v as usize))
        .collect();
    for y in 0..wh {
        let sy = ((y as f32 + 0.5 - oy) / s).floor();
        if sy < 0.0 || sy >= H as f32 {
            continue;
        }
        let row = &canvas.px[sy as usize * W as usize..][..W as usize];
        for (px, sx) in out[y * ww..][..ww].iter_mut().zip(&src_x) {
            if let Some(sx) = sx {
                *px = row[*sx];
            }
        }
    }
}

fn windowed(mut game: Game, size: (usize, usize)) -> Result<(), minifb::Error> {
    let (fw, fh) = (W as usize, H as usize);
    let opts = WindowOptions { resize: true, ..WindowOptions::default() };
    let mut window = Window::new("Below Stairs, Above Stars", size.0, size.1, opts)?;
    let mut scaled = Vec::new();
    window.set_target_fps(60);
    let mut canvas = Canvas::new(W, H);
    let mut last = Instant::now();
    let (mut was_down, mut last_mouse) = (false, (0, 0));

    while window.is_open() && !game.quit {
        let dt = last.elapsed().as_secs_f32().min(0.1);
        last = Instant::now();
        let pressed = |keys: &[Key]| keys.iter().any(|k| window.is_key_pressed(*k, KeyRepeat::No));
        let down = window.get_mouse_down(MouseButton::Left);
        // map window coordinates to the letterboxed frame
        let (ww, wh) = window.get_size();
        let (ww, wh) = (ww.max(1), wh.max(1));
        let (s, ox, oy) = letterbox(ww, wh);
        let mouse = window
            .get_mouse_pos(MouseMode::Pass)
            .map(|(x, y)| (((x - ox) / s) as i32, ((y - oy) / s) as i32))
            .unwrap_or(last_mouse);
        let inp = Input {
            advance: pressed(&[Key::Space, Key::Enter, Key::Z]),
            up: pressed(&[Key::Up, Key::W]),
            down: pressed(&[Key::Down, Key::S]),
            skip: window.is_key_down(Key::LeftCtrl) || window.is_key_down(Key::RightCtrl),
            save: pressed(&[Key::F5]),
            load: pressed(&[Key::F9]),
            mute: pressed(&[Key::M]),
            back: pressed(&[Key::Escape]),
            click: down && !was_down,
            mouse,
            mouse_moved: mouse != last_mouse,
        };
        was_down = down;
        last_mouse = mouse;

        game.update(&inp, dt);
        game.render(&mut canvas);
        if (ww, wh) == (fw, fh) {
            window.update_with_buffer(&canvas.px, fw, fh)?;
        } else {
            present(&canvas, &mut scaled, ww, wh);
            window.update_with_buffer(&scaled, ww, wh)?;
        }
    }
    Ok(())
}

/// Play the story with no window, optionally writing screenshots.
fn headless(mut game: Game, picks: Vec<usize>, shots: Option<String>, start: &str) {
    let mut canvas = Canvas::new(W, H);
    let (mut next_pick, mut says, mut n) = (0, 0, 0);
    let shoot_to = |canvas: &Canvas, path: String| {
        if let Err(e) = canvas.save_png(Path::new(&path), false) {
            eprintln!("{}", e);
            std::process::exit(1);
        }
    };
    if let Some(dir) = &shots {
        std::fs::create_dir_all(dir).expect("create screenshot directory");
        game.update(&Input::default(), 1.0);
        game.render(&mut canvas);
        shoot_to(&canvas, format!("{}/000_title.png", dir));
    }
    game.start_at(start);
    for _ in 0..100_000 {
        // let fades and the typewriter finish
        game.update(&Input::default(), 5.0);
        game.update(&Input::default(), 5.0);
        let kind = game.kind();
        let shoot = match kind {
            "say" => {
                says += 1;
                says % 4 == 1
            }
            _ => true,
        };
        if let (Some(dir), true) = (&shots, shoot) {
            n += 1;
            game.render(&mut canvas);
            shoot_to(&canvas, format!("{}/{:03}_{}.png", dir, n, kind));
        }
        match kind {
            "choice" => {
                game.pick(picks.get(next_pick).copied().unwrap_or(0));
                next_pick += 1;
            }
            "ending" => {
                if !game.audio.log.is_empty() {
                    let voiced = game.audio.log.iter().filter(|l| l.starts_with("voice ")).count();
                    let music: Vec<&str> = game.audio.log.iter().filter(|l| l.starts_with("music ")).map(|l| &l[6..]).collect();
                    println!("sound: {} voiced line(s); music: {}", voiced, if music.is_empty() { "none".to_string() } else { music.join(", ") });
                }
                println!(
                    "ending: {} (peace {}, heat {}, favor {}, trust {})",
                    game.ending_title().unwrap_or("?"),
                    game.var("peace"),
                    game.var("heat"),
                    game.var("favor"),
                    game.var("trust")
                );
                return;
            }
            "title" => return,
            _ => game.update(&Input { advance: true, ..Input::default() }, 0.0),
        }
    }
}
