#![allow(clippy::type_complexity)]

use bevy::{
    prelude::*,
    window::{CompositeAlphaMode, WindowMode, WindowResolution},
};

use crate::{
    market::ItemData,
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
        .add_plugins(cap::waycap_plugin)
        .add_plugins(market::market_plugin)
        .add_plugins(input::input_plugin)
        // .add_plugins(rag::rustautogui_plugin)
        .add_systems(Startup, setup)
        .add_systems(Update, keybinds)
        .add_observer(display_plat)
        .run();
}

fn setup(mut commands: Commands) {
    commands.spawn(Camera2d);
}

fn keybinds(kb: Res<ButtonInput<KeyCode>>, mut commands: Commands) {
    // see input.rs for why KeyI works but nothing else will
    if kb.just_pressed(KeyCode::KeyI) {
        println!("Start capture");
        commands.trigger(StartOcr);
    }
}

#[derive(Component)]
pub struct ShouldDisplay;

fn display_plat(
    evt: On<Insert, ItemData>,
    q: Query<&ItemData, With<ShouldDisplay>>,
    mut commands: Commands,
) {
    if let Ok(data) = q.get(evt.entity) {
        commands.entity(evt.entity).with_child((
            Transform::from_xyz(130., -35., 0.),
            Text2d(format!(
                "avg: {}\nmin: {}\nmax: {}",
                data.avg, data.min, data.max
            )),
        ));
    }
}
