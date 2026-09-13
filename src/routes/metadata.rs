use std::{
    collections::HashMap,
    ffi::OsStr,
    fs::{read_dir, File},
    io::Write,
    path::PathBuf,
};

use epub::doc::EpubDoc;
use regex::Regex;
use rocket::{http::Status, State};
use scraper::{ElementRef, Html, Selector};

use crate::{
    common, config,
    domain::work::{SeriesLink, Work},
};

#[get("/meta")]
pub fn meta(config: &State<config::Config>) -> (Status, String) {
    //let mut doc = EpubDoc::new(PathBuf::from("downloads/Another Time.epub")).unwrap();
    let mut doc = EpubDoc::new(PathBuf::from(
        "/home/justin/Downloads/Becoming_Biwa_Hayahide.epub",
    ))
    .unwrap();
    let document = doc.get_current_str().unwrap().0;
    let document_html = Html::parse_document(&document);

    let id_selector = Selector::parse("p.message a").unwrap();
    let title_selector = Selector::parse("p.message b").unwrap();
    let tag_type_selector = Selector::parse("dl.tags dt").unwrap();
    let tag_element_selector = Selector::parse("dl.tags dd").unwrap();
    let tag_name_selector = Selector::parse("a").unwrap();
    let author_selector = Selector::parse("div.byline a").unwrap();

    // println!(
    //     "{}",
    //     String::from_utf8(doc.get_current().unwrap().0).unwrap()
    // );

    let title: String = document_html
        .select(&title_selector)
        .next()
        .unwrap()
        .text()
        .collect();

    let id: i64 = document_html
        .select(&id_selector)
        .nth(1)
        .unwrap()
        .text()
        .collect::<String>()
        .split("/")
        .last()
        .unwrap()
        .parse()
        .unwrap();

    let tag_types = document_html.select(&tag_type_selector);
    let tag_elements = document_html.select(&tag_element_selector);

    let mut fandoms: Vec<String> = Vec::new();
    let mut relationships: Vec<String> = Vec::new();
    let mut characters: Vec<String> = Vec::new();
    let mut additional_tags: Vec<String> = Vec::new();
    let mut series: HashMap<i64, SeriesLink> = HashMap::new();

    for (tag_type_element, tag_element) in tag_types.zip(tag_elements) {
        let tag_type = tag_type_element
            .text()
            .collect::<String>()
            .strip_suffix(":")
            .unwrap()
            .to_string();

        let tags: Vec<String> = tag_element
            .select(&tag_name_selector)
            .map(|x| x.text().collect::<String>())
            .collect();

        match tag_type.as_str() {
            "Fandom" | "Fandoms" => fandoms = tags,
            "Relationship" | "Relationships" => relationships = tags,
            "Characters" => characters = tags,
            "Additional Tags" => additional_tags = tags,
            "Series" => series = parse_series_tag(tag_element),
            "Stats" => {}
            "Rating" | "Archive Warning" | "Category" | "Language" => {}
            unknown => eprintln!("Unknown tag: {}", unknown),
        }
    }

    doc.go_next();
    let document_2 = Html::parse_document(&doc.get_current_str().unwrap().0);

    let authors: Vec<String> = document_2
        .select(&author_selector)
        .map(|x| x.text().collect::<String>())
        .collect();

    let filtered_fandom = common::filter_fandoms(&fandoms, config);

    let work = Work {
        id,
        title,
        authors,
        download_links: HashMap::new(),
        fandoms,
        filtered_fandom,
        relationships,
        characters,
        additional_tags,
        series,
    };

    println!("{}", work);

    (Status::Ok, "Ok".to_string())
}

fn parse_series_tag(series_element: ElementRef) -> HashMap<i64, SeriesLink> {
    let mut part_in_series = 0;
    let mut series_links: HashMap<i64, SeriesLink> = HashMap::new();
    let part_re = Regex::new(r"Part (\d+) of").unwrap();

    for child in series_element.children() {
        if let Some(text_node) = child.value().as_text() {
            if let Some(capture) = part_re.captures(text_node) {
                part_in_series = capture[1].parse().unwrap();
            }
        } else if let Some(element) = child.value().as_element() {
            if element.name() == "a" {
                let series_id: i64 = element
                    .attr("href")
                    .unwrap()
                    .split("/")
                    .last()
                    .unwrap()
                    .parse()
                    .unwrap();
                let series_name: String =
                    scraper::ElementRef::wrap(child).unwrap().text().collect();

                series_links.insert(
                    series_id,
                    SeriesLink {
                        series_id,
                        series_title: series_name,
                        part_in_series,
                    },
                );
            }
        }
    }

    series_links
}
