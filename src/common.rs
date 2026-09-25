use crate::{config::Config, domain::user::User};

use anyhow::{Context, Error, Result};
use enum_iterator::Sequence;
use regex::Regex;
use reqwest::{Response, StatusCode};
use scraper::{Html, Selector};
use serde::{Deserialize, Serialize};
use std::{collections::HashSet, str::FromStr};
use strum_macros::{Display, EnumString};
use url::Url;

#[derive(
    Debug,
    Default,
    EnumString,
    PartialEq,
    Eq,
    Hash,
    Display,
    Sequence,
    Clone,
    Copy,
    Serialize,
    Deserialize,
)]
pub enum DownloadFormat {
    Azw3,
    #[default]
    Epub,
    Mobi,
    Pdf,
    Html,
}

#[derive(EnumString, PartialEq, Debug)]
pub enum PageType {
    #[strum(serialize = "works")]
    Work,
    #[strum(serialize = "series")]
    Series,
}

impl std::fmt::Display for PageType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PageType::Work => write!(f, "work"),
            PageType::Series => write!(f, "series"),
        }
    }
}

#[derive(PartialEq, Debug)]
pub struct UrlInfo {
    pub page_type: PageType,
    pub id: i64,
}

pub struct UploadError {
    pub successes: Vec<String>,
    pub failure: String,
}

impl UploadError {
    pub fn to_response_string(&self) -> String {
        format!(
            "'{}'\nSuccessfully uploaded to device(s) '{}'",
            self.failure,
            self.successes.join(", ")
        )
    }
}

//TODO check for proxy error page, timeout page, check if session is bad
pub async fn get_page(id: i64, page: Option<u8>, user: &User) -> Result<Html> {
    let url = if let Some(i) = page {
        format!("https://archiveofourown.org/series/{id}?page={i}")
    } else {
        format!("https://archiveofourown.org/works/{id}")
    };

    let response = request_with_user(url.clone(), user)
        .await
        .with_context(|| format!("Failed to fetch page {url}"))?;

    if response.url().as_str() == "https://archiveofourown.org/users/login?restricted=true" {
        eprint!("This work/series is restricted and requires an AO3 account");
        return Err(Error::msg("Restricted Error"));
    }

    if response.status() == StatusCode::NOT_FOUND {
        return Err(Error::msg(format!("URL {url} is not a valid page")));
    } else if response.status() == 525 {
        return Err(Error::msg("Cloudflare SSL Error"));
    } else if !response.status().is_success() {
        return Err(Error::msg(format!(
            "Unknown HTTP error {}",
            response.status()
        )));
    }

    println!("response code: {}", response.status());

    let html_content = Html::parse_document(&response.text().await?);

    let errors_selector = Selector::parse("div.errors");

    let error_404: String = match errors_selector {
        Ok(errors_selector) => {
            if let Some(e) = html_content.select(&errors_selector).next() {
                e.text().collect::<String>()
            } else {
                String::new()
            }
        }
        _ => String::new(),
    };

    if error_404.is_empty() {
        Ok(html_content)
    } else {
        Err(Error::msg(format!(
            "Got error {error_404} while fetching {url}"
        )))
    }
}

pub async fn get_series_pages(id: i64, user: &User) -> Result<Vec<Html>> {
    let response = get_page(id, Some(1), user).await?;

    let response_text = response.html();
    let num_pages = if response_text.contains("Pages Navigation") {
        let response_substring = response_text
            .split("Pages Navigation")
            .nth(1)
            .with_context(|| format!("Failed to page selector for series {id}"))?
            .split('\n')
            .next()
            .with_context(|| format!("Failed to page selector for series {id}"))?;

        u8::try_from(
            Regex::new(r">\d+<")?
                .captures_iter(response_substring)
                .count(),
        )
        .with_context(|| format!("Failed to parse num of pages for series {id}"))?
    } else {
        1
    };

    let mut raw_html: Vec<String> = vec![response_text];

    for page in 2..=num_pages {
        let response = get_page(id, Some(page), user)
            .await
            .with_context(|| format!("Failed to fetch series page {page}"))?;
        raw_html.push(response.html());
    }

    Ok(raw_html.iter().map(|a| Html::parse_document(a)).collect())
}

