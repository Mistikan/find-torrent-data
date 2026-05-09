use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::fs::File;
use std::io::{Read, Result as IOResult, Seek, SeekFrom, Write};
use std::iter::{zip, Iterator};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::vec;

use clap::{Parser, Subcommand};
use lava_torrent::torrent::v1::Torrent;
use log::{debug, error, info, warn};
use permutator::CartesianProductIterator;
use rayon::prelude::*;
use searchengine::filedata::FileData;
use searchengine::postgresql::Postgresql;
use serde::Serialize;
use sha1::{Digest, Sha1};
use walkdir::WalkDir;

mod logging;
mod searchengine;
use crate::searchengine::SearchEngineA;
use searchengine::{calculate_crc32, SearchEngine};

#[derive(Parser, Debug)]
#[command(version, author, about, long_about = None)]
#[command(
    name = "find-torrent-data",
    // TODO: автор
    author = "Richard Patel <me@terorie.dev>, Sergei Ermeikin <sereja.ermeikin@gmail.com>",
    // TODO: версия - можно ещё хещ и время билда указывать?
    // TODO: и добавить семантик релиз?
    version = "2.0",
    about = "Search for files that are part of a torrent and prepare a directory with links to these files"
)]
struct Args {
    #[command(subcommand)]
    cmd: Commands,
}

// TODO: Есть подозрение, что лучше убрать в отдельный файл
#[derive(clap::ValueEnum, Clone, Debug, Serialize)]
enum SearchEngineType {
    FileData,
    Postgresql,
}

#[derive(clap::ValueEnum, Clone, Debug, Serialize)]
enum SearchEngineCalcHash {
    All,
    Size,
    None,
}

#[derive(Subcommand, Debug, Clone)]
enum Commands {
    GetHash {
        #[arg(short, long)]
        torrent: PathBuf,
    },
    Torrent {
        #[arg(short, long)]
        torrent: PathBuf,

        #[arg(short, long)]
        output: PathBuf,

        // Настройки поискового движка
        #[arg(long)]
        search_engine_type: SearchEngineType,

        // TODO: пока так, а в идеале для каждого движка как-то надо разделять настройки
        #[arg(long)]
        search_engine_settings: String,
    },
    SearchEngine {
        #[arg(name = "type")]
        search_engine_type: SearchEngineType,

        #[arg(short, long)]
        input: Vec<PathBuf>,

        #[arg(short, long, default_value_t = String::from("./output/search.json"))]
        output: String,

        #[arg(long)]
        calc_hash: SearchEngineCalcHash,

        #[arg(long)]
        calc_hash_size: Option<u64>,

        #[arg(short, long, default_value_t = false)]
        follow_links: bool,
    },
}
// TODO: Есть подозрение, что лучше убрать в отдельный файл
fn get_search_engine(
    search_engine_type: SearchEngineType,
    search_engine_settings: String,
) -> Box<(dyn SearchEngine + Send + Sync)> {
    match search_engine_type {
        SearchEngineType::FileData => {
            let mut x = FileData::new(search_engine_settings);
            if let Err(e) = x.connect() {
                error!("{}", e);
                std::process::exit(1);
            }
            return Box::new(x);
        }
        SearchEngineType::Postgresql => {
            let mut x = Postgresql::new(search_engine_settings);
            if let Err(e) = x.connect() {
                error!("{}", e);
                std::process::exit(1);
            }
            return Box::new(x);
        }
    };
}

