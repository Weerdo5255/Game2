//! Below Stairs, Above Stars - a 16-bit style visual novel.
//!
//!   cargo run --release               play
//!   cargo run --release -- --scale 4  bigger window (1-6, default 3)
//!   cargo run -- --autoplay 0,1,2 --shots shots/   headless playthrough,
//!       picking the listed option at each choice and saving screenshots

mod art;
mod game;
mod gfx;
mod script;

use game::{Game, Input};
use gfx::{Canvas, H, W};
use minifb::{Key, KeyRepeat, MouseButton, MouseMode, Window, WindowOptions};
use std::time::Instant;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let arg = |name: &str| args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned();

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
    let game = Game::new(script);

    if let Some(list) = arg("--autoplay") {
        let picks = list.split(',').filter_map(|s| s.trim().parse().ok()).collect();
        headless(game, picks, arg("--shots"));
        return;
    }
    let scale = arg("--scale").and_then(|s| s.parse::<usize>().ok()).unwrap_or(3).clamp(1, 6);
    if let Err(e) = windowed(game, scale) {
        eprintln!("could not open a window: {}", e);
        std::process::exit(1);
    }
}

fn windowed(mut game: Game, scale: usize) -> Result<(), minifb::Error> {
    let (bw, bh) = (W as usize * scale, H as usize * scale);
    let mut window = Window::new("Below Stairs, Above Stars", bw, bh, WindowOptions::default())?;
    window.set_target_fps(60);
    let mut canvas = Canvas::new(W, H);
    let mut big = vec![0u32; bw * bh];
    let mut last = Instant::now();
    let (mut was_down, mut last_mouse) = (false, (0, 0));

    while window.is_open() && !game.quit {
        let dt = last.elapsed().as_secs_f32().min(0.1);
        last = Instant::now();
        let pressed = |keys: &[Key]| keys.iter().any(|k| window.is_key_pressed(*k, KeyRepeat::No));
        let down = window.get_mouse_down(MouseButton::Left);
        let mouse = window
            .get_mouse_pos(MouseMode::Clamp)
            .map(|(x, y)| ((x as usize / scale) as i32, (y as usize / scale) as i32))
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
        for y in 0..bh {
            let row = &canvas.px[(y / scale) * W as usize..][..W as usize];
            for (x, out) in big[y * bw..][..bw].iter_mut().enumerate() {
                *out = row[x / scale];
            }
        }
        window.update_with_buffer(&big, bw, bh)?;
    }
    Ok(())
}

/// Play the story with no window, optionally writing screenshots.
fn headless(mut game: Game, picks: Vec<usize>, shots: Option<String>) {
    let mut canvas = Canvas::new(W, H);
    let (mut next_pick, mut says, mut n) = (0, 0, 0);
    if let Some(dir) = &shots {
        std::fs::create_dir_all(dir).expect("create screenshot directory");
        game.update(&Input::default(), 1.0);
        game.render(&mut canvas);
        save_png(&canvas, &format!("{}/000_title.png", dir));
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
            save_png(&canvas, &format!("{}/{:03}_{}.png", dir, n, kind));
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

fn save_png(c: &Canvas, path: &str) {
    const S: usize = 2;
    let (w, h) = (c.w as usize * S, c.h as usize * S);
    let mut data = Vec::with_capacity(w * h * 3);
    for y in 0..h {
        for x in 0..w {
            let p = c.px[(y / S) * c.w as usize + x / S];
            data.extend_from_slice(&[(p >> 16) as u8, (p >> 8) as u8, p as u8]);
        }
    }
    let file = std::io::BufWriter::new(std::fs::File::create(path).expect("create png"));
    let mut enc = png::Encoder::new(file, w as u32, h as u32);
    enc.set_color(png::ColorType::Rgb);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header().and_then(|mut wr| wr.write_image_data(&data)).expect("write png");
}
