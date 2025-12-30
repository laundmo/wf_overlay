use ashpd::desktop::{
    PersistMode,
    screencast::{CursorMode, Screencast, SourceType, Stream},
};
use bevy::prelude::*;
use bevy::tasks::AsyncComputeTaskPool;
use crossbeam_channel::{Receiver, Sender, bounded};
use image::RgbaImage;
use pipewire as pw;
use pw::{properties::properties, spa};
use std::{
    fs,
    os::fd::{IntoRawFd, OwnedFd},
};

/// Plugin for capturing screencast on Linux/Wayland
pub struct ScreencastPlugin;

impl Plugin for ScreencastPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(ScreencastSession::from_disk_or_default())
            .init_resource::<LatestImage>()
            .add_systems(Startup, setup_screencast)
            .add_systems(Update, receive_frames);
    }
}

#[derive(Resource)]
pub struct ScreencastReceiver {
    frames: Receiver<ScreencastFrame>,
    meta: Receiver<ScreencastMeta>,
}
struct ScreencastSender {
    frames: Sender<ScreencastFrame>,
    frames_rx: Receiver<ScreencastFrame>,
}

struct MetaSender {
    meta: Sender<ScreencastMeta>,
    meta_rx: Receiver<ScreencastMeta>,
}

/// A captured screencast frame
#[derive(Clone)]
pub struct ScreencastFrame(Vec<u8>);

#[derive(Clone, Default)]
pub struct ScreencastMeta {
    pub width: u32,
    pub height: u32,
    pub format: VideoFormat,
}

#[derive(Clone, Debug, Default)]
pub enum VideoFormat {
    #[default]
    Bgra,
    Rgba,
    BGRx,
    RGBx,
    Other(String),
}

/// Resource storing the screencast session information
#[derive(Resource, Default)]
pub struct ScreencastSession {
    /// Session token for restoring the session
    pub restore_token: Option<String>,
}
impl ScreencastSession {
    const FILE: &'static str = "screen_session.txt";
    fn from_disk_or_default() -> Self {
        if let Ok(s) = fs::read_to_string(Self::FILE) {
            Self {
                restore_token: Some(s),
            }
        } else {
            Self {
                restore_token: None,
            }
        }
    }
    fn save_to_disk(&self) {
        if let Some(ref token) = self.restore_token {
            fs::write(Self::FILE, token).unwrap();
        }
    }
}

/// Setup the screencast session
fn setup_screencast(session_res: Res<ScreencastSession>, mut commands: Commands) {
    let restore_token = session_res.restore_token.clone();
    let (tx, rx) = bounded(1);
    let (tx_m, rx_m) = bounded(1);
    commands.insert_resource(ScreencastReceiver {
        frames: rx.clone(),
        meta: rx_m.clone(),
    });
    let send = ScreencastSender {
        frames: tx,
        frames_rx: rx,
    };
    let send_m = MetaSender {
        meta: tx_m,
        meta_rx: rx_m,
    };

    let task_pool = AsyncComputeTaskPool::get();
    task_pool
        .spawn(async move {
            let (stream, fd, new_token) = open_portal(restore_token)
                .await
                .expect("failed to open portal");
            let s = ScreencastSession {
                restore_token: new_token,
            };
            s.save_to_disk();
            let pipewire_node_id = stream.pipe_wire_node_id();

            println!(
                "node id {}, fd {}",
                pipewire_node_id,
                &fd.try_clone().unwrap().into_raw_fd()
            );

            if let Err(e) = start_streaming(pipewire_node_id, fd, send, send_m).await {
                eprintln!("Error: {}", e);
            };
        })
        .detach();
}

async fn open_portal(
    restore_token: Option<String>,
) -> ashpd::Result<(Stream, OwnedFd, Option<String>)> {
    let proxy = Screencast::new().await?;
    let session = proxy.create_session().await?;
    proxy
        .select_sources(
            &session,
            CursorMode::Hidden,
            SourceType::Monitor.into(),
            false,
            restore_token.as_deref(),
            PersistMode::ExplicitlyRevoked,
        )
        .await?;

    let response = proxy.start(&session, None).await?.response()?;
    let stream = response
        .streams()
        .first()
        .expect("no stream found / selected")
        .to_owned();
    let restore_token = response.restore_token().map(ToString::to_string);

    let fd = proxy.open_pipe_wire_remote(&session).await?;

    Ok((stream, fd, restore_token))
}