fn main() -> Result<(), Box<dyn Error>> {
    logging::init_from_env().unwrap();

    let cli = Args::parse();

    match cli.cmd {
        Commands::GetHash { torrent } => {
            match read_torrent_file(&torrent) {
                Ok((torrent_file, _, _)) => {
                    println!("{}", torrent_file.info_hash());
                }
                Err(e) => {
                    error!("Failed to read torrent: {}", e);
                    std::process::exit(1);
                }
            };
        }
        Commands::Torrent {
            torrent,
            output,
            search_engine_type,
            search_engine_settings,
        } => {
            // : Box<dyn SearchEngine>
            let search_engine = get_search_engine(search_engine_type, search_engine_settings);
            // , search_engine
            if let Err(e) = torrent_run(torrent, output, search_engine) {
                error!("{}", e);
                std::process::exit(1);
            }
        }
        Commands::SearchEngine {
            input,
            output,
            search_engine_type,
            calc_hash,
            calc_hash_size,
            follow_links,
        } => {
            let files = input
                .iter()
                .flat_map(|path| {
                    WalkDir::new(path)
                        .follow_links(follow_links)
                        .into_iter()
                        // Print and filter errors
                        .filter_map(|entry| entry.map_err(|err| error!("{}", err)).ok())
                        // Ignore directories
                        .filter(|entry| entry.file_type().is_file())
                        // Get metadata
                        .filter_map(|entry| {
                            entry
                                .metadata()
                                .map_err(|err| error!("{}", err))
                                .ok()
                                .map(|meta| (entry, meta))
                        })
                })
                // Lookup sizes to get matches
                // TODO: есть подозрение, что без move конструкция с file_map и помещением туда значений заработает
                .map(move |(entry, meta)| {
                    let size = meta.len();
                    let path = entry.into_path();
                    let mut hash = None;
                    match calc_hash {
                        SearchEngineCalcHash::All => {
                            hash = match calculate_crc32(&path) {
                                Ok(crc) => Some(crc),
                                Err(e) => {
                                    error!("Error calculating CRC32: {}", e);
                                    None
                                }
                            };
                        }
                        SearchEngineCalcHash::Size => {
                            let calc_hash_size = calc_hash_size.unwrap();
                            match size < calc_hash_size {
                                true => {
                                    hash = match calculate_crc32(&path) {
                                        Ok(crc) => Some(crc),
                                        Err(e) => {
                                            error!("Error calculating CRC32: {}", e);
                                            None
                                        }
                                    };
                                }
                                false => hash = None,
                            }
                        }
                        SearchEngineCalcHash::None => hash = None,
                    }

                    (path, size, hash)
                });

            match search_engine_type {
                SearchEngineType::FileData => {
                    let x = FileData::new(output);
                    if let Err(e) = x.init_db(files) {
                        error!("{}", e);
                        std::process::exit(1);
                    }
                }
                SearchEngineType::Postgresql => {
                    let x = Postgresql::new(output);
                    if let Err(e) = x.init_db(files) {
                        error!("{}", e);
                        std::process::exit(1);
                    }
                }
            }
        }
    }
    Ok(())
}

#[derive(PartialEq, Debug, Clone)]
struct TorrentFile {
    path: PathBuf,
    size: u64,
}

#[derive(serde::Serialize)]
struct TorrentInfo {
    path: PathBuf,
    hash: String,
}

#[derive(serde::Serialize)]
struct Report {
    torrent_info: TorrentInfo,
    files: HashMap<std::path::PathBuf, TorrentFileSearch>,
}

#[derive(Debug, PartialEq, Clone)]
enum TorrentFileSearch {
    FileFound(PathBuf),
    FileNotFound,
    FileNeedSearch,
}

impl serde::Serialize for TorrentFileSearch {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let file_path: Option<&PathBuf> = match self {
            TorrentFileSearch::FileFound(path_buf) => Some(path_buf),
            TorrentFileSearch::FileNotFound => None,
            TorrentFileSearch::FileNeedSearch => unreachable!(),
        };
        file_path.serialize(serializer)
    }
}

#[derive(PartialEq, Debug, Clone)]
struct GroupBlockMultiFile {
    files: Vec<TorrentFile>,
    // Оценка сложности поиска. Чем ниже, тем лучше
    diff_score: usize,
    piece: Vec<u8>,
    file_offset: u64,
}

impl GroupBlockMultiFile {
    // TODO: надо переписать функцию следующим образом
    // У TorrentFile надо добавить path на реальном диске.
    // И просто их посчитать
    fn update_diff_score(&mut self, files_found: &HashSet<PathBuf>) {
        let files_torrent: HashSet<PathBuf> = self.files.iter().map(|x| x.path.clone()).collect();
        let files_torrent_cnt = files_torrent.len();
        // Находим пересечение
        let files_common = files_found.intersection(&files_torrent);
        self.diff_score = files_torrent_cnt - files_common.count();
    }

    fn verify_file<T>(&self, files: Vec<&mut T>, block_size: u64) -> Result<bool, std::io::Error>
    where
        T: Seek + Read,
    {
        let count_files = files.len();

        assert!(count_files > 1);

        let mut flag_first_file = true;
        let mut state = Sha1::new();
        let mut bytes_hashed = 0u64;

        for (index, file) in files.into_iter().enumerate() {
            // Seek to block
            let seek = match flag_first_file {
                true => self.file_offset,
                false => 0,
            };
            flag_first_file = false;
            file.seek(SeekFrom::Start(seek))?;

            // Read data from file
            if index < count_files - 1 {
                bytes_hashed = bytes_hashed + std::io::copy(&mut *file, &mut state)?;
            } else {
                // Last file
                bytes_hashed = bytes_hashed
                    + std::io::copy(&mut file.take(block_size - bytes_hashed), &mut state)?;
            }
        }

        // Compare hashes
        let hash = state.finalize();
        let ret = hash.as_slice() == self.piece;
        debug!("Success block: {}", ret);
        return Ok(ret);
    }
}

#[derive(PartialEq, Debug, Clone)]
struct GroupBlockMultiPiece {
    file: TorrentFile,
    file_found: PathBuf,
    pieces: Vec<Vec<u8>>,
    file_offset: u64,
}

