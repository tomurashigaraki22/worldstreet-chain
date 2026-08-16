#!/usr/bin/env bash
set -euo pipefail

SOLANA_BIN="${SOLANA_BIN:-/root/.local/share/solana/install/active_release/bin/solana}"
KEYGEN_BIN="${SOLANA_KEYGEN_BIN:-/root/.local/share/solana/install/active_release/bin/solana-keygen}"
RPC_URL="${WSC_SOLANA_RPC_URL:-https://api.devnet.solana.com}"
VAULT_KEYPAIR="${WSC_SOLANA_VAULT_KEYPAIR:-/root/.config/solana/intertrain-wsol-vault.json}"
NODE_ENV="${WSC_NODE_ENV_FILE:-/root/worldstreet-chain/devnet/.env}"
RELAYER_ENV="${WSC_RELAYER_ENV_FILE:-/etc/worldstreet/relayer.env}"
WSOL_MINT="So11111111111111111111111111111111111111112"

[[ -x "$SOLANA_BIN" ]] || { echo "missing solana CLI: $SOLANA_BIN" >&2; exit 1; }
[[ -x "$KEYGEN_BIN" ]] || { echo "missing solana-keygen: $KEYGEN_BIN" >&2; exit 1; }
install -d -m 700 "$(dirname "$VAULT_KEYPAIR")"
if [[ ! -f "$VAULT_KEYPAIR" ]]; then
  "$KEYGEN_BIN" new --no-bip39-passphrase --silent --outfile "$VAULT_KEYPAIR"
fi
chmod 600 "$VAULT_KEYPAIR"
VAULT_ADDRESS="$($SOLANA_BIN address --keypair "$VAULT_KEYPAIR")"
ASSET_ID="solana:WSOL:devnet:vault:${VAULT_ADDRESS}:${WSOL_MINT}"

set_env() {
  local file="$1" key="$2" value="$3"
  install -d -m 700 "$(dirname "$file")"
  touch "$file"
  chmod 600 "$file"
  if grep -q "^${key}=" "$file"; then
    sed -i "s#^${key}=.*#${key}=${value}#" "$file"
  else
    printf '%s=%s\n' "$key" "$value" >> "$file"
  fi
}

for file in "$NODE_ENV" "$RELAYER_ENV"; do
  set_env "$file" WSC_SOLANA_MODE custody
  set_env "$file" WSC_SOLANA_NETWORK devnet
  set_env "$file" WSC_SOLANA_RPC_URL "$RPC_URL"
  set_env "$file" WSC_SOLANA_VAULT_ADDRESS "$VAULT_ADDRESS"
  set_env "$file" WSC_SOLANA_VAULT_KEYPAIR "$VAULT_KEYPAIR"
  set_env "$file" WSC_SOLANA_WSOL_MINT "$WSOL_MINT"
  set_env "$file" WSC_WSOL_ASSET_ID "$ASSET_ID"
  set_env "$file" WSC_SOLANA_COMMITMENT finalized
  set_env "$file" WSC_SOLANA_CONFIRMATIONS 32
done

echo "WSOL custody vault configured (no custom Solana program deployed)."
printf 'Vault address: %s\nWSOL mint: %s\nAsset ID: %s\nKeypair: %s\n' "$VAULT_ADDRESS" "$WSOL_MINT" "$ASSET_ID" "$VAULT_KEYPAIR"
echo "Fund the devnet vault only if needed:"
echo "  $SOLANA_BIN airdrop 1 $VAULT_ADDRESS --url $RPC_URL"
