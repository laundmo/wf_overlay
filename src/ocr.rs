use std::{
    ops::Range,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use anyhow::anyhow;
use bevy::{
    math::bounding::{Aabb2d, BoundingVolume},
    prelude::*,
    tasks::{AsyncComputeTaskPool, Task, block_on, futures_lite::future},
};
use ocrs::{ImageSource, OcrEngine, OcrEngineParams, TextItem};
use rten::Model;

use crate::{ShouldDisplay, cap::LatestImage};

fn file_path(path: &str) -> PathBuf {
    let mut abs_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    abs_path.push(path);
    abs_path
}

pub(crate) fn ocrs_plugin(app: &mut App) {
    app.init_resource::<Engine>()
        .init_resource::<OcrTask>()
        .add_observer(start_ocr_task)
        .add_systems(Startup, setup_items_container)
        .add_systems(Update, (get_ocr_result, debug_ocr_result).chain());
}
fn setup_items_container(mut commands: Commands) {
    commands.spawn(ItemsContainer(
        Aabb2d {
            min: Vec2::ZERO,
            max: Vec2::ZERO,
        },
        Color::BLACK,
    ));
}
#[derive(Resource, Clone)]
struct Engine(Arc<Mutex<OcrEngine>>);

impl Default for Engine {
    fn default() -> Self {
        let detection_model_path = file_path("assets/text-detection.rten");
        let rec_model_path = file_path("assets/text-recognition.rten");

        let detection_model = Model::load_file(detection_model_path).unwrap();
        let recognition_model = Model::load_file(rec_model_path).unwrap();

        let engine = OcrEngine::new(OcrEngineParams {
            detection_model: Some(detection_model),
            recognition_model: Some(recognition_model),
            ..Default::default()
        })
        .unwrap();
        Self(Arc::new(Mutex::new(engine)))
    }
}

#[derive(Debug, Clone)]
pub struct Line {
    pub bounds: Aabb2d,
    pub word_range: Range<usize>,
}

#[derive(Debug, Clone)]
pub struct Word {
    pub text: String,
    pub bounds: Aabb2d,
}

#[derive(Component, Debug)]
pub struct Item {
    pub name: String,
    pub bounds: Aabb2d,
}

#[derive(Debug)]
struct OcrResults {
    detect_aabb: Aabb2d,
    words: Vec<Word>,
    lines: Vec<Line>,
    items: Vec<Item>,
}
impl OcrResults {
    fn convert_aabbs_inplace(&mut self, cam: (&Camera, &GlobalTransform)) {
        let conv = |v: &mut Vec2| *v = cam.0.viewport_to_world_2d(cam.1, *v).unwrap();
        let conv_aabb = |aabb: &mut Aabb2d| {
            conv(&mut aabb.max);
            conv(&mut aabb.min)
        };
        self.words.iter_mut().for_each(|w| conv_aabb(&mut w.bounds));
        self.lines.iter_mut().for_each(|w| conv_aabb(&mut w.bounds));
        self.items.iter_mut().for_each(|w| conv_aabb(&mut w.bounds));
        conv_aabb(&mut self.detect_aabb);
    }
}

const OFFSET: UVec2 = UVec2::new(478, 411);
const SIZE: UVec2 = UVec2::new(965, 49);
const ASSUMING: UVec2 = UVec2::new(1920, 1080);

pub fn detect_columns(words: &[Word], gap_threshold: f32) -> Vec<Item> {
    if words.is_empty() {
        return Vec::new();
    }

    // Sort all words by x position
    let mut sorted = words.to_vec();
    sorted.sort_by(|a, b| a.bounds.min.x.partial_cmp(&b.bounds.min.x).unwrap());

    // Find column boundaries (large gaps)
    let mut boundaries = Vec::new();
    for i in 0..sorted.len() - 1 {
        let gap = sorted[i + 1].bounds.min.x - sorted[i].bounds.max.x;
        if gap > gap_threshold {
            boundaries.push((sorted[i].bounds.max.x + sorted[i + 1].bounds.min.x) / 2.0);
        }
    }

    // Assign words to columns
    let mut columns: Vec<Vec<Word>> = vec![Vec::new(); boundaries.len() + 1];
    for word in words {
        let x = word.bounds.center().x;
        let col_idx = boundaries.iter().filter(|&&b| x > b).count();
        columns[col_idx].push(word.clone());
    }

    // Merge column words into a Item
    columns
        .into_iter()
        .filter_map(|mut col| {
            if col.is_empty() {
                return None;
            }

            let name = col
                .iter()
                .map(|w| w.text.as_ref())
                .collect::<Vec<_>>()
                .join(" ");

            let mut bounds = col[0].bounds;
            for word in &col[1..] {
                bounds = bounds.merge(&word.bounds);
            }

            Some(Item { name, bounds })
        })
        .collect()
}

fn detect_once(engine: Engine, img: image::RgbaImage) -> Result<OcrResults> {
    let actual = UVec2::from(img.dimensions());
    let factor = actual / ASSUMING;
    let offset = OFFSET * factor;
    let size = SIZE * factor;

    let detect_aabb = Aabb2d {
        min: offset.as_vec2() - 1.,
        max: (offset + size).as_vec2() + 1.,
    };
    // if size.x > 600 {
    //     size.y = (size.y / size.x) * 600;
    //     size.x = 600;
    //     let mut dst_image = image::DynamicImage::new(size.x, size.y, subimg.color);
    // }
    let subimg = image::imageops::crop_imm(&img, offset.x, offset.y, size.x, size.y).to_image();
    if subimg.dimensions().0 == 0 || subimg.dimensions().1 == 0 {
        return Err(anyhow!("Image dimensions are 0 in one direction").into());
    }

    // let mut resizer = engine.1.lock().expect("non-poisoned lock");
    // let subimg = DynamicImage::ImageRgba8(subimg);
    // let mut dest_subimg = subimg.clone();

    // resizer
    //     .resize(
    //         &subimg,
    //         &mut dest_subimg,
    //         &ResizeOptions::new()
    //             .use_alpha(false)
    //             .resize_alg(fast_image_resize::ResizeAlg::Nearest),
    //     )
    //     .unwrap();
    // let subimg = dest_subimg.as_rgba8().unwrap();
    let img_source = ImageSource::from_bytes(subimg.as_raw(), subimg.dimensions())?;

    // use a block to only lock engine for as little time as possible
    let ocr_engine = engine.0.lock().expect("non-poisoned lock");
    let ocr_input = ocr_engine.prepare_input(img_source)?;
    let word_rects = ocr_engine.detect_words(&ocr_input)?;
    let line_rects = ocr_engine.find_text_lines(&ocr_input, &word_rects);
    let line_texts = ocr_engine.recognize_text(&ocr_input, &line_rects)?;
    drop(ocr_engine); // unlock engine asap

    let mut words: Vec<Word> = Vec::new();
    let mut lines: Vec<Line> = Vec::new();
    for line in line_texts.iter() {
        let Some(line) = line else {
            // Skip lines where recognition produced no output.
            continue;
        };
        let line_aabb = Aabb2d::from_point_cloud(
            Vec2::ZERO,
            &line
                .bounding_rect()
                .corners()
                .map(|i| Vec2::new(i.x as f32, i.y as f32) + offset.as_vec2()),
        );
        let first_index = words.len();

        for text_word in line.words() {
            let word_aabb = Aabb2d::from_point_cloud(
                Vec2::ZERO,
                &text_word
                    .rotated_rect()
                    .corners()
                    .map(|i| Vec2::new(i.x, i.y) + offset.as_vec2()),
            );
            words.push(Word {
                text: text_word.to_string(),
                bounds: word_aabb,
            });
        }
        lines.push(Line {
            bounds: line_aabb,
            word_range: first_index..words.len(),
        });
    }
    let items = detect_columns(&words, 15.);
    Ok(OcrResults {
        detect_aabb,
        words,
        lines,
        items,
    })
}

#[derive(Event)]
pub struct StartOcr;

#[derive(Resource, Default)]
struct OcrTask(Option<Task<Result<OcrResults>>>);

fn start_ocr_task(
    e: On<StartOcr>,
    mut img: ResMut<LatestImage>,
    engine: Res<Engine>,
    mut current_task: ResMut<OcrTask>,
    mut items: Single<&mut ItemsContainer>,
) {
    if current_task.0.is_none()
        && let Some(img) = img.get_latest_rgba()
    {
        let engine = engine.clone();
        current_task.0 = Some(AsyncComputeTaskPool::get().spawn(async move {
            let start = Instant::now();
            let res = detect_once(engine, img);
            dbg!(start.elapsed().as_millis());
            res
        }));
        items.1 = Color::linear_rgb(0.1, 0.9, 0.1);
    }
}

#[derive(Component)]
#[require(Transform, Visibility)]
pub struct ItemsContainer(pub Aabb2d, pub Color);

fn get_ocr_result(
    mut current_task: ResMut<OcrTask>,
    mut commands: Commands,
    cam: Single<(&Camera, &GlobalTransform)>,
    mut items: Single<(Entity, &mut ItemsContainer)>,
) -> Result<()> {
    if let Some(ref mut task) = current_task.0
        && let Some(result) = block_on(future::poll_once(task))
    {
        let mut result = result?;
        result.convert_aabbs_inplace(*cam);
        items.1.0 = result.detect_aabb;
        items.1.1 = Color::linear_rgb(0.9, 0.1, 0.9);

        let mut items_container = commands.entity(items.0);
        items_container.despawn_children();
        items_container.with_children(|c| {
            for (idx, item) in result.items.into_iter().enumerate() {
                if idx > 3 {
                    break;
                }
                let center = item.bounds.center().extend(0.);
                c.spawn((
                    item,
                    ShouldDisplay,
                    Visibility::Inherited,
                    Transform::from_xyz(center.x, result.detect_aabb.max.y, 0.0),
                ));
            }
        });
        current_task.0 = None;
    }

    Ok(())
}

fn debug_ocr_result(
    mut gizmos: Gizmos,
    items: Single<(&ItemsContainer, &Children)>,
    query: Query<Ref<Item>>,
    time: Res<Time>,
    mut timer: Local<Timer>,
) {
    // gizmos.rect_2d(items.0.0.center(), items.0.0.half_size() * 2., items.0.1);

    for (i, child) in items.1.iter().enumerate() {
        if let Ok(item) = query.get(child) {
            if i == 0 && item.is_changed() {
                timer.set_duration(Duration::from_secs_f32(1.0));
                timer.set_mode(TimerMode::Once);
                timer.reset();
                timer.unpause();
            }
            timer.tick(time.delta());
            gizmos.rect_2d(
                item.bounds.center(),
                item.bounds.half_size() * 2.,
                Color::WHITE.with_alpha(timer.fraction_remaining()),
            );
        }
    }
    if items.1.is_empty() {
        timer.finish();
    };
}
