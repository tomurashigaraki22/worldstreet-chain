#!/usr/bin/env bash
set -euo pipefail
exec "$(dirname "$0")/deploy-wsol-solana-program.sh" "$@"
