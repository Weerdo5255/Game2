//! Below Stairs, Above Stars - a sci-fi visual novel.
//!
//!   cargo run --release                    play
//!   cargo run --release -- --procedural    ignore assets/enhanced
//!   cargo run --release -- --art-dir DIR   load enhanced art from DIR
//!   cargo run --release -- --export-guides DIR
//!       write every painting as a PNG guide for the ComfyUI suite
//!   cargo run -- --autoplay 0,1,2 --shots shots/
//!       headless playthrough, picking the listed option at each choice
//!       and saving screenshots

mod art;
mod game;
mod gfx;
mod script;

use game::{Game, Input};
use gfx::{Canvas, H, W};
use minifb::{Key, KeyRepeat, MouseButton, MouseMode, ScaleMode, Window, WindowOptions};
use std::path::{Path, PathBuf};
use std::time::Instant;

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

    let script = match script::Script::parse(include_str!("../assets/story.txt")) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("{}", e);
            std::process::exit(1);
        }
    };
    let errs = script.validate(game::SPEAKERS, art::SPRITES, art::EXPRS, art::BACKGROUNDS);
    if !errs.is_empty() {
        eprintln!("story script problems:\n  {}", errs.join("\n  "));
        std::process::exit(1);
    }

    let art_dir = if flag("--procedural") {
        None
    } else {
        arg("--art-dir").map(PathBuf::from).or_else(art::find_enhanced_dir)
    };
    if let Some(dir) = &art_dir {
        println!("enhanced art: {}", dir.display());
    }
    let game = Game::new(script, art_dir);

    if let Some(list) = arg("--autoplay") {
        let picks = list.split(',').filter_map(|s| s.trim().parse().ok()).collect();
        headless(game, picks, arg("--shots"));
        return;
    }
    if let Err(e) = windowed(game) {
        eprintln!("could not open a window: {}", e);
        std::process::exit(1);
    }
}

fn windowed(mut game: Game) -> Result<(), minifb::Error> {
    let (fw, fh) = (W as usize, H as usize);
    let opts = WindowOptions { resize: true, scale_mode: ScaleMode::AspectRatioStretch, ..WindowOptions::default() };
    let mut window = Window::new("Below Stairs, Above Stars", fw, fh, opts)?;
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
        let s = (ww as f32 / fw as f32).min(wh as f32 / fh as f32).max(0.01);
        let (ox, oy) = ((ww as f32 - fw as f32 * s) / 2.0, (wh as f32 - fh as f32 * s) / 2.0);
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
            back: pressed(&[Key::Escape]),
            click: down && !was_down,
            mouse,
            mouse_moved: mouse != last_mouse,
        };
        was_down = down;
        last_mouse = mouse;

        game.update(&inp, dt);
        game.render(&mut canvas);
        window.update_with_buffer(&canvas.px, fw, fh)?;
    }
    Ok(())
}

/// Play the story with no window, optionally writing screenshots.
fn headless(mut game: Game, picks: Vec<usize>, shots: Option<String>) {
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
    game.new_game();
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
