use crate::{
    clients::client::Client,
    common::DownloadFormat,
    config::{Config, Device},
    domain::{series::Series, work::Work},
};

use anyhow::{anyhow, Context, Result};
use reqwest_websocket::{Message, Upgrade};
use rocket::futures::{SinkExt, StreamExt};
use serde::Deserialize;
use std::path::Path;

#[derive(Clone, Debug, Deserialize)]
pub struct Crosspoint {}

impl Client for Crosspoint {
    async fn upload_work(
        &self,
        work: &Work,
        device: &Device,
        config: &Config,
        download_format: DownloadFormat,
        series: Option<&Series>,
    ) -> Result<()> {
        upload_work_bulk(self, work, device, config, download_format, series, false).await?;
        Ok(())
    }

    async fn upload_series(
        &self,
        series: &Series,
        device: &Device,
        config: &Config,
        download_format: DownloadFormat,
    ) -> Result<()> {
        let remote_series_folder =
            self.generate_remote_path(None, Some(series), None, &device.download_folder);

        create_missing_folders_on_remote(device.ip.clone(), &remote_series_folder).await?;

        for work in &series.works {
            upload_work_bulk(
                self,
                work,
                device,
                config,
                download_format,
                Some(series),
                true,
            )
            .await?;
        }

        Ok(())
    }
}

//TODO try out https://lib.rs/crates/fast_websocket_client
//TODO fix trying to upload duplicate file
async fn upload_work_bulk(
    parent: &Crosspoint,
    work: &Work,
    device: &Device,
    config: &Config,
    download_format: DownloadFormat,
    series: Option<&Series>,
    is_bulk: bool,
) -> Result<()> {
    let filename = work.get_filename(download_format, series.map(|x| x.id));
    let (file, size) = parent.get_file_with_size(work, series, &filename, &config.download_path)?;

    let remote_file_path =
        parent.generate_remote_path(Some(work), series, None, &device.download_folder);

    if !is_bulk {
        create_missing_folders_on_remote(device.ip.clone(), &remote_file_path).await?;
    }

    let websocket = reqwest::Client::default()
        .get(format!("ws://{}:{}", device.ip, device.port))
        .upgrade()
        .send()
        .await?
        .into_websocket()
        .await?;

    let (mut sink, mut stream) = websocket.split();

    let start_message = format!(
        "START:{filename}:{size}:{}",
        remote_file_path.to_str().with_context(|| {
            format!("failed to covert file path '{remote_file_path:?}' to string",)
        })?
    );

    println!("{start_message}");

    let _write_task = tokio::spawn(async move {
        sink.send(Message::Text(start_message)).await.unwrap();

        let chunk_size = 8192;

        for chunk in file.chunks(chunk_size) {
            let message = Message::Binary(chunk.to_vec().into());
            if let Some(error) = sink.send(message).await.err() {
                return Err(anyhow!("Failed to send chunk with error: {error}"));
            }
        }

        Ok(())
    });

    let title = work.title.clone();
    let device_name = device.name.clone();
    let read_task = tokio::spawn(async move {
        while let Some(Ok(message)) = stream.next().await {
            if let Message::Text(text) = message {
                println!("Received: {text}");

                if text.starts_with("DONE") {
                    break;
                } else if text.starts_with("ERROR") {
                    return Err(anyhow!(
                        "Error uploading work {} with filename {} to device '{}': {}",
                        title,
                        filename,
                        device_name,
                        text.split_once(':').unwrap().1
                    ));
                }
            }
        }

        Ok(())
    });

    read_task.await?
}

pub async fn create_missing_folders_on_remote(ip: String, path_to_create: &Path) -> Result<()> {
    let remote_file_ancestors = path_to_create.ancestors().collect::<Vec<&Path>>();
    let remote_file_iterator = remote_file_ancestors.iter().rev().skip(1);

    let client = reqwest::Client::new();
    let mut parent_path = String::new();

    for path in remote_file_iterator {
        let path_str = path.file_name().unwrap().to_str().unwrap();
        let params = [("name", path_str), ("path", &parent_path)];

        let response = client
            .post(format!("http://{ip}/mkdir"))
            .form(&params)
            .send()
            .await?;

        match response.status() {
            reqwest::StatusCode::OK => {}
            reqwest::StatusCode::BAD_REQUEST => {
                println!(
                    "Got: '{}' when trying to create folder {path_str} with parent_path {parent_path}",
                    response.text().await?,
                );
            }
            _ => return Err(anyhow!(response.text().await?)),
        }
        parent_path = parent_path + "/" + path_str;
    }

    Ok(())
}
