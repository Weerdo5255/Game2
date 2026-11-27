//! The story's places as a 3D world to walk through, drawn by the Bevy
//! engine (cargo feature `world3d`).
//!
//! The story itself runs exactly as it does over paintings: the same
//! `Game` is stepped every frame. Where a place is marked 3D, this module
//! shows a built room instead of the painting, stands the cast in it as
//! upright cut-outs that turn to face you, and lets you walk about while
//! the text, choices and meters are drawn over the top. Places that stay
//! 2D are shown as their paintings, in the same window.
//!
//!   W A S D         walk            Shift            hurry
//!   Left / Right    turn (or Q/E)   right mouse drag look around
//!   Up / Down       pick a choice   Enter / Space / click: advance
//!   Tab (held)      hide the text to look around
//!
//! With the `vr` feature and `--vr` the same world is shown in a headset
//! through OpenXR: see the `vr` module at the end of this file.

use std::collections::HashMap;
use std::f32::consts::{PI, TAU};

use bevy::input::mouse::AccumulatedMouseMotion;
use bevy::pbr::{DistanceFog, FogFalloff, NotShadowCaster};
use bevy::prelude::*;
use bevy::render::camera::Viewport;
use bevy::render::render_asset::RenderAssetUsages;
use bevy::render::render_resource::{Extent3d, Face, TextureDimension, TextureFormat};
use bevy::render::view::screenshot::{save_to_disk, Screenshot};
use bevy::window::{PrimaryWindow, WindowResolution};
use bevy::winit::WinitSettings;

use crate::art;
use crate::game::{Game, Input};
use crate::gfx::{Canvas, H, W};

/// Places that have been built in 3D. A place marked 3D that is not here
/// is shown as its painting.
pub const PLACES: &[&str] = &["pantry", "axis", "dock", "vista", "vista_night", "ballroom", "study"];

pub struct Options {
    pub title: String,
    pub size: (usize, usize),
    /// Take one picture of the window once this many lines have been
    /// shown, then quit (for documentation and tests).
    pub shot: Option<(String, usize)>,
    /// Show the world in a VR headset as well (needs the `vr` feature).
    pub vr: bool,
}

/// What a headset and its controllers ask for this frame. Without one it
/// stays at rest, and everything below behaves as on a desktop.
#[derive(Resource, Default)]
struct Headset {
    /// A headset session is running.
    active: bool,
    /// Walking: x to the right, y forward, relative to where the head faces.
    stick: Vec2,
    /// Which way the head faces, relative to the body (radians, left positive).
    heading: f32,
    /// Turn the body by this much now (snap turning).
    turn: f32,
    advance: bool,
    up: bool,
    down: bool,
    /// Where the eyes are in the world, for the cast to face.
    eye: Option<Vec3>,
    /// Where the head is within the tracked space (its floor-level origin is the middle of the play area).
    #[cfg_attr(not(feature = "vr"), allow(dead_code))]
    local: Vec3,
    /// How far the head has really moved across the floor since the last frame, in the tracked space.
    stepped: Vec2,
}

/// Where the player stands, for whatever carries the headset's viewpoint.
#[derive(Resource, Default)]
struct Body {
    feet: Vec3,
    yaw: f32,
    in_place: bool,
    /// Clear floor ahead of the player, in metres (for placing the text panel).
    reach: f32,
    fog: Option<(Color, f32, f32)>,
    /// The story's frame was redrawn this frame.
    redrawn: bool,
    /// The walkable floor of the place.
    floor: (Vec2, Vec2),
}

const EYE: f32 = 1.62;
const BODY: f32 = 0.32;

/// Everything that is not Bevy's: the story and the frames it draws.
struct Runner {
    game: Game,
    black: Canvas,
    white: Canvas,
    place: Option<Place>,
    built: String,
    portraits: HashMap<(String, String), Handle<Image>>,
    shown: [Option<(String, String)>; 3],
    last_mouse: (i32, i32),
    yaw: f32,
    pitch: f32,
    shot: Option<(String, usize)>,
    /// The overlay as last sent to the engine, to send it again only when it changes.
    sent: Vec<u8>,
    /// 0..255 of the story's own (display-space) mixing to what the engine's (linear-light) blending needs.
    alpha: [u8; 256],
    shot_timer: Option<f32>,
    auto: f32,
}

/// One built place: where you start, where you may walk, where the cast stands.
struct Place {
    spawn: Vec3,
    yaw: f32,
    /// The walkable rectangle, in x and z.
    min: Vec2,
    max: Vec2,
    /// Things you walk around: centre and half size, in x and z.
    solids: Vec<(Vec2, Vec2)>,
    /// Where the left, centre and right of the cast stand (their feet).
    anchors: [Vec3; 3],
    /// People drift here rather than stand.
    floating: bool,
}

/// The system that steps the story and the world, for others to run before or after.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
struct Story;

#[derive(Component)]
struct PlacePart;
#[derive(Component)]
struct Eye;
#[derive(Component)]
struct CastSlot(usize);
#[derive(Component)]
struct Overlay;

pub fn run(game: Game, opts: Options) {
    let runner = Runner {
        game,
        black: Canvas::new(W, H),
        white: Canvas::new(W, H),
        place: None,
        built: String::new(),
        portraits: HashMap::new(),
        shown: [None, None, None],
        last_mouse: (0, 0),
        yaw: 0.0,
        pitch: 0.0,
        shot: opts.shot,
        sent: vec![0; (W * H * 4) as usize],
        alpha: std::array::from_fn(|a| (255.0 * (1.0 - (1.0 - a as f32 / 255.0).powf(2.2))).round() as u8),
        shot_timer: None,
        auto: 0.0,
    };
    let window = WindowPlugin {
        primary_window: Some(Window { title: opts.title, resolution: WindowResolution::new(opts.size.0 as f32, opts.size.1 as f32), ..default() }),
        ..default()
    };
    let mut app = App::new();
    #[cfg_attr(not(feature = "vr"), allow(unused_mut))]
    let mut window = Some(window);
    #[cfg(feature = "vr")]
    if opts.vr {
        vr::plugins(&mut app, window.take().unwrap());
    }
    #[cfg(not(feature = "vr"))]
    if opts.vr {
        eprintln!("--vr needs a build with the VR feature (cargo build --features vr): showing the world on the desktop");
    }
    if let Some(window) = window {
        app.add_plugins(DefaultPlugins.set(window));
    }
    app
        .init_resource::<Headset>()
        .init_resource::<Body>()
        // keep the story moving when the window is not in front (a headset never waits for the window at all)
        .insert_resource(if opts.vr && cfg!(feature = "vr") { WinitSettings { focused_mode: bevy::winit::UpdateMode::Continuous, unfocused_mode: bevy::winit::UpdateMode::Continuous } } else { WinitSettings::game() })
        .insert_resource(ClearColor(Color::BLACK))
        .insert_resource(AmbientLight { color: Color::WHITE, brightness: 300.0, ..default() })
        .insert_non_send_resource(runner)
        .add_systems(Startup, setup)
        .add_systems(Update, frame.in_set(Story))
        .run();
}

fn setup(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>, mut materials: ResMut<Assets<StandardMaterial>>, mut images: ResMut<Assets<Image>>) {
    commands.spawn((
        Camera3d::default(),
        Camera { order: 0, ..default() },
        Projection::Perspective(PerspectiveProjection { fov: 62f32.to_radians(), near: 0.08, far: 4000.0, ..default() }),
        Transform::from_xyz(0.0, EYE, 3.0),
        DistanceFog { color: Color::BLACK, falloff: FogFalloff::Linear { start: 40.0, end: 400.0 }, ..default() },
        Eye,
    ));
    // the story's own drawing (text, choices, meters, or a whole painting) lies over the world
    let frame = Image::new_fill(
        Extent3d { width: W as u32, height: H as u32, depth_or_array_layers: 1 },
        TextureDimension::D2,
        &[0, 0, 0, 255],
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    );
    commands.spawn((Camera2d, Camera { order: 1, clear_color: ClearColorConfig::None, ..default() }));
    commands.spawn((Sprite { image: images.add(frame), custom_size: Some(Vec2::new(W as f32, H as f32)), ..default() }, Overlay));
    // three cut-outs for the cast, shown and textured as the story asks
    let (w, h) = (CAST_HEIGHT * art::SPR_W as f32 / art::SPR_H as f32, CAST_HEIGHT);
    for i in 0..3 {
        commands.spawn((
            Mesh3d(meshes.add(Rectangle::new(w, h))),
            MeshMaterial3d(materials.add(StandardMaterial { unlit: true, alpha_mode: AlphaMode::Blend, double_sided: true, cull_mode: None::<Face>, ..default() })),
            Transform::default(),
            Visibility::Hidden,
            NotShadowCaster,
            CastSlot(i),
        ));
    }
}

