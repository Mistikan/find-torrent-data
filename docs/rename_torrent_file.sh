#!/bin/bash
# TODO: в душе не понимаю почему нормально не выводится BAD_FILES

BAD_FILES=("")

mkdir -p hash

find . \
  -type f \
  -name "*.torrent" \
  -print0 | while read -d $'\0' file
do
    echo "----"
    echo "$file"
    INFO_HASH=$(find-torrent-data get-hash --torrent "$file")
    if [ $? -eq 0 ]
    then
      echo "$file -> $INFO_HASH"
      ln -sf "../$file" "./hash/$INFO_HASH.torrent"
    else
      echo "Error parsing: $file"
      BAD_FILES+=($file)
      echo "${BAD_FILES[*]}"
    fi
done

echo "Bad files:"
echo "${BAD_FILES[*]}"