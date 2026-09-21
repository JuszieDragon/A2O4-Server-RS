use crate::{
    clients::{client::Clients, crosspoint::Crosspoint, sftp::Sftp},
    common::DownloadFormat,
};
use derive_builder::Builder;
use directories::ProjectDirs;
use indexmap::IndexMap;
use serde::Deserialize;
use std::{
    collections::HashMap,
    fs::{create_dir, File},
    io::Read,
};

//TODO consider setting up default values
#[derive(Builder, Debug, Default, Deserialize)]
#[builder(default)]
pub struct Config {
    pub port: u16,
    pub download_path: String,
    pub db_path: String,
    pub ao3_username: Option<String>,
    pub ao3_password: Option<String>,
    pub default_format: DownloadFormat,
    pub devices: Vec<Device>,
    pub fandom_map: HashMap<String, String>,
    pub fandom_filter: IndexMap<String, Vec<String>>,
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
}

#[derive(Debug, Deserialize, Clone)]
pub struct Device {
    pub name: String,
    pub ip: String,
    pub port: u16,
    pub username: String,
    pub password: String,
    pub download_folder: String,
    pub uses_koreader: Option<bool>,
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

pub async fn read_config() -> Result<Config, String> {
    if let Some(proj_dirs) = ProjectDirs::from("", "", env!("CARGO_PKG_NAME")) {
        let config_dir = proj_dirs.config_dir();
        if config_dir.exists() {
            let Ok(mut file) = File::open(config_dir.join("config.toml")) else {
                return Err(format!(
                    "Failed to open config.toml at {}, make sure the file exists and has the right permissions",
                    config_dir.display()
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
        } else {
            create_dir(proj_dirs.config_dir()).unwrap();
            Err(format!(
                "First time run, create a config file at {}",
                config_dir.join("config.toml").display()
            ))
        }
    } else {
        Err(String::from(
            "Failed to get home directory from OS, make sure home path is set correctly in OS",
        ))
    }
}
