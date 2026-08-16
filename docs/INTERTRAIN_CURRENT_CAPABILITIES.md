# Intertrain current blockchain capabilities

**Current environment:** public four-validator devnet

**Client brand:** Intertrain

**Protocol chain ID:** `worldstreet-devnet-1`

**Current truth:** this is an experimental devnet. It can process signed MNA transactions, native-token operations, blocks, validator votes, finality, RPC queries, wallet login challenges, a live Ethereum WETH bridge path, and a reserve-gated USDC↔MNA lane. It is still devnet software and has no funded production reserve.

## 1. Public services

| Service | URL | Purpose |
|---|---|---|
| Explorer | https://wsc.watchup.space | Read-only Intertrain explorer, chain health, blocks, validators, assets, tokens, and transaction lookup |
| Wallet client | https://dev-wallets.watchup.space | Browser-local wallet vault, multiple wallets, MNA faucet, signed transfers, native-token creation and transfers, login challenge flow |
| JSON-RPC | https://rpc-worldstreet.watchup.space/rpc | JSON-RPC 2.0 API |
| Health | https://rpc-worldstreet.watchup.space/healthz | Node health and heights |
| Metrics | https://rpc-worldstreet.watchup.space/metrics | Prometheus-style process/network counters |

The VPS services are intended to remain running through Docker restart policies, nginx, and `worldstreet-relayer.service`.

## 2. Current live status

At the time this document was updated:

- Intertrain RPC health: `ok`.
- Chain ID: `worldstreet-devnet-1`.
- Four devnet nodes are running.
- Ethereum Sepolia WETH bridge: enabled and connected.
- Solana WSOL: enabled on Solana devnet in native program mode; the PDA vault has a 1 SOL devnet reserve and the VPS relayer is active.
- Direct SOL→MNA lane: active on the original `FyAuUc2pPkz1nt2vR27R6NfE3Lgb4Z69sjUoPSeU7PCw` program after the tag-5 upgrade. The relayer is enabled and fetches fresh Pyth snapshots per deposit.
- Sepolia test-USDC reserve contract: deployed and enabled at `0xab4056bCb0369897d6D5Ca1A13f670f76C75ef3e`.
- USDC→MNA minting: implemented at `2 USDC = 1 MNA`, and blocked unless the reserve ledger has sufficient collateral.
- MNA→USDC redemption: implemented as an in-chain conversion to the approved wrapped-USDC asset; external USDC release is handled through the relayer and reserve contract.
- Dynamic 0.5–5% congestion fees: not implemented; the current devnet uses its configured minimum fee.
- Validator fee distribution: not implemented as the requested stablecoin economics; existing fee accounting is devnet protocol accounting only.

The live `bridge_status` response is the authority for connection/gate state. A disabled asset may still appear in `asset_list` as metadata so clients can explain why it is unavailable.

## 3. Native Intertrain asset: MNA

MNA is the native asset, displayed as MANNA/MNA in the current protocol.

- Base unit: microMNA.
- Decimals: 6.
- Address prefix: `mna1...` Bech32m.
- Native asset ID: `worldstreet:MNA:native`.
- Current devnet minimum transaction fee: configured in genesis; the current devnet uses a fixed minimum rather than a congestion percentage.
- Devnet faucet: available only because the chain ID contains `devnet`.

### What MNA can do now

- Hold balances in Intertrain accounts.
- Transfer between signed Intertrain wallets.
- Pay current transaction and token-operation fees.
- Be displayed in the browser wallet and explorer.
- Be queried through `account_get`, blocks, transactions, and status methods.

### What MNA cannot claim yet

MNA is reserve-gated on devnet. The accounting and swap controls are implemented, but the reserve is currently empty until a test-USDC deposit is made. It does not yet have:

- a funded production reserve;
- a published guaranteed floor price backed by redeemable liquidity;
- a production minting multisig;
- independent proof-of-reserves reconciliation;
- stablecoin-specific validator fee distribution.

The full target design is documented in [MNA stablecoin Phases 0–10](MNA_STABLECOIN_AND_BRIDGE_PHASES_0_10.md).


### Solana wSOL program deployment

The live devnet wSOL lane uses the canonical Solana WSOL mint and an Intertrain-owned native program:

