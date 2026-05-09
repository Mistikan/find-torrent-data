use std::io::Write;
use std::{collections::HashMap, error::Error, path::PathBuf};

use log::{debug, info};

use super::SearchEngine;
use crate::searchengine::SearchEngineA;
use crate::TorrentFile;

pub struct FileData {
    file_database: String,
    file_map: HashMap<u64, Vec<PathBuf>>,
}

impl SearchEngineA for FileData {
    fn init_db(
        &self,
        files: impl Iterator<Item = (PathBuf, u64, Option<u32>)>,
    ) -> Result<(), Box<dyn Error>> {
        info!("Init file_map");
        let mut file_map: HashMap<u64, Vec<PathBuf>> = HashMap::new();
        for (path, size, hash) in files {
            file_map.entry(size).or_insert_with(Vec::new).push(path);
        }

        info!("Convert file_map to string");
        let file_map = serde_json::to_string_pretty(&file_map)?;

        info!("Save file_map");
        let mut file = std::fs::File::create(self.file_database.clone())?;
        file.write_all(file_map.as_bytes())?;

        Ok(())
    }
}

impl SearchEngine for FileData {
    fn connect(&mut self) -> Result<(), Box<dyn Error>> {
        info!("TODO connect");
        let file = std::fs::File::open(self.file_database.clone())?;
        self.file_map = serde_json::from_reader(file)?;
        Ok(())
    }

    // TODO: надо вернуть, так как это может помочь другим реализациям поискового движка, torrent: &Torrent
    fn search(&self, file: &TorrentFile) -> Box<dyn Iterator<Item = PathBuf> + '_> {
        debug!("Search file. Path: {:?}, Size: {}", file.path, file.size);
        if let Some(paths) = self.file_map.get(&file.size) {
            // Возвращаем итератор по вектору путей
            Box::new(paths.iter().cloned()) // clonned() для получения PathBuf
        } else {
            Box::new(std::iter::empty())
        }
    }
}

impl FileData {
    // TODO: rename fields
    pub fn new(file_database: String) -> FileData {
        FileData {
            file_database,
            file_map: HashMap::new(),
        }
    }
}
