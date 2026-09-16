use sqlx::prelude::FromRow;
use strum_macros::{Display, EnumString};

#[derive(Display, EnumString, sqlx::Type)]
#[strum(serialize_all = "snake_case")]
#[sqlx(rename_all = "lowercase")]
pub enum TagType {
    Fandom,
    Characters,
    Relationships,
    Additional,
}

#[derive(FromRow)]
pub struct Tags {
    pub fandoms: Vec<String>,
    pub characters: Vec<String>,
    pub relationships: Vec<String>,
    pub additional: Vec<String>,
}

impl From<Vec<(TagType, String)>> for Tags {
    fn from(vec: Vec<(TagType, String)>) -> Self {
        let mut fandoms: Vec<String> = Vec::new();
        let mut characters: Vec<String> = Vec::new();
        let mut relationships: Vec<String> = Vec::new();
        let mut additional: Vec<String> = Vec::new();

        for (tag_type, tag) in vec {
            match tag_type {
                TagType::Fandom => fandoms.push(tag),
                TagType::Characters => characters.push(tag),
                TagType::Relationships => relationships.push(tag),
                TagType::Additional => additional.push(tag),
            }
        }

        Tags {
            fandoms,
            characters,
            relationships,
            additional,
        }
    }
}
