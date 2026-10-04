use crate::{
    clients::{client::Clients, crosspoint::Crosspoint, sftp::Sftp},
    common::DownloadFormat,
};
use derive_builder::Builder;
use serde::{Deserialize, Deserializer};
use std::{
    collections::{BTreeMap, HashMap},
    fs::File,
    io::Read,
    path::PathBuf,
};

#[derive(Builder, Debug, Default, Deserialize)]
#[builder(default)]
pub struct Config {
    pub port: u16,
    pub download_dir: String,
    pub state_dir: String,
    pub default_format: DownloadFormat,
    pub devices: Vec<Device>,
    pub fandom_map: HashMap<String, String>,
    #[serde(deserialize_with = "deserialize_fandom_filter")]
    pub fandom_filters: Vec<(String, Vec<String>)>,
}

#[derive(Deserialize)]
struct FandomFilterEntry(BTreeMap<String, Vec<String>>);

fn deserialize_fandom_filter<'de, D>(
    deserializer: D,
) -> Result<Vec<(String, Vec<String>)>, D::Error>
where
    D: Deserializer<'de>,
{
    let entries = Vec::<FandomFilterEntry>::deserialize(deserializer)?;

    let flattened = entries
        .into_iter()
        .filter_map(|entry| entry.0.into_iter().next())
        .collect();

    Ok(flattened)
}

impl Config {
    pub fn get_device_by_name(&self, name: &str) -> Option<&Device> {
        self.devices.iter().find(|d| d.name == name)
    }

    pub fn get_devices(&self, names: Vec<String>) -> Result<Vec<&Device>, String> {
        names
            .into_iter()
            .map(|x| self.get_device_by_name(&x).ok_or(x))
            .collect()
    }

    pub fn get_cookies_path(&self) -> PathBuf {
        PathBuf::from(&self.state_dir).join("cookies.json")
    }
}

#[derive(Debug, Deserialize, Clone)]
pub struct Device {
    pub name: String,
    pub ip: String,
    pub port: u16,
    pub username: String,
    pub password: String,
    pub upload_dir: String,
    // pub uses_koreader: Option<bool>,
    #[serde(deserialize_with = "deserialize_client")]
    pub client: Clients,
}

fn deserialize_client<'de, D>(deserializer: D) -> Result<Clients, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;

    match s.as_str() {
        "sftp" => Ok(Clients::Sftp(Sftp {})),
        "crosspoint" => Ok(Clients::Crosspoint(Crosspoint {})),
        _ => Err(serde::de::Error::custom(format!("invalid client: '{s}'"))),
    }
}

pub async fn read_config(path: Option<String>) -> Result<Config, String> {
    let config_path = path.unwrap_or("/var/lib/a2o4-server/config.toml".to_owned());

    let Ok(mut file) = File::open(&config_path) else {
        return Err(format!(
            "Failed to open config file at {}, make sure the file exists and has the right permissions",
            config_path
        ));
    };
    let mut file_contents = String::new();
    let read_result = file.read_to_string(&mut file_contents);
    if read_result.is_err() {
        return Err(read_result.err().unwrap().to_string());
    }

    match toml::from_str::<Config>(&file_contents) {
        Ok(config) => Ok(config),
        Err(error) => Err(error.to_string()),
    }
}
