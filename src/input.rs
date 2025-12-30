use std::process::exit;

use bevy::{input::InputSystems, prelude::*, tasks::IoTaskPool};
use crossbeam_channel::{Receiver, unbounded};
use evdev::{Device, EventType, KeyCode as EvDevKeyCode};

pub fn input_plugin(app: &mut App) {
    app.add_systems(Startup, setup_input_listening)
        .add_systems(PreUpdate, handle_input_events.after(InputSystems));
}

#[derive(Resource)]
struct InputReceiver(Receiver<evdev::InputEvent>);

fn setup_input_listening(mut commands: Commands) {
    let (tx, rx) = unbounded(); // std sync channel

    commands.insert_resource(InputReceiver(rx));

    let devices: Vec<Device> = evdev::enumerate()
        .filter_map(|(_, device)| {
            // Only keyboards (devices that support key events)
            if device
                .supported_keys()
                .is_some_and(|keys| keys.contains(EvDevKeyCode::KEY_I))
            {
                Some(device)
            } else {
                None
            }
        })
        .collect();
    if devices.is_empty() {
        warn!(
            "Your user needs to be added to the `input` group, to allow this app to get global inputs. Hint: Run `sudo usermod -a -G input $USER` and then reboot!"
        );
        exit(1);
    }
    for device in devices {
        let tx = tx.clone();

        if let Ok(mut stream) = device.into_event_stream() {
            IoTaskPool::get()
                .spawn(async move {
                    loop {
                        if let Ok(event) = stream.next_event().await {
                            let _ = tx.send(event);
                        }
                    }
                })
                .detach();
        }
    }
}

fn handle_input_events(
    receiver: Res<InputReceiver>,
    mut button_input: ResMut<ButtonInput<KeyCode>>,
) {
    // Non-blocking read in your Bevy system
    while let Ok(event) = receiver.0.try_recv() {
        if let evdev::EventSummary::Key(_, key_code, value) = event.destructure() {
            let code = match key_code {
                EvDevKeyCode::KEY_I => KeyCode::KeyI,
                _ => KeyCode::Unidentified(bevy::input::keyboard::NativeKeyCode::Unidentified),
            };
            if value == 1 {
                button_input.press(code);
            } else {
                button_input.release(code);
            }
        };
    }
}
