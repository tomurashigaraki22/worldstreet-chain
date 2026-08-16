# Intertrain MNA stablecoin and bridge implementation plan

**Status:** architecture and activation plan for Phases 0–10. The fixed-rate reserve-backed genesis specification is in [MNA_RESERVE_BACKED_GENESIS_SPEC.md](MNA_RESERVE_BACKED_GENESIS_SPEC.md). The existing devnet MNA and Ethereum WETH bridge remain operational. Solana WSOL custody is currently disabled until SOL liquidity is funded. The MNA/USDC stablecoin, reserve redemption, validator fee distribution, and production multisig are **not activated yet**.

This document is deliberately explicit about the difference between code that already exists, devnet-only custody, and controls that are required before real USDC is accepted.

## Executive design

MNA will be a redeemable, reserve-backed Intertrain stablecoin. The minimum price is not created by minting more MNA. It is created by a binding redemption rule:

```text
A holder may redeem eligible MNA for USDC at or above the configured floor price,
subject to reserve availability, limits, and the published redemption policy.
```

The fixed supply rule is:

```text
2 USDC = 1 MNA
required_usdc_reserve = circulating_mna × 2
```

New MNA may be minted only when the reserve policy permits it. Validator rewards should come from collected MNA fees or separately reserved rewards; unlimited unbacked minting would undermine the floor.

wSOL is a separate liability and is backed 1:1 by SOL. It must never be counted as USDC collateral for MNA. A USDC-to-wSOL purchase is an exchange: the system must use USDC to acquire or reserve the corresponding SOL before crediting wSOL.

The recommended production custody topology is a threshold-controlled treasury (for example, 2-of-3 or 3-of-5) with signers on separate trust domains. Keeping all keys on one VPS is acceptable only for labelled devnet/testnet operation; it is not independent multisig security because one VPS compromise can expose every signer.

## Monetary invariants

1. **Floor:** every redeemable MNA unit has a published minimum USDC redemption value.
2. **Collateralization:** reserve-backed issuance never exceeds verified USDC reserves divided by the floor price.
3. **No cross-collateral confusion:** SOL backs wSOL; USDC backs redeemable MNA; neither is silently substituted for the other.
4. **No arbitrary reward inflation:** validator rewards are paid from fee revenue or a separately collateralized reward budget.
5. **Atomic accounting:** a source-chain payment, Intertrain credit, redemption, and reserve ledger entry have one idempotent operation ID.
6. **Pauseability:** minting, purchases, and redemptions can be paused independently.
7. **Reconciliation:** external balances, Intertrain liabilities, pending operations, and fees reconcile continuously.
8. **Transparency:** users can inspect quote, fee, source transaction, confirmations, credited amount, and reserve coverage.

## Phase 0 — policy, scope, and risk acceptance

### Decisions to lock

- MNA decimals: existing 6 decimals.
- USDC decimals: 6 on common deployments; read the actual contract/mint metadata instead of assuming.
- Initial floor price: governance parameter, not hard-coded in the browser.
- Whether the floor is a guaranteed redemption price or only an informational reference. A stablecoin claim requires redemption.
- Reserve policy: 100% floor coverage for the first release; overcollateralization can be added later.
- Maximum redeemable supply and per-account/per-day limits.
- Fee policy: 0.5% minimum and 5% maximum for bridge/exchange operations, selected by a congestion policy.
- Internal Intertrain transaction fee: charged in MNA and distributed to validators after accounting for any reserve or operating allocation.
- Devnet policy: test USDC and fixed test prices are explicitly non-monetary and must not be described as real value.

### Acceptance criteria

- Published terms explain what “redeemable” means, when redemptions can be paused, and what limits apply.
- The wallet does not display “stablecoin” or “$ value” until the reserve and redemption gates are enabled.
- A reserve cannot be counted twice across MNA, wSOL, and WETH liabilities.

## Phase 1 — asset registry and monetary accounting

Extend the asset registry with:

- `worldstreet:MNA:native` — native Intertrain fee and stablecoin asset.
- `ethereum:USDC:<network>:<contract>` — source collateral identity.
  - Current devnet token: Circle Sepolia USDC `0x1c7D4B196Cb0C7B01d743Fbc6116a902379C7238`.
- `solana:USDC:<network>:<mint>` — optional Solana source collateral identity.
- `solana:WSOL:<network>:vault:<vault>:<mint>` — current devnet custody identity.
- `ethereum:WETH:<network>:<bridge>` — existing WETH identity.

