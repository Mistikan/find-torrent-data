#!/bin/bash

set -e

export INPUT_DIRECTORY="/var/mnt/zfs/pool_data/data"
export REPORT_DIR="/var/mnt/zfs/pool_data/torrent_find_data_report/"
export SEARCH_ENGINE_TYPE="postgresql"
export SEARCH_ENGINE_SETTINGS="postgresql://postgres:changeme@192.168.88.210/find-torrent-data"
export BAD_TORRENT_FILES="/var/mnt/zfs/pool_data/bad_torrent_files.txt"


# echo "Create search database"
# find-torrent-data search-engine $SEARCH_ENGINE_TYPE \
#   --input $INPUT_DIRECTORY \
#   --output $SEARCH_ENGINE_SETTINGS

echo "Work with torrents"
find $INPUT_DIRECTORY \
  -type f \
  -name "*.torrent" \
  -print0 | while read -d $'\0' file
do
    # Получение хеша
    set +e
    echo "Begin $file"
    INFO_HASH=$(find-torrent-data get-hash --torrent "$file")
    if [ $? -ne 0 ]
    then
      echo "Bad torrent file: $file"
      echo "$file" >> $BAD_TORRENT_FILES
      continue
    fi
    set -e


    if [ ! -f "$REPORT_DIR/$INFO_HASH.json" ]; then
      # Репорт не существует, анализируем
      echo "Create report"
      RUST_LOG=info find-torrent-data torrent \
        --torrent "$file" \
        --search-engine-type $SEARCH_ENGINE_TYPE \
        --search-engine-settings $SEARCH_ENGINE_SETTINGS \
        --output "$REPORT_DIR/$INFO_HASH.json" 2>&1 | tee -a $REPORT_DIR/$INFO_HASH.log
    else
      echo "Report exist"
    fi

    echo "End $file"
done
