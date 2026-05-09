# TODO: нарисовать документацию как использовать в баш скриптах
# TODO: когда закончу программу, отписаться в треде qbittorrent
# TODO: скрипты превратить в одну историю, чтобы ими было удобно пользоваться самому
# TODO: например, сначала поиск и переименование в хеш, потом поиск файлов и сохранение отчёта, после копирование данных по отчёту
# Search-engine
```sh
RUST_LOG=info cargo run -- search-engine postgresql \
  --input /home/user/Загрузки/torrents \
  --output postgresql://postgres:postgres@localhost:7432/find_torrent_data \
  --calc-hash size --calc-hash-size 8388608
```

# Torrent
```sh
RUST_LOG=info cargo run -- torrent \
  --torrent "/home/user/Загрузки/torrents/torrent-file/Sabaton 2016 - The Last Stand (Limited Edition) - (EAC-FLAC).torrent" \
  --search-engine-type postgresql \
  --search-engine-settings postgresql://postgres:changeme@192.168.88.210/find-torrent-data
```
