use crate::{
    config::Device,
    domain::{
        series::Series,
        work::{SeriesLink, Work},
    },
};

use core::slice;
use rocket_db_pools::sqlx;
use sqlx::{prelude::FromRow, Acquire, QueryBuilder, Row, Sqlite, SqliteConnection};
use std::collections::HashMap;
use strum_macros::{Display, EnumString};

#[derive(Display, EnumString, sqlx::Type)]
#[strum(serialize_all = "snake_case")]
#[sqlx(rename_all = "lowercase")]
pub enum TagType {
    Fandom,
    Characters,
    Relationships,
    Additional,
}

#[derive(FromRow)]
struct Tags {
    fandoms: Vec<String>,
    characters: Vec<String>,
    relationships: Vec<String>,
    additional: Vec<String>,
}

impl From<Vec<(TagType, String)>> for Tags {
    fn from(vec: Vec<(TagType, String)>) -> Self {
        let mut fandoms: Vec<String> = Vec::new();
        let mut characters: Vec<String> = Vec::new();
        let mut relationships: Vec<String> = Vec::new();
        let mut additional: Vec<String> = Vec::new();

        for (tag_type, tag) in vec {
            match tag_type {
                TagType::Fandom => fandoms.push(tag),
                TagType::Characters => characters.push(tag),
                TagType::Relationships => relationships.push(tag),
                TagType::Additional => additional.push(tag),
            }
        }

        Tags {
            fandoms,
            characters,
            relationships,
            additional,
        }
    }
}

pub async fn insert_work<'a, A>(
    //Allows this function to be called with either a raw connection or a transaction
    attachable: A,
    work: &Work,
) -> Result<(), sqlx::Error>
where
    A: Acquire<'a, Database = Sqlite>,
{
    let mut tx = attachable.begin().await?;

    //TODO check if work is already downloaded before inserting
    sqlx::query("INSERT OR IGNORE INTO work (id, title, filtered_fandom) VALUES ($1, $2, $3)")
        .bind(work.id)
        .bind(&work.title)
        .bind(&work.filtered_fandom)
        .execute(&mut *tx)
        .await?;

    let author_ids = insert_authors(&mut tx, &work.authors).await?;
    insert_work_author_link(&mut tx, &author_ids, work.id).await?;

    let tag_ids = insert_tags(
        &mut tx,
        &work.fandoms,
        &work.characters,
        &work.relationships,
        &work.additional_tags,
    )
    .await?;
    insert_work_tags_links(&mut tx, tag_ids, work.id).await?;

    insert_work_series_link(&mut tx, slice::from_ref(work)).await?;

    tx.commit().await?;

    Ok(())
}

pub async fn get_work(db: &mut SqliteConnection, work_id: i64) -> Result<Work, sqlx::Error> {
    let work = sqlx::query("SELECT * FROM work WHERE id = ?")
        .bind(work_id)
        .fetch_one(&mut *db)
        .await?;

    let authors = get_linked_authors(&mut *db, work_id, true).await?;

    let tags = get_tags(&mut *db, work_id, true).await?;

    let series_links = get_work_series_link(&mut *db, work_id).await?;

    Ok(Work {
        id: work_id,
        title: work.get("title"),
        authors,
        download_links: HashMap::new(),
        fandoms: tags.fandoms,
        filtered_fandom: work.get("filtered_fandom"),
        relationships: tags.relationships,
        characters: tags.characters,
        additional_tags: tags.additional,
        series: series_links,
    })
}

