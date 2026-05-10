use std::{error::Error, path::PathBuf, str::FromStr};

use crate::searchengine::SearchEngineA;
use crate::TorrentFile;
use log::{debug, info};
use native_tls::TlsConnector;
use postgres::config::SslMode;
use postgres::types::ToSql;
use postgres::{Client, Config, NoTls};
use postgres_native_tls::MakeTlsConnector;

use super::SearchEngine;

/// Rows per `INSERT`; fewer round-trips than one statement per file (PostgreSQL allows up to
/// 65535 parameters per statement; 3 columns → stay well under that limit).
const FILE_INFO_INSERT_BATCH: usize = 1000;

fn flush_file_info_batch(
    client: &mut Client,
    batch: &mut Vec<(String, i64, Option<i64>)>,
) -> Result<(), Box<dyn Error>> {
    if batch.is_empty() {
        return Ok(());
    }
    let n = batch.len();
    let mut query = String::with_capacity(64 + n * 24);
    query.push_str("INSERT INTO public.file_info (path, size, hash) VALUES ");
    for i in 0..n {
        if i > 0 {
            query.push_str(", ");
        }
        let p = i * 3 + 1;
        use std::fmt::Write as _;
        write!(&mut query, "(${}, ${}, ${})", p, p + 1, p + 2).expect("string write");
    }
    let mut params: Vec<&(dyn ToSql + Sync)> = Vec::with_capacity(n * 3);
    for (path, size, hash) in batch.iter() {
        params.push(path);
        params.push(size);
        params.push(hash);
    }
    client.execute(&query, &params)?;
    batch.clear();
    Ok(())
}

fn connect_postgres(url: &str) -> Result<Client, Box<dyn Error>> {
    let config = Config::from_str(url)?;
    // URIs omitting sslmode default to `prefer`. Using TLS for `prefer` fails on many local
    // Postgres installs (broken certs); tokio-postgres uses plaintext when tls is NoTls.
    match config.get_ssl_mode() {
        SslMode::Require => {
            let tls = TlsConnector::new()?;
            Ok(config.connect(MakeTlsConnector::new(tls))?)
        }
        SslMode::Disable | SslMode::Prefer => Ok(config.connect(NoTls)?),
        _ => Ok(config.connect(NoTls)?),
    }
}

pub struct Postgresql {
    // TODO: использовать специальный тип для url
    postgresql_url: String,
}

impl SearchEngineA for Postgresql {
    fn init_db(
        &self,
        files: impl Iterator<Item = (PathBuf, u64, Option<u32>)>,
    ) -> Result<(), Box<dyn Error>> {
        info!("Connect to postgresql");
        let mut client = connect_postgres(&self.postgresql_url)?;

        info!("Create table public.file_info");
        // https://kotiri.com/2018/01/31/postgresql-diesel-rust-types.html
        client.batch_execute(
            "
        CREATE TABLE IF NOT EXISTS public.file_info (
            id                    SERIAL PRIMARY KEY,
            path                  TEXT NOT NULL UNIQUE,
            size                  BIGINT NOT NULL,
            hash                  BIGINT,
            bitmagnet_created_at  TIMESTAMPTZ
            )
        ",
        )?;

        info!("Create function public.get_files_by_size_and_extension");
        client.batch_execute("
        CREATE OR REPLACE FUNCTION public.get_files_by_size_and_extension(p_size bigint, p_extension text)
            RETURNS TABLE(file_id integer, path text, size bigint, hash bigint)
            LANGUAGE plpgsql
            AS $function$
            BEGIN
                RETURN QUERY
                SELECT DISTINCT on (f.hash) *
                FROM public.file_info f
                WHERE f.size = p_size AND f.path LIKE '%' || p_extension
                order by f.hash, f.id;
            END;
            $function$;
        ")?;

        info!("Insert data");
        let mut batch: Vec<(String, i64, Option<i64>)> = Vec::with_capacity(FILE_INFO_INSERT_BATCH);
        for (i, (path, size, hash)) in files.enumerate() {
            let path = path
                .to_str()
                .ok_or_else(|| format!("path is not valid UTF-8: {:?}", path))?
                .to_string();
            let hash = hash.map(|h| h as i64);
            batch.push((path, size as i64, hash));
            if batch.len() >= FILE_INFO_INSERT_BATCH {
                flush_file_info_batch(&mut client, &mut batch)?;
            }
            if i % 10 == 0 {
                info!("Insert files: {}", i);
            }
        }
        flush_file_info_batch(&mut client, &mut batch)?;

        Ok(())
    }
}

impl SearchEngine for Postgresql {
    fn connect(&mut self) -> Result<(), Box<dyn Error>> {
        info!("TODO connect");
        // let file = std::fs::File::open(self.filedatabase.clone())?;
        // self.file_map = serde_json::from_reader(file)?;
        Ok(())
    }

    // TODO: надо вернуть, так как это может помочь другим реализациям поискового движка, torrent: &Torrent
    fn search(&self, file: &TorrentFile) -> Box<dyn Iterator<Item = PathBuf> + '_> {
        let file_extension = file.path.extension().unwrap_or_default();
        debug!(
            "Search file. Path: {:?}, Extension: {:?}, Size: {}",
            file.path, file_extension, file.size
        );

        debug!("Connect to postgresql");
        // TODO: убрать unwrap, сделать нормальный возврат ошибок
        let mut client = connect_postgres(&self.postgresql_url).unwrap();

        debug!("Get paths from postgresql");
        // TODO: i64 - не знаю верно или нет
        // TODO: заменить unwrap
        // TODO: к сожалению, сортировка идёт по полному пути
        // TODO: можно добавить булевую переменную, что проверять файлы только с таким же расширением или нет - должно много лишних попыток отсечь
        let paths = client
            .query(
                "
            SELECT path
            FROM public.get_files_by_size_and_extension($1, $2);
            ",
                &[&(file.size as i64), &file_extension.to_str()],
            )
            .unwrap()
            .into_iter()
            .map(|row| {
                let path: String = row.get(0);
                PathBuf::from(path)
            });

        debug!("Returned paths: {:?}", paths);
        return Box::new(paths);
    }
}

impl Postgresql {
    pub fn new(postgresql_url: String) -> Postgresql {
        Postgresql { postgresql_url }
    }
}
