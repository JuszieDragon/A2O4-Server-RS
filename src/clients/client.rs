use crate::{
    clients::{crosspoint::Crosspoint, sftp::Sftp},
    common::DownloadFormat,
    config::{Config, Device},
    domain::{series::Series, work::Work},
};

use anyhow::{Context, Error, Result};
use enum_dispatch::enum_dispatch;
use serde::Deserialize;
use std::{
    fs::File,
    io::Read,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, Deserialize)]
#[enum_dispatch(Client)]
pub enum Clients {
    Crosspoint(Crosspoint),
    Sftp(Sftp),
}

#[enum_dispatch]
pub trait Client {
    async fn upload_work(
        &self,
        work: &Work,
        device: &Device,
        config: &Config,
        download_format: DownloadFormat,
        series: Option<&Series>,
    ) -> Result<()>;

    async fn upload_series(
        &self,
        series: &Series,
        device: &Device,
        config: &Config,
        download_format: DownloadFormat,
    ) -> Result<()>;

    fn get_file_with_size(
        &self,
        work: &Work,
        series: Option<&Series>,
        filename: &str,
        download_path: &str,
    ) -> Result<(Vec<u8>, u64), Error> {
        let file_path = if let Some(unwrapped_series) = series {
            Path::new(&download_path)
                .join(unwrapped_series.title.clone())
                .join(filename)
        } else {
            Path::new(&download_path).join(filename)
        };

        let mut file = File::open(&file_path).with_context(|| {
            format!(
                "Failed to open file {} for work {}",
                file_path.display(),
                work.title
            )
        })?;
        let mut file_contents = Vec::new();
        file.read_to_end(&mut file_contents).with_context(|| {
            format!(
                "Failed to read file {} for work {}",
                file_path.display(),
                work.title
            )
        })?;
        Ok((file_contents, file.metadata()?.len()))
    }

    fn generate_remote_path(
        &self,
        work: Option<&Work>,
        series: Option<&Series>,
        filename: Option<String>,
        remote_download_folder: &str,
    ) -> PathBuf {
        let mut remote_file_path = PathBuf::from(remote_download_folder);

        if let Some(unwrapped_series) = series {
            remote_file_path.push(unwrapped_series.filtered_fandom.clone());
            if unwrapped_series.filtered_fandom == "Original Work" {
                remote_file_path.push(unwrapped_series.creators.first().unwrap().clone());
            }
            remote_file_path.push(unwrapped_series.title.clone());
        } else {
            let unwrapped_work = work.unwrap();
            remote_file_path.push(unwrapped_work.filtered_fandom.clone());
            if unwrapped_work.filtered_fandom == "Original Work" {
                remote_file_path.push(unwrapped_work.authors.first().unwrap().clone());
            }
        }
        if let Some(unwrapped_filename) = filename {
            remote_file_path.push(unwrapped_filename);
        }

        remote_file_path
    }
}