/// How tall a portrait stands in the world, in metres.
const CAST_HEIGHT: f32 = 1.6;
/// How far above the floor its lower edge is (portraits end at the waist,
/// and this puts their faces at your eye level).
const CAST_LIFT: f32 = 0.72;

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn frame(
    mut run: NonSendMut<Runner>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut ambient: ResMut<AmbientLight>,
    mut clear: ResMut<ClearColor>,
    (keys, buttons, motion, time, headset, mut body): (Res<ButtonInput<KeyCode>>, Res<ButtonInput<MouseButton>>, Res<AccumulatedMouseMotion>, Res<Time>, Res<Headset>, ResMut<Body>),
    window: Single<&Window, With<PrimaryWindow>>,
    mut eye: Single<(&mut Transform, &mut DistanceFog, &mut Camera), (With<Eye>, Without<CastSlot>, Without<Overlay>)>,
    mut cast: Query<(&CastSlot, &mut Transform, &mut Visibility, &MeshMaterial3d<StandardMaterial>), (Without<Eye>, Without<Overlay>)>,
    mut overlay: Single<(&mut Sprite, &mut Transform), (With<Overlay>, Without<Eye>, Without<CastSlot>)>,
    parts: Query<Entity, With<PlacePart>>,
    mut exit: EventWriter<AppExit>,
) {
    let run = &mut *run;
    let dt = time.delta_secs().min(0.1);
    let (ww, wh) = (window.width().max(1.0), window.height().max(1.0));
    let scale = (ww / W as f32).min(wh / H as f32).max(0.01);
    let in_world = run.game.world_view().map_or(false, |v| PLACES.contains(&v.place.as_str()));

    // ---- the story's own controls
    let pressed = |list: &[KeyCode]| list.iter().any(|k| keys.just_pressed(*k));
    let mouse = window
        .cursor_position()
        .map(|p| (((p.x - (ww - W as f32 * scale) / 2.0) / scale) as i32, ((p.y - (wh - H as f32 * scale) / 2.0) / scale) as i32))
        .unwrap_or(run.last_mouse);
    let mut inp = Input {
        advance: pressed(&[KeyCode::Space, KeyCode::Enter, KeyCode::KeyZ]) || headset.advance,
        // W and S walk in a 3D place; over a painting they pick, as ever
        up: pressed(&[KeyCode::ArrowUp]) || (!in_world && pressed(&[KeyCode::KeyW])) || headset.up,
        down: pressed(&[KeyCode::ArrowDown]) || (!in_world && pressed(&[KeyCode::KeyS])) || headset.down,
        skip: keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight),
        save: pressed(&[KeyCode::F5]),
        load: pressed(&[KeyCode::F9]),
        mute: pressed(&[KeyCode::KeyM]),
        back: pressed(&[KeyCode::Escape]),
        click: buttons.just_pressed(MouseButton::Left),
        mouse,
        mouse_moved: mouse != run.last_mouse,
    };
    run.last_mouse = mouse;
    // taking a picture: press on by itself until enough has been said
    if let Some((_, wanted)) = &run.shot {
        run.auto += dt;
        if run.shot_timer.is_none() && run.game.lines_begun < *wanted && run.auto > 0.4 {
            run.auto = 0.0;
            inp.advance = true;
        }
    }
    run.game.update(&inp, dt);
    if run.game.quit {
        exit.write(AppExit::Success);
        return;
    }
    let kind = run.game.kind();
    // the world is drawn where the story's frame is: the same letterboxed part of the window
    let (pw, ph) = (window.physical_width().max(1), window.physical_height().max(1));
    let fit = (pw as f32 / W as f32).min(ph as f32 / H as f32);
    let size = UVec2::new(((W as f32 * fit) as u32).clamp(1, pw), ((H as f32 * fit) as u32).clamp(1, ph));
    let viewport = Viewport { physical_position: UVec2::new((pw - size.x) / 2, (ph - size.y) / 2), physical_size: size, ..default() };
    if eye.2.viewport.as_ref().map_or(true, |v| v.physical_position != viewport.physical_position || v.physical_size != viewport.physical_size) {
        eye.2.viewport = Some(viewport);
    }

    // ---- the place
    let view = run.game.world_view().filter(|v| PLACES.contains(&v.place.as_str()));
    let place_name = view.as_ref().map_or("", |v| v.place.as_str());
    if place_name != run.built {
        for e in &parts {
            commands.entity(e).despawn();
        }
        run.built = place_name.to_string();
        run.place = None;
        clear.0 = Color::BLACK;
        if !place_name.is_empty() {
            let mut kit = Kit { commands: &mut commands, meshes: &mut meshes, materials: &mut materials, solids: Vec::new(), seed: 7 };
            let (place, air) = build(place_name, &mut kit);
            ambient.color = air.ambient;
            ambient.brightness = air.brightness;
            clear.0 = air.sky;
            eye.1.color = air.sky;
            eye.1.falloff = FogFalloff::Linear { start: air.fog.0, end: air.fog.1 };
            eye.0.translation = place.spawn;
            run.yaw = place.yaw;
            run.pitch = 0.0;
            run.place = Some(place);
            body.fog = Some((air.sky, air.fog.0, air.fog.1));
        } else {
            body.fog = None;
        }
    }

    // ---- walking and looking
    if let Some(place) = &run.place {
        let turn = (keys.pressed(KeyCode::ArrowLeft) || keys.pressed(KeyCode::KeyQ)) as i32 - (keys.pressed(KeyCode::ArrowRight) || keys.pressed(KeyCode::KeyE)) as i32;
        run.yaw += turn as f32 * 1.9 * dt + headset.turn;
        if buttons.pressed(MouseButton::Right) {
            run.yaw -= motion.delta.x * 0.004;
            run.pitch = (run.pitch - motion.delta.y * 0.004).clamp(-1.2, 1.2);
        }
        let forward = Vec2::new(-run.yaw.sin(), -run.yaw.cos());
        let right = Vec2::new(forward.y * -1.0, forward.x);
        let keys_step = forward * ((keys.pressed(KeyCode::KeyW) as i32 - keys.pressed(KeyCode::KeyS) as i32) as f32) + right * ((keys.pressed(KeyCode::KeyD) as i32 - keys.pressed(KeyCode::KeyA) as i32) as f32);
        let speed = if keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight) { 5.2 } else { 2.6 };
        // a thumbstick walks where the head is facing, as fast as it is pushed (and never faster than a walk)
        let facing = run.yaw + headset.heading;
        let (head_forward, head_right) = (Vec2::new(-facing.sin(), -facing.cos()), Vec2::new(facing.cos(), -facing.sin()));
        let stick_step = (head_forward * headset.stick.y + head_right * headset.stick.x).clamp_length_max(1.0) * 2.0;
        let at = Vec2::new(eye.0.translation.x, eye.0.translation.z);
        // real steps taken in the room move you through the place too (and stop at its walls)
        let (sin, cos) = run.yaw.sin_cos();
        let real_step = Vec2::new(headset.stepped.x * cos + headset.stepped.y * sin, -headset.stepped.x * sin + headset.stepped.y * cos);
        let to = walk(place, at, (keys_step.normalize_or_zero() * speed + stick_step) * dt + real_step);
        // (no swaying in a headset: the world should move only when you do)
        let bob = if place.floating && !headset.active { (time.elapsed_secs() * 0.9).sin() * 0.06 } else { 0.0 };
        eye.0.translation = Vec3::new(to.x, place.spawn.y + bob, to.y);
        eye.0.rotation = Quat::from_euler(EulerRot::YXZ, run.yaw, run.pitch, 0.0);
        body.feet = Vec3::new(to.x, place.spawn.y - EYE, to.y);
        body.yaw = run.yaw;
        // how far the floor runs on ahead, to keep the text panel out of the walls
        let ahead = [(place.max.x - to.x, forward.x), (to.x - place.min.x, -forward.x), (place.max.y - to.y, forward.y), (to.y - place.min.y, -forward.y)];
        body.floor = (place.min, place.max);
        body.reach = ahead.iter().filter(|(_, along)| *along > 1e-3).map(|(gap, along)| gap / along).fold(f32::MAX, f32::min);
    }
    body.in_place = run.place.is_some();

    // ---- the cast, standing in the place and turned towards you
    let mut wanted: [Option<(String, String, f32, bool)>; 3] = [None, None, None];
    if let (Some(view), Some(_)) = (&view, &run.place) {
        wanted = view.cast.clone();
    }
    let eye_at = headset.eye.filter(|_| headset.active).unwrap_or(eye.0.translation);
    for (slot, mut tf, mut vis, material) in &mut cast {
        let i = slot.0;
        let (Some((who, expr, appear, dim)), Some(place)) = (&wanted[i], &run.place) else {
            *vis = Visibility::Hidden;
            run.shown[i] = None;
            continue;
        };
        let key = (who.clone(), expr.clone());
        if run.shown[i].as_ref() != Some(&key) {
            if !run.portraits.contains_key(&key) {
                let Some(canvas) = run.game.portrait(who, expr) else {
                    // nobody the story knows: nothing stands there (as over a painting)
                    *vis = Visibility::Hidden;
                    run.shown[i] = None;
                    continue;
                };
                let image = picture(canvas);
                run.portraits.insert(key.clone(), images.add(image));
            }
            if let Some(m) = materials.get_mut(&material.0) {
                m.base_color_texture = run.portraits.get(&key).cloned();
            }
            run.shown[i] = Some(key);
        }
        let light = if *dim { 0.55 } else { 1.0 };
        let colour = Color::srgba(light, light, light, appear.clamp(0.0, 1.0));
        // only touched when it changes: touching a material sends it to the renderer again
        if materials.get(&material.0).map_or(false, |m| m.base_color != colour) {
            if let Some(m) = materials.get_mut(&material.0) {
                m.base_color = colour;
            }
        }
        let foot = place.anchors[i];
        let drift = if place.floating { (time.elapsed_secs() * 1.3 + i as f32 * 1.7).sin() * 0.09 } else { 0.0 };
        tf.translation = foot + Vec3::Y * (CAST_LIFT + CAST_HEIGHT / 2.0 + drift);
        tf.rotation = Quat::from_rotation_y((eye_at.x - foot.x).atan2(eye_at.z - foot.z));
        *vis = Visibility::Visible;
    }

    // ---- what the story draws over it
    overlay.0.custom_size = Some(Vec2::new(W as f32 * scale, H as f32 * scale));
    overlay.1.translation = Vec3::ZERO;
    let mut frame = std::mem::take(&mut run.sent);
    let mut changed = false;
    let mut put = |out: &mut [u8], px: [u8; 4]| {
        if out != px {
            out.copy_from_slice(&px);
            changed = true;
        }
    };
    if run.place.is_some() {
        // Drawn once over black and once over white: where the two differ
        // the world shows through, by exactly that much.
        run.game.overlay = Some(0xFF00_0000);
        run.game.render(&mut run.black);
        run.game.overlay = Some(0xFFFF_FFFF);
        run.game.render(&mut run.white);
        // holding Tab clears the talk away for a look around (choices and endings stay)
        let hide = keys.pressed(KeyCode::Tab) && kind == "say";
        for ((out, b), w) in frame.chunks_exact_mut(4).zip(&run.black.px).zip(&run.white.px) {
            let (br, bg, bb) = ((b >> 16) & 255, (b >> 8) & 255, b & 255);
            let a = if hide { 0 } else { 255 - (((w >> 8) & 255).saturating_sub(bg)).min(255) };
            if a == 0 {
                put(out, [0, 0, 0, 0]);
            } else {
                put(out, [(br * 255 / a).min(255) as u8, (bg * 255 / a).min(255) as u8, (bb * 255 / a).min(255) as u8, run.alpha[a as usize]]);
            }
        }
    } else {
        run.game.overlay = None;
        run.game.render(&mut run.black);
        for (out, p) in frame.chunks_exact_mut(4).zip(&run.black.px) {
            put(out, [(p >> 16) as u8, (p >> 8) as u8, *p as u8, 255]);
        }
    }
    // sent to the engine only when something on it has changed
    body.redrawn = changed;
    if changed {
        if let Some(data) = images.get_mut(&overlay.0.image).and_then(|i| i.data.as_mut()) {
            data.copy_from_slice(&frame);
        }
    }
    run.sent = frame;

    // ---- a picture, when asked for
    if let Some((path, wanted)) = run.shot.clone() {
        match &mut run.shot_timer {
            None if run.game.lines_begun >= wanted => run.shot_timer = Some(0.0),
            None if run.game.ending_title().is_some() => {
                eprintln!("the story ended before {} line(s) were shown", wanted);
                exit.write(AppExit::error());
            }
            Some(t) => {
                let before = *t;
                *t += dt;
                // let the text finish and the lights settle, then shoot, then leave
                if before < 2.5 && *t >= 2.5 {
                    commands.spawn(Screenshot::primary_window()).observe(save_to_disk(path));
                } else if *t > 4.0 {
                    exit.write(AppExit::Success);
                }
            }
            _ => {}
        }
    }
}

