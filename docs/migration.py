# IMPORTANT: СКРИПТ ПЕРЕМЕЩАЕТ ФАЙЛЫ!

import json
from pathlib import Path
import shutil
import glob


TORRENT_STORAGE = ["/var/mnt/zfs/wd_green_1tb_pool/torrent"]
TORRENT_OUTPUT_STORAGE = "/var/mnt/zfs/wd_blue_1tb_pool/torrent"
TORRENT_REPORTS = "/var/mnt/zfs/pool_data/torrent_find_data_report/*.json"


def check_exist(hash):
    for storage in TORRENT_STORAGE:
        file_path = Path(storage + "/metadata/" + hash + ".torrent")
        return file_path.exists()


def handler_report(path):
    print (f"Path: {path}")

    with open(path, 'r') as file:
        # Load the JSON data
        data = json.load(file)

    file_torrent = data["torrent_info"]["path"]
    hash = data["torrent_info"]["hash"]
    files = data["files"]
    
    result = check_exist(hash)
    if result:
        print ("Torrent exist, skip")
        return
    
    all_none = any([file is None for file in files.values()])
    if all_none:
        print ("All not found files, skip")
        return

    for torrent_path, real_path in files.items():
        if real_path is None:
            print (f"File {torrent_path} not found")
            continue

        src_path = Path(real_path)
        dst_path = Path(TORRENT_OUTPUT_STORAGE + "/data/" + hash + "/" + torrent_path)

        dst_path.parent.mkdir(parents=True, exist_ok=True)
        print (f"Move {src_path} => {dst_path}")
        shutil.move(src_path, dst_path)
    
    print ("Move torrent file")
    dst_path = Path(TORRENT_OUTPUT_STORAGE + "/metadata/" + hash + ".torrent")
    shutil.move(file_torrent, dst_path)

def main():
    json_files = glob.glob(TORRENT_REPORTS)
    for json_file in json_files:
        handler_report(json_file)

if __name__ == "__main__":
    main()
