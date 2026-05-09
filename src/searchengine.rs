use std::{
    error::Error,
    fs::File,
    io::{self, Read},
    path::{Path, PathBuf},
};

use crc32fast::Hasher;

use crate::TorrentFile;

pub mod filedata;
pub mod postgresql;

pub trait SearchEngineA {
    // Методы в search-engine
    // Метод для создания и наполнения базы данных
    fn init_db(
        &self,
        files: impl Iterator<Item = (PathBuf, u64, Option<u32>)>,
    ) -> Result<(), Box<dyn Error>>;
}

// TODO: надо эти trait всё-таки объединить
pub trait SearchEngine {
    // Методы в torrent
    // Метод для первого подключения
    fn connect(&mut self) -> Result<(), Box<dyn Error>>;

    // Метод для поиска файлов
    // TODO: torrent: &Torrent, - надо вернуть, так как это может помочь другим реализациям поискового движка
    fn search(&self, file: &TorrentFile) -> Box<dyn Iterator<Item = PathBuf> + '_>;
}

pub fn calculate_crc32<P>(file_path: &P) -> io::Result<u32>
where
    P: AsRef<Path>,
{
    let mut file = File::open(file_path)?;
    let mut hasher = Hasher::new();
    let mut buffer = [0; 4096]; // Buffer to read the file in chunks

    loop {
        let bytes_read = file.read(&mut buffer)?;
        if bytes_read == 0 {
            break; // End of file
        }
        hasher.update(&buffer[..bytes_read]);
    }

    Ok(hasher.finalize())
}