impl GroupBlockMultiPiece {
    fn verify_file<T>(&self, file: &mut T, block_size: u64) -> Result<(u64, u64), std::io::Error>
    where
        T: Seek + Read,
    {
        let (mut success_blocks, mut error_blocks) = (0 as u64, 0 as u64);

        // Seek to block
        file.seek(SeekFrom::Start(self.file_offset))?;

        for piece in self.pieces.iter() {
            // Hash single block
            let mut state = Sha1::new();
            std::io::copy(&mut file.take(block_size), &mut state)?;
            // Compare hashes
            let hash = state.finalize();
            // info!("{:?} - {:?}", hash.as_slice(), piece);
            if hash.as_slice() == piece {
                success_blocks += 1;
            } else {
                error_blocks += 1;
            }
            debug!(
                "Success blocks: {}; error blocks: {}",
                success_blocks, error_blocks
            );
        }
        Ok((success_blocks, error_blocks))
    }
}

fn calculate_groups_block(
    files: &Vec<lava_torrent::torrent::v1::File>,
    piece_length: &u64,
    pieces: &Vec<Vec<u8>>,
) -> (Vec<GroupBlockMultiPiece>, Vec<GroupBlockMultiFile>) {
    let (mut gbmp_vec, mut gbmf_vec): (Vec<GroupBlockMultiPiece>, Vec<GroupBlockMultiFile>) =
        (vec![], vec![]);
    // TODO: удаления в pieces (mut) можно избежать, если не удалять элементы, а перемещать индекс
    let mut pieces = pieces.clone();
    let piece_length = piece_length.clone();

    // Проверяем, что количество pieces равно ожидаемому
    let size_all_files = files.iter().fold(0i64, |sum, file| sum + file.length) as u64;
    let count_pieces_by_files = size_all_files.div_ceil(piece_length) as usize;
    debug_assert!(pieces.len() == count_pieces_by_files);

    let mut piece_length_tmp = 0u64; // Длина piece, уменьшаемая размером файла
    let mut file_offset = 0u64;
    let mut group_block_multifile = GroupBlockMultiFile {
        files: vec![],
        diff_score: 0,
        piece: vec![],
        file_offset,
    };

    for file in files {
        // По какой-то причине file.length является знаковым типом.
        debug_assert!(file.length >= 0);
        let file_length = file.length as u64;

        // Если длина меньше оставшегося блока
        if file_length <= piece_length_tmp {
            // Уменьшаем длину оставшегося блока
            piece_length_tmp = piece_length_tmp - file_length;
            // Добавляем файл в мультифайл
            group_block_multifile.files.push(TorrentFile {
                path: file.path.clone(),
                size: file_length,
            });
        } else {
            let mut file_length_cnt = file_length;
            // Если есть мультифайл
            if group_block_multifile.files.len() > 0 {
                // Закрываем мультифайл
                group_block_multifile.files.push(TorrentFile {
                    path: file.path.clone(),
                    size: file_length,
                });
                // Откусываем начало и указываем смещение файла
                file_length_cnt = file_length - piece_length_tmp;
                file_offset = piece_length_tmp;
                // Пушим его в результат
                gbmf_vec.push(group_block_multifile.clone());
                // Обнуляем для следующих свершений
                group_block_multifile = GroupBlockMultiFile {
                    files: vec![],
                    diff_score: 0,
                    piece: vec![],
                    file_offset: 0u64,
                };
            } else {
                file_offset = 0;
            }
            // Открываем мультиблок
            let count_pieces = (file_length_cnt / piece_length) as usize;
            if count_pieces > 0 {
                let group_block_multipiece = GroupBlockMultiPiece {
                    file: TorrentFile {
                        path: file.path.clone(),
                        size: file_length,
                    },
                    file_found: PathBuf::new(),
                    pieces: pieces.drain(..count_pieces).collect(),
                    file_offset,
                };
                gbmp_vec.push(group_block_multipiece);
            }
            // Открываем мультифайл
            let piece_length_tmp_end = file_length_cnt % piece_length;
            if piece_length_tmp_end > 0 {
                group_block_multifile.files.push(TorrentFile {
                    path: file.path.clone(),
                    size: file_length,
                });
                group_block_multifile.file_offset = file_length - piece_length_tmp_end;
                group_block_multifile.piece = pieces.remove(0);
            };
            // Сколько требуется в следующем файле
            piece_length_tmp = piece_length - piece_length_tmp_end;
        }
    }

    // Закрываем последний блок multifle
    match group_block_multifile.files.len() {
        0 => {}
        1 => {
            let i = GroupBlockMultiPiece {
                file: group_block_multifile.files.remove(0),
                file_found: PathBuf::new(),
                pieces: vec![group_block_multifile.piece],
                file_offset: group_block_multifile.file_offset,
            };
            gbmp_vec.push(i);
        }
        _ => gbmf_vec.push(group_block_multifile),
    }

    // Если торрент состоит из одного файла, то MP блоки требуется объединить в один MP
    if gbmp_vec.len() == 2 {
        let a = gbmp_vec.get(0).unwrap().clone();
        let b = gbmp_vec.get(1).unwrap().clone();
        if a.file == b.file {
            let last_piece = b.pieces.get(0).unwrap().clone();
            let mut gbmp_single = gbmp_vec.get(0).unwrap().clone();
            gbmp_single.pieces.push(last_piece);
            gbmp_vec.clear();
            gbmp_vec.push(gbmp_single);
        }
    }

    debug!("gbmp_vec: {:?}", gbmp_vec);
    debug!("gbmf_vec: {:?}", gbmf_vec);

    // Забрали все pieces
    debug_assert!(pieces.is_empty());

    return (gbmp_vec, gbmf_vec);
}

