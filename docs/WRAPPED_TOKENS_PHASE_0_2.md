# Intertrain wrapped-token implementation: Phase 0–2

## Phase 0: decisions and safety

- Current protocol chain ID remains `worldstreet-devnet-1`; visible branding is `Intertrain`.
- The first bridge target is Ethereum Sepolia.
- Native ETH is escrowed on Ethereum and represented as `wETH` on Intertrain.
- No real-value bridge is enabled by default. RPC URL, contract address, and deposit topic are required before activation.
- The bridge contract in `contracts/ethereum/IntertrainWethBridge.sol` is testnet scaffolding and requires review/audit.

## Phase 1: multi-asset foundation

- `AssetId` provides canonical origin-chain/symbol/reference identity.
- Genesis supports optional asset definitions without changing existing MNA genesis hashes.
- The core has canonical wrapped-asset metadata and a safe registry path; existing persisted MNA snapshots remain unchanged.
- RPC exposes `asset_list`; account responses now include consensus asset balances.
- Versioned asset mint/burn operations are included in block roots, persisted state, and processed-operation idempotency records. Existing MNA snapshots decode through a legacy fallback.

## Phase 2: Ethereum wETH setup

- `wsc-bridge` contains Ethereum JSON-RPC connectivity, block/log query types, configuration, and bridge operation status types.
- `bridge_status` reports Sepolia configuration and connectivity.
- The wallet and explorer show registered assets and bridge readiness.
- The remaining activation work is deployment of the bridge contract, event-topic configuration, and production security review. The testnet relayer service and consensus mint/burn path are now present but disabled until configuration is supplied.

## Environment

Configure the variables in `devnet/bridge.env.example` only for a controlled Sepolia deployment. Never point this at mainnet before bridge authorization, replay protection, reserve monitoring, and an independent security review are complete.
