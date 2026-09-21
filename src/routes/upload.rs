use rocket::{http::Status, serde::json::Json, State};
use rocket_db_pools::Connection;
use serde::Deserialize;

use crate::{
    clients::client::Client,
    common::DownloadFormat,
    config, db,
    domain::{series::Series, work::Work},
    A2O4Db,
};

#[derive(Clone, Deserialize)]
pub struct UploadWorkSeries {
    title: String,
    fandom: String,
}

#[derive(Deserialize)]
pub struct UploadWorkRequest {
    work: String,
    fandom: String,
    series: Option<UploadWorkSeries>,
    part_in_series: Option<u8>,
    devices: Vec<String>,
}

#[post("/upload/work", format = "json", data = "<request>")]
pub async fn upload_work(
    request: Json<UploadWorkRequest>,
    config: &State<config::Config>,
) -> (Status, String) {
    let devices = match config.get_devices(request.devices.clone()) {
        Ok(devices) => devices,
        Err(device) => {
            return (
                Status::BadRequest,
                format!("Could not find device {}", device),
            )
        }
    };

    let work = Work::test_work(
        request.work.clone(),
        request.fandom.clone(),
        request.series.clone().map(|x| x.title),
        request.part_in_series,
    );

    if let Some(unwrapped_series) = &request.series {
        let series = Series::test_series(
            &unwrapped_series.title,
            unwrapped_series.fandom.clone(),
            config,
        )
        .unwrap();

        let upload_result = series
            .upload_to_devices(config, devices, DownloadFormat::EPUB)
            .await;
        if let Err(error) = upload_result {
            return (Status::BadGateway, error.to_response_string());
        };
    } else {
        let upload_result = work
            .upload_to_devices(config, devices, DownloadFormat::EPUB)
            .await;
        if let Err(error) = upload_result {
            return (Status::BadGateway, error.to_response_string());
        };
    }

    (
        Status::Ok,
        format!(
            "Successfully uploaded {} to devices: {}",
            request.work,
            request.devices.join(", ")
        ),
    )
}

#[derive(Deserialize)]
pub struct UploadSeriesRequest {
    series: String,
    fandom: String,
    device: String,
}

#[post("/upload/series", format = "json", data = "<request>")]
pub async fn upload_series(
    request: Json<UploadSeriesRequest>,
    config: &State<config::Config>,
) -> (Status, String) {
    let Some(device) = config.get_device_by_name(&request.device) else {
        return (
            Status::BadRequest,
            format!("Could not find device {}", request.device),
        );
    };

    let series = match Series::test_series(&request.series, request.fandom.clone(), config) {
        Ok(series) => series,
        Err(error) => {
            return (
                Status::InternalServerError,
                format!("Series Upload Error: {error}"),
            )
        }
    };

    let upload_result = device
        .client
        .upload_series(&series, device, config, DownloadFormat::EPUB)
        .await;
    let Ok(()) = upload_result else {
        let error = upload_result.err().unwrap();
        println!("{}", error.root_cause());
        return (Status::BadRequest, error.to_string());
    };

    (
        Status::Ok,
        format!(
            "Successfully uploaded {} to {}",
            request.series, request.device
        ),
    )
}

//TODO fix error on duplicate file upload for crosspoint
#[get("/upload/queue/<device>")]
pub async fn upload_queued_works_and_series(
    device: &str,
    mut db: Connection<A2O4Db>,
    config: &State<config::Config>,
) -> (Status, String) {
    let Some(device) = config.get_device_by_name(device) else {
        return (
            Status::BadRequest,
            format!("Could not find device {}", device),
        );
    };

    let (work_ids, series_ids) =
        match db::get_queued_uploads_for_device(&mut db, &device.name).await {
            Ok(result) => result,
            Err(error) => {
                return (
                    Status::InternalServerError,
                    format!("Failed to read from queue: {}", error),
                )
            }
        };

    for id in &work_ids {
        let work = match db::get_work(&mut db, *id).await {
            Ok(work) => work,
            Err(error) => {
                return (
                    Status::InternalServerError,
                    format!("Failed to load work {} from db: {}", id, error),
                )
            }
        };
        let upload_result = work
            .upload_to_devices(config, vec![device], DownloadFormat::EPUB)
            .await;
        if let Err(error) = upload_result {
            return (Status::BadGateway, error.to_response_string());
        };
        let delete_result = db::delete_from_queue(&mut db, &device.name, true, *id).await;
        if let Err(error) = delete_result {
            return (
                Status::InternalServerError,
                format!(
                    "Failed to delete work {} from upload queue for device {}: {}",
                    id, device.name, error,
                ),
            );
        }
    }

    for id in &series_ids {
        let series = match db::get_series(&mut db, *id).await {
            Ok(series) => series,
            Err(error) => {
                return (
                    Status::InternalServerError,
                    format!("Failed to load series {} from db: {}", id, error),
                )
            }
        };
        let upload_result = series
            .upload_to_devices(config, vec![device], DownloadFormat::EPUB)
            .await;
        match upload_result {
            Ok(_) => {}
            Err(error) => return (Status::BadGateway, error.to_response_string()),
        }
        let delete_result = db::delete_from_queue(&mut db, &device.name, false, *id).await;
        if let Err(error) = delete_result {
            return (
                Status::InternalServerError,
                format!(
                    "Failed to delete series {} from upload queue for device {}: {}",
                    id, device.name, error,
                ),
            );
        }
    }

    (
        Status::Ok,
        format!(
            "Successfully uploaded works {:?} and series {:?} ",
            work_ids, series_ids
        ),
    )
}