fn read_torrent_file<P>(
    torrent_path: P,
) -> Result<(Torrent, u64, Vec<lava_torrent::torrent::v1::File>), Box<dyn Error>>
where
    P: AsRef<Path>,
{
    let torrent = Torrent::read_from_file(torrent_path)?;

    // По какой-то причине torrent.piece_length является знаковым типом.
    debug_assert!(torrent.piece_length >= 0);
    let torrent_piece_length = torrent.piece_length as u64;

    let files = match torrent.files.clone() {
        Some(files) => files,
        None => {
            vec![lava_torrent::torrent::v1::File {
                length: torrent.length,
                path: PathBuf::from(torrent.name.clone()),
                extra_fields: None,
            }]
        }
    };

    return Ok((torrent, torrent_piece_length, files));
}

fn get_groups_block(
    files: &Vec<lava_torrent::torrent::v1::File>,
    pieces: &Vec<Vec<u8>>,
    piece_length: &u64,
) -> (Vec<GroupBlockMultiPiece>, Vec<GroupBlockMultiFile>) {
    let (mut gbmp_vec, mut gbmf_vec): (Vec<GroupBlockMultiPiece>, Vec<GroupBlockMultiFile>) =
        (vec![], vec![]);

    if files.is_empty() || pieces.is_empty() {
        return (gbmp_vec, gbmf_vec);
    }

    (gbmp_vec, gbmf_vec) = calculate_groups_block(files, piece_length, pieces);

    // Sort increase gbmp by count piece
    gbmp_vec.sort_by(|a, b| b.pieces.len().cmp(&a.pieces.len()));

    // Sort decrease gbmf by count files
    // Не требуется, т.к. потом будем сортировать по оценке
    // gbmf.sort_by(|a, b| b.files_torrent.len().cmp(&a.files_torrent.len()));

    info!("MP: {}, MF: {}", gbmp_vec.len(), gbmf_vec.len());
    return (gbmp_vec, gbmf_vec);
}

