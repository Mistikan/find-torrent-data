use std::{error::Error, path::PathBuf, str::FromStr};

use crate::searchengine::SearchEngineA;
use crate::TorrentFile;
use log::{debug, info};
use native_tls::TlsConnector;
use postgres::config::SslMode;
use postgres::{Client, Config, NoTls};
use postgres_native_tls::MakeTlsConnector;

use super::SearchEngine;

fn connect_postgres(url: &str) -> Result<Client, Box<dyn Error>> {
    let config = Config::from_str(url)?;
    match config.get_ssl_mode() {
        SslMode::Disable => Ok(config.connect(NoTls)?),
        _ => {
            let tls = TlsConnector::new()?;
            Ok(config.connect(MakeTlsConnector::new(tls))?)
        }
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
        for (i, (path, size, hash)) in files.enumerate() {
            let hash = match hash {
                Some(h) => Some(h as i64),
                None => None,
            };
            client.execute(
                "INSERT INTO public.file_info (path, size, hash) VALUES ($1, $2, $3)",
                // TODO: не знаю насколько это законно приводить к i64
                &[&path.to_str(), &(size as i64), &(hash)],
            )?;
            if i % 10 == 0 {
                info!("Insert files: {}", i);
            }
        }

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
