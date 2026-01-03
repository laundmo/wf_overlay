use std::ops::{Deref, DerefMut};

use bevy::{
    app::{App, AppExit, Last},
    color::{ColorToPacked, Srgba, color_difference::EuclideanDistance},
    ecs::{message::MessageReader, resource::Resource, system::ResMut, world::FromWorld},
    input::keyboard::KeyCode,
    log::error,
    math::UVec2,
    platform::collections::{HashMap, HashSet},
    prelude::Result,
    utils::default,
};

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use toml_edit::{DocumentMut, Item, Table, Value};

pub fn config_plugin(app: &mut App) {
    app.init_resource::<ConfigManager>().add_systems(
        Last,
        |mut exit: MessageReader<AppExit>, mut conf: ResMut<ConfigManager>| {
            for e in exit.read() {
                if let AppExit::Success = e {
                    conf.merge_and_save().unwrap();
                }
            }
        },
    );
}

#[derive(Debug, Clone, Copy)]
pub struct PixelCheck {
    pub x: u32,
    pub y: u32,
    pub color: Srgba,
    pub tolerance: f32,
}

impl PixelCheck {
    /// Check if a pixel matches the expected color
    pub fn matches_pixel(&self, pixel: &Srgba) -> bool {
        if self.tolerance == 0. {
            // Exact match
            self.color == *pixel
        } else {
            let distance = self.color.distance(pixel);
            distance <= self.tolerance
        }
    }
}

// Custom serialization for PixelCheck: "x,y,#hexcolor,tolerance"
impl Serialize for PixelCheck {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let hex = self.color.to_hex();
        let s = format!("{},{},{},{}", self.x, self.y, hex, self.tolerance);
        serializer.serialize_str(&s)
    }
}

impl<'de> Deserialize<'de> for PixelCheck {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        let parts: Vec<&str> = s.split(',').collect();

        if parts.len() != 4 {
            return Err(serde::de::Error::custom(format!(
                "PixelCheck format must be 'x,y,#hexcolor,tolerance', got: {}",
                s
            )));
        }

        let x = parts[0]
            .trim()
            .parse::<u32>()
            .map_err(|e| serde::de::Error::custom(format!("Invalid x coordinate: {}", e)))?;
        let y = parts[1]
            .trim()
            .parse::<u32>()
            .map_err(|e| serde::de::Error::custom(format!("Invalid y coordinate: {}", e)))?;
        let color = Srgba::hex(parts[2].trim())
            .map_err(|e| serde::de::Error::custom(format!("Invalid hex color: {:?}", e)))?;
        let tolerance = parts[3]
            .trim()
            .parse::<f32>()
            .map_err(|e| serde::de::Error::custom(format!("Invalid tolerance: {}", e)))?;

        Ok(PixelCheck {
            x,
            y,
            color,
            tolerance,
        })
    }
}

/// A config variant selected by aspect ratio and pixel checks
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LayoutOption {
    #[serde(
        serialize_with = "serialize_aspect_ratio",
        deserialize_with = "deserialize_aspect_ratio"
    )]
    pub aspect_ratio: [u32; 2],
    pub pixel_checks: Vec<PixelCheck>,
    #[serde(flatten)]
    pub config: Layout,
}

impl LayoutOption {
    fn aspect_ratio_matches(&self, img_width: u32, img_height: u32) -> bool {
        self.aspect_ratio[0] * img_height == self.aspect_ratio[1] * img_width
    }

    fn verify_pixel_checks(&self, image: &image::RgbaImage) -> bool {
        let (width, height) = image.dimensions();

        self.pixel_checks.iter().all(|check| {
            // Ensure pixel is within bounds
            if check.x >= width || check.y >= height {
                return false;
            }

            let pixel = image.get_pixel(check.x, check.y);
            let srgba = Srgba::from_u8_array(pixel.0);
            check.matches_pixel(&srgba)
        })
    }

    pub fn matches(&self, image: &image::RgbaImage) -> bool {
        let (width, height) = image.dimensions();
        self.aspect_ratio_matches(width, height) && self.verify_pixel_checks(image)
    }
}

fn serialize_aspect_ratio<S>(aspect_ratio: &[u32; 2], serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    let s = format!("{}:{}", aspect_ratio[0], aspect_ratio[1]);
    serializer.serialize_str(&s)
}