pub async fn insert_series(db: &mut SqliteConnection, series: &Series) -> Result<(), sqlx::Error> {
    let mut tx = db.begin().await?;

    sqlx::query("
        INSERT INTO series (id, title, begun, updated, description, num_words, num_works, is_completed, num_bookmarks, filtered_fandom)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
    ")
        .bind(series.id)
        .bind(&series.title)
        .bind(&series.begun)
        .bind(&series.updated)
        .bind(&series.description)
        .bind(series.num_words)
        .bind(series.num_works)
        .bind(series.is_completed)
        .bind(series.num_bookmarks)
        .bind(&series.filtered_fandom)
        .execute(&mut *tx)
        .await?;

    let author_ids = insert_authors(&mut tx, &series.creators).await?;
    insert_series_author_link(&mut tx, &author_ids, series.id).await?;

    let tag_ids = insert_tags(
        &mut tx,
        &series.fandoms.clone().into_iter().collect(),
        &Vec::new(),
        &Vec::new(),
        &Vec::new(),
    )
    .await?;
    insert_series_tags_links(&mut tx, tag_ids, series.id).await?;

    for work in &series.works {
        insert_work(&mut tx, work).await?;
    }

    insert_work_series_link(&mut tx, &series.works).await?;

    tx.commit().await?;

    Ok(())
}

//TODO pass in a connection pool so all the works can be fetched concurrently
pub async fn get_series(db: &mut SqliteConnection, series_id: i64) -> Result<Series, sqlx::Error> {
    let series = sqlx::query("SELECT * FROM series WHERE id = ?")
        .bind(series_id)
        .fetch_one(&mut *db)
        .await?;

    let series_authors = sqlx::query_scalar::<_, String>(
        "
        SELECT author.name FROM series_author_link link
        JOIN author ON author.id = link.author
        WHERE link.series = ?
    ",
    )
    .bind(series_id)
    .fetch_all(&mut *db)
    .await?;

    let fandoms = get_tags(&mut *db, series_id, false).await?.fandoms;
    println!("Loaded fandoms");

    let works = get_works_linked_to_series(&mut *db, series_id).await?;
    println!("Loaded works");

    Ok(Series {
        id: series_id,
        title: series.get("title"),
        creators: series_authors,
        begun: series.get("begun"),
        updated: series.get("updated"),
        description: series.get("description"),
        num_words: series.get("num_words"),
        num_works: series.get("num_works"),
        is_completed: series.get("is_completed"),
        num_bookmarks: series.get("num_bookmarks"),
        works,
        fandoms: fandoms.into_iter().collect(),
        filtered_fandom: series.get("filtered_fandom"),
    })
}

async fn insert_work_series_link(
    tx: &mut SqliteConnection,
    works: &[Work],
) -> Result<(), sqlx::Error> {
    let series_work_links: Vec<(&i64, &SeriesLink)> = works
        .iter()
        .flat_map(|work| work.series.values().map(|link| (&work.id, link)))
        .collect();

    let mut query_builder: QueryBuilder<Sqlite> =
        QueryBuilder::new("INSERT OR IGNORE INTO work_series_link (work, series, part_in_series) ");

    query_builder.push_values(series_work_links, |mut query, link| {
        query
            .push_bind(link.0)
            .push_bind(link.1.series_id)
            .push_bind(link.1.part_in_series);
    });

    query_builder.build().execute(tx).await?;

    Ok(())
}

async fn get_works_linked_to_series(
    db: &mut SqliteConnection,
    series_id: i64,
) -> Result<Vec<Work>, sqlx::Error> {
    let work_ids: Vec<i64> = sqlx::query_scalar(
        "
        SELECT work.id FROM work
        JOIN work_series_link link ON link.work = work.id
        WHERE link.series = ?
    ",
    )
    .bind(series_id)
    .fetch_all(&mut *db)
    .await?;

    println!("got work ids");

    let mut works = Vec::with_capacity(work_ids.len());
    for id in work_ids {
        let work = get_work(&mut *db, id).await?;
        works.push(work);
    }

    Ok(works)
}

async fn get_work_series_link(
    db: &mut SqliteConnection,
    work_id: i64,
) -> Result<HashMap<i64, SeriesLink>, sqlx::Error> {
    let series_links = sqlx::query_as::<_, SeriesLink>(
        "
        SELECT link.series as series_id, series.title as series_title, link.part_in_series
        FROM work_series_link link
        JOIN series ON series.id = link.series
        WHERE link.work = ?
    ",
    )
    .bind(work_id)
    .fetch_all(db)
    .await?;

    println!("got work series link");

    Ok(series_links
        .into_iter()
        .map(|link| (link.series_id, link))
        .collect())
}

async fn insert_authors(
    tx: &mut SqliteConnection,
    authors: &[String],
) -> Result<Vec<i64>, sqlx::Error> {
    let mut query_builder: QueryBuilder<Sqlite> = QueryBuilder::new("INSERT INTO author (name) ");

    query_builder.push_values(authors, |mut query, author| {
        query.push_bind(author);
    });

    query_builder
        //Do a dummy update so sqlite will still return the id for an existing author
        .push(" ON CONFLICT(name) DO UPDATE SET name=excluded.name")
        .push(" RETURNING id");

    let ids: Vec<i64> = query_builder.build_query_scalar().fetch_all(tx).await?;

    Ok(ids)
}

async fn get_linked_authors(
    db: &mut SqliteConnection,
    id: i64,
    is_work: bool,
) -> Result<Vec<String>, sqlx::Error> {
    let query_string = format!(
        "
        SELECT author.name FROM author
        JOIN {}_author_link link ON link.author = author.id
        WHERE link.{} = ?
        ",
        if is_work { "work" } else { "series" },
        if is_work { "work" } else { "series" }
    );

    sqlx::query_scalar(&query_string)
        .bind(id)
        .fetch_all(&mut *db)
        .await
}

async fn insert_work_author_link(
    tx: &mut SqliteConnection,
    author_ids: &[i64],
    work_id: i64,
) -> Result<(), sqlx::Error> {
    let mut query_builder: QueryBuilder<Sqlite> =
        QueryBuilder::new("INSERT OR IGNORE INTO work_author_link (work, author) ");

    query_builder.push_values(author_ids, |mut query, author| {
        query.push_bind(work_id).push_bind(author);
    });

    query_builder.build().execute(tx).await?;

    Ok(())
}

async fn insert_series_author_link(
    tx: &mut SqliteConnection,
    author_ids: &[i64],
    series_id: i64,
) -> Result<(), sqlx::Error> {
    let mut query_builder: QueryBuilder<Sqlite> =
        QueryBuilder::new("INSERT OR IGNORE INTO series_author_link (series, author) ");

    query_builder.push_values(author_ids, |mut query, author| {
        query.push_bind(series_id).push_bind(author);
    });

    query_builder.build().execute(tx).await?;

    Ok(())
}

async fn insert_tags(
    tx: &mut SqliteConnection,
    fandoms: &Vec<String>,
    characters: &Vec<String>,
    relationships: &Vec<String>,
    additional: &Vec<String>,
) -> Result<Vec<i64>, sqlx::Error> {
    let mut all_tags = Vec::new();

    for tag in fandoms {
        all_tags.push((TagType::Fandom.to_string(), tag));
    }
    for tag in characters {
        all_tags.push((TagType::Characters.to_string(), tag));
    }
    for tag in relationships {
        all_tags.push((TagType::Relationships.to_string(), tag));
    }
    for tag in additional {
        all_tags.push((TagType::Additional.to_string(), tag));
    }

    if all_tags.is_empty() {
        return Ok(Vec::new());
    }

    let mut query_builder: QueryBuilder<Sqlite> =
        QueryBuilder::new("INSERT INTO tag (type, name) ");

    query_builder.push_values(all_tags, |mut query, (tag_type, tag_name)| {
        query.push_bind(tag_type).push_bind(tag_name);
    });

    query_builder
        .push(" ON CONFLICT(name) DO UPDATE SET name=excluded.name")
        .push(" RETURNING id");

    let ids: Vec<i64> = query_builder.build_query_scalar().fetch_all(tx).await?;

    Ok(ids)
}

async fn insert_work_tags_links(
    tx: &mut SqliteConnection,
    tag_ids: Vec<i64>,
    work_id: i64,
) -> Result<(), sqlx::Error> {
    let mut query_builder: QueryBuilder<Sqlite> =
        QueryBuilder::new("INSERT OR IGNORE INTO work_tag_link (work, tag) ");

    query_builder.push_values(tag_ids, |mut query, tag_id| {
        query.push_bind(work_id).push_bind(tag_id);
    });

    query_builder.build().execute(tx).await?;

    Ok(())
}

async fn insert_series_tags_links(
    tx: &mut SqliteConnection,
    tag_ids: Vec<i64>,
    series_id: i64,
) -> Result<(), sqlx::Error> {
    let mut query_builder: QueryBuilder<Sqlite> =
        QueryBuilder::new("INSERT OR IGNORE INTO series_tag_link (series, tag) ");

    query_builder.push_values(tag_ids, |mut query, tag_id| {
        query.push_bind(series_id).push_bind(tag_id);
    });

    query_builder.build().execute(tx).await?;

    Ok(())
}

async fn get_tags(db: &mut SqliteConnection, id: i64, is_work: bool) -> Result<Tags, sqlx::Error> {
    let query_string = format!(
        "
        SELECT tag.type, tag.name FROM tag
        JOIN {}_tag_link link ON link.tag = tag.id
        WHERE link.{} = ?
        ",
        if is_work { "work" } else { "series" },
        if is_work { "work" } else { "series" }
    );

    Ok(sqlx::query_as::<_, (TagType, String)>(&query_string)
        .bind(id)
        .fetch_all(&mut *db)
        .await?
        .into())
}

pub async fn insert_queue(
    db: &mut SqliteConnection,
    devices: Vec<&Device>,
    is_work: bool,
    id: i64,
) -> Result<(), sqlx::Error> {
    if devices.is_empty() {
        return Ok(());
    }

    let mut query_builder: QueryBuilder<Sqlite> =
        QueryBuilder::new("INSERT INTO upload_queue (type, device, id_to_upload) ");

    query_builder.push_values(devices, |mut query, device| {
        query
            .push_bind(if is_work { "work" } else { "series" })
            .push_bind(device.name.clone())
            .push_bind(id);
    });

    query_builder.build().execute(db).await?;

    Ok(())
}

pub async fn delete_from_queue(
    db: &mut SqliteConnection,
    device: &String,
    is_work: bool,
    id: i64,
) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE from upload_queue WHERE type = $1 AND device = $2 AND id_to_upload = $3")
        .bind(if is_work { "work" } else { "series" })
        .bind(device)
        .bind(id)
        .execute(db)
        .await?;
    Ok(())
}

pub async fn get_queued_uploads_for_device(
    db: &mut SqliteConnection,
    device: &str,
) -> Result<(Vec<i64>, Vec<i64>), sqlx::Error> {
    let queue = sqlx::query_as::<_, (String, i64)>(
        "SELECT type, id_to_upload FROM upload_queue WHERE device = ?",
    )
    .bind(device)
    .fetch_all(db)
    .await?;

    let mut works: Vec<i64> = Vec::new();
    let mut series: Vec<i64> = Vec::new();

    queue.iter().for_each(|x| {
        if x.0 == "work" {
            works.push(x.1)
        } else {
            series.push(x.1);
        }
    });

    Ok((works, series))
}
