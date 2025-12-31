use std::{
    collections::BTreeMap,
    fs::File,
    io::{BufReader, BufWriter, Write},
    time::Duration,
};

use bevy::{platform::collections::HashMap, prelude::*, time::common_conditions::on_real_timer};
use bevy_mod_req::{ReqError, ReqPlugin, ReqRequest, ReqResponse, req_type_plugin};
use serde::{Deserialize, Deserializer, Serialize};
use simsearch::{SearchOptions, SimSearch};

use crate::{
    market_api::{ItemsRoot, TopOrdersRoot},
    ocr::{self, ItemsContainer},
};

const BACKGROUND_FETCH_DELAY: u64 = 8;
const MAX_ITEMS_ESTIMATE: u64 = 1000;
// Overestimate the time needed to fetch everything thrice over
const MAX_AGE: u64 = BACKGROUND_FETCH_DELAY * MAX_ITEMS_ESTIMATE * 3;

pub fn market_plugin(app: &mut App) {
    let req_plugin = ReqPlugin {
        requests_per_second: 3.0,
        make_config: |c| {
            c.timeout_global(None).user_agent(format!(
                "{} {} from: {}",
                env!("CARGO_PKG_NAME"),
                env!("CARGO_PKG_VERSION"),
                env!("CARGO_PKG_REPOSITORY")
            ))
        },
    };
    app.add_plugins(req_plugin)
        .add_plugins(req_type_plugin::<ItemsRoot>)
        .add_plugins(req_type_plugin::<TopOrdersRoot>)
        .insert_resource(DataManager::restore_from_disk_or_empty())
        .add_systems(Startup, setup)
        .add_systems(Update, resolve_items)
        .add_systems(
            Update,
            fetch_oldest.run_if(on_real_timer(Duration::from_secs_f32(8.0))),
        )
        .add_observer(fetch_items)
        .add_observer(insert_new_into_storage)
        .add_observer(|e: On<ReqError>| error!("Request error: {:?}", e.err));
}

#[derive(Component)]
struct ItemsRequestHandler;

#[derive(Component, Deref, DerefMut)]
struct ItemSearchIndex(SimSearch<String>);

fn setup(mut commands: Commands) {
    commands
        .spawn(ItemsRequestHandler)
        .trigger(|entity| {
            ReqRequest::<ItemsRoot>::new(entity, "https://api.warframe.market/v2/items")
        })
        .observe(
            |e: On<ReqResponse<ItemsRoot>>,
             mut commands: Commands,
             mut data: ResMut<DataManager>| {
                let items = &e.data.data;
                let options = SearchOptions::new()
                    .levenshtein(true)
                    .stop_whitespace(false);
                let mut engine: SimSearch<String> = SimSearch::new_with(options);
                engine.insert("".to_string(), "Format Blueprint");
                items
                    .iter()
                    .filter(|i| i.tags.contains(&"prime".to_string()))
                    .for_each(|i| {
                        engine.insert(i.slug.clone(), &i.i18n.en.name);
                        data.insert_unknown(i.slug.clone(), i.ducats);
                    });
                commands.spawn(ItemSearchIndex(engine));
            },
        );
}

fn resolve_items(
    items_index: Single<&ItemSearchIndex>,
    items: Single<(&ItemsContainer, &Children)>,
    query: Query<Ref<ocr::Item>>,
    mut commands: Commands,
) {
    for child in items.1.iter() {
        if let Ok(item) = query.get(child)
            && item.is_changed()
        {
            let results = items_index.search(&item.name);
            if results.is_empty() {
                info!("Unknown item {}, please report", item.name);
                continue;
            }
            let slug = results[0].clone();
            info!("Matched {} as {}", item.name, results[0]);
            if !slug.is_empty() {
                commands.entity(child).insert((Slug(slug), WantsFetch));
            }
        }
    }
}

#[derive(Component)]
pub(crate) struct WantsFetch;

fn fetch_items(
    e: On<Insert, WantsFetch>,
    mut commands: Commands,
    query: Query<&Slug>,
    data: Res<DataManager>,
) {
    let slug = query.get(e.entity).unwrap().0.clone();
    // if cache is good, insert that and off we go!
    if let Some(data) = data.get_if_fresh(&slug) {
        commands
            .entity(e.entity)
            .remove::<WantsFetch>()
            .insert((data.clone(), SkipStore));
        return;
    };
    let ducats = data.get_ducats(&slug);
    info!("Starting fetch for Item: {slug}");
    commands
        .entity(e.entity)
        .trigger(|entity| {
            ReqRequest::<TopOrdersRoot>::new(
                entity,
                format!("https://api.warframe.market/v2/orders/item/{slug}/top"),
            )
        })
        .observe(
            move |e: On<ReqResponse<TopOrdersRoot>>, mut commands: Commands| {
                let (sum, min, max) = e
                    .data
                    .data
                    .sell
                    .iter()
                    .map(|s| s.platinum as f32)
                    .fold((0.0f32, f32::MAX, f32::MIN), |acc, p| {
                        (acc.0 + p, acc.1.min(p), acc.2.max(p))
                    });
                let avg = sum / e.data.data.sell.len() as f32;
                commands
                    .entity(e.entity)
                    .remove::<WantsFetch>()
                    .insert((ItemData {
                        last_fetch: unix_now(),
                        avg,
                        min,
                        max,
                        ducats,
                    },));
            },
        );
}