fn deserialize_aspect_ratio<'de, D>(deserializer: D) -> Result<[u32; 2], D::Error>
where
    D: Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;
    let parts: Vec<&str> = s.split(':').collect();

    if parts.len() != 2 {
        return Err(serde::de::Error::custom(format!(
            "Aspect ratio must be in format 'width:height', got: {}",
            s
        )));
    }

    let width = parts[0]
        .trim()
        .parse::<u32>()
        .map_err(|e| serde::de::Error::custom(format!("Invalid width: {}", e)))?;
    let height = parts[1]
        .trim()
        .parse::<u32>()
        .map_err(|e| serde::de::Error::custom(format!("Invalid height: {}", e)))?;

    Ok([width, height])
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Layout {
    pub offset: UVec2,
    pub size: UVec2,
    pub reference_resolution: UVec2,
    #[serde(
        serialize_with = "serialize_color",
        deserialize_with = "deserialize_color"
    )]
    pub theme_text_color: Srgba,
    pub item_name_distance: u32,
}
fn serialize_color<S: Serializer>(color: &Srgba, serializer: S) -> Result<S::Ok, S::Error> {
    serializer.serialize_str(&color.to_hex())
}
fn deserialize_color<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Srgba, D::Error> {
    let s = String::deserialize(deserializer)?;
    Srgba::hex(s).map_err(|e| serde::de::Error::custom(e.to_string()))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub overlay: bool,
    pub overlay_key: KeyCode,
    pub close_layout_after: f32,
    pub refresh_market_after: u64,
    pub show_corner_boxes: f32,
    pub font_size: f32,
    pub show_keys: bool,
    pub save_to_disk: bool,
    pub layouts: Vec<LayoutOption>,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            overlay: true,
            overlay_key: KeyCode::Equal,
            close_layout_after: 14.5,
            refresh_market_after: 60 * 60 * 24 * 2, // 2 days
            show_corner_boxes: 5.,
            font_size: 18.0,
            show_keys: false,
            save_to_disk: false,
            layouts: vec![LayoutOption {
                aspect_ratio: [16, 9],
                pixel_checks: vec![],
                config: Layout {
                    offset: UVec2::new(478, 411),
                    size: UVec2::new(965, 49),
                    reference_resolution: UVec2::new(1920, 1080),
                    theme_text_color: Srgba::hex("#bea966").unwrap(), // vitruvian
                    item_name_distance: 90,
                },
            }],
        }
    }
}

impl Config {
    pub fn find_matching_layout(&self, image: &image::RgbaImage) -> Option<&Layout> {
        self.layouts
            .iter()
            .find(|variant| variant.matches(image))
            .map(|variant| &variant.config)
    }

    /// Find all matching config variants (useful for debugging)
    pub fn find_all_matching_layouts(&self, image: &image::RgbaImage) -> Vec<&LayoutOption> {
        self.layouts
            .iter()
            .filter(|variant| variant.matches(image))
            .collect()
    }
}

#[derive(Resource)]
pub struct ConfigManager {
    pub config: Config,
    original_doc: DocumentMut,
}
impl Deref for ConfigManager {
    type Target = Config;

    fn deref(&self) -> &Self::Target {
        &self.config
    }
}
impl DerefMut for ConfigManager {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.config
    }
}
impl FromWorld for ConfigManager {
    fn from_world(world: &mut bevy::ecs::world::World) -> Self {
        match Self::load() {
            Ok(conf) => conf,
            Err(e) => {
                if std::fs::exists(PATH).unwrap_or(true) {
                    error!(
                        "Encountered {e} while loading config, config renamed to .bak and recreated"
                    );

                    if !std::fs::exists(BAK_PATH).unwrap_or(true) {
                        if std::fs::rename(PATH, PATH.replace(".toml", ".bak.toml")).is_err() {
                            error!(
                                "Could not backup invalid config file: renaming to {BAK_PATH} failed. Exiting."
                            );
                            world.write_message(AppExit::error());
                            return Self::blank();
                        };
                    } else {
                        error!(
                            "Could not backup invalid config file: {BAK_PATH} already exists. Exiting."
                        );
                        world.write_message(AppExit::error());
                        return Self::blank();
                    };
                }
                Self::new_from_default()
            }
        }
    }
}
const PATH: &str = concat!(env!("CARGO_PKG_NAME"), ".toml");
const BAK_PATH: &str = concat!(env!("CARGO_PKG_NAME"), ".bak.toml");

impl ConfigManager {
    fn blank() -> Self {
        Self {
            config: default(),
            original_doc: default(),
        }
    }