/// A portrait as a texture: the canvas keeps ARGB in one number per pixel.
fn picture(canvas: &Canvas) -> Image {
    let mut data = Vec::with_capacity(canvas.px.len() * 4);
    for p in &canvas.px {
        data.extend_from_slice(&[(p >> 16) as u8, (p >> 8) as u8, *p as u8, (p >> 24) as u8]);
    }
    Image::new(
        Extent3d { width: canvas.w as u32, height: canvas.h as u32, depth_or_array_layers: 1 },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    )
}

/// Move within the place: stay inside its bounds and slide around solid things.
fn walk(place: &Place, from: Vec2, step: Vec2) -> Vec2 {
    let mut to = (from + step).clamp(place.min + BODY, place.max - BODY);
    for _ in 0..2 {
        for (centre, half) in &place.solids {
            let d = to - *centre;
            let reach = *half + BODY;
            if d.x.abs() < reach.x && d.y.abs() < reach.y {
                // leave by the nearer side
                let (px, py) = (reach.x - d.x.abs(), reach.y - d.y.abs());
                if px < py {
                    to.x = centre.x + reach.x * d.x.signum();
                } else {
                    to.y = centre.y + reach.y * d.y.signum();
                }
            }
        }
    }
    let to = to.clamp(place.min + BODY, place.max - BODY);
    // pushed back into something by a wall (a thing standing close to one): stay put rather than pass through
    let inside = |p: Vec2| place.solids.iter().any(|(centre, half)| (p.x - centre.x).abs() < half.x + BODY - 1e-3 && (p.y - centre.y).abs() < half.y + BODY - 1e-3);
    if inside(to) && !inside(from) {
        return from;
    }
    to
}

// ------------------------------------------------------------- building

/// The light and air of a place.
struct Air {
    sky: Color,
    ambient: Color,
    brightness: f32,
    fog: (f32, f32),
}

/// A box of parts for building places out of simple shapes.
struct Kit<'a, 'w, 's> {
    commands: &'a mut Commands<'w, 's>,
    meshes: &'a mut Assets<Mesh>,
    materials: &'a mut Assets<StandardMaterial>,
    solids: Vec<(Vec2, Vec2)>,
    seed: u32,
}

fn rgb(r: u8, g: u8, b: u8) -> Color {
    Color::srgb_u8(r, g, b)
}

impl Kit<'_, '_, '_> {
    fn rand(&mut self) -> f32 {
        self.seed = self.seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (self.seed >> 8) as f32 / (1u32 << 24) as f32
    }

    fn matte(&mut self, colour: Color, rough: f32, metal: f32) -> Handle<StandardMaterial> {
        self.materials.add(StandardMaterial { base_color: colour, perceptual_roughness: rough, metallic: metal, ..default() })
    }

    fn glowing(&mut self, colour: Color, strength: f32) -> Handle<StandardMaterial> {
        let c = colour.to_linear();
        self.materials.add(StandardMaterial { base_color: colour, emissive: LinearRgba::new(c.red * strength, c.green * strength, c.blue * strength, 1.0), ..default() })
    }

    /// A box, centred at `at`.
    fn block(&mut self, at: Vec3, size: Vec3, colour: Color) -> Entity {
        let material = self.matte(colour, 0.85, 0.0);
        self.block_of(at, size, material)
    }

    fn block_of(&mut self, at: Vec3, size: Vec3, material: Handle<StandardMaterial>) -> Entity {
        let mesh = self.meshes.add(Cuboid::new(size.x, size.y, size.z));
        self.commands.spawn((Mesh3d(mesh), MeshMaterial3d(material), Transform::from_translation(at), PlacePart)).id()
    }

    /// A box you cannot walk through.
    fn solid(&mut self, at: Vec3, size: Vec3, colour: Color) -> Entity {
        self.solids.push((Vec2::new(at.x, at.z), Vec2::new(size.x / 2.0, size.z / 2.0)));
        self.block(at, size, colour)
    }

    /// An upright cylinder, centred at `at`.
    fn post(&mut self, at: Vec3, radius: f32, height: f32, material: Handle<StandardMaterial>) -> Entity {
        let mesh = self.meshes.add(Cylinder::new(radius, height));
        self.commands.spawn((Mesh3d(mesh), MeshMaterial3d(material), Transform::from_translation(at), PlacePart)).id()
    }

    fn ball(&mut self, at: Vec3, radius: f32, material: Handle<StandardMaterial>) -> Entity {
        let mesh = self.meshes.add(Sphere::new(radius));
        self.commands.spawn((Mesh3d(mesh), MeshMaterial3d(material), Transform::from_translation(at), PlacePart)).id()
    }

    /// A lamp: a glowing globe and the light it gives.
    fn lamp(&mut self, at: Vec3, colour: Color, lumens: f32, shadows: bool) {
        let glow = self.glowing(colour, 6.0);
        let globe = self.ball(at, 0.11, glow);
        self.commands.entity(globe).insert(NotShadowCaster);
        self.commands.spawn((PointLight { color: colour, intensity: lumens, range: 30.0, shadows_enabled: shadows, ..default() }, Transform::from_translation(at), PlacePart));
    }

    /// Light from far away (a sun, a sunline, a floodlight).
    fn sun(&mut self, from: Vec3, colour: Color, lux: f32, shadows: bool) {
        self.commands.spawn((
            DirectionalLight { color: colour, illuminance: lux, shadows_enabled: shadows, ..default() },
            Transform::from_translation(from).looking_at(Vec3::ZERO, Vec3::Y),
            PlacePart,
        ));
    }

    /// Floor, ceiling and four walls of a room centred on the origin. Returns its walkable bounds.
    fn room(&mut self, width: f32, depth: f32, height: f32, floor: Color, walls: Color, ceiling: Color) -> (Vec2, Vec2) {
        let (hw, hd) = (width / 2.0, depth / 2.0);
        self.block(Vec3::new(0.0, -0.1, 0.0), Vec3::new(width, 0.2, depth), floor);
        self.block(Vec3::new(0.0, height + 0.1, 0.0), Vec3::new(width, 0.2, depth), ceiling);
        self.block(Vec3::new(0.0, height / 2.0, -hd - 0.1), Vec3::new(width + 0.4, height, 0.2), walls);
        self.block(Vec3::new(0.0, height / 2.0, hd + 0.1), Vec3::new(width + 0.4, height, 0.2), walls);
        self.block(Vec3::new(-hw - 0.1, height / 2.0, 0.0), Vec3::new(0.2, height, depth), walls);
        self.block(Vec3::new(hw + 0.1, height / 2.0, 0.0), Vec3::new(0.2, height, depth), walls);
        (Vec2::new(-hw, -hd), Vec2::new(hw, hd))
    }

    /// A table with four legs; you walk around it.
    fn table(&mut self, at: Vec3, size: Vec3, colour: Color) {
        self.solids.push((Vec2::new(at.x, at.z), Vec2::new(size.x / 2.0, size.z / 2.0)));
        self.block(at + Vec3::Y * (size.y - 0.04), Vec3::new(size.x, 0.08, size.z), colour);
        for (sx, sz) in [(-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)] {
            self.block(at + Vec3::new(sx * (size.x / 2.0 - 0.08), (size.y - 0.08) / 2.0, sz * (size.z / 2.0 - 0.08)), Vec3::new(0.08, size.y - 0.08, 0.08), colour);
        }
    }
}

