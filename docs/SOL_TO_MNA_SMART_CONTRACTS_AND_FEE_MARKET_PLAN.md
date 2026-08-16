# SOL-priced MNA, Intertrain smart contracts, and fee-market implementation plan

**Target network:** `worldstreet-devnet-1`  
**Client brand:** Intertrain  
**Status:** selected design for implementation; direct SOL-backed MNA is the devnet and initial-launch path. This document does not activate it yet.

## Executive answer

Yes, the desired system can be built, but there are two separate products hidden inside the request:

1. A SOL-priced MNA purchase lane.
2. A deterministic smart-contract platform with compilers, deployment fees, gas, and validator economics.

The selected order is to implement direct SOL-backed purchasing first, then the VM/compiler on a separate activation version. DEX-first purchasing is explicitly deferred until mainnet liquidity and route reliability are available. Arbitrary Python or JavaScript cannot be executed directly by validators without nondeterminism and security problems. The practical design is:

- Rust compiled to deterministic WASM first;
- JavaScript/TypeScript through an AssemblyScript-like restricted subset;
- Python through a restricted typed subset or an SDK that compiles to the same intermediate representation;
- all programs packaged as `.it` files and verified before deployment.

A normal JavaScript or Python runtime with filesystem, network, dynamic imports, reflection, or unrestricted floating point must not run inside consensus.

## 1. SOL value of MNA

The policy remains:

```text
1 MNA = 2 USD
2 USDC = 1 MNA
```

For every purchase, the system needs a fresh SOL/USD price snapshot. If the oracle says SOL = P USD, the amount of SOL corresponding to one MNA is:

```text
sol_lamports_per_mna = ceil((2 USD * 10^9 lamports/SOL) / P)
```

For a user deposit of `L` lamports:

```text
mna_out = floor(L * P / 2 USD) - network_fee - protocol_fee
```

All calculations use fixed-point integers. No JavaScript floating-point values are accepted in consensus.

### Oracle requirements

The oracle snapshot must include:

- feed identifier, such as SOL/USD;
- signed price and exponent;
- publish time and slot/height;
- confidence interval;
- maximum allowed age;
- maximum allowed confidence-to-price ratio;
- source and oracle configuration version.

