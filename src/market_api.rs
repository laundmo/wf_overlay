pub(crate) use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug)]
pub(crate) struct En1 {
    pub icon: String,
    pub name: String,
    pub thumb: String,
}
#[derive(Serialize, Deserialize, Debug)]
pub(crate) struct I18n1 {
    pub en: En1,
}
#[derive(Serialize, Deserialize, Debug)]
pub(crate) struct MarketItem {
    #[serde(rename = "gameRef")]
    pub gameRef: String,
    pub i18n: I18n1,
    pub id: String,
    pub slug: String,
    pub tags: Vec<String>,
}
#[derive(Serialize, Deserialize, Debug)]
pub(crate) struct ItemsRoot {
    #[serde(rename = "apiVersion")]
    pub api_version: String,
    pub data: Vec<MarketItem>,
    pub error: (),
}

#[derive(Serialize, Deserialize, Debug)]
pub(crate) struct CurrentActivity {
    pub details: String,
    #[serde(rename = "startedAt")]
    pub started_at: String,
    pub r#type: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub(crate) struct MarketUser {
    pub activity: CurrentActivity,
    pub crossplay: bool,
    pub id: String,
    #[serde(rename = "ingameName")]
    pub ingame_name: String,
    #[serde(rename = "lastSeen")]
    pub last_seen: String,
    pub locale: String,
    pub platform: String,
    pub reputation: i64,
    pub slug: String,
    pub status: String,
}
#[derive(Serialize, Deserialize, Debug)]
pub(crate) struct Order {
    #[serde(rename = "createdAt")]
    pub created_at: String,
    pub id: String,
    #[serde(rename = "itemId")]
    pub item_id: String,
    #[serde(rename = "perTrade")]
    pub per_trade: i64,
    pub platinum: i64,
    pub quantity: i64,
    pub r#type: String,
    #[serde(rename = "updatedAt")]
    pub updated_at: String,
    pub user: MarketUser,
    pub visible: bool,
}

#[derive(Serialize, Deserialize, Debug)]
pub(crate) struct TopOrdersData {
    pub buy: Vec<Order>,
    pub sell: Vec<Order>,
}
#[derive(Serialize, Deserialize, Debug)]
pub(crate) struct TopOrdersRoot {
    #[serde(rename = "apiVersion")]
    pub api_version: String,
    pub data: TopOrdersData,
    pub error: (),
}