- Program ID: `FyAuUc2pPkz1nt2vR27R6NfE3Lgb4Z69sjUoPSeU7PCw`
- Authority/deployer: `EQQvAukwEiwiXShm93HTSLC2vTftMnPzeRxUD4LGd6rv`
- State PDA: `76UrihHJQAH88E6JQXMUFDkXM3PhL1Vf56od4XUdV29t`
- Vault PDA: `829dNAZ1DfpsQKxaw9gqcuxfMRyFsCg6chtKCajQeaGe`
- Asset ID: `solana:WSOL:devnet:program:FyAuUc2pPkz1nt2vR27R6NfE3Lgb4Z69sjUoPSeU7PCw:So11111111111111111111111111111111111111112`

A finalized Solana `lock` instruction transfers native SOL to the program vault and emits a structured deposit record. The VPS relayer verifies that record, mints the matching WSOL amount on Intertrain, and records idempotent operation state. Intertrain burns can be released through the program client to a Solana recipient. This is devnet infrastructure, not a production bridge or audited custody system.

#### Solana token support boundary

The deployed program does not bridge arbitrary Solana SPL or Token-2022 assets; it supports only the allowlisted Circle devnet USDC mint. It only handles native SOL deposits/releases that are represented as Intertrain WSOL. The canonical Solana WSOL mint is currently an asset-identity reference; the program does not accept SPL WSOL token-account transfers. The deployed program validates standard SPL Token accounts, the allowlisted mint, 6 decimals, vault ownership, replay PDAs, and uses checked CPI transfers. Token-2022 and other mints remain disabled. Solana USDC is supported only through this configured devnet mint.

Adding any additional SPL mint would require a separate reviewed instruction path, per-mint configuration and vault accounting, safe CPI calls to the SPL Token programs, token-account validation, replay/rate-limit rules, and relayer support.

## 4. Wallet and login capabilities

The browser client supports:

1. Create multiple wallets in one browser vault.
2. Keep recovery material in browser-local encrypted vault storage.
3. Select between wallets.
4. Derive and display each `mna1...` address.
5. Request devnet faucet MNA.
6. Refresh balances and nonces.
7. Sign and broadcast MNA transfers locally.
8. Create and transfer native custom tokens.
9. Create a one-time login challenge and sign it locally.
10. Quote and sign reserve-gated USDC↔MNA swaps.
11. Use MetaMask for Ethereum Sepolia USDC approval/deposit into the dedicated reserve contract.
12. Derive a Solana devnet account and submit native SOL→wSOL or allowlisted SPL-USDC deposits.
13. Inspect transaction and token-operation IDs.

Private keys and recovery phrases are not sent to the RPC server. Browser vault backups and recovery phrases remain the user's responsibility.

The current login flow proves wallet control for an application domain. It is authentication, not a blockchain transaction and not a transfer of funds.

## 5. Native custom-token capabilities

The native token lane is implemented without executing arbitrary smart-contract bytecode. A separate Rust-only program MVP provides verified `.it` packages, signed and fee-paying deploy/call operations, fuel-metered WASM execution, consensus receipts, and bounded replicated program storage with owner-authorized close; see [PROGRAM_PLATFORM_MVP.md](PROGRAM_PLATFORM_MVP.md).

Supported operations:

- Create;
- Transfer;
- Mint;
- Burn;
- SetAuthorities;
- Freeze;
- Unfreeze;
- Pause;
- Unpause;
- UpdateMetadata.

Supported token properties:

- name;
- symbol;
- decimals;
- initial supply;
- optional maximum supply;
- mint, burn, and freeze authorities;
- metadata URI/hash;
- paused state;
- deterministic token ID derived from the signed create operation.

Security/state behavior includes Ed25519 signatures, address binding, chain-ID binding, account nonces, MNA fee payment, memo limits, supply-cap checks, freeze/pause checks, overflow/underflow checks, atomic application, persistence, block inclusion, and idempotent operation processing.

This is a native protocol token standard, separate from the experimental Rust-only `.it` WASM MVP. Program operations are consensus-replicated and persist in normal chain snapshots, but the restricted runtime is not yet an EVM/Solana-equivalent general smart-contract environment.

Detailed guide: [Native token operations](NATIVE_TOKEN_OPERATIONS.md).

## 6. Transactions, blocks, and persistence

The chain currently supports:

- signed Ed25519 transactions;
- native MNA transfers;
- account nonces;
- minimum fees;
- mempool admission;
- block production;
- block hashes and state roots;
- asset/token operation inclusion in block roots;
- persisted block/state storage using the node storage layer;
- restart recovery;
- transaction and operation status lookup;
- block lookup by hash and height.

The node rejects invalid signatures, wrong chain IDs, wrong nonces, insufficient balances, low fees, malformed addresses, and invalid operation payloads.

