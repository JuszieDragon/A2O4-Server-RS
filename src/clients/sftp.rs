use crate::{
    clients::client::Client,
    common::DownloadFormat,
    config::{Config, Device},
    domain::{series::Series, work::Work},
};

use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::Path;
use std::{io::Write, net::TcpStream};

#[derive(Clone, Debug, Deserialize)]
pub struct Sftp {}

impl Client for Sftp {
    async fn upload_work(
        &self,
        work: &Work,
        device: &Device,
        config: &Config,
        download_format: DownloadFormat,
        series: Option<&Series>,
    ) -> Result<()> {
        upload_work_bulk(self, work, device, config, download_format, None, series)?;

        Ok(())
    }

    async fn upload_series(
        &self,
        series: &Series,
        device: &Device,
        config: &Config,
        download_format: DownloadFormat,
    ) -> Result<()> {
        let sftp = create_sftp_connection(device)?;

        let remote_series_folder =
            self.generate_remote_path(None, Some(series), None, &device.download_folder);

        create_missing_folders_on_remote(&sftp, &remote_series_folder)?;

        for work in &series.works {
            upload_work_bulk(
                self,
                work,
                device,
                config,
                download_format,
                Some(&sftp),
                Some(series),
            )?;
        }
        Ok(())
    }
}

fn upload_work_bulk(
    parent: &Sftp,
    work: &Work,
    device: &Device,
    config: &Config,
    download_format: DownloadFormat,
    existing_sftp: Option<&ssh2::Sftp>,
    series: Option<&Series>,
) -> Result<()> {
    let using_existing_connection = existing_sftp.is_some();

    let sftp = if using_existing_connection {
        existing_sftp.unwrap()
    } else {
        &create_sftp_connection(device)?
    };

    let filename = work.get_filename(download_format, series.map(|x| x.id));
    let (file, size) = parent.get_file_with_size(work, series, &filename, &config.download_path)?;

    println!("Starting to upload file: {}", filename);
    println!("file is {size} bytes");

    let remote_file_path =
        parent.generate_remote_path(Some(work), series, Some(filename), &device.download_folder);

    if !using_existing_connection {
        create_missing_folders_on_remote(sftp, remote_file_path.parent().unwrap())?;
    }

    let mut remote_file = sftp.create(Path::new(&remote_file_path)).with_context(|| {
        format!(
            "Failed to create remote file {} for work {}",
            remote_file_path.to_str().unwrap(),
            work.title
        )
    })?;

    let chunk_size = 15000;

    for chunk in file.chunks(chunk_size).enumerate() {
        remote_file.write_all(chunk.1).with_context(|| {
            format!(
                "Failed while writing remote file chunk for work {}",
                work.title
            )
        })?;
    }

    Ok(())
}

fn create_missing_folders_on_remote(sftp: &ssh2::Sftp, path_to_create: &Path) -> Result<()> {
    let remote_file_ancestors = path_to_create.ancestors().collect::<Vec<&Path>>();
    let remote_file_iterator = remote_file_ancestors.iter().rev().skip(1);

    for path in remote_file_iterator {
        if sftp.lstat(path).is_err() {
            sftp.mkdir(path, 0o777).with_context(|| {
                format!(
                    "Failed to make missing folder {} on remote for work",
                    path.display()
                )
            })?;
        }
    }

    Ok(())
}

fn create_sftp_connection(device: &Device) -> Result<ssh2::Sftp> {
    let tcp = TcpStream::connect((device.ip.clone(), device.port)).with_context(|| {
        format!(
            "Failed to connect to device {} at {}:{}",
            device.name, device.ip, device.port
        )
    })?;
    let mut session = ssh2::Session::new().with_context(|| {
        format!(
            "Failed to setup SSH session for device {} at {}:{}",
            device.name, device.ip, device.port
        )
    })?;
    session.set_tcp_stream(tcp);
    session.handshake().with_context(|| {
        format!(
            "Failed handshake with device {} at {}:{}",
            device.name, device.ip, device.port
        )
    })?;
    session
        .userauth_password(&device.username, &device.password)
        .with_context(|| {
            format!(
                "Failed to authenticate with device {} at {}:{}",
                device.name, device.ip, device.port
            )
        })?;
    session.set_blocking(true);
    session.sftp().with_context(|| {
        format!(
            "Failed to initialise SFTP with device {} at {}:{}",
            device.name, device.ip, device.port
        )
    })
}
