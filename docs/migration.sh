#!/bin/bash
# Example usage: bash migration.sh report.json /var/mnt/zfs/wd_blue_1tb_pool/torrent/

# TODO: требуется игнорировать null в files
# TODO: требуется ещё ставить права, чтобы другие могли читать
# TODO: 666 - на момент докачки для файлов, другие права для директории

set -e

REPORT_FILE=$1
DESTINATION_DIRECTORY=$2 #

if [ ! -d "$DESTINATION_DIRECTORY/metadata" ]; then
  echo "$DIRECTORY/metadata does not exist."
  exit 1
fi

if [ ! -d "$DESTINATION_DIRECTORY/data" ]; then
  echo "$DIRECTORY/data does not exist."
  exit 1
fi

export TORRENT_INFO_HASH=$(cat $REPORT_FILE | jq -r .torrent_info.hash)
export TORRENT_INFO_PATH=$(cat $REPORT_FILE | jq -r .torrent_info.path)
export TORRENT_INFO_FILE=$(cat $REPORT_FILE | jq '.files | to_entries | .[] | [.key, .value] | @sh')
export TORRENT_DATA="$DESTINATION_DIRECTORY/data/$TORRENT_INFO_HASH/"

function processRow() {
  torrent_file=$1
  disk_file=$2
}

echo "Copy torrent file"
mkdir -p $DESTINATION_DIRECTORY/metadata
#file "$TORRENT_INFO_PATH"
cp "$TORRENT_INFO_PATH" $DESTINATION_DIRECTORY/metadata/$TORRENT_INFO_HASH.torrent

echo "Copy torrent data"
mkdir -p $TORRENT_DATA

IFS=$'\n' # Each iteration of the for loop should read until we find an end-of-line
for row in $TORRENT_INFO_FILE
do
  # Run the row through the shell interpreter to remove enclosing double-quotes
  stripped=$(echo $row | xargs echo)
  # Call our function to process the row
  # eval must be used to interpret the spaces in $stripped as separating arguments
  eval processRow $stripped

  dest_file=$TORRENT_DATA/$torrent_file
  dest_dir=$(dirname "$dest_file")

  mkdir -p $dest_dir

  echo "Copy $disk_file => $dest_file"
  # cp "$disk_file" "$dest_file"
  ln "$disk_file" "$dest_file"
done
unset IFS