Pyth documents SOL/USD feeds sponsored on Solana devnet and supports on-chain price updates from Hermes data. The implementation should use verified on-chain price updates, not trust a browser HTTP response as truth. See the [Pyth Solana feed documentation](https://docs.pyth.network/price-feeds/core/push-feeds/solana) and [Pyth price-update documentation](https://docs.pyth.network/price-feeds/core/fetch-price-updates).

### Purchase routes

#### Selected route — direct SOL-backed purchase (devnet and initial launch)

```text
SOL in the user's Solana wallet
  -> Intertrain Solana program vault
  -> finalized SOL deposit + oracle snapshot
  -> relayer submits verified SOL reserve operation
  -> reserve ledger credits USD value
  -> Intertrain issues MNA at $2 per MNA
```

This is the selected devnet and initial-launch route. The Solana program vault holds SOL, so it is a custody lane and must have explicit limits, release controls, reconciliation, and multisig/HSM authority before real funds. The wallet shows the oracle price, SOL amount, MNA output, fees, expiry, and slippage bound before signing.

#### Deferred route — DEX-first purchase (mainnet)

```text
SOL in the user's Solana wallet
  -> Solana DEX route (for example Jupiter)
  -> Solana USDC
  -> existing Intertrain USDC deposit lane
  -> wrapped USDC on Intertrain
  -> signed MNA swap at 2 USDC = 1 MNA
```

This route is reserved for the mainnet phase after liquidity, routing, slippage controls, and a supported DEX provider are available. It remains disabled on devnet and initial launch.

### Selected rollout behavior

- `direct_sol_mna`: the only enabled SOL purchase lane for devnet and initial launch, with a small cap, oracle staleness check, confidence bound, replay protection, and reserve reconciliation.
- `dex_sol_to_usdc`: disabled until mainnet liquidity, route reliability, slippage controls, and provider monitoring are approved.
- if the direct oracle or vault health check fails, the wallet shows “SOL purchase temporarily unavailable”; it never fabricates a price.

## 2. Reserve model extension

The current reserve ledger is USDC-based. Add a multi-collateral ledger with separate balances:

```text
verified_usdc_reserve_usd
verified_sol_reserve_lamports
verified_sol_reserve_usd
released_usdc_usd
released_sol_lamports
reserve_backed_mna_usd
```

Each external deposit records:

```text
source_chain
asset
external_transaction
external_event
amount_native
oracle_snapshot
usd_value_at_acceptance
recipient
operation_id
status
```

Do not silently mix SOL, USDC, WETH, and WSOL backing. WETH/WSOL backing remains a separate wrapped-asset accounting lane unless governance explicitly adds it as MNA collateral.

The reserve invariant becomes:

```text
verified_reserve_usd - released_reserve_usd
  >= reserve_backed_mna_supply * 2 USD
```

Use conservative rounding: round collateral value down and required collateral up.

## 3. Dynamic congestion fees

Do not encode a random percentage in the wallet. The chain must calculate fees from consensus-visible inputs.

### Fee components

```text
fee = base_fee_usd
    + execution_gas_fee
    + storage_fee
    + congestion_surcharge
    + external_bridge_fee (when applicable)
```

The user-requested 0.5–5% range should be treated as a bounded congestion surcharge, not as an unexplained percentage removed from every transfer.

### Congestion inputs

At each block/epoch, calculate a deterministic utilization score from:

- previous block gas used / gas limit;
- mempool demand and queued bytes;
- validator processing capacity;
- storage growth;
- bridge queue depth, if a bridge operation is selected.

Example bounded multiplier:

```text
utilization = weighted_average(gas, bytes, mempool)
congestion_bps = clamp(50 + utilization * 450, 50, 500)
```

That yields a 0.50% to 5.00% surcharge. The exact weights and cap belong in versioned chain configuration, not frontend code.

### Fee payment assets

V1 should charge normal protocol fees in MNA micro-units. Add a fee router later for USDC or SOL:

```text
user-selected fee asset
  -> oracle conversion at transaction inclusion
  -> fee escrow in the selected asset
  -> canonical USD fee record
```

A transaction must include a maximum fee and the oracle/configuration version it accepted. If the price moves beyond the user's limit, the transaction expires rather than overcharging.

## 4. Stablecoin-based validator distribution

Validators should not receive an arbitrary amount of newly minted MNA. Create epoch accounting:

```text
FeeEscrow
  - collected_mna
  - collected_usdc
  - collected_sol
  - value_usd_at_inclusion
  - epoch_id

ValidatorRewardEpoch
  - validator_id
  - signed_blocks
  - valid_votes
  - uptime_score
  - slashing_adjustment
  - reward_usd
  - payout_asset
  - payout_status
```

Initial devnet split can be configuration, for example:

```text
80% validator rewards
10% protocol treasury
10% insurance/maintenance or burn
```

Do not hard-code those percentages permanently; activate them through a governance/configuration version.

For stable economics, calculate rewards in USD accounting units and settle either:

- USDC from a funded reserve; or
- MNA only when the reserve-backed issuance invariant permits it.

If the reserve is empty, rewards must remain an accounting liability or be paid from explicitly marked devnet faucet supply. Never claim that unfunded MNA rewards are stablecoin-backed.

## 5. Intertrain smart-contract runtime

### Execution model

Use an account-based deterministic WASM runtime:

```text
DeployProgram -> ProgramAccount + immutable code hash + manifest
CallProgram   -> instruction + accounts + input bytes
Runtime       -> gas metering + host ABI + state transition
Receipt       -> events + return data + gas used + state writes
```

Every validator executes the same bytecode with the same host functions. Program code cannot access the network, filesystem, wall clock, operating system, threads, or nondeterministic randomness.

### `.it` package format

`.it` is a signed deterministic deployment container, not merely a renamed WASM file:

```text
magic: IT01
vm_version
compiler_version
source_language
code_hash
abi_hash
manifest_hash
capability_flags
code_size
memory_limit
max_gas
ABI/metadata
WASM bytecode
optional source map
optional reproducible-build manifest
```

The chain stores the code hash and manifest. The explorer displays source language, compiler version, ABI, hash, creator, deployment fee, and verification status.

### Host ABI V1

Only expose deterministic calls:

```text
get_caller()
get_program_id()
get_block_height()
get_block_timestamp()       # consensus timestamp, bounded
read_account(key)
write_account(key, value)
transfer(asset, from, to, amount)
emit_event(topic, data)
call_program(program_id, accounts, data)
read_oracle(feed_id)
consume_gas(units)
```

Cross-program calls need a call-depth limit, account locks, reentrancy rules, and a gas budget inherited from the parent call.

## 6. Compiler toolchain

### Common pipeline

```text
source
  -> parser
  -> type checker
  -> restricted intermediate representation (IR)
  -> deterministic WASM/code generation
  -> ABI and manifest generation
  -> static verifier
  -> .it package
  -> deployment transaction
```

The compiler must reject unsupported features instead of silently changing behavior.

### Rust

Phase-one language target:

- Rust `no_std` contract SDK;
- `wasm32-unknown-unknown` target;
- deterministic allocator and panic behavior;
- explicit entrypoint and ABI macros;
- no filesystem, network, threads, or floating point;
- reproducible release builds.

### JavaScript/TypeScript

Do not compile arbitrary Node.js/browser JavaScript. Use an AssemblyScript-like typed subset:

- explicit integer types;
- no `eval`, dynamic imports, prototypes, timers, promises, filesystem, or network;
- deterministic standard library;
- explicit ABI exports;
- compile to the same IR/WASM target.

### Python

Do not embed CPython in validators. Start with a Python-like restricted language or typed DSL:

- no dynamic imports, reflection, `eval`, native extensions, I/O, or unbounded recursion;
- explicit integer/string/bytes/map types;
- compile to the common IR;
- publish a clear compatibility matrix rather than promising “all Python.”

Python full-language support can be a later non-consensus tooling language that generates IR, but it must not run as an unrestricted interpreter inside a validator.

## 7. Program deployment and fees

Add protocol operations:

```text
deploy_program
upgrade_program       # governance or immutable-authority controlled
call_program
close_program         # only if storage policy allows
```

Deployment validation:

- `.it` magic and VM version;
- code hash and ABI hash;
- signature by deployer;
- code size and memory limits;
- maximum gas declaration;
- capability allowlist;
- verifier/static-analysis result;
- reproducible compiler metadata;
- deployment fee and storage deposit.

Deployment fee formula:

```text
deploy_fee = base_deploy_fee
           + byte_fee * code_bytes
           + abi_fee * abi_bytes
           + storage_deposit
           + verification_fee
```

Call fee formula:

```text
call_fee = base_call_fee + gas_used * gas_price + storage_delta_fee
```

The transaction carries `max_fee`; unused gas is refunded according to deterministic rules.

## 8. RPC, explorer, and wallet changes

Add RPC methods:

```text
program_prepare_deploy
program_broadcast_deploy
program_get
program_list
program_prepare_call
program_broadcast_call
program_call_status
program_events
fee_quote
fee_config
validator_rewards
oracle_price
```

Extend blocks and receipts with:

```text
program_deployments
program_calls
program_events
gas_used
storage_delta
fee_paid
oracle_snapshot_id
```

The wallet should provide:

- upload `.it` file;
- display code hash, ABI, capabilities, limits, compiler version, and fee quote;
- deploy confirmation;
- contract-call form generated from ABI;
- account and storage permission review;
- gas/max-fee control;
- receipt/event viewer.

The explorer should never present an unverified source upload as verified source code. Verification requires a matching reproducible build hash.

## 9. Security and determinism gates

Before enabling arbitrary user programs:

- deterministic WASM interpreter or sandbox;
- fuel/gas metering tested against denial-of-service cases;
- memory, stack, recursion, call-depth, and code-size limits;
- account lock and reentrancy rules;
- deterministic serialization and ABI versioning;
- capability-based host functions;
- upgrade and emergency pause policy;
- static verifier and malformed-bytecode rejection;
- state rent and storage garbage collection;
- cross-program call isolation;
- reproducible builds and source verification;
- fuzzing, differential execution, and consensus replay tests;
- independent audit before mainnet.

A compiler does not make arbitrary languages safe automatically. The runtime and host ABI are the security boundary.

## 10. Phased implementation plan

### Phase 0 — Monetary and governance specification

- Freeze the $2 MNA policy and rounding rules.
- Define oracle feeds, staleness, confidence, and fallback behavior.
- Decide whether direct SOL purchases are devnet-only or a supported mainnet collateral lane.
- Define fee caps, reward split, treasury, insurance, and governance activation.
- Define the `.it` container and VM version policy.

### Phase 1 — Oracle and SOL quote service

- Add a consensus-visible `oracle_price` record.
- Integrate verified Pyth SOL/USD updates; do not trust browser-only prices.
- Add quote expiry, confidence checks, and stale-feed rejection.
- Add `sol_mna_quote` RPC with lamports, MNA output, fee, price, timestamp, and expiry.
- Add wallet display and explicit slippage/max-fee confirmation.

### Phase 2 — Direct SOL-backed MNA devnet lane

- Extend the reserve ledger with SOL collateral.
- Add signed/idempotent `sol_reserve_verify` and `sol_reserve_release` operations.
- Include oracle snapshot and external Solana signature in the operation.
- Add a small per-wallet and daily devnet cap.
- Reconcile program-vault lamports against the ledger.

### Phase 3 — Mainnet DEX-first SOL purchase (deferred)

- Keep disabled on devnet and initial launch.
- Add an optional Jupiter quote/swap adapter only after mainnet liquidity and route monitoring are approved.
- Require a valid route, output minimum, price impact, and expiry.
- Track the Solana swap hash separately from the bridge deposit.
- Disable automatically when devnet liquidity or provider health is unavailable.

### Phase 4 — Dynamic congestion fee engine

- Add deterministic gas/byte/mempool utilization metrics.
- Add versioned 0.5–5% congestion surcharge configuration.
- Add `fee_quote` RPC and max-fee transaction fields.
- Enforce inclusion-time fee checks and refunds.

### Phase 5 — Validator stablecoin economics

- Add fee escrow and epoch snapshots.
- Record fee value in USD using the inclusion oracle snapshot.
- Add validator reward weights, uptime, votes, slashing, and payout status.
- Start with devnet accounting-only rewards; do not imply funded stablecoin settlement.

### Phase 6 — VM and `.it` verifier

- Choose the deterministic WASM engine and version.
- Implement `.it` parsing, code hash, manifest, capability flags, and verification.
- Add gas/fuel metering, memory limits, storage charges, and host ABI V1.
- Reject unsupported imports and nondeterministic instructions.

### Phase 7 — Rust contract SDK and deployment

- Ship Rust `no_std` SDK, entrypoint macros, account APIs, events, and ABI generation.
- Implement deploy/call receipts and deployment fees.
- Add reproducible-build tooling and local simulator.
- Deploy sample counter, escrow, and token programs to a private devnet.

### Phase 8 — TypeScript/JavaScript and Python frontends

- Implement typed AssemblyScript/TypeScript subset compiler.
- Implement restricted Python-like compiler to the common IR.
- Publish language compatibility matrices and examples.
- Add compiler version pinning and `.it` build reproducibility.
- Do not claim full Python or full JavaScript compatibility.

### Phase 9 — Frontend and developer experience

- Add SDK methods for quotes, program upload, ABI-driven calls, gas estimation, and receipts.
- Add contract explorer pages and verified-source badges.
- Add wallet deployment/call review screens.
- Add CLI commands: `it build`, `it verify`, `it deploy`, `it call`, `it logs`.
- Add local testnet and simulator workflows.

### Phase 10 — Audit and activation gates

- Run consensus replay, fuzzing, differential VM, compiler, oracle, bridge, and fee tests.
- Audit VM sandbox, host ABI, deployment authority, bridges, and reserve accounting.
- Replace VPS-held single-key authority with multisig/HSM controls.
- Run a capped public devnet trial.
- Only after audit, governance approval, and funded reserves consider mainnet activation.

## 10A. Implementation landed in this devnet checkout

The direct SOL-backed lane now has the following code paths: an explicit Solana tag-5 deposit marker (`INTERTRAIN_SOL_MNA_DEPOSIT`), SOL collateral/oracle fields in reserve operations, aggregate USDC+SOL reserve invariants, idempotent MNA crediting, a `sol_mna_quote` RPC, a Pyth Hermes snapshot adapter in the relayer, and a wallet button that displays the quote before signing. The existing tag-1 wSOL lane remains separate.

The tag-5 upgrade is now published on the original `FyAuUc2pPkz1nt2vR27R6NfE3Lgb4Z69sjUoPSeU7PCw` program and `WSC_SOLANA_MNA_ENABLED=true` is active in the persistent relayer environment.

The quote endpoint currently uses the node's configured devnet snapshot for browser previews; the relayer fetches the official Pyth SOL/USD feed per deposit, rejects stale data, and submits the exact oracle snapshot into state. Before public use, make the node quote endpoint use the same live oracle adapter and add confidence-band checks.

## 11. What should be built first

The recommended next implementation order is:

1. `sol_mna_quote` with Pyth-verified pricing and stale/confidence checks.
2. Direct SOL-backed devnet purchase with strict caps.
3. Dynamic fee quote and congestion metrics.
4. Accounting-only validator reward epochs.
5. Deterministic WASM runtime and `.it` verifier.
6. Rust contract SDK and sample programs.
7. Restricted TypeScript and Python compilers.
8. Deployment wallet, explorer, CLI, tests, and audits.

Do not start with unrestricted Python/JavaScript execution. Start with the VM, deterministic host ABI, gas metering, and Rust contracts; add language frontends after the execution core is safe.

### Activation command

After funding the existing devnet program upgrade authority (`EQQvAukwEiwiXShm93HTSLC2vTftMnPzeRxUD4LGd6rv`) with at least 1 SOL, run:

```bash
/root/worldstreet-chain/ops/activate-sol-mna-devnet.sh
```

The script checks the balance, upgrades the existing no-Anchor program, enables the relayer flag, and restarts the persistent service. It does not print or handle any private key.