Add durable state records:

- `collateral_positions` — verified external reserve balances;
- `mna_liabilities` — outstanding redeemable MNA;
- `fee_epochs` — fees collected and validator allocations;
- `quotes` — price, expiry, source, fee, minimum output;
- `settlements` — source payment to destination credit;
- `redemptions` — MNA burn to USDC payout;
- `pause_state` — independent pause switches;
- `reserve_snapshots` — timestamped reconciliation records.

Every record must include network, asset ID, source transaction reference, operation ID, status, timestamps, and operator/governance authorization where applicable.

## Phase 2 — treasury, multisig, and key custody

### Required production roles

1. **MNA reserve treasury** — USDC collateral and redemption payouts.
2. **SOL reserve vault** — SOL backing wSOL releases.
3. **Ethereum bridge authority** — contract mint/release/pause authority.
4. **Solana bridge authority** — reviewed program authority if moving beyond custody mode.
5. **Fee distributor** — receives protocol fees and distributes validator rewards.
6. **Governance multisig** — changes floor, caps, fee bounds, signers, and pause policy.
7. **Emergency pause signer** — can stop new issuance/redemptions without moving reserves.

### Recommended topology

- Governance: 2-of-3 or 3-of-5 multisig.
- Treasury: separate 2-of-3 or 3-of-5 multisig.
- Relayer: hot key with limited permissions and strict amount/rate limits.
- Validators: separate consensus keys; never reuse treasury or bridge keys.
- Emergency key: offline or separately hosted.

A VPS-held 2-of-3 arrangement is only operational separation, not true independent security. For production, at least one signer should be outside the VPS and preferably hardware-backed.

### Key rules

- No seed phrase or private key in the browser, repository, or ordinary `.env` file.
- Encrypted keystores must be root-only and backed up offline.
- Rotate relayer keys without changing reserve ownership.
- Reconcile signer set and contract authority after every rotation.
- Do not use the current single Sepolia relayer as a permanent mainnet treasury authority.

## Phase 3 — external USDC and bridge contracts

### Ethereum

Implement or deploy an audited USDC settlement/bridge contract with:

- `depositUSDC(destination, amount, quoteId)`;
- `releaseUSDC(recipient, amount, redemptionId)`;
- replay protection keyed by source transaction/operation;
- mint/release limits;
- pause and rate-limit controls;
- role-based authority or multisig ownership;
- emitted events containing quote and operation IDs;
- emergency recovery that cannot silently inflate MNA.

The existing WETH contract is not an MNA stablecoin contract and must not be repurposed without review.

### Solana

For devnet, the existing low-cost custody mode can remain. For production, choose one of:

- a reviewed Solana program controlling USDC/SOL vaults; or
- a reviewed multisig custody provider and a relayer with verifiable settlement records.

The current ordinary Solana WSOL vault is not trustless and should remain devnet-only until replaced or formally accepted as a custodial product.

### Intertrain authorization

The relayer must not be able to mint unlimited MNA. Its authorization should be scoped to:

- a configured asset;
- a bounded amount;
- a valid quote or settlement;
- a source-chain finality proof;
- a nonce/replay record;
- reserve and supply checks.

## Phase 4 — pricing and quote engine

### Price sources

- Devnet: fixed administrative price, clearly labelled.
- Production MNA/USDC: reserve policy plus governance floor; optionally an AMM/oracle reference price.
- SOL/USD: Pyth/Chainlink-class oracle or audited market source.
- USDC/USD: treated as approximately 1 only with a depeg monitor and a configurable haircut.

Do not use a single unverified HTTP price in the browser as an authority.

### Quote formula

```text
output = input × source_price / destination_price × (1 - fee)
```

Use integer smallest units and explicit rounding direction. A quote must contain:

- quote ID;
- source and destination assets;
- input and output amounts;
- price and price source;
- fee percentage and fixed fee;
- minimum received;
- expiry time;
- maximum allowed slippage;
- reserve coverage at quote time;
- congestion snapshot.

### Congestion-based fee policy

The requested 0.5–5% range should be a bounded policy, not arbitrary relayer discretion:

- 0.5% baseline;
- increases with source-chain gas, confirmation delay, RPC lag, queue depth, and reserve/market risk;
- hard cap of 5%;
- published reason and timestamp;
- quote remains immutable after acceptance;
- governance can pause rather than exceed 5%.

## Phase 5 — USDC purchase and redemption settlement

