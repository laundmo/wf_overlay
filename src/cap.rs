use bevy::{asset::RenderAssetUsages, prelude::*};

use crossbeam_channel::Receiver;
use image::DynamicImage;
use waycap_rs::{Capture, RgbaImageEncoder};

pub(crate) fn waycap_plugin(app: &mut App) {
    app.add_systems(Startup, setup_waycap)
        .add_systems(Update, update_image)
        .init_resource::<LatestImage>();
}

#[derive(Resource)]
struct Waycap(Capture<RgbaImageEncoder>);

#[derive(Resource, Deref, DerefMut)]
struct WaycapReciever(Receiver<image::RgbaImage>);

#[derive(Resource, Deref, DerefMut, Default)]
pub struct LatestImage(pub Option<image::RgbaImage>);

fn setup_waycap(mut commands: Commands) {
    let mut cap = Capture::new_with_encoder(RgbaImageEncoder::default(), false, 60).unwrap();
    let recv = cap.get_output();
    let recv_thread = recv.clone();
    std::thread::spawn(move || {
        loop {
            // drain channel to get latest message
            while recv_thread.len() > 1 {
                recv_thread.try_recv().ok();
            }
            // avoid busy waiting
            std::thread::yield_now();
        }
    });
    commands.insert_resource(Waycap(cap));
    commands.insert_resource(WaycapReciever(recv));
}

fn update_image(rec: ResMut<WaycapReciever>, mut latest: If<ResMut<LatestImage>>) {
    if rec.is_empty() {
        return;
    }
    let img = rec
        .try_recv()
        .expect("reciever was not empty, but could not get a image");
    **latest.0 = Some(img);
}
