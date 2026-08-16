# Intertrain native token operations

This document describes the native token lane now implemented for `worldstreet-devnet-1` (the client-facing name is **Intertrain**).

## What is implemented

- Deterministic token IDs derived from the signed create operation.
- Token definitions with name, symbol, decimals, total supply, optional cap, authorities, metadata URI/hash, and pause state.
- Locally signed operations: `Create`, `Transfer`, `Mint`, `Burn`, `SetAuthorities`, `Freeze`, `Unfreeze`, `Pause`, `Unpause`, and `UpdateMetadata`.
- Ed25519 signature and address binding, chain-ID binding, account nonce sequencing, MNA fee payment, memo limits, amount overflow/underflow checks, supply-cap checks, paused/frozen checks, and atomic state application.
- Idempotent operation processing. The operation ID is the hash of the canonical unsigned operation, so retries do not mint or transfer twice.
- Token operations are included in block transaction roots, block storage, state roots, block views, and restart recovery.
- RPC registry, balances, operation status, explorer lookup, and browser wallet creation/transfer controls.

This is the native asset standard. It does not execute user-supplied smart-contract bytecode; a VM/program-token lane remains a separate security and execution project.

## Browser workflow

Open [dev-wallets.watchup.space](https://dev-wallets.watchup.space):

1. Unlock the local vault or create a wallet. The seed stays in browser storage encrypted with the vault password.
2. Use **Fund with faucet** on devnet. Token operations still require an MNA fee balance.
3. Create another wallet if you want to test transfers.
4. In **Create native token**, enter the name, symbol, decimals, initial base-unit supply, optional cap, and metadata URI. The creator becomes the default creator/mint/freeze authority.
5. Click **Sign and create token**. The wallet asks the node for canonical signing bytes, signs them locally, then broadcasts the signed operation.
6. Wait for the token operation hash to become confirmed. The registry and balances refresh automatically.
7. Select the token and recipient under **Transfer native token** and broadcast a signed transfer.

The explorer at [wsc.watchup.space](https://wsc.watchup.space) shows the native-token registry and accepts normal transaction hashes, token-operation hashes, and token IDs in its search box.

Important: a token ID and its creation operation ID are different values. The token registry now shows both. Use **Creation operation** to inspect the signed create operation; use **Token ID** to inspect the token definition and supply.

## RPC methods

All calls use JSON-RPC 2.0 at the node RPC endpoint (`/rpc`, proxied by the hosted sites).

- `token_operation_prepare` — accepts `{ "unsigned": { ... } }`, returns `signing_bytes`, `operation_id`, and the derived `token_id` for a create.
- `token_operation_broadcast` — accepts `{ "operation": { "unsigned": { ... }, "signature": "..." } }` and queues the signed operation.
- `token_operation_status` — accepts `{ "hash": "<operation-id>" }` and returns `pending`, `confirmed`, or `not_found` with operation details.
- `token_list` — returns all token definitions.
- `token_get` — accepts `{ "hash": "<token-id>" }`.
- `token_balance` — accepts `{ "address": "<mna-address>", "token_id": "<token-id>" }`.
- `asset_list` — now includes MNA, wrapped assets, and native token definitions.
- `account_get` — includes the address's canonical asset balance map.
- `block_latest`, `block_get`, and `block_get_by_height` — include `token_operations` alongside transactions and bridge asset operations.

`amount`, `fee`, and `max_supply` are integer strings in base units. A token's `decimals` only controls display; it does not change consensus arithmetic.

## Operation fields

The unsigned object contains:

- `version`, `chain_id`, `nonce`, `from`, `kind`, `token_id`, `to`, `amount`, `fee`, `public_key`;
- create metadata: `name`, `symbol`, `decimals`, `max_supply`;
- authority fields: `mint_authority`, `burn_authority`, `freeze_authority`;
- `metadata_uri`, `metadata_hash`, and `memo`.

Use 32-byte lowercase hex for hashes, 32-byte hex for public keys, and 64-byte hex for signatures. `token_id` is all zeroes for `Create`; the prepare response returns the deterministic ID to use for later operations.

## State and authority rules

- Every token operation consumes the sender's next account nonce and pays at least the chain fee minimum in MNA.
- `Create` credits the initial supply to the creator. `max_supply: null` is uncapped; a numeric cap must be at least the initial supply.
- `Transfer` moves existing units and is blocked while the token is paused or either account is frozen.
- `Mint` requires `mint_authority` and cannot exceed `max_supply`.
- `Burn` destroys units held by the signer and reduces total supply.
- `SetAuthorities`, `Pause`, `Unpause`, and `UpdateMetadata` require the token creator.
- `Freeze` and `Unfreeze` require `freeze_authority` and target the `to` address.
- All failed operations are rejected atomically; the fee, nonce, balances, and registry are unchanged on failure.

## PowerShell JSON-RPC example

```powershell
$body = @{
  jsonrpc = "2.0"
  id = 1
  method = "token_list"
  params = @{}
} | ConvertTo-Json -Depth 8

Invoke-RestMethod `
  -Uri "https://rpc-worldstreet.watchup.space/rpc" `
  -Method Post `
  -ContentType "application/json" `
  -Body $body
```

For signing, call `token_operation_prepare`, decode its `signing_bytes` hex, sign those bytes with the wallet's Ed25519 key, and send the resulting signature in `token_operation_broadcast`. The browser wallet performs those steps without exposing the key to the server.
