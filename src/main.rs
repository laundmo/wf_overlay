#![allow(clippy::type_complexity)]

use std::time::Duration;

use bevy::{
    ecs::world::CommandQueue,
    math::bounding::BoundingVolume,
    prelude::*,
    sprite::{Anchor, Text2dShadow},
    window::{CompositeAlphaMode, CursorOptions, WindowMode, WindowResolution},
};

use crate::{
    config::ConfigManager,
    market::{ItemData, Slug},
    ocr::ItemsContainer,
};

mod cap;
mod config;
mod input;
mod market;
mod market_api;
mod ocr;

fn main() {
    App::new()
        .insert_resource(ClearColor(Color::NONE))
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                name: Some("wf_overlay".to_string()),
                mode: WindowMode::BorderlessFullscreen(MonitorSelection::Primary),
                transparent: true,
                composite_alpha_mode: CompositeAlphaMode::PreMultiplied,
                decorations: false,
                window_level: bevy::window::WindowLevel::AlwaysOnTop,
                resolution: WindowResolution::default().with_scale_factor_override(1.0),
                ..default()
            }),
            primary_cursor_options: Some(CursorOptions {
                hit_test: false,
                ..default()
            }),
            ..default()
        }))
        .add_plugins(ocr::ocrs_plugin)
        .add_plugins(cap::ScreencastPlugin)
        .add_plugins(market::market_plugin)
        .add_plugins(input::input_plugin)
        .add_plugins(config::config_plugin)
        .init_state::<AppState>()
        .add_sub_state::<PlatOverlayPhase>()
        .add_systems(Startup, setup)
        .add_systems(Update, (keybinds, command_after))
        .add_observer(display_plat)
        .run();
}

fn setup(mut commands: Commands, conf: Res<ConfigManager>) {
    commands.spawn(Camera2d);
    let border = Val::VMax(0.1);
    let size = Val::VMax(5.);
    if conf.show_corner_boxes > 0.0 {
        let rulers = commands
            .spawn((
                Node {
                    position_type: PositionType::Relative,
                    width: Val::Vw(100.),
                    height: Val::Vh(100.),
                    ..default()
                },
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
            })
            .id();
        commands.delayed(
            Duration::from_secs_f32(conf.show_corner_boxes),
            move |mut c| {
                c.entity(rulers).despawn();
            },
        );
    }
}

#[derive(Debug, Clone, Copy, Default, Eq, PartialEq, Hash, States)]
pub enum AppState {
    #[default]
    Waiting,
    PlatOverlay,
    EditOverlay,
}
#[derive(SubStates, Clone, PartialEq, Eq, Hash, Debug, Default)]
#[source(AppState = AppState::PlatOverlay)]
pub enum PlatOverlayPhase {
    #[default]
    Ocr,
    Displaying,
}

fn keybinds(kb: Res<ButtonInput<KeyCode>>, conf: Res<ConfigManager>, mut commands: Commands) {
    if conf.show_keys {
        kb.get_just_pressed()
            .for_each(|key| info!("Key event: {key:?}"));
    }
    if kb.just_pressed(conf.overlay_key) {
        println!("Start capture");
        commands.set_state(AppState::PlatOverlay);
        commands.set_state(PlatOverlayPhase::Ocr);
    }
}

struct DelayedCommands<C: FnOnce(Commands) + Send + Sync + 'static> {
    delay: Duration,
    write_commands: C,
}
trait DelayedCommandsExt {
    fn delayed(
        &mut self,
        delay: Duration,
        write_commands: impl FnOnce(Commands) + Send + Sync + 'static,
    );
}
impl DelayedCommandsExt for Commands<'_, '_> {
    fn delayed(
        &mut self,
        delay: Duration,
        write_commands: impl FnOnce(Commands) + Send + Sync + 'static,
    ) {
        self.queue(DelayedCommands {
            delay,
            write_commands,
        });
    }
}
impl<C: FnOnce(Commands) + Send + Sync + 'static> Command for DelayedCommands<C> {
    fn apply(self, world: &mut World) {
        let mut command_queue = CommandQueue::default();
        (self.write_commands)(Commands::new(&mut command_queue, world));
        world.spawn(DelayedCommandQueue(
            Timer::new(self.delay, TimerMode::Once),
            command_queue,
        ));
    }
}

#[derive(Component)]
struct DelayedCommandQueue(Timer, CommandQueue);

fn command_after(
    cmds: Query<(Entity, &mut DelayedCommandQueue)>,
    time: Res<Time>,
    mut commands: Commands,
) {
    for (e, mut cmd) in cmds {
        if cmd.0.tick(time.delta()).just_finished() {
            commands.entity(e).despawn();
            commands.append(&mut cmd.1);
        }
    }
}

#[derive(Component)]
pub struct ShouldDisplay;

fn display_plat(
    evt: On<Insert, ItemData>,
    cont: Query<&ItemsContainer>,
    q: Query<(&ItemData, &Slug, &ChildOf), With<ShouldDisplay>>,
    conf: Res<ConfigManager>,
    // main_state: Res<State<AppState>>,
    maybe_state: Option<Res<State<PlatOverlayPhase>>>,
    mut commands: Commands,
) {
    if let Some(state) = maybe_state
        && let PlatOverlayPhase::Ocr = state.get()
    {
        commands.set_state(PlatOverlayPhase::Displaying);
        commands.delayed(Duration::from_secs_f32(conf.close_layout_after), |mut c| {
            c.set_state(AppState::Waiting)
        });
    }

    if let Ok((data, slug, child_of)) = q.get(evt.entity) {
        let mut scale = 0.5;
        if let Ok(container) = cont.get(child_of.parent()) {
            let width = container.0.half_size().x * 2.;
            scale = 2000. / width;
        }

        commands.entity(evt.entity).with_child((
            Transform::from_xyz(150. * scale, -10. * scale, 0.),
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
            TextFont::from_font_size(conf.font_size),
            Anchor::TOP_CENTER,
            Text2dShadow {
                offset: Vec2::new(1. + scale, -(1. + scale)),
                color: Color::BLACK,
            },
            DespawnOnExit(PlatOverlayPhase::Displaying),
        ));
    }
}