fn torrent_run(
    torrent_path: PathBuf,
    output_path: PathBuf,
    search_engine: Box<dyn SearchEngine + Send + Sync>,
) -> Result<(), Box<dyn Error>> {
    let (torrent_file, torrent_piece_length, torrent_files) =
        read_torrent_file(&torrent_path).map_err(|e| format!("Failed to read torrent: {}", e))?;
    let torrent_info_hash = torrent_file.info_hash();

    let (gbmp_vec, mut gbmf_vec) =
        get_groups_block(&torrent_files, &torrent_file.pieces, &torrent_piece_length);

    // Словарь PathBuf Torrent = PathBuf Filesystem
    let file_search: HashMap<PathBuf, TorrentFileSearch> = torrent_files
        .into_iter()
        .map(|file| (file.path, TorrentFileSearch::FileNeedSearch))
        .collect();

    // Ставим mutex на file_search
    let file_search = Arc::new(Mutex::new(file_search));

    // Обрабатываем параллельно элементы gbmp_vec
    gbmp_vec
        .par_iter()
        .enumerate()
        .for_each(|(gbmp_index, gbmp)| {
            let gbmp_file_path = gbmp.file.path.clone();
            info!(
                "Search torrent file. Path: {:?}. Size: {}",
                gbmp_file_path, gbmp.file.size
            );
            let mut gbmp_half_successed_path = PathBuf::new();
            let mut gbmp_half_successed_score = 0u64;
            let mut gbmp_flag_search_end = false;
            for (file_index, file_path) in search_engine.search(&gbmp.file).enumerate() {
                let file_path = file_path.as_path();
                info!(
                    "GBMP index: {}; File index: {}. Verify file: {:?}",
                    gbmp_index, file_index, file_path
                );
                // TODO: убрать по всему коду unwrap и знаки вопроса
                let mut file = File::open(file_path).unwrap();
                let (success, error) = gbmp.verify_file(&mut file, torrent_piece_length).unwrap();
                match (success, error) {
                    (_, 0) => {
                        gbmp_flag_search_end = true;
                        info!(
                            "File found! {} => {}",
                            gbmp_file_path.to_string_lossy(),
                            file_path.to_string_lossy()
                        );
                        let mut file_search = file_search.lock().unwrap();
                        file_search.insert(
                            gbmp_file_path.clone(),
                            TorrentFileSearch::FileFound(file_path.to_path_buf()),
                        );
                        break;
                    }
                    (0, _) => {}
                    (_, _) => {
                        if success > gbmp_half_successed_score {
                            gbmp_half_successed_score = success;
                            gbmp_half_successed_path = file_path.to_path_buf();
                        }
                    }
                }
            }
            // Поиск завершён, продолжаем с другим gbmp
            if gbmp_flag_search_end {
                return;
            }
            // Полууспешные файлы или не найден
            match gbmp_half_successed_score > 0 {
                true => {
                    info!(
                        "File found with bad blocks! {} => {}",
                        gbmp_file_path.to_string_lossy(),
                        gbmp_half_successed_path.to_string_lossy()
                    );
                    let mut file_search = file_search.lock().unwrap();
                    file_search.insert(
                        gbmp_file_path,
                        TorrentFileSearch::FileFound(gbmp_half_successed_path),
                    );
                }
                false => {
                    info!("File not found! {}", gbmp_file_path.to_string_lossy());
                    let mut file_search = file_search.lock().unwrap();
                    file_search.insert(gbmp_file_path, TorrentFileSearch::FileNotFound);
                }
            }
        });

    // Снимаем lock для file_search после параллельной обработки
    // TODO: подозреваю, что следующая строка ужасна
    let mut file_search = file_search.lock().unwrap().clone();

    // Если файл не был найден, то ставлю FileNotFound
    let mut file_search_not_found = HashSet::new();
    for gbmp in gbmp_vec.iter() {
        let path = gbmp.file.path.clone();
        let val = file_search.get(&path).unwrap();
        if val == &TorrentFileSearch::FileNeedSearch {
            warn!("Torrent file {} not found!", path.to_string_lossy());
            file_search_not_found.insert(path.clone());
            file_search.insert(path, TorrentFileSearch::FileNotFound);
        }
    }

    // Удаляю варианты, которые не могу проверить
    // Это файлы, которые я не смог найти - тогда и блок нельзя найти, если там этот файл присутствует
    gbmf_vec = gbmf_vec
        .into_iter()
        .filter(|g| {
            let set1: HashSet<PathBuf> = g
                .files
                .iter()
                .map(|f| f.path.clone())
                .clone()
                .into_iter()
                .collect();
            let x: HashSet<&PathBuf> = set1.intersection(&file_search_not_found).collect();
            x.len() == 0
        })
        .collect();

    loop {
        // Если все gbmf блоки обработали, то не имеет смысла работать
        if gbmf_vec.len() == 0 {
            break;
        }

        // Список найденных файлов для сопоставления
        let files_found: HashSet<PathBuf> = file_search
            .iter()
            .filter_map(|(torrent_path, torrent_file_search)| {
                if let TorrentFileSearch::FileFound(_) = torrent_file_search {
                    Some(torrent_path.clone()) // Клонируем PathBuf, чтобы вернуть его
                } else {
                    None
                }
            })
            .collect();

        // Обновляю оценку
        gbmf_vec = gbmf_vec
            .into_iter()
            .map(|mut x| {
                x.update_diff_score(&files_found);
                x
            })
            .collect();

        // Сортирую по оценке, чтобы работать с самым выгодным блоком
        gbmf_vec.sort_by(|a, b| a.diff_score.cmp(&b.diff_score));

        // Беру в обработку самый выгодный блок по оценке
        let gbmf = gbmf_vec.remove(0);
        info!("Looking for files for the block {:x?}", gbmf.piece);

        // Ищу файлы для комбинаций
        // TODO: подумать над идеей, что ищется не файл, а содержимое файла
        // TODO: тогда просто вытащил в память все содержимое блока и его валидируешь
        // TODO: требуется не уникальный файл, а уникальное содержимое
        let mut find_files_combination = vec![];
        let mut flag_files_not_found_candidate = false;
        for file in gbmf.files.iter() {
            let find_files_for_torrent_file = match file_search.get(&file.path).unwrap() {
                TorrentFileSearch::FileFound(path_buf) => vec![path_buf.clone()],
                _ => search_engine.search(file).collect(),
            };
            info!(
                "For file path: {:?} , size: {} found {} files",
                file.path,
                file.size,
                find_files_for_torrent_file.len()
            );
            // Если кол-во найденных файлов равно 0, то мы не сможем проверить блок
            if find_files_for_torrent_file.len() == 0 {
                flag_files_not_found_candidate = true;
                break;
            }
            find_files_combination.push(find_files_for_torrent_file);
        }

        let find_files_combination: Vec<&[PathBuf]> = find_files_combination
            .iter()
            .map(|v| v.as_slice())
            .collect();

        let mut flag_files_not_found = true;
        if !flag_files_not_found_candidate {
            // Bad analog - https://docs.rs/itertools/latest/itertools/trait.Itertools.html#method.cartesian_product
            // No need in analog
            let combinations = CartesianProductIterator::new(&find_files_combination);
            let combinations_len = combinations.len();
            info!("Count combinations: {}", combinations_len);
            // Перебираем комбинации файлов, проверяя каждую
            for (i, find_file) in combinations.enumerate() {
                info!("Check {}/{}", i + 1, combinations_len);
                let mut files: Vec<File> = Vec::new();
                for path in &find_file {
                    let file = File::open(path)?; // Create the file (or open it)
                    files.push(file);
                }

                let mut file_refs: Vec<&mut File> = Vec::new();
                for file in &mut files {
                    file_refs.push(file);
                }

                let result = gbmf.verify_file(file_refs, torrent_piece_length).unwrap();
                match result {
                    true => {
                        info!("Files found!");
                        flag_files_not_found = false;
                        for (torrent_file_path, real_file_path) in zip(
                            gbmf.files
                                .iter()
                                .map(|torrent_file| torrent_file.path.clone()),
                            find_file,
                        ) {
                            file_search.insert(
                                torrent_file_path,
                                TorrentFileSearch::FileFound(real_file_path.to_path_buf()),
                            );
                        }
                        break;
                    }
                    false => {}
                }
            }
        } else {
            info!("Skip check block. Reason: not found candidate file.");
        }

        // Если файлы не были найдены, то требуется поставить FileNotFound
        if flag_files_not_found {
            for file in gbmf.files.iter() {
                // Существуют ли блоки, нуждающиеся в этом файле
                let need_gbmf = gbmf_vec.iter().any(|x| x.files.contains(file));
                // Принимает решение
                match need_gbmf {
                    true => info!(
                        "Ещё есть блоки, где нужен данный файл ({:?}). Пытаем удачу дальше...",
                        file.path
                    ),
                    false => {
                        info!(
                            "Данный файл ({:?}) больше никому не нужен, убираем",
                            file.path
                        );
                        file_search.insert(file.path.clone(), TorrentFileSearch::FileNotFound);
                    }
                }
            }
        }
    }

    // Проверка, что все файлы для поиска были обработаны
    let assert = file_search
        .values()
        .any(|f| f == &TorrentFileSearch::FileNeedSearch);
    assert!(!assert, "Есть блоки с TorrentFileSearch::FileNeedSearch");

    // Сохранение отчёта
    info!("Save report");
    let torrent_info = TorrentInfo {
        path: torrent_path,
        hash: torrent_info_hash,
    };
    let report = Report {
        torrent_info,
        files: file_search,
    };
    let report = serde_json::to_string_pretty(&report)?;
    let mut file = std::fs::File::create(output_path)?;
    file.write_all(report.as_bytes())?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{env, io::Write, vec};

    use lava_torrent::torrent::v1::File;

    use super::*;

    #[test]
    fn group_blocks_1() {
        let success_value = (
            vec![GroupBlockMultiPiece {
                file: TorrentFile {
                    path: PathBuf::from("a.txt"),
                    size: 1,
                },
                file_found: PathBuf::new(),
                pieces: vec![vec![1]],
                file_offset: 0,
            }],
            vec![],
        );

        let files = vec![File {
            length: 1,
            path: PathBuf::from("a.txt"),
            extra_fields: None,
        }];
        let piece_length = 1;
        let pieces = vec![vec![1]];
        let value = calculate_groups_block(&files, &piece_length, &pieces);

        assert_eq!(success_value, value);
    }

    #[test]
    fn group_blocks_2() {
        let success_value = (
            vec![GroupBlockMultiPiece {
                file: TorrentFile {
                    path: PathBuf::from("a.txt"),
                    size: 4,
                },
                file_found: PathBuf::new(),
                pieces: vec![vec![1], vec![2], vec![3], vec![4]],
                file_offset: 0,
            }],
            vec![],
        );

        let files = vec![File {
            length: 4,
            path: PathBuf::from("a.txt"),
            extra_fields: None,
        }];
        let piece_length = 1;
        let pieces = vec![vec![1], vec![2], vec![3], vec![4]];
        let value = calculate_groups_block(&files, &piece_length, &pieces);

        assert_eq!(success_value, value);
    }

    #[test]
    fn group_blocks_3() {
        let success_value = (
            vec![
                GroupBlockMultiPiece {
                    file: TorrentFile {
                        path: PathBuf::from("a.txt"),
                        size: 4,
                    },
                    file_found: PathBuf::new(),
                    pieces: vec![vec![1], vec![2]],
                    file_offset: 0,
                },
                GroupBlockMultiPiece {
                    file: TorrentFile {
                        path: PathBuf::from("b.txt"),
                        size: 4,
                    },
                    file_found: PathBuf::new(),
                    pieces: vec![vec![3], vec![4]],
                    file_offset: 0,
                },
            ],
            vec![],
        );

        let files = vec![
            File {
                length: 4,
                path: PathBuf::from("a.txt"),
                extra_fields: None,
            },
            File {
                length: 4,
                path: PathBuf::from("b.txt"),
                extra_fields: None,
            },
        ];
        let piece_length = 2;
        let pieces = vec![vec![1], vec![2], vec![3], vec![4]];
        let value = calculate_groups_block(&files, &piece_length, &pieces);

        assert_eq!(success_value, value);
    }

    #[test]
    fn group_blocks_4() {
        let success_value = (
            vec![
                GroupBlockMultiPiece {
                    file: TorrentFile {
                        path: PathBuf::from("a.txt"),
                        size: 4,
                    },
                    file_found: PathBuf::new(),
                    pieces: vec![vec![1]],
                    file_offset: 0,
                },
                GroupBlockMultiPiece {
                    file: TorrentFile {
                        path: PathBuf::from("b.txt"),
                        size: 4,
                    },
                    file_found: PathBuf::new(),
                    pieces: vec![vec![3]],
                    file_offset: 2,
                },
            ],
            vec![GroupBlockMultiFile {
                files: vec![
                    TorrentFile {
                        path: PathBuf::from("a.txt"),
                        size: 4,
                    },
                    TorrentFile {
                        path: PathBuf::from("b.txt"),
                        size: 4,
                    },
                ],
                diff_score: 0,
                piece: vec![2],
                file_offset: 3,
            }],
        );

        let files = vec![
            File {
                length: 4,
                path: PathBuf::from("a.txt"),
                extra_fields: None,
            },
            File {
                length: 4,
                path: PathBuf::from("b.txt"),
                extra_fields: None,
            },
        ];
        let piece_length = 3;
        let pieces = vec![vec![1], vec![2], vec![3]];
        let value = calculate_groups_block(&files, &piece_length, &pieces);

        assert_eq!(success_value, value);
    }

    #[test]
    fn group_blocks_5() {
        let success_value = (
            vec![
                GroupBlockMultiPiece {
                    file: TorrentFile {
                        path: PathBuf::from("a.txt"),
                        size: 10,
                    },
                    file_found: PathBuf::new(),
                    pieces: vec![vec![1], vec![2]],
                    file_offset: 0,
                },
                GroupBlockMultiPiece {
                    file: TorrentFile {
                        path: PathBuf::from("b.txt"),
                        size: 15,
                    },
                    file_found: PathBuf::new(),
                    pieces: vec![vec![4], vec![5], vec![6]],
                    file_offset: 2,
                },
            ],
            vec![
                GroupBlockMultiFile {
                    files: vec![
                        TorrentFile {
                            path: PathBuf::from("a.txt"),
                            size: 10,
                        },
                        TorrentFile {
                            path: PathBuf::from("b.txt"),
                            size: 15,
                        },
                    ],
                    diff_score: 0,
                    piece: vec![3],
                    file_offset: 8,
                },
                GroupBlockMultiFile {
                    files: vec![
                        TorrentFile {
                            path: PathBuf::from("b.txt"),
                            size: 15,
                        },
                        TorrentFile {
                            path: PathBuf::from("c.txt"),
                            size: 3,
                        },
                    ],
                    diff_score: 0,
                    piece: vec![7],
                    file_offset: 14,
                },
            ],
        );

        let files = vec![
            File {
                length: 10,
                path: PathBuf::from("a.txt"),
                extra_fields: None,
            },
            File {
                length: 15,
                path: PathBuf::from("b.txt"),
                extra_fields: None,
            },
            File {
                length: 3,
                path: PathBuf::from("c.txt"),
                extra_fields: None,
            },
        ];
        let piece_length = 4;
        let pieces = vec![
            vec![1],
            vec![2],
            vec![3],
            vec![4],
            vec![5],
            vec![6],
            vec![7],
            // vec![8]
        ];
        let value = calculate_groups_block(&files, &piece_length, &pieces);

        assert_eq!(success_value, value);
    }

    #[test]
    fn group_blocks_6() {
        let success_value = (
            vec![
                GroupBlockMultiPiece {
                    file: TorrentFile {
                        path: PathBuf::from("a.txt"),
                        size: 10,
                    },
                    file_found: PathBuf::new(),
                    pieces: vec![vec![1], vec![2]],
                    file_offset: 0,
                },
                GroupBlockMultiPiece {
                    file: TorrentFile {
                        path: PathBuf::from("b.txt"),
                        size: 15,
                    },
                    file_found: PathBuf::new(),
                    pieces: vec![vec![4], vec![5], vec![6]],
                    file_offset: 2,
                },
                GroupBlockMultiPiece {
                    file: TorrentFile {
                        path: PathBuf::from("c.txt"),
                        size: 4,
                    },
                    file_found: PathBuf::new(),
                    pieces: vec![vec![8]],
                    file_offset: 3,
                },
            ],
            vec![
                GroupBlockMultiFile {
                    files: vec![
                        TorrentFile {
                            path: PathBuf::from("a.txt"),
                            size: 10,
                        },
                        TorrentFile {
                            path: PathBuf::from("b.txt"),
                            size: 15,
                        },
                    ],
                    diff_score: 0,
                    piece: vec![3],
                    file_offset: 8,
                },
                GroupBlockMultiFile {
                    files: vec![
                        TorrentFile {
                            path: PathBuf::from("b.txt"),
                            size: 15,
                        },
                        TorrentFile {
                            path: PathBuf::from("c.txt"),
                            size: 4,
                        },
                    ],
                    diff_score: 0,
                    piece: vec![7],
                    file_offset: 14,
                },
            ],
        );

        let files = vec![
            File {
                length: 10,
                path: PathBuf::from("a.txt"),
                extra_fields: None,
            },
            File {
                length: 15,
                path: PathBuf::from("b.txt"),
                extra_fields: None,
            },
            File {
                length: 4,
                path: PathBuf::from("c.txt"),
                extra_fields: None,
            },
        ];
        let piece_length = 4;
        let pieces = vec![
            vec![1],
            vec![2],
            vec![3],
            vec![4],
            vec![5],
            vec![6],
            vec![7],
            vec![8],
        ];
        let value = calculate_groups_block(&files, &piece_length, &pieces);

        assert_eq!(success_value, value);
    }

    #[test]
    fn verify_file_0() {
        let file_name = "verify_file_0";
        let binding = env::var("CARGO_TARGET_TMPDIR");
        let test_tmpdir = binding.as_deref().unwrap_or("/tmp/");
        let file_path = PathBuf::from(test_tmpdir).join(file_name);
        let (success_blocks, error_blocks) = (1, 0);

        let mut file = std::fs::File::create(&file_path).unwrap();
        file.write_all(b"Hello, world!").unwrap();

        let gbmp = GroupBlockMultiPiece {
            file: TorrentFile {
                path: PathBuf::from(file_name),
                size: 0,
            },
            file_found: PathBuf::new(),
            pieces: vec![vec![
                148, 58, 112, 45, 6, 243, 69, 153, 174, 225, 248, 218, 142, 249, 247, 41, 96, 49,
                214, 153,
            ]],
            file_offset: 0,
        };

        let mut file = std::fs::File::open(&file_path).unwrap();
        let (success_blocks_f, error_blocks_f) = gbmp.verify_file(&mut file, 13).unwrap();

        assert_eq!(
            (success_blocks, error_blocks),
            (success_blocks_f, error_blocks_f)
        );
    }

    #[test]
    fn verify_file_1() {
        let file_name_0 = "verify_file_1_0";
        let file_name_1 = "verify_file_1_1";
        let binding = env::var("CARGO_TARGET_TMPDIR");
        let test_tmpdir = binding.as_deref().unwrap_or("/tmp/");
        let file_path_0 = PathBuf::from(test_tmpdir).join(file_name_0);
        let file_path_1 = PathBuf::from(test_tmpdir).join(file_name_1);
        let success_value = true;

        let mut file_0 = std::fs::File::create(&file_path_0).unwrap();
        file_0.write_all(b"Hello, world!").unwrap();

        let mut file_1 = std::fs::File::create(&file_path_1).unwrap();
        file_1.write_all(b"Good bye, world!").unwrap();

        let gbmf = GroupBlockMultiFile {
            files: vec![
                TorrentFile {
                    path: PathBuf::from(file_name_0),
                    size: 0,
                },
                TorrentFile {
                    path: PathBuf::from(file_name_1),
                    size: 0,
                },
            ],
            diff_score: 0,
            // echo -n 'world!Good' | sha1sum
            piece: vec![
                215, 144, 52, 84, 157, 120, 159, 222, 152, 154, 141, 197, 137, 32, 188, 99, 253,
                246, 28, 174,
            ],
            file_offset: 7,
        };

        let mut file_0 = std::fs::File::open(&file_path_0).unwrap();
        let mut file_1 = std::fs::File::open(&file_path_1).unwrap();
        let value = gbmf
            .verify_file(vec![&mut file_0, &mut file_1], 10)
            .unwrap();

        assert_eq!(success_value, value);
    }
}
