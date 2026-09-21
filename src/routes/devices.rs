use rocket::{http::Status, serde::json::Json, State};

use crate::config;

type JsonResponse<T> = Result<Json<T>, (Status, String)>;

#[get("/devices")]
pub async fn get_devices(config: &State<config::Config>) -> JsonResponse<Vec<String>> {
    Ok(Json(
        config.devices.iter().map(|x| x.name.clone()).collect(),
    ))
}