/// Build a place out of the kit. The story's cast stands at the anchors;
/// you start a few steps back from them, looking their way (towards -z).
fn build(name: &str, k: &mut Kit) -> (Place, Air) {
    let anchors = |z: f32| [Vec3::new(-1.25, 0.0, z), Vec3::new(0.0, 0.0, z - 0.25), Vec3::new(1.25, 0.0, z)];
    let (place, air) = match name {
        "pantry" => {
            // the servants' hall: flagstones, a scrubbed table, dressers of china, a black range
            let (min, max) = k.room(9.0, 11.0, 3.1, rgb(92, 88, 84), rgb(196, 184, 158), rgb(214, 206, 188));
            for i in 0..9 {
                for j in 0..11 {
                    if (i + j) % 2 == 0 {
                        k.block(Vec3::new(-4.0 + i as f32, 0.005, -5.0 + j as f32), Vec3::new(0.96, 0.01, 0.96), rgb(110, 105, 99));
                    }
                }
            }
            k.table(Vec3::new(0.0, 0.0, 1.4), Vec3::new(1.3, 0.78, 3.4), rgb(150, 112, 70));
            for z in [0.2, 1.4, 2.6] {
                for x in [-1.0, 1.0] {
                    k.solid(Vec3::new(x, 0.24, z), Vec3::new(0.4, 0.48, 0.4), rgb(120, 86, 54));
                }
            }
            // crockery on the table
            for i in 0..5 {
                let white = k.matte(rgb(236, 232, 222), 0.3, 0.0);
                k.post(Vec3::new(-0.3 + (i % 2) as f32 * 0.6, 0.81, 0.3 + i as f32 * 0.55), 0.13, 0.02, white);
            }
            // dressers along the left wall
            for z in [-3.4, -0.6, 2.2] {
                k.solid(Vec3::new(-4.1, 0.5, z), Vec3::new(0.7, 1.0, 2.2), rgb(98, 70, 46));
                k.block(Vec3::new(-4.3, 1.75, z), Vec3::new(0.3, 1.5, 2.2), rgb(86, 60, 40));
                for (r, y) in [1.2f32, 1.7, 2.2].iter().enumerate() {
                    k.block(Vec3::new(-4.12, *y, z), Vec3::new(0.34, 0.04, 2.1), rgb(120, 88, 58));
                    for c in 0..6 {
                        let glaze = if (c + r) % 3 == 0 { rgb(80, 110, 170) } else { rgb(232, 228, 216) };
                        let m = k.matte(glaze, 0.25, 0.0);
                        k.post(Vec3::new(-4.1, *y + 0.1, z - 0.85 + c as f32 * 0.34), 0.1, 0.14, m);
                    }
                }
            }
            // the range, in its chimney breast on the right
            k.solid(Vec3::new(3.9, 1.55, -1.5), Vec3::new(1.2, 3.1, 3.0), rgb(150, 140, 122));
            k.solid(Vec3::new(3.45, 0.5, -1.5), Vec3::new(0.9, 1.0, 2.0), rgb(28, 28, 32));
            let ember = k.glowing(rgb(255, 120, 40), 5.0);
            k.block_of(Vec3::new(2.99, 0.42, -1.5), Vec3::new(0.02, 0.3, 0.7), ember);
            let copper = k.matte(rgb(190, 110, 70), 0.3, 0.9);
            for i in 0..4 {
                k.post(Vec3::new(3.3, 2.0, -2.3 + i as f32 * 0.5), 0.16 - i as f32 * 0.02, 0.16, copper.clone());
            }
            // bells on the end wall, and the green baize door
            k.block(Vec3::new(0.0, 2.5, -5.44), Vec3::new(3.2, 0.5, 0.06), rgb(70, 50, 36));
            let brass = k.matte(rgb(214, 176, 90), 0.3, 1.0);
            for i in 0..8 {
                k.ball(Vec3::new(-1.4 + i as f32 * 0.4, 2.42, -5.36), 0.07, brass.clone());
            }
            k.block(Vec3::new(2.6, 1.05, -5.44), Vec3::new(1.0, 2.1, 0.08), rgb(52, 96, 70));
            // a high window, and lamps over the table
            let day = k.glowing(rgb(190, 214, 255), 2.5);
            k.block_of(Vec3::new(-2.4, 2.3, 5.44), Vec3::new(1.6, 0.9, 0.06), day);
            k.lamp(Vec3::new(0.0, 2.2, 0.4), rgb(255, 214, 160), 260_000.0, true);
            k.lamp(Vec3::new(0.0, 2.2, 2.6), rgb(255, 214, 160), 200_000.0, false);
            k.commands.spawn((PointLight { color: rgb(255, 130, 60), intensity: 120_000.0, range: 12.0, ..default() }, Transform::from_xyz(2.4, 0.6, -1.5), PlacePart));
            (
                Place { spawn: Vec3::new(1.9, EYE, -0.2), yaw: 0.5, min, max, solids: Vec::new(), anchors: [Vec3::new(-1.6, 0.0, -2.2), Vec3::new(-0.2, 0.0, -2.6), Vec3::new(1.2, 0.0, -2.3)], floating: false },
                Air { sky: rgb(20, 16, 14), ambient: rgb(255, 232, 205), brightness: 170.0, fog: (30.0, 90.0) },
            )
        }
        "axis" => {
            // the breakfast room at the hub: a drum of glass and white ribs, nothing has a down
            let floor = k.matte(rgb(214, 220, 232), 0.5, 0.1);
            k.post(Vec3::new(0.0, -0.1, 0.0), 6.2, 0.2, floor.clone());
            k.post(Vec3::new(0.0, 5.3, 0.0), 6.2, 0.2, floor);
            let rib = k.matte(rgb(236, 238, 244), 0.4, 0.2);
            let glass = k.glowing(rgb(120, 170, 240), 1.3);
            for i in 0..16 {
                let a = i as f32 / 16.0 * TAU;
                let (x, z) = (a.sin() * 6.0, a.cos() * 6.0);
                k.post(Vec3::new(x, 2.6, z), 0.16, 5.2, rib.clone());
                let b = a + TAU / 32.0;
                let pane = k.block_of(Vec3::new(b.sin() * 6.1, 2.6, b.cos() * 6.1), Vec3::new(2.3, 5.0, 0.05), glass.clone());
                k.commands.entity(pane).insert((Transform::from_xyz(b.sin() * 6.1, 2.6, b.cos() * 6.1).with_rotation(Quat::from_rotation_y(b)), NotShadowCaster));
            }
            // the table, bolted to nothing in particular, and breakfast adrift above it
            let silver = k.matte(rgb(210, 214, 222), 0.2, 1.0);
            k.solids.push((Vec2::new(0.0, -0.6), Vec2::new(1.1, 1.1)));
            k.post(Vec3::new(0.0, 0.9, -0.6), 1.1, 0.06, silver.clone());
            k.post(Vec3::new(0.0, 0.45, -0.6), 0.12, 0.9, silver.clone());
            for i in 0..14 {
                // (kept overhead, clear of the faces of whoever is at the table)
                let (x, y, z) = (k.rand() * 3.4 - 1.7, 2.75 + k.rand() * 1.5, -0.6 + k.rand() * 3.0 - 1.5);
                match i % 3 {
                    0 => {
                        let soup = k.matte(rgb(226, 150, 60), 0.2, 0.0);
                        let size = 0.07 + k.rand() * 0.05;
                        k.ball(Vec3::new(x, y, z), size, soup);
                    }
                    1 => {
                        let toast = k.matte(rgb(200, 150, 90), 0.9, 0.0);
                        let e = k.block_of(Vec3::new(x, y, z), Vec3::new(0.16, 0.02, 0.16), toast);
                        let spin = Quat::from_euler(EulerRot::XYZ, k.rand() * 3.0, k.rand() * 3.0, k.rand() * 3.0);
                        k.commands.entity(e).insert(Transform::from_xyz(x, y, z).with_rotation(spin));
                    }
                    _ => {
                        k.post(Vec3::new(x, y, z), 0.05, 0.1, silver.clone());
                    }
                }
            }
            k.sun(Vec3::new(4.0, 9.0, 6.0), rgb(255, 250, 240), 9_000.0, true);
            k.lamp(Vec3::new(0.0, 4.6, 0.0), rgb(235, 242, 255), 600_000.0, false);
            let r = 5.5;
            (
                Place { spawn: Vec3::new(0.0, EYE, 1.9), yaw: 0.0, min: Vec2::splat(-r * 0.72), max: Vec2::splat(r * 0.72), solids: Vec::new(), anchors: [Vec3::new(-1.7, 0.25, -1.4), Vec3::new(0.0, 0.35, -2.3), Vec3::new(1.7, 0.2, -1.4)], floating: true },
                Air { sky: rgb(150, 190, 240), ambient: rgb(226, 236, 255), brightness: 520.0, fog: (60.0, 400.0) },
            )
        }
        "dock" => {
            // the arrivals dock: a cold hangar, a visiting hull, forty trunks
            let (min, max) = k.room(26.0, 30.0, 11.0, rgb(58, 62, 72), rgb(74, 80, 94), rgb(40, 44, 54));
            let line = k.glowing(rgb(240, 200, 60), 0.6);
            for i in 0..12 {
                k.block_of(Vec3::new(-6.0, 0.006, -13.0 + i as f32 * 2.4), Vec3::new(0.25, 0.01, 1.4), line.clone());
                k.block_of(Vec3::new(6.0, 0.006, -13.0 + i as f32 * 2.4), Vec3::new(0.25, 0.01, 1.4), line.clone());
            }
            // the Meridian lord's yacht, nose in
            let hull = k.matte(rgb(170, 178, 196), 0.35, 0.8);
            k.solids.push((Vec2::new(0.0, -9.5), Vec2::new(7.4, 3.0)));
            let body = k.meshes.add(Capsule3d::new(2.4, 9.0));
            k.commands.spawn((Mesh3d(body), MeshMaterial3d(hull.clone()), Transform::from_xyz(0.0, 3.6, -9.5).with_rotation(Quat::from_rotation_z(PI / 2.0)), PlacePart));
            let fin = k.block_of(Vec3::new(5.0, 6.6, -9.5), Vec3::new(2.6, 2.4, 0.2), hull.clone());
            k.commands.entity(fin).insert(Transform::from_xyz(5.0, 6.4, -9.5).with_rotation(Quat::from_rotation_z(0.5)));
            let trim = k.matte(rgb(150, 40, 50), 0.4, 0.3);
            k.block_of(Vec3::new(0.0, 3.6, -7.12), Vec3::new(8.6, 0.25, 0.05), trim);
            for x in [-3.0, 3.0] {
                k.block(Vec3::new(x, 0.6, -9.5), Vec3::new(0.3, 1.2, 0.3), rgb(50, 54, 62));
                k.block(Vec3::new(x, 0.08, -9.5), Vec3::new(1.2, 0.16, 1.6), rgb(50, 54, 62));
            }
            let port = k.glowing(rgb(255, 226, 170), 3.0);
            for i in 0..7 {
                k.ball(Vec3::new(-3.6 + i as f32 * 1.0, 4.3, -7.3), 0.2, port.clone());
            }
            let cockpit = k.glowing(rgb(120, 200, 255), 2.0);
            k.ball(Vec3::new(-5.6, 4.2, -9.5), 1.5, cockpit);
            // the ramp, and the luggage
            let ramp = k.block(Vec3::new(0.0, 0.6, -5.4), Vec3::new(2.2, 0.12, 4.0), rgb(96, 102, 116));
            k.commands.entity(ramp).insert(Transform::from_xyz(0.0, 0.6, -5.4).with_rotation(Quat::from_rotation_x(0.3)));
            for i in 0..40 {
                let (col, row, tier) = (i % 5, (i / 5) % 4, i / 20);
                let leather = [rgb(120, 70, 44), rgb(70, 44, 36), rgb(148, 104, 60), rgb(52, 60, 84)][(i * 7 + tier) % 4];
                let at = Vec3::new(6.2 + col as f32 * 1.05, 0.32 + tier as f32 * 0.62, -1.5 + row as f32 * 0.8);
                if tier == 0 {
                    k.solid(at, Vec3::new(0.95, 0.6, 0.7), leather);
                } else {
                    k.block(at, Vec3::new(0.9, 0.58, 0.66), leather);
                }
            }
            // cargo cans, a gantry, the great door with the dark beyond it
            for (x, z, c) in [(-9.5, -6.0, rgb(150, 70, 50)), (-9.5, -1.0, rgb(60, 110, 120)), (-9.5, 4.0, rgb(150, 130, 60))] {
                k.solid(Vec3::new(x, 1.3, z), Vec3::new(2.6, 2.6, 4.2), c);
            }
            k.block(Vec3::new(0.0, 9.6, -2.0), Vec3::new(26.0, 0.5, 0.8), rgb(210, 160, 50));
            let space = k.glowing(rgb(10, 14, 40), 1.0);
            k.block_of(Vec3::new(0.0, 5.5, -14.9), Vec3::new(16.0, 9.0, 0.05), space);
            let star = k.glowing(rgb(255, 255, 255), 8.0);
            for _ in 0..70 {
                let at = Vec3::new(k.rand() * 15.0 - 7.5, 1.4 + k.rand() * 8.0, -14.84);
                k.block_of(at, Vec3::splat(0.035), star.clone());
            }
            for (x, z) in [(-7.0, -8.0), (7.0, -8.0), (-7.0, 4.0), (7.0, 4.0)] {
                k.lamp(Vec3::new(x, 9.0, z), rgb(200, 224, 255), 9_000_000.0, x < 0.0 && z > 0.0);
            }
            (
                Place { spawn: Vec3::new(0.9, EYE, 3.9), yaw: 0.1, min, max, solids: Vec::new(), anchors: anchors(1.0), floating: false },
                Air { sky: rgb(22, 26, 38), ambient: rgb(190, 210, 255), brightness: 260.0, fog: (35.0, 140.0) },
            )
        }
        "vista" | "vista_night" => {
            let night = name == "vista_night";
            // a terrace of the house, and beyond its balustrade the inside of the world:
            // three strips of land and three of window, rolled into a tube eight kilometres round
            let stone = rgb(216, 208, 190);
            k.block(Vec3::new(0.0, -0.25, 1.0), Vec3::new(22.0, 0.5, 14.0), stone);
            for i in 0..11 {
                for j in 0..7 {
                    if (i + j) % 2 == 0 {
                        k.block(Vec3::new(-10.0 + i as f32 * 2.0, 0.006, -5.0 + j as f32 * 2.0), Vec3::new(1.96, 0.012, 1.96), rgb(196, 186, 166));
                    }
                }
            }
            // balustrade
            k.block(Vec3::new(0.0, 1.0, -6.0), Vec3::new(22.0, 0.14, 0.4), stone);
            k.block(Vec3::new(0.0, 0.1, -6.0), Vec3::new(22.0, 0.2, 0.4), stone);
            // and round both ends of the terrace
            for x in [-10.9, 10.9] {
                k.block(Vec3::new(x, 1.0, 1.0), Vec3::new(0.4, 0.14, 14.0), stone);
                k.block(Vec3::new(x, 0.5, 1.0), Vec3::new(0.16, 0.9, 14.0), stone);
            }
            let baluster = k.matte(stone, 0.8, 0.0);
            for i in 0..45 {
                k.post(Vec3::new(-11.0 + i as f32 * 0.5, 0.55, -6.0), 0.08, 0.8, baluster.clone());
            }
            // the house wall behind, with tall lit windows; urns; a pair of clipped trees
            k.block(Vec3::new(0.0, 4.0, 8.2), Vec3::new(22.0, 8.0, 0.5), rgb(226, 214, 190));
            let pane = k.glowing(rgb(255, 214, 150), if night { 3.5 } else { 0.5 });
            for i in 0..5 {
                k.block_of(Vec3::new(-8.0 + i as f32 * 4.0, 2.6, 7.93), Vec3::new(1.6, 4.0, 0.05), pane.clone());
            }
            let leaf = k.matte(rgb(52, 110, 60), 0.9, 0.0);
            for x in [-9.0, 9.0] {
                k.solid(Vec3::new(x, 0.45, -4.3), Vec3::new(1.3, 0.9, 1.3), rgb(170, 160, 140));
                k.ball(Vec3::new(x, 1.7, -4.3), 0.75, leaf.clone());
            }
            // the world
            let radius = 620.0;
            let length = 2600.0;
            let (land_glow, window_glow) = if night { (0.03, 0.02) } else { (0.5, 1.6) };
            for strip in 0..6 {
                for part in 0..5 {
                    let a = (strip as f32 * 5.0 + part as f32 + 0.5 - 2.5) / 30.0 * TAU;
                    let centre = Vec3::new(a.sin() * radius, radius - a.cos() * radius - 40.0, -length / 2.0 + 300.0);
                    let width = TAU * radius / 30.0 + 2.0;
                    let turn = Quat::from_rotation_z(a);
                    if strip % 2 == 0 {
                        let ground = k.glowing(rgb(70, 132, 74), land_glow);
                        let e = k.block_of(centre, Vec3::new(width, 3.0, length), ground);
                        k.commands.entity(e).insert((Transform::from_translation(centre).with_rotation(turn), NotShadowCaster));
                        // fields, woods, lakes and villages, as patches on the land
                        for _ in 0..26 {
                            let (u, v) = (k.rand() - 0.5, k.rand() - 0.5);
                            let kind = (k.rand() * 5.0) as u32;
                            let (colour, glow, size) = match kind {
                                0 => (rgb(190, 180, 96), land_glow, Vec3::new(36.0, 0.6, 60.0)),
                                1 => (rgb(40, 96, 56), land_glow, Vec3::new(44.0, 6.0, 50.0)),
                                2 => (rgb(70, 130, 200), land_glow * 1.5, Vec3::new(40.0, 0.5, 90.0)),
                                3 => (rgb(120, 170, 90), land_glow, Vec3::new(50.0, 0.5, 40.0)),
                                _ => (if night { rgb(255, 210, 130) } else { rgb(214, 200, 180) }, if night { 6.0 } else { land_glow }, Vec3::new(14.0, 5.0, 18.0)),
                            };
                            let local = Vec3::new(u * (width - size.x), 1.8 + size.y / 2.0, v * (length - 100.0));
                            let m = k.glowing(colour, glow);
                            let e = k.block_of(centre + turn * local, size, m);
                            k.commands.entity(e).insert((Transform::from_translation(centre + turn * local).with_rotation(turn), NotShadowCaster));
                        }
                    } else {
                        let sky = k.glowing(if night { rgb(8, 10, 30) } else { rgb(150, 196, 255) }, window_glow);
                        let e = k.block_of(centre, Vec3::new(width, 2.0, length), sky);
                        k.commands.entity(e).insert((Transform::from_translation(centre).with_rotation(turn), NotShadowCaster));
                    }
                }
            }
            // the far end cap, and the sunline down the axis
            let cap = k.glowing(if night { rgb(14, 18, 40) } else { rgb(150, 190, 240) }, if night { 0.3 } else { 1.1 });
            let disc = k.post(Vec3::new(0.0, radius - 40.0, -length + 300.0), radius + 10.0, 4.0, cap);
            k.commands.entity(disc).insert((Transform::from_xyz(0.0, radius - 40.0, -length + 300.0).with_rotation(Quat::from_rotation_x(PI / 2.0)), NotShadowCaster));
            let line = k.glowing(if night { rgb(120, 140, 220) } else { rgb(255, 246, 220) }, if night { 2.0 } else { 30.0 });
            let sunline = k.post(Vec3::new(0.0, radius - 40.0, -length / 2.0 + 300.0), if night { 2.5 } else { 7.0 }, length, line);
            k.commands.entity(sunline).insert((Transform::from_xyz(0.0, radius - 40.0, -length / 2.0 + 300.0).with_rotation(Quat::from_rotation_x(PI / 2.0)), NotShadowCaster));
            if night {
                k.sun(Vec3::new(-2.0, 10.0, -4.0), rgb(150, 170, 255), 500.0, true);
                for x in [-6.0, 0.0, 6.0] {
                    k.lamp(Vec3::new(x, 2.6, 6.6), rgb(255, 200, 130), 700_000.0, x == 0.0);
                }
            } else {
                k.sun(Vec3::new(1.5, 10.0, -3.0), rgb(255, 246, 226), 11_000.0, true);
            }
            (
                Place { spawn: Vec3::new(0.8, EYE, -0.3), yaw: 0.08, min: Vec2::new(-10.6, -5.5), max: Vec2::new(10.6, 7.7), solids: Vec::new(), anchors: anchors(-3.2), floating: false },
                if night {
                    Air { sky: rgb(6, 8, 22), ambient: rgb(150, 170, 255), brightness: 120.0, fog: (700.0, 3600.0) }
                } else {
                    Air { sky: rgb(168, 206, 250), ambient: rgb(220, 234, 255), brightness: 700.0, fog: (300.0, 2700.0) }
                },
            )
        }
        "ballroom" => {
            // the long gallery on a gala night: chequered marble, gilt columns, chandeliers
            let (min, max) = k.room(20.0, 30.0, 9.0, rgb(232, 226, 214), rgb(226, 208, 176), rgb(240, 232, 214));
            let dark = k.matte(rgb(60, 52, 60), 0.12, 0.1);
            for i in 0..10 {
                for j in 0..15 {
                    if (i + j) % 2 == 0 {
                        k.block_of(Vec3::new(-9.0 + i as f32 * 2.0, 0.006, -14.0 + j as f32 * 2.0), Vec3::new(2.0, 0.012, 2.0), dark.clone());
                    }
                }
            }
            let gilt = k.matte(rgb(226, 186, 96), 0.25, 1.0);
            let marble = k.matte(rgb(240, 234, 224), 0.3, 0.0);
            for j in 0..6 {
                for x in [-8.6, 8.6] {
                    let z = -12.5 + j as f32 * 5.0;
                    k.solids.push((Vec2::new(x, z), Vec2::splat(0.45)));
                    k.post(Vec3::new(x, 4.5, z), 0.38, 9.0, marble.clone());
                    k.post(Vec3::new(x, 0.25, z), 0.55, 0.5, gilt.clone());
                    k.post(Vec3::new(x, 8.7, z), 0.55, 0.5, gilt.clone());
                }
            }
            // tall windows down one side, looking out on the dark and the fleet
            let glass = k.glowing(rgb(16, 22, 60), 1.0);
            let hullglow = k.glowing(rgb(200, 214, 255), 4.0);
            for j in 0..5 {
                let z = -10.0 + j as f32 * 5.0;
                k.block_of(Vec3::new(-9.93, 4.6, z), Vec3::new(0.05, 7.0, 3.0), glass.clone());
                k.block(Vec3::new(-9.9, 8.3, z), Vec3::new(0.12, 0.4, 3.4), rgb(140, 40, 50));
                for _ in 0..5 {
                    let at = Vec3::new(-9.88, 2.0 + k.rand() * 5.5, z - 1.3 + k.rand() * 2.6);
                    k.block_of(at, Vec3::new(0.02, 0.05, 0.16), hullglow.clone());
                }
            }
            // mirrors opposite, the dais and the orchestra's chairs at the far end
            let mirror = k.matte(rgb(200, 210, 220), 0.03, 1.0);
            for j in 0..5 {
                k.block_of(Vec3::new(9.93, 4.0, -10.0 + j as f32 * 5.0), Vec3::new(0.05, 5.0, 2.6), mirror.clone());
            }
            k.solid(Vec3::new(0.0, 0.2, -13.0), Vec3::new(9.0, 0.4, 3.0), rgb(120, 34, 44));
            for i in 0..7 {
                k.block(Vec3::new(-3.0 + i as f32, 0.85, -13.4), Vec3::new(0.45, 0.9, 0.45), rgb(226, 186, 96));
            }
            // supper tables along the mirrors
            for z in [-4.0, 2.0, 8.0] {
                k.table(Vec3::new(6.6, 0.0, z), Vec3::new(1.4, 0.8, 3.2), rgb(244, 240, 232));
                for i in 0..4 {
                    k.post(Vec3::new(6.6, 0.95, z - 1.1 + i as f32 * 0.75), 0.05, 0.3, gilt.clone());
                }
            }
            // chandeliers
            let crystal = k.glowing(rgb(255, 236, 190), 9.0);
            for z in [-9.0, -1.0, 7.0] {
                k.post(Vec3::new(0.0, 8.0, z), 0.04, 2.0, gilt.clone());
                for ring in 0..2 {
                    let (r, y, n) = if ring == 0 { (1.3, 6.6, 12) } else { (0.7, 7.1, 8) };
                    for i in 0..n {
                        let a = i as f32 / n as f32 * TAU;
                        let e = k.ball(Vec3::new(a.sin() * r, y, z + a.cos() * r), 0.09, crystal.clone());
                        k.commands.entity(e).insert(NotShadowCaster);
                    }
                }
                k.commands.spawn((PointLight { color: rgb(255, 226, 170), intensity: 2_600_000.0, range: 40.0, shadows_enabled: z == -1.0, ..default() }, Transform::from_xyz(0.0, 6.2, z), PlacePart));
            }
            (
                Place { spawn: Vec3::new(0.9, EYE, 2.4), yaw: 0.1, min, max, solids: Vec::new(), anchors: anchors(-0.6), floating: false },
                Air { sky: rgb(20, 14, 12), ambient: rgb(255, 232, 196), brightness: 240.0, fog: (40.0, 160.0) },
            )
        }
        _ => {
            // the study: panelling, books to the ceiling, a desk under a green lamp, a fire
            let (min, max) = k.room(8.0, 9.0, 3.6, rgb(74, 48, 34), rgb(92, 60, 40), rgb(60, 42, 32));
            k.block(Vec3::new(0.0, 0.01, 0.2), Vec3::new(4.6, 0.02, 5.6), rgb(120, 36, 44));
            k.block(Vec3::new(0.0, 0.016, 0.2), Vec3::new(4.0, 0.02, 5.0), rgb(150, 52, 54));
            // bookcases on the left and far walls
            let spines = [rgb(120, 40, 40), rgb(40, 70, 110), rgb(50, 96, 64), rgb(150, 120, 60), rgb(90, 60, 100), rgb(170, 150, 120)];
            for (wall, n) in [(0, 9), (1, 6)] {
                for shelf in 0..6 {
                    let y = 0.35 + shelf as f32 * 0.52;
                    let mut u = 0.0;
                    while u < n as f32 * 0.9 {
                        let w = 0.05 + k.rand() * 0.07;
                        let h = 0.3 + k.rand() * 0.14;
                        let c = spines[(k.rand() * 6.0) as usize % 6];
                        let at = if wall == 0 { Vec3::new(-3.78, y + h / 2.0, -4.0 + u) } else { Vec3::new(-2.6 + u, y + h / 2.0, -4.28) };
                        let size = if wall == 0 { Vec3::new(0.24, h, w) } else { Vec3::new(w, h, 0.24) };
                        k.block(at, size, c);
                        u += w + 0.006;
                    }
                    let at = if wall == 0 { Vec3::new(-3.75, y - 0.02, 0.05) } else { Vec3::new(0.1, y - 0.02, -4.25) };
                    let size = if wall == 0 { Vec3::new(0.34, 0.04, 8.2) } else { Vec3::new(5.6, 0.04, 0.34) };
                    k.block(at, size, rgb(60, 38, 26));
                }
            }
            k.solids.push((Vec2::new(-3.75, 0.0), Vec2::new(0.25, 4.5)));
            k.solids.push((Vec2::new(0.0, -4.25), Vec2::new(4.0, 0.25)));
            // the desk, its lamp, a globe, two chairs
            k.table(Vec3::new(0.6, 0.0, -2.2), Vec3::new(2.4, 0.8, 1.1), rgb(70, 40, 26));
            k.block(Vec3::new(0.6, 0.82, -2.2), Vec3::new(1.5, 0.01, 0.7), rgb(40, 84, 60));
            let brass = k.matte(rgb(214, 176, 90), 0.3, 1.0);
            k.post(Vec3::new(1.4, 1.0, -2.3), 0.025, 0.4, brass.clone());
            let shade = k.glowing(rgb(70, 190, 120), 2.0);
            k.post(Vec3::new(1.4, 1.22, -2.3), 0.16, 0.1, shade);
            k.commands.spawn((PointLight { color: rgb(255, 232, 180), intensity: 160_000.0, range: 12.0, shadows_enabled: true, ..default() }, Transform::from_xyz(1.4, 1.1, -2.0), PlacePart));
            let sea = k.matte(rgb(60, 110, 170), 0.4, 0.0);
            k.ball(Vec3::new(-0.3, 1.03, -2.3), 0.2, sea);
            for x in [-1.1, 1.9] {
                k.solid(Vec3::new(x, 0.45, 0.6), Vec3::new(0.8, 0.9, 0.8), rgb(110, 40, 44));
            }
            // the fireplace on the right, a portrait above it, a window behind you
            k.solid(Vec3::new(3.75, 0.9, -0.5), Vec3::new(0.5, 1.8, 2.4), rgb(150, 144, 136));
            k.block(Vec3::new(3.5, 0.55, -0.5), Vec3::new(0.06, 0.9, 1.2), rgb(16, 12, 12));
            let fire = k.glowing(rgb(255, 130, 40), 7.0);
            k.block_of(Vec3::new(3.44, 0.3, -0.5), Vec3::new(0.05, 0.36, 0.8), fire);
            k.commands.spawn((PointLight { color: rgb(255, 140, 60), intensity: 240_000.0, range: 14.0, shadows_enabled: false, ..default() }, Transform::from_xyz(3.0, 0.5, -0.5), PlacePart));
            k.block(Vec3::new(3.94, 2.5, -0.5), Vec3::new(0.06, 1.2, 0.95), rgb(214, 176, 90));
            k.block(Vec3::new(3.9, 2.5, -0.5), Vec3::new(0.06, 1.05, 0.8), rgb(50, 60, 80));
            let dusk = k.glowing(rgb(90, 120, 200), 1.6);
            k.block_of(Vec3::new(1.0, 2.0, 4.44), Vec3::new(2.2, 2.0, 0.05), dusk);
            k.lamp(Vec3::new(-1.6, 3.0, 1.0), rgb(255, 214, 160), 220_000.0, false);
            (
                Place { spawn: Vec3::new(0.3, EYE, 2.0), yaw: 0.0, min, max, solids: Vec::new(), anchors: [Vec3::new(-1.3, 0.0, -0.8), Vec3::new(0.1, 0.0, -1.15), Vec3::new(1.5, 0.0, -0.8)], floating: false },
                Air { sky: rgb(14, 10, 8), ambient: rgb(255, 220, 180), brightness: 110.0, fog: (25.0, 80.0) },
            )
        }
    };
    let mut place = place;
    place.solids = std::mem::take(&mut k.solids);
    // nobody starts inside the furniture
    let at = walk(&place, Vec2::new(place.spawn.x, place.spawn.z), Vec2::ZERO);
    place.spawn = Vec3::new(at.x, place.spawn.y, at.y);
    (place, air)
}

