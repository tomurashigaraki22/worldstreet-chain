#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
SOLANA_BIN="${SOLANA_BIN:-/root/.local/share/solana/install/active_release/bin/solana}"
RPC_URL="${WSC_SOLANA_RPC_URL:-https://api.devnet.solana.com}"
DEPLOYER_KEYPAIR="${WSC_SOLANA_PROGRAM_AUTHORITY_KEYPAIR:-/root/.config/solana/intertrain-devnet-deployer.json}"
PROGRAM_KEYPAIR="${WSC_SOLANA_PROGRAM_KEYPAIR:-/root/.config/solana/intertrain-spl-usdc-program.json}"
PROGRAM_SO="${WSC_SOLANA_PROGRAM_SO:-$ROOT_DIR/contracts/solana/wsol-bridge-program/target/deploy/intertrain_wsol_bridge.so}"
CLIENT_MANIFEST="$ROOT_DIR/contracts/solana/wsol-bridge-client/Cargo.toml"
CLIENT_BIN="${WSC_SOLANA_CLIENT_BIN:-/usr/local/bin/intertrain-wsol-client}"
NODE_ENV="${WSC_NODE_ENV_FILE:-$ROOT_DIR/devnet/.env}"
RELAYER_ENV="${WSC_RELAYER_ENV_FILE:-/etc/worldstreet/relayer.env}"
WSOL_MINT="So11111111111111111111111111111111111111112"

[[ -x "$SOLANA_BIN" ]] || { echo "missing solana CLI: $SOLANA_BIN" >&2; exit 1; }
[[ -f "$DEPLOYER_KEYPAIR" ]] || { echo "missing funded deployer keypair: $DEPLOYER_KEYPAIR" >&2; exit 1; }
[[ -f "$PROGRAM_KEYPAIR" ]] || { echo "missing program keypair: $PROGRAM_KEYPAIR" >&2; exit 1; }
[[ -f "$PROGRAM_SO" ]] || { echo "missing program artifact: $PROGRAM_SO" >&2; exit 1; }

if [[ ! -x "$CLIENT_BIN" ]]; then
  cargo +1.89.0-sbpf-solana-v1.54 build --release --manifest-path "$CLIENT_MANIFEST"
  install -m 0755 "$ROOT_DIR/contracts/solana/wsol-bridge-client/target/release/intertrain-wsol-client" "$CLIENT_BIN"
fi

PROGRAM_ID="$($SOLANA_BIN address --keypair "$PROGRAM_KEYPAIR")"
DEPLOYER="$($SOLANA_BIN address --keypair "$DEPLOYER_KEYPAIR")"
echo "Deploying $PROGRAM_ID from $DEPLOYER to Solana devnet..."
"$SOLANA_BIN" program deploy "$PROGRAM_SO" --url "$RPC_URL" --keypair "$DEPLOYER_KEYPAIR" --program-id "$PROGRAM_KEYPAIR" --upgrade-authority "$DEPLOYER_KEYPAIR" --use-rpc --commitment finalized --output json

PDA_OUTPUT="$($CLIENT_BIN pdas --program-id "$PROGRAM_ID")"
STATE_ADDRESS="$(printf '%s\n' "$PDA_OUTPUT" | awk -F= '$1=="state"{print $2}')"
VAULT_ADDRESS="$(printf '%s\n' "$PDA_OUTPUT" | awk -F= '$1=="vault"{print $2}')"
[[ -n "$STATE_ADDRESS" && -n "$VAULT_ADDRESS" ]] || { echo "failed to derive state/vault PDAs" >&2; exit 1; }

if ! "$SOLANA_BIN" account "$STATE_ADDRESS" --url "$RPC_URL" >/dev/null 2>&1; then
  "$CLIENT_BIN" init --rpc-url "$RPC_URL" --program-id "$PROGRAM_ID" --keypair "$DEPLOYER_KEYPAIR"
fi

set_env() {
  local file="$1" key="$2" value="$3"
  install -d -m 700 "$(dirname "$file")"
  touch "$file"; chmod 600 "$file"
  if grep -q "^${key}=" "$file"; then sed -i "s#^${key}=.*#${key}=${value}#" "$file"; else printf '%s=%s\n' "$key" "$value" >> "$file"; fi
}
ASSET_ID="solana:WSOL:devnet:program:${PROGRAM_ID}:${WSOL_MINT}"
for file in "$NODE_ENV" "$RELAYER_ENV"; do
  set_env "$file" WSC_SOLANA_MODE program
  set_env "$file" WSC_SOLANA_NETWORK devnet
  set_env "$file" WSC_SOLANA_RPC_URL "$RPC_URL"
  set_env "$file" WSC_SOLANA_PROGRAM_ID "$PROGRAM_ID"
  set_env "$file" WSC_SOLANA_BRIDGE_PROGRAM "$PROGRAM_ID"
  set_env "$file" WSC_SOLANA_STATE_ADDRESS "$STATE_ADDRESS"
  set_env "$file" WSC_SOLANA_VAULT_ADDRESS "$VAULT_ADDRESS"
  set_env "$file" WSC_SOLANA_PROGRAM_AUTHORITY_KEYPAIR "$DEPLOYER_KEYPAIR"
  set_env "$file" WSC_SOLANA_WSOL_MINT "$WSOL_MINT"
  set_env "$file" WSC_WSOL_ASSET_ID "$ASSET_ID"
  set_env "$file" WSC_SOLANA_CLIENT_BIN "$CLIENT_BIN"
  set_env "$file" WSC_SOLANA_COMMITMENT finalized
  set_env "$file" WSC_SOLANA_CONFIRMATIONS 32
done

printf 'program_id=%s\nstate=%s\nvault=%s\nauthority=%s\nasset_id=%s\n' "$PROGRAM_ID" "$STATE_ADDRESS" "$VAULT_ADDRESS" "$DEPLOYER" "$ASSET_ID"