pub fn filter_fandoms(fandoms: &Vec<String>, config: &Config) -> String {
    let mut mapped_fandoms: HashSet<String> = HashSet::from_iter(fandoms.to_owned());

    for fandom in fandoms {
        if config.fandom_map.contains_key(fandom) {
            mapped_fandoms.remove(fandom);
            mapped_fandoms.insert(config.fandom_map.get(fandom).unwrap().clone());
        }
    }

    let mut mapped_and_filtered_fandoms = mapped_fandoms.clone();

    for filter in &config.fandom_filter {
        if mapped_fandoms.contains(&filter.0) & mapped_and_filtered_fandoms.contains(&filter.0) {
            for fandom_to_remove in &filter.1 {
                if fandom_to_remove == "*" {
                    mapped_and_filtered_fandoms = HashSet::from_iter([filter.0.clone()]);
                } else if mapped_fandoms.contains(fandom_to_remove) {
                    mapped_and_filtered_fandoms.remove(fandom_to_remove);
                }
            }
        }
    }

    if mapped_and_filtered_fandoms.len() > 1 {
        "Multiple".to_string()
    } else {
        mapped_and_filtered_fandoms.iter().next().unwrap().clone()
    }
}

//TODO setup rate limit of 12 per minute
async fn request_with_user(
    url: String,
    user: &User,
) -> std::result::Result<Response, reqwest::Error> {
    user.client.get(url).send().await
}

pub fn parse_url(url: &Url) -> Result<UrlInfo> {
    if url.domain() != Some("archiveofourown.org") {
        return Err(Error::msg("Provided URL is not an AO3 URL"));
    }

    let re = Regex::new(r"(?<type>works|series)/(?<id>\d+)")?;
    let Some(caps) = re.captures(url.as_str()) else {
        let message = format!("URL {url} is not for a work or series");
        eprintln!("{message}");
        return Err(Error::msg(message));
    };

    Ok(UrlInfo {
        page_type: PageType::from_str(&caps["type"])
            .with_context(|| format!("Invalid page type {}", &caps["type"]))?,
        id: caps["id"].parse()?,
    })
}

