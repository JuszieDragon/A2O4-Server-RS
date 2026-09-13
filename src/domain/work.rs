use crate::{
    clients::client::Client,
    common::{filter_fandoms, get_page, sanitise_string, DownloadFormat, UploadError},
    config::{Config, Device},
    domain::{series::Series, user::User},
};

use anyhow::{Context, Result};
use derive_builder::Builder;
use scraper::{ElementRef, Selector};
use sqlx::prelude::FromRow;
use std::{collections::HashMap, path::Path, str::FromStr};
use tokio::{fs::File, io::AsyncWriteExt};

#[derive(Clone, Debug, FromRow, PartialEq)]
pub struct SeriesLink {
    #[sqlx(try_from = "i64")]
    pub series_id: i64,
    pub series_title: String,
    pub part_in_series: u8,
}

// For works downloaded as a series I could load the details missing from the series page from the epub
// after download
#[derive(Builder, Clone, Debug, Default)]
#[builder(default)]
pub struct Work {
    pub id: i64,
    pub title: String,
    pub authors: Vec<String>,
    pub download_links: HashMap<DownloadFormat, String>,
    pub fandoms: Vec<String>,
    pub filtered_fandom: String,
    pub relationships: Vec<String>,
    pub characters: Vec<String>,
    pub additional_tags: Vec<String>,
    pub series: HashMap<i64, SeriesLink>,
}

impl std::fmt::Display for Work {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(
            f,
            "id: {},\ntitle: {},\nauthors: {:?},\ndownload_links: {:?},\nfandoms: {:?},\nfiltered_fandoms: {:?},\nrelationships: {:?},\ncharacters: {:?},\nadditional_tags: {:?}\nseries: {:?}",
            self.id,
            self.title,
            self.authors,
            self.download_links,
            self.fandoms,
            self.filtered_fandom,
            self.relationships,
            self.characters,
            self.additional_tags,
            self.series
        )
    }
}

impl Work {
    pub fn test_work(
        title: String,
        fandom: String,
        series: Option<String>,
        part_in_series: Option<u8>,
    ) -> Self {
        match series {
            None => Self {
                id: 1,
                title,
                authors: vec![String::new()],
                download_links: HashMap::default(),
                fandoms: vec![],
                filtered_fandom: fandom,
                relationships: vec![],
                characters: vec![],
                additional_tags: vec![],
                series: HashMap::default(),
            },
            Some(unwrapped_series) => Self {
                id: 1,
                title,
                authors: vec![String::new()],
                download_links: HashMap::default(),
                fandoms: vec![],
                filtered_fandom: fandom,
                relationships: vec![],
                characters: vec![],
                additional_tags: vec![],
                series: HashMap::from([(
                    1,
                    SeriesLink {
                        series_id: 1,
                        series_title: unwrapped_series,
                        part_in_series: part_in_series.unwrap(),
                    },
                )]),
            },
        }
    }

    pub fn get_series_link(&self, series_id: i64) -> Option<&SeriesLink> {
        self.series.get(&series_id)
    }

    //TODO maybe pass in whole series and get part in series from that
    pub fn get_filename(&self, format: DownloadFormat, series_id: Option<i64>) -> String {
        let non_series_filename = format!("{}.{}", self.title, format.to_string().to_lowercase());

        if let Some(series_id) = series_id {
            match self.get_series_link(series_id) {
                Some(series_link) => format!(
                    "{} - {}.{}",
                    series_link.part_in_series,
                    self.title,
                    format.to_string().to_lowercase()
                ),
                None => non_series_filename,
            }
        } else {
            non_series_filename
        }
    }