### Purchase flow

```text
quoted → awaiting_payment → payment_detected → confirming → confirmed → credited
```

The relayer verifies the source transaction, finality, recipient, asset contract/mint, amount, quote expiry, and replay key before crediting MNA or wSOL.

### MNA redemption flow

```text
redemption_requested → MNA_locked → burn_finalized → USDC_payment_submitted → paid
```

A redemption must:

- lock/burn MNA before payout;
- calculate payout using the published floor and fee;
- check reserve coverage and daily limits;
- submit USDC from the treasury authority;
- wait for finality;
- record the source transaction;
- remain retryable and idempotent.

If the treasury is under-reserved, new issuance must stop. Redemptions may be paused only under the published emergency policy; the event must be visible to users.

### wSOL purchase flow

A USDC-to-wSOL purchase is not free minting. The treasury/bridge must acquire or reserve equal SOL, then credit wSOL 1:1. wSOL burns release equal SOL less the published fee.

## Phase 6 — Intertrain protocol changes

Implement protocol-level parameters rather than browser-only rules:

- `mna_floor_price_usdc`;
- `mna_redeemable_supply_cap`;
- `mna_reserve_ratio_minimum`;
- `mna_mint_pause`;
- `mna_redeem_pause`;
- `bridge_purchase_pause`;
- `fee_min_bps = 50`;
- `fee_max_bps = 500`;
- validator fee share schedule;
- governance-controlled parameter version and activation height.

Add consensus validation for:

- reserve-backed mint caps;
- duplicate settlement/redeem operations;
- fee bounds;
- validator distribution totals;
- integer overflow and decimal conversion;
- pause state and authority signatures.

## Phase 7 — fees and validator distribution

All normal Intertrain transaction fees are denominated in MNA. The fee path should be:

```text
user pays MNA fee → fee escrow → protocol allocation → validator distribution
```

Recommended first schedule:

- validator reward pool: 70–90%;
- protocol reserve/insurance: 10–20%;
- operations/relayer budget: 0–10%.

The exact percentages require governance approval. Validators must receive fees from collected balances rather than unlimited new MNA if the stablecoin floor is to remain credible.

Distribute by finalized fee epochs, with:

- validator uptime/participation rules;
- deterministic allocation records;
- missed-block treatment;
- claim or automatic payout mechanism;
- audit trail in block state.

## Phase 8 — wallet, explorer, and user UX

Add wallet actions:

- `Buy MNA with USDC`;
- `Buy wSOL with USDC`;
- `Redeem MNA for USDC`;
- `Send MNA`;
- `View fee and validator allocation`.

Show before signing:

- rate;
- fee (0.5–5%);
- minimum received;
- quote expiry;
- source-chain network;
- destination chain;
- reserve coverage;
- confirmation estimate.

Show after signing:

- quote ID;
- source transaction hash;
- Intertrain operation ID;
- confirmation progress;
- credited/redeemed amount;
- failure, retry, or refund state.

Never show a generic “stablecoin” label while the reserve or redemption gates are disabled.

## Phase 9 — monitoring, reconciliation, and security testing

Monitor continuously:

- USDC reserve balance;
- SOL reserve balance;
- MNA redeemable liabilities;
- reserve coverage ratio;
- pending settlements;
- quote expiry failures;
- source-chain finality lag;
- relayer retries;
- duplicate operations;
- fee distribution totals;
- validator participation;
- price-source divergence;
- USDC depeg or abnormal liquidity;
- signer and multisig changes.

Create alerts for:

- coverage below minimum;
- reserve mismatch;
- mint cap exhaustion;
- repeated failed payouts;
- vault balance drain;
- unexpected contract authority change;
- fee outside 0.5–5%;
- stale oracle or quote service.

Test adversarially:

- replayed payment;
- underpayment/overpayment;
- expired quote;
- malformed destination;
- chain reorganization;
- relayer restart;
- RPC outage;
- multisig refusal;
- reserve exhaustion;
- concurrent redemption;
- validator outage;
- USDC depeg;
- integer/decimal conversion errors.

## Phase 10 — staged activation and production launch

### Devnet gate

- Use mock/test USDC only.
- Use a fixed price and small issuance cap.
- Keep custody mode visibly labelled.
- Use the existing Solana vault only for devnet.
- Demonstrate purchase, redemption, fee distribution, retries, and reconciliation.

### Testnet gate