fn unix_now() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time flows ever onward")
        .as_secs()
}

#[derive(Component, Clone, Debug, Serialize, Deserialize)]
pub struct ItemData {
    last_fetch: u64,
    pub ducats: Option<u32>,
    #[serde(deserialize_with = "deserialize_null_as_nan")]
    pub max: f32,
    #[serde(deserialize_with = "deserialize_null_as_nan")]
    pub min: f32,
    #[serde(deserialize_with = "deserialize_null_as_nan")]
    pub avg: f32,
}
fn deserialize_null_as_nan<'de, D: Deserializer<'de>>(des: D) -> Result<f32, D::Error> {
    let optional = Option::<f32>::deserialize(des)?;
    Ok(optional.unwrap_or(f32::NAN))
}

#[derive(
    Component, Clone, Debug, PartialEq, PartialOrd, Ord, Eq, Serialize, Deserialize, Deref, DerefMut,
)]
pub struct Slug(pub String);

#[derive(Debug, Resource, Serialize, Deserialize, Default)]
struct DataManager {
    map: HashMap<String, ItemData>,
    #[serde(skip)]
    ordered: BTreeMap<u64, String>,
}
impl DataManager {
    fn insert(&mut self, k: String, v: ItemData) {
        let last_fetch = v.last_fetch;
        // If this item already existed, remove it
        if let Some(data) = self.map.get(&k) {
            self.ordered.remove(&data.last_fetch);
        }
        self.map.insert(k.clone(), v);
        self.ordered.insert(last_fetch, k);
    }

    fn insert_unknown(&mut self, k: String, ducats: Option<u32>) {
        if self.map.contains_key(&k) {
            return;
        }
        // info!("New unknown item {k}");
        let next_free = self
            .ordered
            .last_key_value()
            .map(|(i, _)| i + 1)
            .unwrap_or(0u64);
        self.map.insert(
            k.clone(),
            ItemData {
                last_fetch: next_free,
                max: f32::NAN,
                min: f32::NAN,
                avg: f32::NAN,
                ducats,
            },
        );

        self.ordered.insert(next_free, k);
    }

    fn get_oldest(&self) -> Option<(&u64, &String)> {
        self.ordered.first_key_value()
    }

    fn get_ducats(&self, k: &String) -> Option<u32> {
        self.map.get(k).and_then(|i| i.ducats)
    }

    fn get_if_fresh(&self, k: &String) -> Option<&ItemData> {
        let data = self.map.get(k)?;
        if data.last_fetch + MAX_AGE < unix_now() {
            None
        } else {
            Some(data)
        }
    }

    fn save_to_disk(&self) {
        let file = File::create("result.json").unwrap();
        let mut writer = BufWriter::new(file);
        serde_json::to_writer(&mut writer, self).unwrap();
        writer.flush().unwrap();
    }

    fn restore_from_disk_or_empty() -> Self {
        if let Ok(file) = File::open("result.json") {
            let mut reader = BufReader::new(file);
            dbg!("file!");
            if let Ok(mut m) = serde_json::from_reader::<_, Self>(&mut reader) {
                m.ordered = m
                    .map
                    .iter()
                    .map(|i| (i.1.last_fetch, i.0.clone()))
                    .collect();
                dbg!(&m);
                return m;
            }
        }
        Self::default()
    }
}

#[derive(Component, Debug)]
struct RemoveOnStore;

fn fetch_oldest(data: Res<DataManager>, mut commands: Commands, q: Query<&WantsFetch>) {
    // first == smallest == oldest
    if q.is_empty()
        && let Some((age, k)) = data.get_oldest()
    {
        info!("Item: {k} is oldest at -{}s behind", unix_now() - age);
        commands.spawn((Slug(k.clone()), WantsFetch, RemoveOnStore));
    }
}

#[derive(Component)]
struct SkipStore;

fn insert_new_into_storage(
    evt: On<Insert, ItemData>,
    q: Query<(Entity, &Slug, &ItemData, Has<RemoveOnStore>), Without<SkipStore>>,
    mut data: ResMut<DataManager>,
    mut commands: Commands,
) {
    if let Ok((e, slug, item_data, remove_on_store)) = q.get(evt.entity) {
        info!("Got new data for {slug:?}: {item_data:?}");
        data.insert(slug.0.clone(), item_data.clone());
        if remove_on_store {
            commands.entity(e).try_despawn();
        }
        data.save_to_disk();
    };
}