struct UserData {
    format: spa::param::video::VideoInfoRaw,
}

async fn start_streaming(
    node_id: u32,
    fd: OwnedFd,
    send: ScreencastSender,
    meta: MetaSender,
) -> Result<(), pw::Error> {
    pw::init();

    let mainloop = pw::main_loop::MainLoopBox::new(None)?;
    let context = pw::context::ContextBox::new(mainloop.loop_(), None)?;
    let core = context.connect_fd(fd, None)?;

    let data = UserData {
        format: Default::default(),
    };

    let stream = pw::stream::StreamBox::new(
        &core,
        "video-test",
        properties! {
            *pw::keys::MEDIA_TYPE => "Video",
            *pw::keys::MEDIA_CATEGORY => "Capture",
            *pw::keys::MEDIA_ROLE => "Screen",
        },
    )?;

    let _listener = stream
        .add_local_listener_with_user_data(data)
        .state_changed(|_, _, old, new| {
            println!("State changed: {:?} -> {:?}", old, new);
        })
        .param_changed(move |_, user_data, id, param| {
            let Some(param) = param else {
                return;
            };
            if id != pw::spa::param::ParamType::Format.as_raw() {
                return;
            }

            let (media_type, media_subtype) =
                match pw::spa::param::format_utils::parse_format(param) {
                    Ok(v) => v,
                    Err(_) => return,
                };

            if media_type != pw::spa::param::format::MediaType::Video
                || media_subtype != pw::spa::param::format::MediaSubtype::Raw
            {
                return;
            }

            user_data
                .format
                .parse(param)
                .expect("Failed to parse param changed to VideoInfoRaw");

            println!("got video format:");
            println!(
                "\tformat: {} ({:?})",
                user_data.format.format().as_raw(),
                user_data.format.format()
            );
            println!(
                "\tsize: {}x{}",
                user_data.format.size().width,
                user_data.format.size().height
            );
            println!(
                "\tframerate: {}/{}",
                user_data.format.framerate().num,
                user_data.format.framerate().denom
            );
            let format = match user_data.format.format() {
                spa::param::video::VideoFormat::BGRA => VideoFormat::Bgra,
                spa::param::video::VideoFormat::RGBA => VideoFormat::Rgba,
                spa::param::video::VideoFormat::BGRx => VideoFormat::BGRx,
                spa::param::video::VideoFormat::RGBx => VideoFormat::RGBx,
                other => VideoFormat::Other(format!("{:?}", other)),
            };
            let size = user_data.format.size();
            while meta.meta_rx.try_recv().is_ok() {}
            meta.meta
                .send(ScreencastMeta {
                    width: size.width,
                    height: size.height,
                    format,
                })
                .unwrap();

            // prepare to render video of this size
        })
        .process(move |stream, _| {
            match stream.dequeue_buffer() {
                None => println!("out of buffers"),
                Some(mut buffer) => {
                    let datas = buffer.datas_mut();
                    if datas.is_empty() {
                        return;
                    }

                    // copy frame data to screen
                    let data = &mut datas[0];
                    if let Some(slice) = data.data() {
                        // drain first
                        while send.frames_rx.try_recv().is_ok() {}
                        send.frames.send(ScreencastFrame(slice.to_vec())).unwrap();
                    }
                }
            }
        })
        .register()?;

    println!("Created stream {:#?}", stream);

    let obj = pw::spa::pod::object!(
        pw::spa::utils::SpaTypes::ObjectParamFormat,
        pw::spa::param::ParamType::EnumFormat,
        pw::spa::pod::property!(
            pw::spa::param::format::FormatProperties::MediaType,
            Id,
            pw::spa::param::format::MediaType::Video
        ),
        pw::spa::pod::property!(
            pw::spa::param::format::FormatProperties::MediaSubtype,
            Id,
            pw::spa::param::format::MediaSubtype::Raw
        ),
        pw::spa::pod::property!(
            pw::spa::param::format::FormatProperties::VideoFormat,
            Choice,
            Enum,
            Id,
            pw::spa::param::video::VideoFormat::RGB,
            pw::spa::param::video::VideoFormat::RGB,
            pw::spa::param::video::VideoFormat::RGBA,
            pw::spa::param::video::VideoFormat::RGBx,
            pw::spa::param::video::VideoFormat::BGRx,
        ),
        pw::spa::pod::property!(
            pw::spa::param::format::FormatProperties::VideoSize,
            Choice,
            Range,
            Rectangle,
            pw::spa::utils::Rectangle {
                width: 320,
                height: 240
            },
            pw::spa::utils::Rectangle {
                width: 1,
                height: 1
            },
            pw::spa::utils::Rectangle {
                width: 4096,
                height: 4096
            }
        ),
        pw::spa::pod::property!(
            pw::spa::param::format::FormatProperties::VideoFramerate,
            Choice,
            Range,
            Fraction,
            pw::spa::utils::Fraction { num: 25, denom: 1 },
            pw::spa::utils::Fraction { num: 0, denom: 1 },
            pw::spa::utils::Fraction {
                num: 1000,
                denom: 1
            }
        ),
    );
    let values: Vec<u8> = pw::spa::pod::serialize::PodSerializer::serialize(
        std::io::Cursor::new(Vec::new()),
        &pw::spa::pod::Value::Object(obj),
    )
    .unwrap()
    .0
    .into_inner();

    let mut params = [spa::pod::Pod::from_bytes(&values).unwrap()];

    stream.connect(
        spa::utils::Direction::Input,
        Some(node_id),
        pw::stream::StreamFlags::AUTOCONNECT | pw::stream::StreamFlags::MAP_BUFFERS,
        &mut params,
    )?;

    println!("Connected stream");

    mainloop.run();

    Ok(())
}