/// The world in a headset, through OpenXR.
///
/// The headset's viewpoint is carried by a tracking root that stands where
/// the player stands: walking moves the root, and within it you are free to
/// look and lean as the headset tracks you. The story's text hangs in the
/// air a couple of metres ahead as a panel. Places left as paintings, the
/// title and the chapter cards are shown on that same panel, in the dark.
///
///   left stick             walk, where you are looking
///   right stick, sideways  turn in steps
///   right stick, up/down   pick a choice
///   either trigger         advance, confirm
///
/// The keyboard and mouse still work, and the window shows what the game
/// shows on a desktop.
#[cfg(feature = "vr")]
mod vr {
    use super::*;
    use bevy::render::pipelined_rendering::PipelinedRenderingPlugin;
    use bevy::render::view::RenderLayers;
    use bevy_mod_openxr::{add_xr_plugins, helper_traits::{ToQuat, ToVec3}, resources::OxrViews};
    use bevy_mod_xr::{camera::XrCamera, session::XrTrackingRoot};
    use bevy_xr_utils::xr_utils_actions::{ActiveSet, XRUtilsAction, XRUtilsActionSet, XRUtilsActionState, XRUtilsActionSystemSet, XRUtilsActionsPlugin, XRUtilsBinding};

    use bevy::window::PresentMode;
    use bevy_mod_xr::actions::ActionType;
    use bevy_mod_xr::session::XrState;