- Deploy audited testnet bridge/treasury contracts.
- Configure a threshold signer set.
- Fund only a limited test reserve.
- Test pause, recovery, signer rotation, and refunds.
- Publish reserve and liability reports.

### Mainnet gate

Do not enable real USDC until:

- contracts/programs are independently audited;
- multisig signers are independent and recoverable;
- reserve and liability accounting reconciles;
- redemption terms are legally reviewed;
- rate limits and emergency pauses are tested;
- a staged cap and incident process are approved.

## Wallets and contracts currently created

These are public identities only. No private key or password is included here.

| Role | Network | Public identity | Current state |
|---|---|---|---|
| Sepolia deployer/relayer | Ethereum Sepolia | `0x286c46f1f17d4C948586D2fAB7F571198405ad4b` | Existing single testnet relayer; not a production treasury multisig |
| WETH bridge | Ethereum Sepolia | `0xaA82D61ACBcED55CF4cC49bE9018d3E5A6Ba2A9D` | Existing WETH bridge contract |
| Solana WSOL vault | Solana devnet | `GkCm35JZP1iVUUsNdPiKMCacBdQ2sSZtq87U93HvAd58` | Existing custody vault; devnet-only |
| Solana devnet deployer | Solana devnet | `EQQvAukwEiwiXShm93HTSLC2vTftMnPzeRxUD4LGd6rv` | Existing testnet keypair |
| Optional Solana program key | Solana devnet | `hLnLBGa1oCByrMsRxLsJhCLtqaYVYsY42rvTXeE36xR` | Extra devnet deployment; not referenced by wallet, node, or relayer; live bridge remains Fy... |
| Canonical WSOL mint | Solana | `So11111111111111111111111111111111111111112` | External canonical mint identity |
| Intertrain validator 1 | Intertrain devnet | `8a88e3dd7409f195fd52db2d3cba5d72ca6709bf1d94121bf3748801b40f6f5c` | Deterministic devnet validator key |
| Intertrain validator 2 | Intertrain devnet | `8139770ea87d175f56a35466c34c7ecccb8d8a91b4ee37a25df60f5b8fc9b394` | Deterministic devnet validator key |
| Intertrain validator 3 | Intertrain devnet | `ed4928c628d1c2c6eae90338905995612959273a5c63f93636c14614ac8737d1` | Deterministic devnet validator key |
| Intertrain validator 4 | Intertrain devnet | `ca93ac1705187071d67b83c7ff0efe8108e8ec4530575d7726879333dbdabe7c` | Deterministic devnet validator key |

### Existing secret file locations

- Foundry encrypted keystore: `/root/.foundry/keystores/intertrain-sepolia-deployer`
- Foundry password file: `/etc/worldstreet/relayer-password`
- Solana WSOL vault keypair: `/root/.config/solana/intertrain-wsol-vault.json`
- Solana devnet deployer keypair: `/root/.config/solana/intertrain-devnet-deployer.json`
- Optional Solana program keypair: `/root/.config/solana/intertrain-wsol-program.json`
- Relayer environment: `/etc/worldstreet/relayer.env`
- Relayer durable state: `/var/lib/worldstreet-relayer/state.sqlite3`

These files must remain root-only. The Foundry password and key contents must not be copied into this document, browser code, Git, or chat.

## Wallets still required before MNA stablecoin activation

The following identities do not exist yet and must be created through an approved key-generation ceremony:

1. Ethereum USDC reserve/treasury multisig.
2. Solana USDC reserve/treasury authority, if Solana USDC is enabled.
3. MNA governance multisig.
4. Fee distributor authority and validator reward escrow.
5. Emergency pause authority.
6. Independent backup/recovery signers.
7. Production relayer hot key with bounded permissions.

Do not reuse the current single VPS relayer as all of these roles.

## Activation checklist

- [ ] Governance approves floor, issuance cap, reserve ratio, and redemption terms.
- [ ] USDC contract/mint addresses are pinned per network.
- [ ] Treasury/multisig wallets are created and independently backed up.
- [ ] MNA minting authority is moved behind governance policy.
- [ ] Reserve ledger and redemption state are implemented.
- [ ] Quote and congestion-fee engine is implemented and tested.
- [ ] Validator fee distribution is deterministic and bounded.
- [ ] Purchase, redemption, refund, and pause UX is live.
- [ ] Reconciliation and reserve alerts are live.
- [ ] External audit and legal review are complete.
- [ ] Mainnet activation cap is approved and staged.