// optimizes very well. see: https://github.com/image-rs/image/pull/2712
pub fn from_raw_bgra(width: u32, height: u32, container: Vec<u8>) -> Option<RgbaImage> {
    let mut img = RgbaImage::from_raw(width, height, container)?;

    let (chunked, _) = img.as_chunks_mut::<4>();

    for p in chunked {
        let bgra = u32::from_be_bytes(*p);
        let argb = bgra.swap_bytes();
        let rgba = argb.rotate_left(8);
        *p = rgba.to_be_bytes();
    }

    Some(img)
}

#[derive(Resource, Default)]
pub struct LatestImage(Vec<u8>, ScreencastMeta);
impl LatestImage {
    fn set_latest_img(&mut self, img: Vec<u8>) {
        self.0 = img;
    }
    fn set_latest_meta(&mut self, meta: ScreencastMeta) {
        self.1 = meta;
    }
    pub fn get_latest_rgba(&mut self) -> Option<RgbaImage> {
        if self.0.len() < 4 {
            return None;
        }
        match &self.1.format {
            VideoFormat::Bgra | VideoFormat::BGRx => {
                from_raw_bgra(self.1.width, self.1.height, std::mem::take(&mut self.0))
            }
            VideoFormat::Rgba | VideoFormat::RGBx => {
                RgbaImage::from_raw(self.1.width, self.1.height, std::mem::take(&mut self.0))
            }
            VideoFormat::Other(f) => {
                error_once!("Unknown Screencast image format {f}");
                None
            }
        }
    }
}

/// System to receive frames from the channel and update the resource
fn receive_frames(receiver_res: Res<ScreencastReceiver>, mut img: ResMut<LatestImage>) {
    // Try to receive frames in a non-blocking way
    // Try to receive the latest frame (non-blocking)
    if let Ok(meta) = receiver_res.meta.try_recv() {
        info!(
            "Frame meta changed: {}x{} ({:?})",
            meta.width, meta.height, meta.format
        );
        img.set_latest_meta(meta);
        // Here you would update your texture/image resource
        // For example, you could convert this to a Bevy Image and update a texture
    }
    if let Ok(frame) = receiver_res.frames.try_recv() {
        img.set_latest_img(frame.0);
    }
}
