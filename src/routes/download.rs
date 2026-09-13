use rocket::{http::Status, serde::json::Json, State};
use rocket_db_pools::Connection;
use serde::Deserialize;
use std::{collections::HashSet, path::Path};
use url::Url;

use crate::{
    common::{self, DownloadFormat, PageType},
    config, db,
    domain::{series::Series, user, work::Work},
    A2O4Db,
};

#[derive(Deserialize)]
pub struct RawDownloadRequest {
    url: String,
    devices_to_upload_to: Option<Vec<String>>,
    devices_to_queue: Option<Vec<String>>,
    fandom_override: Option<String>,
    format: Option<DownloadFormat>,
}

pub struct DownloadRequest {
    url: String,
    devices_to_upload_to: Option<Vec<String>>,
    devices_to_queue: Option<Vec<String>>,
    fandom_override: Option<String>,
    format: Option<DownloadFormat>,
}

impl TryFrom<RawDownloadRequest> for DownloadRequest {
    type Error = &'static str;

    fn try_from(raw: RawDownloadRequest) -> Result<Self, Self::Error> {
        let has_upload_devices = raw.devices_to_upload_to.is_some();
        let has_queue_devices = raw.devices_to_queue.is_some();

        match (has_upload_devices, has_queue_devices) {
            (false, false) => {
                Err("Must supply device(s) to upload to and/or to queue an upload for")
            }
            (true, true) => {
                let set: HashSet<&String> =
                    raw.devices_to_upload_to.as_ref().unwrap().iter().collect();
                let is_dup = raw
                    .devices_to_queue
                    .as_ref()
                    .unwrap()
                    .iter()
                    .any(|x| set.contains(x));

                if is_dup {
                    return Err("Must not have the same device in the upload and queue lists");
                }

                Ok(DownloadRequest {
                    url: raw.url,
                    devices_to_upload_to: raw.devices_to_upload_to,
                    devices_to_queue: raw.devices_to_queue,
                    fandom_override: raw.fandom_override,
                    format: raw.format,
                })
            }
            _ => Ok(DownloadRequest {
                url: raw.url,
                devices_to_upload_to: raw.devices_to_upload_to,
                devices_to_queue: raw.devices_to_queue,
                fandom_override: raw.fandom_override,
                format: raw.format,
            }),
        }
    }
}

#[post("/download", format = "json", data = "<request>")]
pub async fn download(
    request: Json<RawDownloadRequest>,
    mut db: Connection<A2O4Db>,
    user: &State<user::User>,
    config: &State<config::Config>,
) -> (Status, String) {
    let validated_request: DownloadRequest = match request.into_inner().try_into() {
        Ok(valid) => valid,
        Err(error) => return (Status::BadRequest, error.to_string()),
    };

    let Ok(url) = Url::parse(&validated_request.url) else {
        return (
            Status::BadRequest,
            String::from("Could not parse provided URL"),
        );
    };

    let url_info = match common::parse_url(&url) {
        Ok(url_info) => url_info,
        Err(error) => return (Status::BadRequest, error.to_string()),
    };

    let devices_to_upload_to = match &validated_request.devices_to_upload_to {
        Some(devices) => match config.get_devices(devices.clone()) {
            Ok(devices) => devices,
            Err(device) => {
                return (
                    Status::BadRequest,
                    format!("Could not find device {}", device),
                )
            }
        },
        None => Vec::new(),
    };

    let devices_to_queue = match &validated_request.devices_to_queue {
        Some(devices) => match config.get_devices(devices.clone()) {
            Ok(devices) => devices,
            Err(device) => {
                return (
                    Status::BadRequest,
                    format!("Could not find device {}", device),
                )
            }
        },
        None => Vec::new(),
    };

    let download_format = validated_request.format.unwrap_or(config.default_format);

    match user.write_cookies() {
        Ok(()) => {}
        Err(error) => {
            return (
                Status::InternalServerError,
                format!("File error while writing cookies: {error}"),
            )
        }
    }

    match url_info.page_type {
        PageType::Work => {
            let work = match Work::parse_work(
                url_info.id,
                user,
                config,
                validated_request.fandom_override.clone(),
            )
            .await
            {
                Ok(work) => work,
                Err(error) => return (Status::BadRequest, error.to_string()),
            };
            match work
                .download(
                    Path::new(&config.download_path),
                    download_format,
                    None,
                    user,
                )
                .await
            {
                Ok(_) => (),
                Err(error) => return (Status::BadRequest, error.to_string()),
            }
            match db::insert_work(&mut **db, &work).await {
                Ok(_) => (),
                Err(error) => return (Status::InternalServerError, error.to_string()),
            }
            if !devices_to_upload_to.is_empty() {
                let upload_result = work
                    .upload_to_devices(config, devices_to_upload_to, download_format)
                    .await;
                if let Err(error) = upload_result {
                    return (Status::BadGateway, error.to_response_string());
                };
            }
            if !devices_to_queue.is_empty() {
                let queue_result = db::insert_queue(&mut db, devices_to_queue, true, work.id).await;
                if let Err(error) = queue_result {
                    return (
                        Status::InternalServerError,
                        format!("Failed to add to queue {}", error),
                    );
                }
            }
        }
        PageType::Series => {
            let series = match Series::parse_series(url_info.id, user, config).await {
                Ok(series) => series,
                Err(error) => return (Status::BadRequest, error.to_string()),
            };
            match series
                .download(Path::new(&config.download_path), download_format, user)
                .await
            {
                Ok(_) => (),
                Err(error) => return (Status::BadRequest, error.to_string()),
            }
            match db::insert_series(&mut db, &series).await {
                Ok(_) => (),
                Err(error) => return (Status::InternalServerError, error.to_string()),
            }
            if !devices_to_upload_to.is_empty() {
                let upload_result = series
                    .upload_to_devices(config, devices_to_upload_to, download_format)
                    .await;
                if let Err(error) = upload_result {
                    return (Status::BadGateway, error.to_response_string());
                };
            }
            if !devices_to_queue.is_empty() {
                let queue_result =
                    db::insert_queue(&mut db, devices_to_queue, false, series.id).await;
                if let Err(error) = queue_result {
                    return (
                        Status::InternalServerError,
                        format!("Failed to add to queue {}", error),
                    );
                }
            }
        }
    }

    (
        Status::Ok,
        format!(
            "Successfully downloaded {} with id {}",
            url_info.page_type, url_info.id
        ),
    )
}
