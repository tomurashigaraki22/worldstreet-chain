# Intertrain bridge operations and security (Phases 3–5)

## Scope and current state

Phase 3 adds Ethereum and Solana external-RPC clients, canonical WETH/WSOL identities, and readiness reporting. Phase 4 exposes those assets and bridge states in the explorer and wallet. Phase 5 adds versioned multi-asset consensus accounting, durable idempotent bridge operations, a retrying VPS relayer, deployment templates, and operational controls. The public devnet remains safe-by-default: only explicitly configured devnet lanes are enabled, with replay protection, confirmations, and operator authorization. Solana devnet now uses the explicitly labelled hardened native-Rust program mode with a funded PDA vault; custody mode remains a documented fallback.

The current chain still has the protocol ID `worldstreet-devnet-1`; the visible client brand is **Intertrain**. MNA snapshots remain backward-compatible. WETH/WSOL operations are included in block transaction roots and persisted state once submitted through the authorized bridge RPC.

## Current devnet asset gates

Circle Sepolia test-USDC is pinned as `0x1c7D4B196Cb0C7B01d743Fbc6116a902379C7238`. The dedicated Intertrain USDC reserve contract is deployed at `0xab4056bCb0369897d6D5Ca1A13f670f76C75ef3e` and is enabled through `bridge_status`; MNA issuance remains collateral-gated until a finalized deposit is recorded. WSOL program mode is enabled on devnet with a 1 SOL PDA reserve; custody mode is not the active path. See `docs/USDC_DEVNET_SETUP.md`.

## Readiness endpoints

- `bridge_status` — external network, contract/program, finality settings, connection, and error/reason fields.
- `asset_list` — native MNA plus registered WETH and WSOL metadata.
- `bridge_operations_pending` and `bridge_operations_recent` — queued/confirmed mint and burn records.
- `healthz` and `chain_info` — Intertrain validator health and heights.

Example Windows PowerShell query:

```powershell
$body = @{ jsonrpc = '2.0'; id = 1; method = 'bridge_status'; params = @{} } | ConvertTo-Json -Depth 10
Invoke-RestMethod -Uri 'https://rpc-worldstreet.watchup.space/rpc' -Method Post -ContentType 'application/json' -Body $body | ConvertTo-Json -Depth 10
```

## Relayer and consensus flow

The VPS Foundry account `intertrain-sepolia-deployer` (address `0x286c46f1f17d4C948586D2fAB7F571198405ad4b`) is configured as the intended testnet contract relayer. The relayer service polls finalized Sepolia `Deposit` logs, records each operation in SQLite, submits an authorized `bridge_mint`, and retries failures with exponential backoff. For burns, it discovers confirmed `bridge_burn` records and signs the Ethereum `release` call from the encrypted Foundry keystore. The node tracks processed operation IDs and asset balances in consensus state; replaying an operation is rejected/idempotent.

The service is installed and enabled as `worldstreet-relayer.service` after the deployed contract address, event topic, operator token, and keystore password file are configured. The live Sepolia deployment uses contract `0xaA82D61ACBcED55CF4cC49bE9018d3E5A6Ba2A9D` (deployment transaction `0x85aa970e9b8a28382d20c64babdd8c765d02d95f1b14ae9c9c13c8170d310420`) with relayer `0x286c46f1f17d4C948586D2fAB7F571198405ad4b`. The VPS deployment helper is `ops/deploy-weth-bridge.sh`; it defaults the constructor relayer to that address.

## Activation gates

Do not enable a bridge until all gates are met:

1. For Ethereum, an independently reviewed bridge contract is deployed on the intended testnet. For Solana program mode, the exact native program, state/vault PDAs, canonical mint, and reserve are recorded; custody mode is available only as a separate fallback.
2. The exact contract (or Solana vault), event/memo schema, mint, and external RPC endpoint are recorded.
3. A relayer uses an HSM/KMS or threshold/multisignature custody for production; no private key belongs in the browser or committed environment files. The current devnet vault key is root-only and is a testnet convenience.
4. Deposits are accepted only after configured external finality (`12` Sepolia confirmations by default; `32` Solana finalized slots by default).
5. Every deposit and burn has a deterministic operation ID and an on-chain replay-protection record.
6. Mint amount cannot exceed verified locked/reserved value; releases cannot exceed bridge reserves.
7. Pause, rate limits, destination allowlists, nonce monotonicity, and audit logs are tested.
8. Monitoring and alerting are active for RPC lag, finality stalls, reserve mismatches, repeated operation IDs, and relayer failures.

## Key custody and incident response

Use separate testnet and mainnet authorities. Restrict relayer permissions to the minimum release/mint operations. Rotate credentials on a schedule, require two-person approval for policy changes, and keep an offline break-glass pause key. If an RPC, relayer, reserve, or replay anomaly occurs: pause the bridge, preserve logs and operation IDs, stop new attestations, reconcile external and Intertrain state, then resume only after review.

## Deployment configuration

Start from `devnet/bridge.env.example`. Empty or placeholder values intentionally leave the bridge offline. Never put a seed phrase, relayer secret, validator key, or contract deployer key in this file. Use a secret manager on production hosts, pin RPC providers, and restrict outbound network access to the selected endpoints.

The Ethereum scaffold is in `contracts/ethereum/IntertrainWethBridge.sol`. The active Solana path is the native program in `contracts/solana/wsol-bridge-program/`, deployed with `ops/deploy-wsol-solana-program.sh`. The companion `intertrain-wsol-client` submits `init`, `lock`, and `release` instructions. The relayer validates structured program logs, mints WSOL on Intertrain, and submits authority-signed program releases with durable idempotency. The active devnet identities and reserve are recorded in `docs/INTERTRAIN_CURRENT_CAPABILITIES.md`.

The legacy custody setup remains in `ops/setup-wsol-vault.sh` for isolated devnet fallback use only.