## 7. Validators, networking, and finality

The current devnet has four deterministic development validators. They are useful for repeatable testing and should not be reused for a public-value network.

Current consensus/network features:

- static validator set from genesis;
- Ed25519 validator vote signatures;
- two-thirds quorum finality;
- round-robin proposer selection in the current MVP;
- persisted finalized height/hash;
- framed TCP peer transport;
- chain ID and genesis hash handshake;
- block synchronization;
- transaction relay;
- vote relay;
- peer/network Prometheus counters.

Not production-complete:

- dynamic validator admission/removal;
- staking/delegation;
- slashing;
- production-grade leader election;
- encrypted peer transport;
- peer discovery/scoring;
- robust DoS protection;
- external validator key management;
- mainnet-grade recovery and governance.

## 8. Current bridges and external assets

### Ethereum Sepolia WETH — active testnet path

Current public identity:

- Bridge contract: `0xaA82D61ACBcED55CF4cC49bE9018d3E5A6Ba2A9D`.
- Network: Ethereum Sepolia.
- Asset ID: `ethereum:WETH:sepolia:0xaA82D61ACBcED55CF4cC49bE9018d3E5A6Ba2A9D`.
- Finality setting: 12 confirmations.
- Relayer: `0x286c46f1f17d4C948586D2fAB7F571198405ad4b`.

The relayer polls finalized external events, records operation IDs in SQLite, submits authorized Intertrain bridge operations, retries failures, and records health. This is a testnet bridge implementation and is not audited for real funds.

### Solana WSOL and allowlisted SPL USDC — active devnet program path

- Canonical mint identity: `So11111111111111111111111111111111111111112`.
- Program ID: `FyAuUc2pPkz1nt2vR27R6NfE3Lgb4Z69sjUoPSeU7PCw`.
- State PDA: `76UrihHJQAH88E6JQXMUFDkXM3PhL1Vf56od4XUdV29t`.
- Program vault PDA: `829dNAZ1DfpsQKxaw9gqcuxfMRyFsCg6chtKCajQeaGe`.
- Current mode: `program`; finalized commitment and 32 confirmation target.
- Reserve: 1 SOL on Solana devnet.

The relayer verifies structured program logs, mints matching WSOL on Intertrain, records durable idempotency state, and submits authority-signed release instructions for Intertrain burns. This is experimental devnet infrastructure and has not been audited for real funds.

### Ethereum Sepolia test-USDC — dedicated reserve contract enabled

The selected devnet stablecoin is Circle testnet USDC on Sepolia:

`0x1c7D4B196Cb0C7B01d743Fbc6116a902379C7238`

