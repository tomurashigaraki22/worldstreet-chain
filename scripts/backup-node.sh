#!/usr/bin/env bash
set -euo pipefail

if [ "$#" -ne 2 ]; then
  echo "usage: $0 NODE_DATA_DIR BACKUP_TAR_GZ" >&2
  exit 2
fi

data_dir="$1"
destination="$2"

if [ ! -d "$data_dir" ]; then
  echo "node data directory does not exist: $data_dir" >&2
  exit 1
fi
if [ -e "$destination" ]; then
  echo "backup destination already exists: $destination" >&2
  exit 1
fi

mkdir -p "$(dirname "$destination")"
tar --xattrs --acls -czf "$destination" -C "$(dirname "$data_dir")" "$(basename "$data_dir")"
echo "Created node backup: $destination"