    pub async fn parse_work(
        id: i64,
        user: &User,
        config: &Config,
        fandom_override: Option<String>,
    ) -> Result<Work> {
        println!("loading work {id}");
        let document = get_page(id, None, user).await?;
        println!("Got AO3 response");

        let title_selector = Selector::parse("h2.title.heading").expect("Error parsing title");
        let authors_selector =
            Selector::parse("h3.byline.heading>a").expect("Error parsing author");
        let anonymous_author_selector =
            Selector::parse("h3.byline.heading").expect("Error parsing author");
        let downloads_selector =
            Selector::parse("li.download>ul>li>a").expect("Error parsing download links");
        let fandoms_selector =
            Selector::parse("dd.fandom.tags>ul>li>a").expect("Error parsing fandom tags");
        let relationships_selector = Selector::parse("dd.relationship.tags>ul>li>a")
            .expect("Error parsing relationship tags");
        let characters_selector =
            Selector::parse("dd.character.tags>ul>li>a").expect("Error parsing character tags");
        let additional_tags_selector =
            Selector::parse("dd.freeform.tags>ul>li>a").expect("Error parsing additional tags");
        let part_in_series_selector = Selector::parse("dd.series>span.series>span.position")
            .expect("Error parsing part in series");

        let test = document.html();
        println!("{test}");

        let title: String = document
            .select(&title_selector)
            .next()
            .with_context(|| format!("Could not find title for work {id}"))?
            .text()
            .collect();
        let mut authors: Vec<String> = document
            .select(&authors_selector)
            .map(|x| x.text().collect())
            .collect();
        if authors.is_empty() {
            authors = vec![document
                .select(&anonymous_author_selector)
                .next()
                .expect("Error parsing assumed anonymous author")
                .text()
                .collect()]
        };
        let downloads_popup = document.select(&downloads_selector);
        let download_links: HashMap<DownloadFormat, String> = downloads_popup
            .map(|link| {
                (
                    DownloadFormat::from_str(&link.text().collect::<String>())
                        .expect("Failed to parse download format enum"),
                    format!(
                        "https://archiveofourown.org{}",
                        link.value().attr("href").unwrap()
                    ),
                )
            })
            .collect();
        let fandoms: Vec<String> = document
            .select(&fandoms_selector)
            .map(|x| x.text().collect())
            .collect();
        let relationships: Vec<String> = document
            .select(&relationships_selector)
            .map(|x| x.text().collect())
            .collect();
        let characters: Vec<String> = document
            .select(&characters_selector)
            .map(|x| x.text().collect())
            .collect();
        let additional_tags: Vec<String> = document
            .select(&additional_tags_selector)
            .map(|x| x.text().collect())
            .collect();
        let series_element = document.select(&part_in_series_selector);
        let series_links: HashMap<i64, SeriesLink> = series_element
            .map(|series| {
                let series_name_element = series.child_elements().next().unwrap();
                let series_id = series_name_element
                    .value()
                    .attr("href")
                    .unwrap()
                    .split_terminator('/')
                    .nth(2)
                    .unwrap()
                    .parse::<i64>()
                    .unwrap();
                (
                    series_id,
                    SeriesLink {
                        series_title: sanitise_string(
                            &series_name_element
                                .text()
                                .collect::<String>()
                                .split_whitespace()
                                .filter(|chunk| *chunk != "series")
                                .collect::<Vec<&str>>()
                                .join(" "),
                        ),
                        series_id,
                        part_in_series: series
                            .text()
                            .collect::<String>()
                            .split_whitespace()
                            .nth(1)
                            .unwrap()
                            .parse::<u8>()
                            .unwrap(),
                    },
                )
            })
            .collect();

        println!("Work loaded");

        Ok(Work {
            id,
            title: sanitise_string(&title),
            authors,
            download_links,
            fandoms: fandoms.clone(),
            filtered_fandom: match fandom_override {
                Some(fandom) => fandom,
                None => filter_fandoms(&fandoms, config),
            },
            relationships,
            characters,
            additional_tags,
            series: series_links,
        })
    }

