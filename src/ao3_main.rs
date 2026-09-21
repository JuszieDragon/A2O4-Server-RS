mod ao3;
mod config;
mod sftp;

use ao3::common::DownloadFormat;
use ao3::series::Series;
use ao3::user::User;
use ao3::work::Work;
use config::{read_config, Device};

use anyhow::{Ok, Result};
use std::{
    env::current_dir,
    path::{Path, PathBuf},
};

fn main() -> Result<()> {
    let config = read_config();

    let user: Option<User> =
        if let (Some(username), Some(password)) = (&config.ao3_username, &config.ao3_password) {
            Some(User::new(username, password))
        } else {
            None
        };

    let kindle = <Vec<Device> as Clone>::clone(&config.devices)
        .into_iter()
        .find(|x| x.name == "Kindle")
        .unwrap();

    let work = Work::parse_work("123457", user.as_ref(), &config).unwrap();
    //let series = Series::parse_series("1742392", user.as_ref(), &config).unwrap();

    let _ = work.download(Path::new(&config.download_path), DownloadFormat::EPUB, None);
    //let _ = series.download(Path::new(&config.download_path), DownloadFormat::EPUB);
    
    sftp::upload_work(&work, &kindle, &config, DownloadFormat::EPUB, None, None);
    //sftp::upload_series(&series, &kindle, &config, DownloadFormat::EPUB);

    //println!("{}", series);
    //println!("{}", work);

    Ok(())
}