Use the [Circle Testnet Faucet](https://faucet.circle.com/) for test USDC. Use a Sepolia ETH faucet for gas. Testnet USDC has no real monetary value.

The dedicated reserve contract is `0xab4056bCb0369897d6D5Ca1A13f670f76C75ef3e`. Approve that contract for Circle Sepolia USDC (`0x1c7D4B196Cb0C7B01d743Fbc6116a902379C7238`) and call its `deposit(bytes32 depositId,uint256 amount,string destination)` method. The relayer observes the `Deposit` event and credits the corresponding approved wrapped-USDC asset on Intertrain. Do not send USDC directly to the relayer/deployer address.

## 9. Current relayer behavior

The VPS relayer currently:

- runs continuously under systemd;
- reads root-only environment/configuration;
- polls Ethereum Sepolia WETH bridge events;
- polls finalized Solana program-vault transactions when program mode is enabled;
- stores durable SQLite operation/retry state;
- uses deterministic operation IDs;
- retries failed work with backoff;
- writes a health JSON file;
- submits Intertrain bridge mint operations;
- submits Solana program releases and Ethereum release operations for supported burns.

It detects Ethereum and Solana USDC deposits, exposes fixed-rate quotes, enforces reserve checks, and submits signed swap/release operations. Validator fee settlement remains a separate follow-up.

## 10. Public JSON-RPC API

All calls use JSON-RPC 2.0 `POST /rpc`.

### Chain and diagnostics

- `node_status` — alias of `chain_info`.
- `chain_info` — chain ID, native asset, genesis hash, latest/finalized heights and hashes.
- `healthz` — HTTP endpoint rather than JSON-RPC; process health and heights.
- `metrics` — HTTP endpoint with Prometheus counters.
- `validator_set` — validator names and public keys.
- `mempool_status` — pending transaction count.
- `finality_status` — finalized height and hash.

### Blocks and transactions

- `block_latest` — latest block, transactions, asset operations, and token operations.
- `block_get` — block by hash.
- `block_get_by_height` — block by height.
- `transaction_get` — transaction by hash.
- `transaction_status` — pending/confirmed/not-found transaction state.
- `transaction_broadcast` — submit a fully signed transaction.

### Accounts and assets

- `account_get` — MNA balance, nonce, and asset balance map.
- `asset_list` — MNA, configured wrapped-asset metadata, USDC readiness metadata, and native tokens.
- `bridge_status` — Ethereum, USDC, Solana, WETH, and WSOL gate/connectivity information.

### Native tokens

- `token_operation_prepare` — canonical signing bytes, operation ID, and derived create token ID.
- `token_operation_broadcast` — submit signed token operation.
- `token_operation_status` — pending/confirmed/not-found token operation.
- `token_list` — token definitions.
- `token_get` — token definition by token ID.
- `token_balance` — token balance for an Intertrain address.

### Bridge operations

- `bridge_mint` — operator-authorized external-asset mint operation for enabled wrapped assets.
- `bridge_burn` — operator-authorized burn operation for enabled wrapped assets.
- `bridge_operation_status` — operation status.
- `bridge_operations_pending` — pending bridge operations.
- `bridge_operations_recent` — recent bridge records.

These bridge methods are operator-controlled and should not be exposed directly to untrusted browsers.

### Wallet authentication

- `auth_challenge` — create a one-time domain-bound challenge.
- `auth_verify` — verify the wallet signature and consume the challenge.

### Devnet-only

- `devnet_faucet` — credit MNA on devnet only. This is not a production issuance mechanism.

## 11. PowerShell examples

### Query health

```powershell
Invoke-RestMethod `
  -Uri "https://rpc-worldstreet.watchup.space/healthz"
```

### Query bridge status

```powershell
$body = @{ jsonrpc = "2.0"; id = 1; method = "bridge_status"; params = @{} } | ConvertTo-Json
Invoke-RestMethod `
  -Uri "https://rpc-worldstreet.watchup.space/rpc" `
  -Method Post `
  -ContentType "application/json" `
  -Body $body | ConvertTo-Json -Depth 12
```

### Query a transaction

```powershell
$body = @{ jsonrpc = "2.0"; id = 1; method = "transaction_get"; params = @{ hash = "TRANSACTION_HASH" } } | ConvertTo-Json
Invoke-RestMethod `
  -Uri "https://rpc-worldstreet.watchup.space/rpc" `
  -Method Post `
  -ContentType "application/json" `
  -Body $body | ConvertTo-Json -Depth 12
```

## 12. Existing public wallet/contract identities

These are public identities only. Secret key contents and passwords are intentionally not documented.

| Role | Network | Identity | State |
|---|---|---|---|
| Sepolia deployer/relayer | Ethereum Sepolia | `0x286c46f1f17d4C948586D2fAB7F571198405ad4b` | Existing single testnet relayer |
| WETH bridge | Ethereum Sepolia | `0xaA82D61ACBcED55CF4cC49bE9018d3E5A6Ba2A9D` | Active testnet bridge |
| Solana WSOL program vault | Solana devnet | `829dNAZ1DfpsQKxaw9gqcuxfMRyFsCg6chtKCajQeaGe` | Active devnet program reserve |
| Solana devnet deployer | Solana devnet | `EQQvAukwEiwiXShm93HTSLC2vTftMnPzeRxUD4LGd6rv` | Testnet keypair |
| Solana wSOL program key | Solana devnet | `FyAuUc2pPkz1nt2vR27R6NfE3Lgb4Z69sjUoPSeU7PCw` | Deployed devnet program |
| Canonical WSOL mint | Solana | `So11111111111111111111111111111111111111112` | External canonical mint |
| Circle test-USDC | Ethereum Sepolia | `0x1c7D4B196Cb0C7B01d743Fbc6116a902379C7238` | Token identity only; no Intertrain deposit contract |

## 13. Root-only VPS files

- Foundry encrypted keystore: `/root/.foundry/keystores/intertrain-sepolia-deployer`.
- Foundry password file: `/etc/worldstreet/relayer-password`.
- Legacy Solana custody vault keypair: `/root/.config/solana/intertrain-wsol-vault.json` (not used by active program mode).
- Solana devnet deployer keypair: `/root/.config/solana/intertrain-devnet-deployer.json`.
- Solana wSOL program keypair: `/root/.config/solana/intertrain-wsol-program.json`.
- Relayer environment: `/etc/worldstreet/relayer.env`.
- Relayer durable state: `/var/lib/worldstreet-relayer/state.sqlite3`.

These files must remain root-only. Never copy private keys, seed phrases, or keystore passwords into the repository, browser, or chat.

## 14. What is not finished

The following are design/implementation work remaining before the requested stablecoin product exists:

1. USDC bridge/treasury contract with pause, replay protection, and bounded authority.
2. Reserve ledger and independent USDC proof-of-reserves.
3. MNA floor-price and reserve-backed issuance cap enforced by consensus/state.
4. Production-grade MNA redemption for externally released USDC (the in-chain conversion is present; release still requires funded collateral and operational controls).
5. Quote/pricing service with expiry, slippage, and 0.5–5% congestion fee bounds.
6. USDC purchase UX and transaction detail view.
7. Validator fee escrow and deterministic fee distribution.
8. Governance, treasury, emergency pause, and independent multisig signers.
9. Stablecoin reconciliation, refunds, rate limits, and incident response.
10. External audits, legal review, and staged mainnet activation.

## Frontend implementation handoff

For a new frontend, use [FRONTEND_WALLET_IMPLEMENTATION_GUIDE.md](FRONTEND_WALLET_IMPLEMENTATION_GUIDE.md). It contains the vault model, key derivation, RPC schemas, canonical signing requirements, MetaMask/Solana bridge instructions, component boundaries, status UX, tests, and deployment security requirements.

The proposed SOL-priced MNA and `.it` smart-contract roadmap is documented in [SOL_TO_MNA_SMART_CONTRACTS_AND_FEE_MARKET_PLAN.md](SOL_TO_MNA_SMART_CONTRACTS_AND_FEE_MARKET_PLAN.md).

## 15. Recommended current path

For development today:

1. Create a wallet at https://dev-wallets.watchup.space.
2. Fund it with the Intertrain devnet faucet.
3. Transfer MNA between browser-created wallets.
4. Create and transfer native custom tokens.
5. Inspect blocks, operations, validators, and transactions at https://wsc.watchup.space.
6. Use the active WETH bridge only with Sepolia test ETH and the documented testnet contract.
7. Use the active Solana devnet wSOL program only with devnet SOL and the recorded program/vault identities.
8. Claim Circle Sepolia test-USDC, use the wallet MetaMask panel to deposit through the dedicated reserve contract, and wait for relayer finality before swapping.

The current implementation is a functioning blockchain devnet plus active WETH, Solana wSOL, Solana USDC, Ethereum USDC reserve, and native-token lanes. The reserve-backed MNA accounting and swap system is implemented, but the devnet reserve is currently unfunded and the system is not audited for production use.

### Solana devnet USDC SPL lane (new deployment)

The fresh genesis now exposes Circle's official Solana devnet USDC as an enabled wrapped asset. The combined program uses checked SPL Token CPI, a mint allowlist, 6-decimal validation, a PDA-owned USDC vault token account, and deterministic replay PDAs. The configured identifiers are:

- Mint: `4zMMC9srt5Ri5X14GAgXhaHii3GnPAEERYPJgZJDncDU`
- Vault token account: `H9q1t5qw2gbwD9RoUSJp4wNiqWMAyr4zmwz53NmM9bG8`
- Program: `FyAuUc2pPkz1nt2vR27R6NfE3Lgb4Z69sjUoPSeU7PCw`

Only this devnet USDC mint is enabled. No Solana USDC tokens have been deposited into the vault yet, so an end-to-end mint smoke test requires a devnet USDC faucet balance first. Ethereum and Solana USDC deposits are now accepted as reserve collateral once finalized and recorded by the relayer.

### Reserve-backed MNA update

The wallet now exposes a fixed-rate reserve swap at `2 USDC = 1 MNA`. The swap is a signed Intertrain operation: it debits an approved wrapped-USDC balance, credits native MNA, and enforces the consensus reserve ledger's 2:1 collateralization. Redemption reverses the in-chain conversion into wrapped USDC; external USDC release remains separately handled by the reserve bridge.

The dedicated Sepolia USDC reserve contract is `0xab4056bCb0369897d6D5Ca1A13f670f76C75ef3e`. The current wallet also derives a Solana devnet account from the local recovery phrase, signs the allowlisted SPL-USDC deposit instruction, and shows the resulting Solana USDC/SOL balances. This is devnet/testnet infrastructure and is not suitable for real funds without audit and stronger key custody.
