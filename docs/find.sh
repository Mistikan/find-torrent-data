#!/bin/bash

DIRECTORY="$(find . -maxdepth 1 -type d)"
TORRENT_DIR="/var/mnt/zfs/pool_data/torrent_file/hash/"
REPORT_DIR="/var/mnt/zfs/pool_data/torrent_find_data_report_blue/"

for DIR in $DIRECTORY
do
  INFO_HASH="${DIR:2}"
  echo "Processing $INFO_HASH"
  
  TORRENT_FILE="$TORRENT_DIR/$INFO_HASH.torrent"
  
  if [ -f "$TORRENT_FILE" ]; then
    echo "$TORRENT_FILE exists."
    echo "Create report"
    RUST_LOG=info find-torrent-data torrent \
        --torrent "$TORRENT_FILE" \
        --search-engine-type postgresql \
        --search-engine-settings postgresql://postgres:changeme@192.168.88.210/find-torrent-data \
        --output "$REPORT_DIR/$INFO_HASH.json" 2>&1 | tee -a $REPORT_DIR/$INFO_HASH.log
  else 
    echo "$TORRENT_FILE does not exist."
    echo $TORRENT_FILE >> /tmp/not_found_files.log
    contunie
  fi

done
