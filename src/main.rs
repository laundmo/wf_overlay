#![allow(clippy::type_complexity)]

use bevy::{
    prelude::*,
    sprite::{Anchor, Text2dShadow},
    window::{CompositeAlphaMode, WindowMode, WindowResolution},
};

use crate::{
    market::{ItemData, Slug},
    ocr::{ItemsContainer, StartOcr},
};

mod cap;
mod input;
mod market;
mod market_api;
mod ocr;

fn main() {
    App::new()
        .insert_resource(ClearColor(Color::NONE))
        .add_plugins(DefaultPlugins.set(
            // set window name for the KDE window rule (or your own)
            WindowPlugin {
                primary_window: Some(Window {
                    //name: "bevy.app".to_string().into(),
                    mode: WindowMode::BorderlessFullscreen(MonitorSelection::Primary),
                    transparent: true,
                    composite_alpha_mode: CompositeAlphaMode::PreMultiplied,
                    decorations: false,
                    window_level: bevy::window::WindowLevel::AlwaysOnTop,
                    name: Some("wf_overlay".to_string()),
                    resolution: WindowResolution::default().with_scale_factor_override(1.0),
                    ..default()
                }),
                primary_cursor_options: Some(bevy::window::CursorOptions {
                    hit_test: false,
                    ..default()
                }),
                ..default()
            },
        ))
        .add_plugins(ocr::ocrs_plugin)
        .add_plugins(cap::ScreencastPlugin)
        .add_plugins(market::market_plugin)
        .add_plugins(input::input_plugin)
        // .add_plugins(rag::rustautogui_plugin)
        .add_systems(Startup, setup)
        .add_systems(Update, (keybinds, despawn_timer))
        .add_observer(display_plat)
        .run();
}

fn setup(mut commands: Commands) {
    commands.spawn(Camera2d);
    let border = Val::VMax(0.1);
    let size = Val::VMax(5.);
    commands
        .spawn((
            Node {
                position_type: PositionType::Relative,
                width: Val::Vw(100.),
                height: Val::Vh(100.),
                ..default()
            },
            DespawnChildrenAfter::new(20.),
            Visibility::Inherited,
        ))
        .with_children(|c| {
            c.spawn((
                Node {
                    position_type: PositionType::Absolute,
                    bottom: border,
                    right: border,
                    width: size,
                    height: size,
                    ..default()
                },
                Outline {
                    width: border,
                    offset: Val::ZERO,
                    color: Color::WHITE,
                },
            ));
            c.spawn((
                Node {
                    position_type: PositionType::Absolute,
                    bottom: border,
                    left: border,
                    width: size,
                    height: size,
                    ..default()
                },
                Outline {
                    width: border,
                    offset: Val::ZERO,
                    color: Color::WHITE,
                },
            ));
            c.spawn((
                Node {
                    position_type: PositionType::Absolute,
                    top: border,
                    right: border,
                    width: size,
                    height: size,
                    ..default()
                },
                Outline {
                    width: border,
                    offset: Val::ZERO,
                    color: Color::WHITE,
                },
            ));
            c.spawn((
                Node {
                    position_type: PositionType::Absolute,
                    top: border,
                    left: border,
                    width: size,
                    height: size,
                    ..default()
                },
                Outline {
                    width: border,
                    offset: Val::ZERO,
                    color: Color::WHITE,
                },
            ));
        });
}

fn keybinds(
    kb: Res<ButtonInput<KeyCode>>,
    mut commands: Commands,
    items: Single<Entity, With<ItemsContainer>>,
) {
    // see input.rs for why KeyI works but nothing else will
    if kb.just_pressed(KeyCode::KeyI) {
        println!("Start capture");
        commands.trigger(StartOcr);
        commands
            .entity(items.entity())
            .insert_if_new(DespawnChildrenAfter::new(14.5));
    }
}

#[derive(Component, Deref, DerefMut)]
pub struct DespawnChildrenAfter(Timer);
impl DespawnChildrenAfter {
    pub fn new(seconds: f32) -> Self {
        Self(Timer::from_seconds(seconds, TimerMode::Once))
    }
}

fn despawn_timer(
    q: Query<(Entity, &mut DespawnChildrenAfter)>,
    time: Res<Time>,
    mut commands: Commands,
) {
    for (e, mut t) in q {
        if t.tick(time.delta()).just_finished() {
            commands
                .entity(e)
                .despawn_children()
                .remove::<DespawnChildrenAfter>();
        }
    }
}

#[derive(Component)]
pub struct ShouldDisplay;

fn display_plat(
    evt: On<Insert, ItemData>,
    q: Query<(&ItemData, &Slug), With<ShouldDisplay>>,
    mut commands: Commands,
) {
    if let Ok((data, slug)) = q.get(evt.entity) {
        commands.entity(evt.entity).with_child((
            Transform::from_xyz(150., -10., 0.),
            Text2d(format!(
                "Avg: {}\nMin: {}\nMax: {}\nDucats: {}\n{}",
                data.avg,
                data.min,
                data.max,
                data.ducats
                    .as_ref()
                    .map_or("-".to_string(), ToString::to_string),
                slug.0
            )),
            TextFont::from_font_size(24.),
            Anchor::TOP_CENTER,
            Text2dShadow {
                offset: Vec2::new(2.0, -2.0),
                color: Color::BLACK,
            },
        ));
    }
}