pub fn sanitise_string(string: &str) -> String {
    string
        .trim()
        .chars()
        .filter(|c| !['!', '?', ':', '"'].contains(c))
        .collect::<String>()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ConfigBuilder;
    use std::collections::HashMap;

    #[test]
    fn parse_valid_work_url() {
        assert_eq!(
            parse_url(&Url::parse("https://archiveofourown.org/works/123456").unwrap()).unwrap(),
            UrlInfo {
                page_type: PageType::Work,
                id: 123456
            }
        );
    }

    #[test]
    fn parse_series_url() {
        assert_eq!(
            parse_url(&Url::parse("https://archiveofourown.org/series/123456").unwrap()).unwrap(),
            UrlInfo {
                page_type: PageType::Series,
                id: 123456
            }
        );
    }

    #[test]
    fn parse_work_in_collection_url() {
        assert_eq!(
            parse_url(
                &Url::parse("https://archiveofourown.org/collections/aaaaa/works/654321").unwrap()
            )
            .unwrap(),
            UrlInfo {
                page_type: PageType::Work,
                id: 654321
            }
        );
    }

    #[test]
    fn error_on_non_ao3_url() {
        assert_eq!(
            parse_url(&Url::parse("https://google.com").unwrap())
                .unwrap_err()
                .to_string(),
            "Provided URL is not an AO3 URL"
        );
    }

    #[test]
    fn error_on_invalid_ao3_url() {
        assert_eq!(
            parse_url(&Url::parse("https://archiveofourown.org/users/bob").unwrap())
                .unwrap_err()
                .to_string(),
            "URL https://archiveofourown.org/users/bob is not for a work or series"
        );
    }

    #[test]
    fn map() {
        let config = ConfigBuilder::default()
            .fandom_map(HashMap::from([
                ("Fandom 1 the big boy".to_owned(), "Fandom 1".to_owned()),
                ("Fandom 1 TBB".to_owned(), "Fandom 1".to_owned()),
                (
                    "Fandom 2 the big boy returns".to_owned(),
                    "Fandom 2".to_owned(),
                ),
            ]))
            .build()
            .unwrap();

        assert_eq!(
            filter_fandoms(
                &vec!["Fandom 1 the big boy".to_owned(), "Fandom 1 TBB".to_owned()],
                &config
            ),
            "Fandom 1"
        );
    }

    #[test]
    fn map_lets_unmatched_fandoms_through() {
        let config = ConfigBuilder::default()
            .fandom_map(HashMap::from([
                ("Fandom 1 the big boy".to_owned(), "Fandom 1".to_owned()),
                ("Fandom 1 TBB".to_owned(), "Fandom 1".to_owned()),
                (
                    "Fandom 2 the big boy returns".to_owned(),
                    "Fandom 2".to_owned(),
                ),
            ]))
            .build()
            .unwrap();

        assert_eq!(
            filter_fandoms(
                &vec!["Fandom 4 how is big boy possibly back once again".to_owned()],
                &config
            ),
            "Fandom 4 how is big boy possibly back once again"
        );
    }

    #[test]
    fn map_removes_all() {
        let config = ConfigBuilder::default()
            .fandom_map(HashMap::from([
                ("Fandom 1 the big boy".to_owned(), "Fandom 1".to_owned()),
                ("Fandom 1 TBB".to_owned(), "Fandom 1".to_owned()),
                (
                    "Fandom 2 the big boy returns".to_owned(),
                    "Fandom 2".to_owned(),
                ),
            ]))
            .fandom_filter(vec![("Fandom 1".to_owned(), vec!["*".to_owned()])])
            .build()
            .unwrap();

        assert_eq!(
            filter_fandoms(
                &vec![
                    "Fandom 1".to_owned(),
                    "Fandom 2".to_owned(),
                    "Fandom 3".to_owned(),
                    "Fandom 4".to_owned(),
                ],
                &config
            ),
            "Fandom 1"
        );
    }

    #[test]
    fn map_applies_in_order() {
        let config = ConfigBuilder::default()
            .fandom_map(HashMap::from([
                ("Fandom 1 the big boy".to_owned(), "Fandom 1".to_owned()),
                ("Fandom 1 TBB".to_owned(), "Fandom 1".to_owned()),
                (
                    "Fandom 2 the big boy returns".to_owned(),
                    "Fandom 2".to_owned(),
                ),
            ]))
            .fandom_filter(vec![
                ("Fandom 1".to_owned(), vec!["Fandom 2".to_owned()]),
                ("Fandom 2".to_owned(), vec!["Fandom 1".to_owned()]),
            ])
            .build()
            .unwrap();

        assert_eq!(
            filter_fandoms(
                &vec!["Fandom 1".to_owned(), "Fandom 2".to_owned(),],
                &config
            ),
            "Fandom 1"
        );
    }

    #[test]
    fn filter() {
        let config = ConfigBuilder::default()
            .fandom_filter(vec![
                ("Fandom 1".to_owned(), vec!["Fandom 2".to_owned()]),
                ("Fandom 2".to_owned(), vec!["Fandom 3".to_owned()]),
            ])
            .build()
            .unwrap();

        assert_eq!(
            filter_fandoms(&vec!["Fandom 1".to_owned(), "Fandom 2".to_owned()], &config),
            "Fandom 1"
        );
    }

    #[test]
    fn map_and_filter() {
        let config = ConfigBuilder::default()
            .fandom_map(HashMap::from([
                ("Fandom 1 the big boy".to_owned(), "Fandom 1".to_owned()),
                ("Fandom 1 TBB".to_owned(), "Fandom 1".to_owned()),
                (
                    "Fandom 2 the big boy returns".to_owned(),
                    "Fandom 2".to_owned(),
                ),
            ]))
            .fandom_filter(vec![
                ("Fandom 1".to_owned(), vec!["Fandom 2".to_owned()]),
                ("Fandom 2".to_owned(), vec!["Fandom 3".to_owned()]),
            ])
            .build()
            .unwrap();

        assert_eq!(
            filter_fandoms(
                &vec![
                    "Fandom 1 the big boy".to_owned(),
                    "Fandom 1 TBB".to_owned(),
                    "Fandom 2 the big boy returns".to_owned()
                ],
                &config
            ),
            "Fandom 1"
        );
    }
}