    /// What a controller input is for.
    #[derive(Component, Clone, Copy, PartialEq)]
    enum Use {
        Walk,
        Look,
        Advance,
        /// A trackpad counts only while it is pressed: a thumb resting on it is not a push.
        WalkPad,
        WalkPadPressed,
        LookPad,
        LookPadPressed,
    }
    #[derive(Component)]
    struct Panel;

    /// How far ahead the text panel hangs, how wide it is and how high its middle is, in metres.
    const PANEL: (f32, f32, f32) = (2.0, 2.3, 1.72);
    const SNAP: f32 = PI / 6.0;

    pub fn plugins(app: &mut App, mut window: WindowPlugin) {
        // the headset sets the pace: the desktop window must not make it wait for the monitor
        if let Some(w) = &mut window.primary_window {
            w.present_mode = PresentMode::AutoNoVsync;
        }
        // without a runtime or a headset this falls back to ordinary rendering by itself
        app.add_plugins(add_xr_plugins(DefaultPlugins.build().disable::<PipelinedRenderingPlugin>()).set(window))
            .add_plugins(XRUtilsActionsPlugin)
            .add_systems(Startup, actions.before(XRUtilsActionSystemSet::CreateEvents))
            .add_systems(Update, (read.before(Story), carry.after(Story)));
    }