    fn new_from_default() -> Self {
        let conf = Config::default();
        let mut doc = toml_edit::ser::to_document(&conf).unwrap();
        doc.decor_mut().set_prefix("# Config for wf_overlay\n");
        doc.get_mut("overlay_key").map(|i| {
            i.as_table_mut().map(|t| {
                t.decor_mut()
                    .set_prefix("# Available keys can be found in src/input.rs\n")
            })
        });
        let mut this = Self {
            config: conf,
            original_doc: doc,
        };
        this.merge_and_save().unwrap();
        this
    }
    fn load() -> Result<Self> {
        let src = std::fs::read_to_string(PATH)?;
        let original_doc: DocumentMut = src.parse()?;
        let cfg: Config = toml_edit::de::from_document(original_doc.clone())?;
        Ok(Self {
            config: cfg,
            original_doc,
        })
    }
    fn merge_and_save(&mut self) -> Result<()> {
        let src_doc: DocumentMut = toml_edit::ser::to_document(&self.config)?;
        Self::merge_tables(self.original_doc.as_table_mut(), src_doc.as_table());
        std::fs::write(PATH, self.original_doc.to_string())?;
        Ok(())
    }

    fn merge_tables(dst: &mut Table, src: &Table) {
        for (key, src_item) in src.iter() {
            match dst.get_mut(key) {
                Some(dst_item) => Self::merge_items(dst_item, src_item),
                None => {
                    // Key only in src → clone item with its formatting
                    dst.insert(key, src_item.clone());
                }
            }
        }
    }

    fn layout_option_key(tbl: &Table) -> Option<String> {
        tbl.get("aspect_ratio")
            .and_then(Item::as_value)
            .and_then(Value::as_str)
            .map(|s| s.to_string())
    }

    fn merge_items(dst: &mut Item, src: &Item) {
        let other = match src.clone().into_table().map(Item::Table) {
            Ok(i) => i,
            Err(i) => i,
        };
        let src = match other.into_array_of_tables().map(Item::ArrayOfTables) {
            Ok(i) => i,
            Err(i) => i,
        };
        match (dst, &src) {
            // recurse
            (Item::Table(dst_tbl), Item::Table(src_tbl)) => {
                Self::merge_tables(dst_tbl, src_tbl);
            }

            // Array of tables (e.g. [[foo]])
            (Item::ArrayOfTables(dst_aot), Item::ArrayOfTables(src_aot)) => {
                // If first element on either side is missing unique_key, overwrite fully
                let dst_first_key = dst_aot.iter().next().and_then(Self::layout_option_key);
                let src_first_key = src_aot.iter().next().and_then(Self::layout_option_key);

                if dst_first_key.is_none() && src_first_key.is_none() {
                    dst_aot.clear();
                    for src_tbl in src_aot.iter() {
                        dst_aot.push(src_tbl.clone());
                    }
                    return;
                }

                let mut dst_index: HashMap<String, usize> = dst_aot
                    .iter()
                    .enumerate()
                    .flat_map(|(i, tbl)| Self::layout_option_key(tbl).map(|k| (k, i)))
                    .collect();

                let src_keys: HashSet<String> =
                    src_aot.iter().flat_map(Self::layout_option_key).collect();

                dst_aot.retain(|tbl| match Self::layout_option_key(tbl) {
                    None => false,
                    Some(key) => src_keys.contains(&key),
                });

                for src_tbl in src_aot.iter() {
                    let src_key = Self::layout_option_key(src_tbl)
                        .expect("Source table should never be missing the key at this point");

                    if let Some(&dst_pos) = dst_index.get(&src_key) {
                        if let Some(dst_tbl) = dst_aot.get_mut(dst_pos) {
                            for (key, src_item) in src_tbl.iter() {
                                match dst_tbl.get_mut(key) {
                                    Some(dst_item) => Self::merge_items(dst_item, src_item),
                                    None => {
                                        dst_tbl.insert(key, src_item.clone());
                                    }
                                }
                            }
                        }
                    } else {
                        // append new entries
                        dst_aot.push(src_tbl.clone());
                        // and update dst_index
                        let new_idx = dst_aot.len() - 1;
                        dst_index.insert(src_key, new_idx);
                    }
                }
            }

            // Primitive values or mismatched kinds:
            // overwrite value but try to preserve any inline comment already on dst
            (Item::Value(dst_val), Item::Value(src_val)) => {
                let decor = dst_val.decor().clone();

                *dst_val = src_val.clone();
                *dst_val.decor_mut() = decor; // restore decor
            }

            // fallback: replace fully
            (dst_item, src_item) => {
                *dst_item = src_item.clone();
            }
        }
    }
}
