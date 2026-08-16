#!/usr/bin/env bash
set -euo pipefail

: "${WSC_ETHEREUM_RPC_URL:?Set WSC_ETHEREUM_RPC_URL to the Sepolia RPC endpoint}"
RELAYER_ADDRESS="${WSC_ETHEREUM_RELAYER_ADDRESS:-0x286c46f1f17d4C948586D2fAB7F571198405ad4b}"
KEYSTORE_DIR="${WSC_KEYSTORE_DIR:-/root/.foundry/keystores}"
ACCOUNT="${WSC_RELAYER_ACCOUNT:-intertrain-sepolia-deployer}"
PASSWORD_FILE="${WSC_KEYSTORE_PASSWORD_FILE:-/etc/worldstreet/relayer-password}"

if [[ ! -r "$PASSWORD_FILE" ]]; then
  echo "Missing encrypted-keystore password file: $PASSWORD_FILE" >&2
  exit 1
fi

KEY_ARGS=(--account "$ACCOUNT")
if [[ -f "$KEYSTORE_DIR/$ACCOUNT" ]]; then
  KEY_ARGS=(--keystore "$KEYSTORE_DIR/$ACCOUNT")
fi

/root/.foundry/bin/forge create \
  /root/worldstreet-chain/contracts/ethereum/IntertrainWethBridge.sol:IntertrainWethBridge \
  --rpc-url "$WSC_ETHEREUM_RPC_URL" \
  "${KEY_ARGS[@]}" \
  --password-file "$PASSWORD_FILE" \
  --broadcast \
  --json \
  --constructor-args "$RELAYER_ADDRESS"