    fn actions(mut commands: Commands) {
        const STICKS: [&str; 3] = ["/interaction_profiles/oculus/touch_controller", "/interaction_profiles/valve/index_controller", "/interaction_profiles/microsoft/motion_controller"];
        const VIVE: &str = "/interaction_profiles/htc/vive_controller";
        let set = commands.spawn((XRUtilsActionSet { name: "story".into(), pretty_name: "Story and walking".into(), priority: 0 }, ActiveSet)).id();
        let mut make = |name: &str, label: &str, kind: ActionType, what: Use, bindings: Vec<(&'static str, String)>| {
            let action = commands.spawn((XRUtilsAction { action_name: name.to_string().into(), localized_name: label.to_string().into(), action_type: kind }, what)).id();
            for (profile, binding) in bindings {
                let b = commands.spawn(XRUtilsBinding { profile: profile.into(), binding: binding.into() }).id();
                commands.entity(action).add_child(b);
            }
            commands.entity(set).add_child(action);
        };
        let sticks = |hand: &str| STICKS.iter().map(|p| (*p, format!("/user/hand/{}/input/thumbstick", hand))).collect::<Vec<_>>();
        let mut triggers: Vec<(&'static str, String)> = Vec::new();
        for hand in ["left", "right"] {
            for profile in STICKS.iter().chain([&VIVE]) {
                triggers.push((*profile, format!("/user/hand/{}/input/trigger/value", hand)));
            }
            triggers.push(("/interaction_profiles/khr/simple_controller", format!("/user/hand/{}/input/select/click", hand)));
        }
        make("walk", "Walk", ActionType::Vector, Use::Walk, sticks("left"));
        make("look", "Turn, and pick a choice", ActionType::Vector, Use::Look, sticks("right"));
        make("advance", "Advance the story", ActionType::Float, Use::Advance, triggers);
        make("walk_pad", "Walk (trackpad)", ActionType::Vector, Use::WalkPad, vec![(VIVE, "/user/hand/left/input/trackpad".into())]);
        make("walk_pad_pressed", "Walk (press the trackpad)", ActionType::Bool, Use::WalkPadPressed, vec![(VIVE, "/user/hand/left/input/trackpad/click".into())]);
        make("look_pad", "Turn, and pick a choice (trackpad)", ActionType::Vector, Use::LookPad, vec![(VIVE, "/user/hand/right/input/trackpad".into())]);
        make("look_pad_pressed", "Turn, and pick (press the trackpad)", ActionType::Bool, Use::LookPadPressed, vec![(VIVE, "/user/hand/right/input/trackpad/click".into())]);
    }

    /// One push counts once: it fires on passing `high`, and not again until it has come back under `low`.
    fn once(armed: &mut bool, value: f32, high: f32, low: f32) -> bool {
        if value < low {
            *armed = true;
        }
        let fire = *armed && value > high;
        if fire {
            *armed = false;
        }
        fire
    }

    /// Controllers and head pose, into what the story's frame understands.
    fn read(
        mut headset: ResMut<Headset>,
        state: Option<Res<XrState>>,
        views: Option<Res<OxrViews>>,
        root: Query<&GlobalTransform, With<XrTrackingRoot>>,
        inputs: Query<(&Use, &XRUtilsActionState)>,
        mut armed: Local<[bool; 5]>,
        mut last: Local<Option<Vec3>>,
    ) {
        *headset = Headset::default();
        // only while a session is really running: what the runtime last said is not news after it has stopped
        let running = state.map_or(false, |s| *s == XrState::Running);
        let Some(view) = views.as_ref().filter(|_| running).and_then(|v| v.first().copied()) else {
            // (nothing is held over to a later session; a trigger already down then must be let go first)
            *armed = [false; 5];
            *last = None;
            return;
        };
        headset.active = true;
        // which way the head faces, flattened to the floor
        let ahead = view.pose.orientation.to_quat() * Vec3::NEG_Z;
        headset.heading = if ahead.xz().length() > 0.2 { (-ahead.x).atan2(-ahead.z) } else { 0.0 };
        let local = view.pose.position.to_vec3();
        headset.local = local;
        // real steps; a jump (the runtime recentring) is not one
        let moved = last.map_or(Vec2::ZERO, |l| (local - l).xz());
        headset.stepped = if moved.length() < 0.5 { moved } else { Vec2::ZERO };
        *last = Some(local);
        if let Ok(root) = root.single() {
            headset.eye = Some(root.transform_point(local));
        }
        let vector = |what: Use| inputs.iter().find(|(u, _)| **u == what).and_then(|(_, s)| if let XRUtilsActionState::Vector(v) = s { Some(Vec2::from(v.current_state)) } else { None }).unwrap_or_default();
        let level = |what: Use| {
            inputs.iter().find(|(u, _)| **u == what).map_or(0.0, |(_, s)| match s {
                XRUtilsActionState::Float(f) => f.current_state,
                XRUtilsActionState::Bool(b) => b.current_state as u8 as f32,
                XRUtilsActionState::Vector(_) => 0.0,
            })
        };
        // a little dead space in the middle of each stick; a trackpad only while pressed
        let dead = |v: Vec2| if v.length() < 0.2 { Vec2::ZERO } else { v };
        let pad = |pad: Use, pressed: Use| if level(pressed) > 0.5 { vector(pad) } else { Vec2::ZERO };
        headset.stick = dead(vector(Use::Walk) + pad(Use::WalkPad, Use::WalkPadPressed)).clamp_length_max(1.0);
        let look = vector(Use::Look) + pad(Use::LookPad, Use::LookPadPressed);
        // sideways turns, up and down picks: whichever the push is mostly
        let (side, updown) = if look.x.abs() >= look.y.abs() { (look.x, 0.0) } else { (0.0, look.y) };
        if once(&mut armed[0], side, 0.7, 0.35) {
            headset.turn = -SNAP;
        }
        if once(&mut armed[1], -side, 0.7, 0.35) {
            headset.turn = SNAP;
        }
        headset.up = once(&mut armed[2], updown, 0.7, 0.35);
        headset.down = once(&mut armed[3], -updown, 0.7, 0.35);
        headset.advance = once(&mut armed[4], level(Use::Advance), 0.6, 0.3);
    }

    /// How far the floor runs on from `from` towards `yaw`, in metres.
    fn clear_ahead(floor: (Vec2, Vec2), from: Vec2, yaw: f32) -> f32 {
        let dir = Vec2::new(-yaw.sin(), -yaw.cos());
        let gaps = [(floor.1.x - from.x, dir.x), (from.x - floor.0.x, -dir.x), (floor.1.y - from.y, dir.y), (from.y - floor.0.y, -dir.y)];
        gaps.iter().filter(|(_, along)| *along > 1e-3).map(|(gap, along)| (gap / along).max(0.0)).fold(f32::MAX, f32::min)
    }

    /// Put the headset's viewpoint where the player stands, give its eyes
    /// the place's air, and hang the story's text in front of them.
    #[allow(clippy::type_complexity)]
    fn carry(
        mut commands: Commands,
        body: Res<Body>,
        headset: Res<Headset>,
        mut meshes: ResMut<Assets<Mesh>>,
        mut materials: ResMut<Assets<StandardMaterial>>,
        mut root: Query<&mut Transform, (With<XrTrackingRoot>, Without<Panel>)>,
        eyes: Query<(Entity, Option<&DistanceFog>, Has<RenderLayers>), With<XrCamera>>,
        frame: Query<&Sprite, With<Overlay>>,
        mut panel: Query<(&mut Transform, &mut Visibility, &MeshMaterial3d<StandardMaterial>), (With<Panel>, Without<XrTrackingRoot>)>,
        mut panel_yaw: Local<Option<f32>>,
        mut was_in_place: Local<bool>,
    ) {
        // The player's position is where their head is over the floor. The tracked
        // space is placed so that the head, wherever it is in the real room, comes
        // out there: turning then pivots about you, not about the middle of the room.
        let turn = Quat::from_rotation_y(body.yaw);
        let dark = Vec3::new(0.0, -1000.0, 0.0);
        if let Ok(mut root) = root.single_mut() {
            if body.in_place {
                root.translation = body.feet - turn * Vec3::new(headset.local.x, 0.0, headset.local.z);
                root.rotation = turn;
            } else if *was_in_place || root.translation.y > -500.0 {
                // nowhere in particular: a dark room with the story on the wall (set once: you may move about in it)
                root.translation = dark;
                root.rotation = Quat::IDENTITY;
            }
        }
        *was_in_place = body.in_place;
        for (eye, fog, layered) in &eyes {
            // the text panel is for the headset's eyes only (the window has the frame itself)
            if !layered {
                commands.entity(eye).insert(RenderLayers::from_layers(&[0, 1]));
            }
            let wanted = body.fog.map(|(colour, start, end)| DistanceFog { color: colour, falloff: FogFalloff::Linear { start, end }, ..default() });
            match (wanted, fog) {
                (Some(w), Some(f)) if f.color == w.color => {}
                (Some(w), _) => {
                    commands.entity(eye).insert(w);
                }
                (None, Some(_)) => {
                    commands.entity(eye).remove::<DistanceFog>();
                }
                (None, None) => {}
            }
        }
        // the panel: made once the story's frame exists
        let Ok(sprite) = frame.single() else { return };
        if panel.is_empty() {
            commands.spawn((
                Mesh3d(meshes.add(Rectangle::new(1.0, H as f32 / W as f32))),
                MeshMaterial3d(materials.add(StandardMaterial { base_color_texture: Some(sprite.image.clone()), unlit: true, alpha_mode: AlphaMode::Blend, cull_mode: None::<Face>, fog_enabled: false, ..default() })),
                Transform::default(),
                Visibility::Hidden,
                NotShadowCaster,
                RenderLayers::layer(1),
                Panel,
            ));
            return;
        }
        let Ok((mut tf, mut vis, material)) = panel.single_mut() else { return };
        // a material is not told when its picture changes: say so
        if body.redrawn {
            materials.get_mut(&material.0);
        }
        if !headset.active {
            *vis = Visibility::Hidden;
            return;
        }
        *vis = Visibility::Visible;
        let (floor_y, base_yaw) = if body.in_place { (body.feet.y, body.yaw) } else { (dark.y, 0.0) };
        let head = headset.eye.unwrap_or(Vec3::new(body.feet.x, floor_y + EYE, body.feet.z));
        // it stays put while you look about, and comes round once you have clearly turned away from it
        let facing = base_yaw + headset.heading;
        let yaw = panel_yaw.get_or_insert(facing);
        let off = (facing - *yaw + PI).rem_euclid(TAU) - PI;
        if off.abs() > 0.9 {
            *yaw = facing;
        }
        // nearer (and smaller, so it looks the same) where a wall is close in that direction
        let distance = if body.in_place { PANEL.0.min(clear_ahead(body.floor, head.xz(), *yaw) - 0.15).max(0.5) } else { PANEL.0 };
        let size = PANEL.1 * distance / PANEL.0 * if body.in_place { 1.0 } else { 1.5 };
        let ahead = Vec3::new(-yaw.sin(), 0.0, -yaw.cos());
        tf.translation = Vec3::new(head.x, floor_y + PANEL.2 * distance / PANEL.0 + (1.0 - distance / PANEL.0) * EYE, head.z) + ahead * distance;
        tf.rotation = Quat::from_rotation_y(*yaw);
        tf.scale = Vec3::splat(size);
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn a_slow_pull_counts_once_and_a_held_one_does_not_repeat() {
            let mut armed = false;
            // down already when the session starts: nothing until it has been let go
            assert!(!once(&mut armed, 1.0, 0.6, 0.3));
            assert!(!once(&mut armed, 0.1, 0.6, 0.3));
            // a pull that takes several frames to get there
            let fired: Vec<bool> = [0.2, 0.45, 0.7, 1.0, 1.0, 0.5, 0.9, 0.2, 0.8].iter().map(|v| once(&mut armed, *v, 0.6, 0.3)).collect();
            assert_eq!(fired, [false, false, true, false, false, false, false, false, true]);
        }

        #[test]
        fn the_panel_keeps_clear_of_the_wall_you_face() {
            let floor = (Vec2::splat(-4.0), Vec2::splat(4.0));
            // facing -z from the middle, then from near that wall, then along it
            assert!((clear_ahead(floor, Vec2::ZERO, 0.0) - 4.0).abs() < 1e-4);
            assert!((clear_ahead(floor, Vec2::new(0.0, -3.4), 0.0) - 0.6).abs() < 1e-4);
            assert!((clear_ahead(floor, Vec2::new(0.0, -3.4), PI / 2.0) - 4.0).abs() < 1e-3);
            // outside the floor (leaning through a wall): never negative
            assert!(clear_ahead(floor, Vec2::new(0.0, -4.5), 0.0) >= 0.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn walking_stays_inside_and_goes_round_things() {
        let place = Place { spawn: Vec3::ZERO, yaw: 0.0, min: Vec2::splat(-4.0), max: Vec2::splat(4.0), solids: vec![(Vec2::ZERO, Vec2::new(1.0, 0.5))], anchors: [Vec3::ZERO; 3], floating: false };
        // walls
        let at = walk(&place, Vec2::new(3.5, 0.0), Vec2::new(5.0, 0.0));
        assert!((at.x - (4.0 - BODY)).abs() < 1e-4);
        // straight at the table from the front: stopped at its edge, not inside it
        let mut at = Vec2::new(0.0, 3.0);
        for _ in 0..200 {
            at = walk(&place, at, Vec2::new(0.0, -0.05));
        }
        assert!((at.y - (0.5 + BODY)).abs() < 1e-3, "{:?}", at);
        // at an angle: slides along it and gets past
        let mut at = Vec2::new(0.9, 3.0);
        for _ in 0..400 {
            at = walk(&place, at, Vec2::new(0.03, -0.05));
        }
        assert!(at.y < -0.5, "{:?}", at);
    }

    #[test]
    fn every_place_the_game_paints_is_built() {
        for place in art::BACKGROUNDS {
            assert!(PLACES.contains(place), "{} has no 3D build", place);
        }
    }
}