    pub fn parse_work_from_blurb(
        blurb: ElementRef,
        series_name: &str,
        config: &Config,
    ) -> Result<Work> {
        let heading_selector = Selector::parse("h4.heading>a").expect("Error parsing heading");
        let fandoms_selector =
            Selector::parse("h5.fandoms.heading>a.tag").expect("Error parsing fandom tags");
        let relationships_selector =
            Selector::parse("li.relationships>a.tag").expect("Error parsing relationship tags");
        let characters_selector =
            Selector::parse("li.characters>a.tag").expect("Error parsing character tags");
        let additional_tags_selector =
            Selector::parse("li.freeforms>a.tag").expect("Error parsing additional tags");
        let series_selector = Selector::parse("ul.series>li").expect("Error parsing series");

        let mut heading = blurb.select(&heading_selector);
        let title_element = heading.next().context("Could not find title for work")?;
        let id: i64 = title_element
            .attr("href")
            .context("Could not find id for work in blurb")?
            .split_terminator('/')
            .nth(2)
            .context("Could not find id for work in blurb")?
            .parse()
            .context("Could not parse work id into i64")?;
        let title: String = title_element.text().collect();

        println!("  Parsing work {id} - {title}");

        let mut authors: Vec<String> = heading.map(|x| x.text().collect()).collect();
        if authors.is_empty() {
            authors = vec!["Anonymous".to_owned()]
        }

        let download_links: HashMap<DownloadFormat, String> =
            enum_iterator::all::<DownloadFormat>()
                .map(|download_format| {
                    (
                        download_format,
                        format!(
                            "https://download.archiveofourown.org/downloads/{}/work.{}",
                            id,
                            download_format.to_string().to_lowercase()
                        ),
                    )
                })
                .collect();
        let fandoms: Vec<String> = blurb
            .select(&fandoms_selector)
            .map(|fandom| fandom.text().collect())
            .collect();
        let relationships: Vec<String> = blurb
            .select(&relationships_selector)
            .map(|relationship| relationship.text().collect())
            .collect();
        let characters: Vec<String> = blurb
            .select(&characters_selector)
            .map(|character| character.text().collect())
            .collect();
        let additional_tags: Vec<String> = blurb
            .select(&additional_tags_selector)
            .map(|tag| tag.text().collect())
            .collect();
        let series_element = blurb.select(&series_selector);
        let series_links: HashMap<i64, SeriesLink> = series_element
            .map(|series| {
                let mut elements = series.child_elements();
                let part_in_series = elements
                    .next()
                    .unwrap()
                    .text()
                    .collect::<String>()
                    .parse::<u8>()
                    .unwrap();
                let series_id = elements
                    .next()
                    .unwrap()
                    .value()
                    .attr("href")
                    .unwrap()
                    .split_terminator('/')
                    .nth(2)
                    .unwrap()
                    .parse::<i64>()
                    .unwrap();

                (
                    series_id,
                    SeriesLink {
                        series_title: sanitise_string(series_name),
                        series_id,
                        part_in_series,
                    },
                )
            })
            .collect();

        println!("  Work parsed\n");

        Ok(Work {
            id,
            title: sanitise_string(&title),
            authors,
            download_links,
            fandoms: fandoms.clone(),
            filtered_fandom: filter_fandoms(&fandoms, config),
            relationships,
            characters,
            additional_tags,
            series: series_links,
        })
    }

    pub async fn download(
        &self,
        download_folder: &Path,
        format: DownloadFormat,
        series: Option<&Series>,
        user: &User,
    ) -> Result<()> {
        let download_link = self.download_links[&format].clone();
        println!("Download link: {download_link}");

        let work_response = user
            .client
            .get(&download_link)
            .send()
            .await
            .with_context(|| {
                format!("Error downloading work {} from {download_link}", self.title)
            })?;

        if work_response.status() == 525 {
            return Err(anyhow::anyhow!(
                "SSL error trying to download work {}: {work_response:?}",
                self.title
            ));
        } else if !work_response.status().is_success() {
            return Err(anyhow::anyhow!(
                "Unknown error trying to download work {}: {work_response:?}",
                self.title
            ));
        }

        let work = work_response
            .bytes()
            .await
            .with_context(|| format!("Error converting work {} to bytes", self.title))?;
        let download_path = download_folder.join(self.get_filename(format, series.map(|x| x.id)));

        println!("Downloading to: {}", download_folder.to_str().unwrap());

        let mut work_file = File::create(&download_path).await.with_context(|| {
            format!(
                "Error creating file for work {} at {}",
                self.title,
                download_path.display()
            )
        })?;
        work_file.write_all(&work).await.with_context(|| {
            format!(
                "Error writing file for work {} at {}",
                self.title,
                download_path.display()
            )
        })?;
        work_file.flush().await.with_context(|| {
            format!(
                "Error writing file during flush for work {} at {}",
                self.title,
                download_path.display()
            )
        })?;
        Ok(())
    }

    pub async fn upload_to_devices(
        &self,
        config: &Config,
        devices: Vec<&Device>,
        download_format: DownloadFormat,
    ) -> Result<(), UploadError> {
        let mut successful_uploads: Vec<String> = Vec::new();

        for device in devices {
            println!("Uploading to device: {}", device.name);
            let result = device
                .client
                .upload_work(self, device, config, download_format, None);

            if let Err(err) = result.await {
                return Err(UploadError {
                    successes: successful_uploads,
                    failure: format!(
                        "Failed to upload to device \"'{}'\": '{}'",
                        device.name, err
                    ),
                });
            }

            successful_uploads.push(device.name.clone());
        }

        Ok(())
    }
}
