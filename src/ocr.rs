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
use jiff::fmt::temporal::DateTimePrinter;
use ocrs::{ImageSource, OcrEngine, OcrEngineParams, TextItem};
use rten::Model;

use crate::{
    PlatOverlayPhase, ShouldDisplay,
    cap::LatestImage,
    config::{ConfigManager, Layout},
};

fn file_path(path: &str) -> PathBuf {
    let mut abs_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    abs_path.push(path);
    abs_path
}

pub(crate) fn ocrs_plugin(app: &mut App) {
    app.init_resource::<Engine>()
        .init_resource::<OcrTask>()
        .add_systems(OnEnter(PlatOverlayPhase::Ocr), start_ocr_task)
        .add_systems(Startup, setup_items_container)
        .add_systems(
            Update,
            (get_ocr_result, debug_ocr_result)
                .chain()
                .run_if(in_state(PlatOverlayPhase::Ocr)),
        );
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
        .filter_map(|col| {
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
impl Layout {
    fn get_ocr_bounds(&self, img_size: (u32, u32)) -> URect {
        let actual = UVec2::from(img_size);
        let factor = actual / self.reference_resolution;
        let offset = self.offset * factor;
        let size = self.size * factor;

        URect {
            min: offset,
            max: (offset + size),
        }
    }
}

fn detect_once(engine: Engine, img: image::RgbaImage, ocr_bounds: URect) -> Result<OcrResults> {
    let processed = image::imageops::crop_imm(
        &img,
        ocr_bounds.min.x,
        ocr_bounds.min.y,
        ocr_bounds.width(),
        ocr_bounds.height(),
    )
    .to_image();
    if processed.dimensions().0 == 0 || processed.dimensions().1 == 0 {
        return Err(anyhow!("Image dimensions are 0 in one direction").into());
    }
    // image::imageops::invert(&mut subimg);
    let mut processed = image::imageops::unsharpen(&processed, 20.0, 15);
    image::imageops::colorops::contrast_in_place(&mut processed, 20.);
    let processed = image::imageops::fast_blur(&processed, 1.);
    let mut processed = image::imageops::unsharpen(&processed, 5.0, 15);
    image::imageops::invert(&mut processed);
    image::imageops::colorops::brighten_in_place(&mut processed, -30);
    image::imageops::colorops::contrast_in_place(&mut processed, 20.);
    // processed.save("unsharpened.png").unwrap();

    let img_source = ImageSource::from_bytes(processed.as_raw(), processed.dimensions())?;

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
                .map(|i| Vec2::new(i.x as f32, i.y as f32) + ocr_bounds.min.as_vec2()),
        );
        let first_index = words.len();

        for text_word in line.words() {
            let word_aabb = Aabb2d::from_point_cloud(
                Vec2::ZERO,
                &text_word
                    .rotated_rect()
                    .corners()
                    .map(|i| Vec2::new(i.x, i.y) + ocr_bounds.min.as_vec2()),
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
        detect_aabb: {
            let Rect { min, max } = ocr_bounds.as_rect();
            Aabb2d { min, max }
        },
        words,
        lines,
        items,
    })
}

#[derive(Resource, Default)]
struct OcrTask(Option<Task<Result<OcrResults>>>);
const PRINTER: DateTimePrinter = DateTimePrinter::new().separator(b'_').precision(Some(0));

fn start_ocr_task(
    mut img: ResMut<LatestImage>,
    engine: Res<Engine>,
    conf: Res<ConfigManager>,
    mut current_task: ResMut<OcrTask>,
    mut items: Single<&mut ItemsContainer>,
) {
    if current_task.0.is_none()
        && let Some(img) = img.get_latest_rgba()
    {
        let engine = engine.clone();
        if conf.save_to_disk {
            std::fs::create_dir_all("images").unwrap();
            let ts = PRINTER
                .timestamp_to_string(&jiff::Timestamp::now())
                .replace(":", "_");
            if let Err(e) = img.save(format!("images/{ts}.png")) {
                error!("Could not save screenshot: {e}");
            };
        }
        let Some(layout) = conf.find_matching_layout(&img) else {
            warn!("Could not detect layout for capture");
            return;
        };
        let ocr_bounds = layout.get_ocr_bounds(img.dimensions());
        current_task.0 = Some(AsyncComputeTaskPool::get().spawn(async move {
            let start = Instant::now();
            let res = detect_once(engine.clone(), img.clone(), ocr_bounds);
            debug!("OCR took {}ms", start.elapsed().as_millis());
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
