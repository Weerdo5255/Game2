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
#[cfg(feature = "vr")]
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
use crate::places::{self, PlaceData, EYE};
use crate::script::CharDef;
use std::path::PathBuf;

/// Can this place be shown in 3D: has the story, or the game, a build of it?
fn has_build(world: Option<&std::path::Path>, place: &str) -> bool {
    world.map_or(false, |w| w.join(format!("{}.json", place)).is_file()) || places::builtin(place).is_some()
}

pub struct Options {
    pub title: String,
    pub size: (usize, usize),
    /// Take one picture of the window once this many lines have been
    /// shown, then quit (for documentation and tests).
    pub shot: Option<(String, usize)>,
    /// Show the world in a VR headset as well (needs the `vr` feature).
    pub vr: bool,
    /// The story's `world` folder: its own places, and models.
    pub world_dir: Option<PathBuf>,
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


/// Everything that is not Bevy's: the story and the frames it draws.
struct Runner {
    game: Game,
    black: Canvas,
    white: Canvas,
    place: Option<Stage>,
    /// The story's own places and models, if it has any.
    world_dir: Option<PathBuf>,
    /// The figure or model standing in each place of the cast: who, how, and with what expression.
    figures: [Option<(String, String, String)>; 3],
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

/// The place being shown: its description, and what stands in the way in it.
struct Stage {
    data: PlaceData,
    obstacles: Vec<([f32; 2], [f32; 2])>,
}

/// The system that steps the story and the world, for others to run before or after.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
struct Story;

#[derive(Component)]
struct PlacePart;
/// Where a 3D figure or model of someone in the cast stands.
#[derive(Component)]
struct FigureSlot(usize);
/// A piece of such a figure.
#[derive(Component)]
struct FigurePart;
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
        world_dir: opts.world_dir.clone(),
        figures: [None, None, None],
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
    // model files are read from the story's own world folder
    let files = AssetPlugin { file_path: opts.world_dir.as_ref().map_or("assets".to_string(), |d| d.to_string_lossy().to_string()), ..default() };
    let mut app = App::new();
    #[cfg_attr(not(feature = "vr"), allow(unused_mut))]
    let mut window = Some((window, files));
    #[cfg(feature = "vr")]
    if opts.vr {
        let (window, files) = window.take().unwrap();
        vr::plugins(&mut app, window, files);
    }
    #[cfg(not(feature = "vr"))]
    if opts.vr {
        eprintln!("--vr needs a build with the VR feature (cargo build --features vr): showing the world on the desktop");
    }
    if let Some((window, files)) = window {
        app.add_plugins(DefaultPlugins.set(window).set(files));
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
        commands.spawn((Transform::default(), Visibility::Hidden, FigureSlot(i)));
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
    (keys, buttons, motion, time, headset, mut body, assets): (Res<ButtonInput<KeyCode>>, Res<ButtonInput<MouseButton>>, Res<AccumulatedMouseMotion>, Res<Time>, Res<Headset>, ResMut<Body>, Res<AssetServer>),
    window: Single<&Window, With<PrimaryWindow>>,
    mut eye: Single<(&mut Transform, &mut DistanceFog, &mut Camera), (With<Eye>, Without<CastSlot>, Without<Overlay>, Without<FigureSlot>)>,
    mut cast: Query<(&CastSlot, &mut Transform, &mut Visibility, &MeshMaterial3d<StandardMaterial>), (Without<Eye>, Without<Overlay>, Without<FigureSlot>)>,
    mut figures: Query<(&FigureSlot, Entity, &mut Transform, &mut Visibility, Option<&Children>), (Without<Eye>, Without<Overlay>, Without<CastSlot>)>,
    mut overlay: Single<(&mut Sprite, &mut Transform), (With<Overlay>, Without<Eye>, Without<CastSlot>, Without<FigureSlot>)>,
    parts: Query<Entity, With<PlacePart>>,
    mut exit: EventWriter<AppExit>,
) {
    let run = &mut *run;
    let dt = time.delta_secs().min(0.1);
    let (ww, wh) = (window.width().max(1.0), window.height().max(1.0));
    let scale = (ww / W as f32).min(wh / H as f32).max(0.01);
    let in_world = run.game.world_view().map_or(false, |v| has_build(run.world_dir.as_deref(), &v.place));

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
        ledger: pressed(&[KeyCode::KeyI]),
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
    let view = run.game.world_view().filter(|v| has_build(run.world_dir.as_deref(), &v.place));
    let place_name = view.as_ref().map_or("", |v| v.place.as_str());
    if place_name != run.built {
        for e in &parts {
            commands.entity(e).despawn();
        }
        run.built = place_name.to_string();
        run.place = None;
        clear.0 = Color::BLACK;
        body.fog = None;
        if let Some(data) = Some(place_name).filter(|n| !n.is_empty()).and_then(|n| places::load(run.world_dir.as_deref(), n)) {
            raise(&data, &mut commands, &mut meshes, &mut materials, &assets);
            let sky = colour(&data.air.sky);
            ambient.color = colour(&data.air.ambient);
            ambient.brightness = data.air.brightness;
            clear.0 = sky;
            eye.1.color = sky;
            eye.1.falloff = FogFalloff::Linear { start: data.air.fog[0], end: data.air.fog[1] };
            body.fog = Some((sky, data.air.fog[0], data.air.fog[1]));
            // nobody starts inside the furniture
            let obstacles = data.obstacles();
            let start = data.walk(&obstacles, [data.spawn[0], data.spawn[2]], [0.0, 0.0]);
            eye.0.translation = Vec3::new(start[0], data.spawn[1] + EYE, start[1]);
            run.yaw = data.yaw.to_radians();
            run.pitch = 0.0;
            run.place = Some(Stage { data, obstacles });
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
        let step = (keys_step.normalize_or_zero() * speed + stick_step) * dt + real_step;
        let to = Vec2::from(place.data.walk(&place.obstacles, [at.x, at.y], [step.x, step.y]));
        let (floor_min, floor_max) = (Vec2::from(place.data.floor[0]), Vec2::from(place.data.floor[1]));
        let feet_y = place.data.spawn[1];
        // (no swaying in a headset: the world should move only when you do)
        let bob = if place.data.floating && !headset.active { (time.elapsed_secs() * 0.9).sin() * 0.06 } else { 0.0 };
        eye.0.translation = Vec3::new(to.x, feet_y + EYE + bob, to.y);
        eye.0.rotation = Quat::from_euler(EulerRot::YXZ, run.yaw, run.pitch, 0.0);
        body.feet = Vec3::new(to.x, feet_y, to.y);
        body.yaw = run.yaw;
        // how far the floor runs on ahead, to keep the text panel out of the walls
        let ahead = [(floor_max.x - to.x, forward.x), (to.x - floor_min.x, -forward.x), (floor_max.y - to.y, forward.y), (to.y - floor_min.y, -forward.y)];
        body.floor = (floor_min, floor_max);
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
        // (someone with a 3D figure or model is not also a cut-out)
        let modelled = wanted[i].as_ref().map_or(false, |(who, ..)| run.game.character(who).map_or(false, |c| !c.model.is_empty()));
        let (Some((who, expr, appear, dim)), Some(place), false) = (&wanted[i], &run.place, modelled) else {
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
        let foot = v3(place.data.anchors[i]);
        let drift = if place.data.floating { (time.elapsed_secs() * 1.3 + i as f32 * 1.7).sin() * 0.09 } else { 0.0 };
        tf.translation = foot + Vec3::Y * (CAST_LIFT + CAST_HEIGHT / 2.0 + drift);
        tf.rotation = Quat::from_rotation_y((eye_at.x - foot.x).atan2(eye_at.z - foot.z));
        *vis = Visibility::Visible;
    }

    // ---- and those who are there as 3D figures or models
    for (slot, entity, mut tf, mut vis, children) in &mut figures {
        let i = slot.0;
        let def = wanted[i].as_ref().and_then(|(who, ..)| run.game.character(who)).filter(|c| !c.model.is_empty()).cloned();
        let (Some((who, expr, appear, _)), Some(place), Some(def)) = (&wanted[i], &run.place, def) else {
            if run.figures[i].take().is_some() {
                for child in children.iter().flat_map(|c| c.iter()) {
                    commands.entity(child).despawn();
                }
            }
            *vis = Visibility::Hidden;
            continue;
        };
        // a model file keeps its own face; a figure is made again for each expression
        let key = (who.clone(), def.model.clone(), if def.model == "figure" { expr.clone() } else { String::new() });
        if run.figures[i].as_ref() != Some(&key) {
            for child in children.iter().flat_map(|c| c.iter()) {
                commands.entity(child).despawn();
            }
            if def.model == "figure" {
                figure(&def, expr, entity, &mut commands, &mut meshes, &mut materials);
            } else {
                let scale = if def.model_scale > 0.0 { def.model_scale } else { 1.0 };
                let scene = assets.load(GltfAssetLabel::Scene(0).from_asset(format!("models/{}", def.model)));
                let child = commands.spawn((SceneRoot(scene), Transform::from_scale(Vec3::splat(scale)), FigurePart)).id();
                commands.entity(entity).add_child(child);
            }
            run.figures[i] = Some(key);
        }
        let foot = v3(place.data.anchors[i]);
        let drift = if place.data.floating { (time.elapsed_secs() * 1.3 + i as f32 * 1.7).sin() * 0.09 } else { 0.0 };
        // they rise into place as they arrive, and turn to you
        let grown = 0.85 + 0.15 * appear.clamp(0.0, 1.0);
        tf.translation = foot + Vec3::Y * (drift - (1.0 - appear.clamp(0.0, 1.0)) * 0.25);
        tf.rotation = Quat::from_rotation_y((eye_at.x - foot.x).atan2(eye_at.z - foot.z));
        tf.scale = Vec3::splat(grown);
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

// ------------------------------------------------------------- building

fn v3(a: [f32; 3]) -> Vec3 {
    Vec3::new(a[0], a[1], a[2])
}

fn colour(hex: &str) -> Color {
    let [r, g, b] = places::rgb(hex);
    Color::srgb(r, g, b)
}

/// A material for a colour, with how rough, how metallic and how much it shines by itself.
fn stuff(materials: &mut Assets<StandardMaterial>, hex: &str, rough: f32, metal: f32, glow: f32) -> Handle<StandardMaterial> {
    let base = colour(hex);
    let lin = base.to_linear();
    materials.add(StandardMaterial {
        base_color: base,
        perceptual_roughness: rough,
        metallic: metal,
        emissive: LinearRgba::new(lin.red * glow, lin.green * glow, lin.blue * glow, 1.0),
        ..default()
    })
}

/// Build a place from its description: every part and every light.
fn raise(data: &PlaceData, commands: &mut Commands, meshes: &mut Assets<Mesh>, materials: &mut Assets<StandardMaterial>, assets: &AssetServer) {
    // one mesh of each plain shape, stretched to each part's size
    let (cube, drum, ball) = (meshes.add(Cuboid::new(1.0, 1.0, 1.0)), meshes.add(Cylinder::new(0.5, 1.0)), meshes.add(Sphere::new(0.5)));
    // parts of one colour and finish share a material
    let mut made: HashMap<String, Handle<StandardMaterial>> = HashMap::new();
    for p in &data.parts {
        let turn = Quat::from_euler(EulerRot::YXZ, p.rot[1].to_radians(), p.rot[0].to_radians(), p.rot[2].to_radians());
        let place = Transform::from_translation(v3(p.at)).with_rotation(turn);
        if p.shape == "model" {
            let scene = assets.load(GltfAssetLabel::Scene(0).from_asset(format!("models/{}", p.file)));
            commands.spawn((SceneRoot(scene), place.with_scale(v3(p.size)), PlacePart));
            continue;
        }
        let key = format!("{} {} {} {}", p.color, p.rough, p.metal, p.glow);
        let material = made.entry(key).or_insert_with(|| stuff(materials, &p.color, p.rough, p.metal, p.glow)).clone();
        let (mesh, place) = match p.shape.as_str() {
            "cylinder" => (drum.clone(), place.with_scale(v3(p.size))),
            "sphere" => (ball.clone(), place.with_scale(v3(p.size))),
            // (a capsule's round ends would be squashed by stretching: it is made to measure)
            "capsule" => (meshes.add(Capsule3d::new(p.size[0] / 2.0, (p.size[1] - p.size[0]).max(0.0))), place),
            _ => (cube.clone(), place.with_scale(v3(p.size))),
        };
        let mut part = commands.spawn((Mesh3d(mesh), MeshMaterial3d(material), place, PlacePart));
        if !p.shadow {
            part.insert(NotShadowCaster);
        }
    }
    for l in &data.lights {
        if l.kind == "sun" {
            commands.spawn((
                DirectionalLight { color: colour(&l.color), illuminance: l.power, shadows_enabled: l.shadows, ..default() },
                Transform::from_translation(v3(l.at)).looking_at(Vec3::ZERO, Vec3::Y),
                PlacePart,
            ));
        } else {
            commands.spawn((
                PointLight { color: colour(&l.color), intensity: l.power, range: if l.range > 0.0 { l.range } else { 30.0 }, shadows_enabled: l.shadows, ..default() },
                Transform::from_translation(v3(l.at)),
                PlacePart,
            ));
        }
    }
}

/// A 3D figure of a character, made from their look: the colours of their
/// skin, hair and coat, and a face that follows their expression. It stands
/// at its parent's origin facing +z.
fn figure(def: &CharDef, expr: &str, parent: Entity, commands: &mut Commands, meshes: &mut Assets<Mesh>, materials: &mut Assets<StandardMaterial>) {
    let mut look = art::look_defaults(&def.sprite);
    look.extend(def.look.clone());
    let tone = |key: &str, or: &str| look.get(key).filter(|v| v.starts_with('#') && v.len() == 7).cloned().unwrap_or_else(|| or.to_string());
    let (skin, hair, coat, iris, lips) = (tone("skin", "#d8a880"), tone("hair", "#4a3828"), tone("coat", "#3c4660"), tone("iris", "#3a2a20"), tone("lips", "#a85a50"));
    // trousers a shade darker than the coat
    let [r, g, b] = places::rgb(&coat);
    let dark = format!("#{:02x}{:02x}{:02x}", (r * 140.0) as u8, (g * 140.0) as u8, (b * 140.0) as u8);
    let (cube, drum, ball) = (meshes.add(Cuboid::new(1.0, 1.0, 1.0)), meshes.add(Cylinder::new(0.5, 1.0)), meshes.add(Sphere::new(0.5)));
    let mut made: HashMap<String, Handle<StandardMaterial>> = HashMap::new();
    let mut put = |mesh: &Handle<Mesh>, at: [f32; 3], size: [f32; 3], tilt: f32, hex: &str, rough: f32| {
        let material = made.entry(format!("{} {}", hex, rough)).or_insert_with(|| stuff(materials, hex, rough, 0.0, 0.0)).clone();
        let part = commands.spawn((Mesh3d(mesh.clone()), MeshMaterial3d(material), Transform::from_translation(v3(at)).with_rotation(Quat::from_rotation_z(tilt)).with_scale(v3(size)), FigurePart)).id();
        commands.entity(parent).add_child(part);
    };
    for side in [-1.0f32, 1.0] {
        put(&drum, [side * 0.1, 0.44, 0.0], [0.17, 0.82, 0.17], 0.0, &dark, 0.9);
        put(&cube, [side * 0.1, 0.04, 0.05], [0.14, 0.08, 0.28], 0.0, "#1c1a1c", 0.5);
        put(&drum, [side * 0.29, 1.13, 0.0], [0.12, 0.6, 0.12], side * 0.09, &coat, 0.8);
        put(&ball, [side * 0.32, 0.8, 0.0], [0.1, 0.12, 0.1], 0.0, &skin, 0.7);
        put(&ball, [side * 0.2, 1.4, 0.0], [0.17, 0.14, 0.17], 0.0, &coat, 0.8);
    }
    put(&cube, [0.0, 1.13, 0.0], [0.42, 0.62, 0.24], 0.0, &coat, 0.8);
    put(&cube, [0.0, 1.2, 0.123], [0.1, 0.4, 0.01], 0.0, "#f2efe8", 0.6);
    put(&drum, [0.0, 1.47, 0.0], [0.11, 0.08, 0.11], 0.0, &skin, 0.7);
    put(&ball, [0.0, 1.62, 0.0], [0.25, 0.29, 0.26], 0.0, &skin, 0.7);
    // hair: over the top and down the back
    put(&ball, [0.0, 1.665, -0.025], [0.275, 0.25, 0.27], 0.0, &hair, 0.9);
    put(&ball, [0.0, 1.6, -0.06], [0.26, 0.26, 0.2], 0.0, &hair, 0.9);
    put(&ball, [0.0, 1.6, 0.128], [0.03, 0.045, 0.03], 0.0, &skin, 0.7);
    // the face: brows, eyes and mouth take the expression
    let (brow_tilt, brow_lift, eye_open, mouth_w, mouth_h, mouth_y) = match expr {
        "smile" => (0.0, 0.0, 0.8, 0.085, 0.014, 0.0),
        "worried" => (-0.35, 0.006, 1.0, 0.05, 0.012, -0.004),
        "angry" => (0.4, -0.008, 0.8, 0.06, 0.012, -0.004),
        "smug" => (0.15, 0.002, 0.7, 0.07, 0.012, 0.002),
        "shock" => (0.0, 0.014, 1.35, 0.04, 0.045, -0.008),
        _ => (0.0, 0.0, 1.0, 0.06, 0.012, 0.0),
    };
    for side in [-1.0f32, 1.0] {
        put(&ball, [side * 0.048, 1.635, 0.112], [0.036, 0.03 * eye_open, 0.02], 0.0, "#f4f1ec", 0.3);
        put(&ball, [side * 0.048, 1.635, 0.121], [0.02, 0.022 * eye_open, 0.012], 0.0, &iris, 0.3);
        put(&cube, [side * 0.048, 1.667 + brow_lift, 0.118], [0.045, 0.008, 0.01], -side * brow_tilt, &hair, 0.9);
    }
    put(&cube, [0.0, 1.548 + mouth_y, 0.118], [mouth_w, mouth_h, 0.012], 0.0, &lips, 0.6);
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

    pub fn plugins(app: &mut App, mut window: WindowPlugin, files: AssetPlugin) {
        // the headset sets the pace: the desktop window must not make it wait for the monitor
        if let Some(w) = &mut window.primary_window {
            w.present_mode = PresentMode::AutoNoVsync;
        }
        // without a runtime or a headset this falls back to ordinary rendering by itself
        app.add_plugins(add_xr_plugins(DefaultPlugins.build().disable::<PipelinedRenderingPlugin>()).set(window).set(files))
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
