#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
SOLANA_BIN="${SOLANA_BIN:-/root/.local/share/solana/install/active_release/bin/solana}"
RPC_URL="${WSC_SOLANA_RPC_URL:-https://api.devnet.solana.com}"
DEPLOYER="${WSC_SOLANA_PROGRAM_AUTHORITY_KEYPAIR:-/root/.config/solana/intertrain-devnet-deployer.json}"
ARTIFACT="${WSC_SOLANA_PROGRAM_SO:-$ROOT_DIR/contracts/solana/wsol-bridge-program/target/deploy/intertrain_wsol_bridge.so}"
RELAYER_ENV="${WSC_RELAYER_ENV_FILE:-/etc/worldstreet/relayer.env}"

[[ -x "$SOLANA_BIN" && -f "$DEPLOYER" && -f "$ARTIFACT" ]] || { echo "missing Solana CLI, deployer, or artifact" >&2; exit 1; }
BALANCE="$($SOLANA_BIN balance --lamports --url "$RPC_URL" --keypair "$DEPLOYER" | awk '{print $1}')"
if (( BALANCE < 1000000000 )); then
  echo "deployer has $BALANCE lamports; fund it with at least 1,000,000,000 lamports before activation" >&2
  exit 2
fi
WSC_SOLANA_PROGRAM_SO="$ARTIFACT" /root/worldstreet-chain/ops/deploy-wsol-solana-program.sh
if grep -q '^WSC_SOLANA_MNA_ENABLED=' "$RELAYER_ENV"; then
  sed -i 's#^WSC_SOLANA_MNA_ENABLED=.*#WSC_SOLANA_MNA_ENABLED=true#' "$RELAYER_ENV"
else
  printf '%s\n' 'WSC_SOLANA_MNA_ENABLED=true' >> "$RELAYER_ENV"
fi
systemctl restart worldstreet-relayer.service
printf 'SOL→MNA lane activated; relayer=%s\n' "$(systemctl is-active worldstreet-relayer.service)"
