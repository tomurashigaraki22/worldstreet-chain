# MNA reserve-backed genesis specification

**Status:** implemented on the current devnet as a reserve-gated lane; a fresh production genesis and audit are still pending.

The pasted protocol specification is now the target monetary policy for Intertrain MNA:

```text
2 USDC = 1 MNA
1 MNA = 2 USDC on redemption
```

Both assets use six decimals. All accounting uses integer base units:

```text
mna_micro = usdc_base_units / 2
required_usdc_reserve = circulating_mna_micro * 2
```

Deposits that cannot be converted exactly are rejected; no remainder is silently discarded.

## Genesis target

A reserve-backed genesis must start with:

- `initial_supply = 0`;
- no redeemable MNA allocations;
- verified USDC reserves = 0;
- circulating reserve-backed MNA = 0;
- faucet/test supply tracked separately and never counted as backed MNA.

The current devnet genesis already has zero initial allocation, but its devnet faucet can create experimental MNA. That faucet must be explicitly marked test-only or disabled when reserve-backed mode is activated. Existing faucet balances cannot be silently reclassified as USDC-backed.

## Required protocol changes

1. Add a consensus-visible MNA reserve ledger containing verified deposits, releases, current reserves, reserve-backed minted supply, redeemed supply, required reserve, surplus, and pause state.
2. Add a dedicated `mna_reserve_mint` operation. Generic wrapped-asset `bridge_mint` must not mint reserve-backed MNA.
3. Validate approved source chain, USDC contract, finalized deposit reference, recipient, exact 2:1 conversion, operation uniqueness, overflow safety, and post-mint collateralization in state transition logic.
4. Add an idempotent redemption state machine: requested → finalized → release_pending → released/retry/terminal failure.
5. Add RPC methods for reserve status, deposit status, redemption status, recent settlements, and fixed-integer quotes.
6. Add an audited dedicated Sepolia USDC reserve contract. The existing WETH bridge remains separate and cannot collateralize MNA.
7. Extend the relayer to submit deposit evidence rather than receiving unlimited MNA mint authority.
8. Add explorer and wallet reserve-backed labeling, conversion display, finality progress, operation IDs, and external transaction hashes.
9. Keep WETH and native-SOL-backed WSOL accounting separate from MNA collateral.
10. Add invariant, replay, restart, finality, wrong-contract, wrong-chain, overflow, and redemption-failure tests.

## Current live state

- Intertrain devnet remains operational under `worldstreet-devnet-1`.
- WETH bridge is active on Sepolia.
- Native-SOL WSOL program bridge is active on Solana devnet.
- Ethereum and Solana USDC identities are configured; the dedicated Sepolia reserve contract is deployed and MNA reserve issuance is active but blocked until collateral is deposited.
- Existing faucet-created MNA is experimental and is not represented as reserve-backed.

## Migration decision

Changing the current running devnet to a clean reserve-backed genesis would discard its current blocks, faucet balances, token records, and wallet-visible state. The safe choices are:

- **Fresh reserve-backed devnet:** create a new data directory/genesis with a new chain ID, validate it, then switch public services; or
- **In-place migration:** preserve the chain ID but add a versioned activation height, quarantine all pre-activation faucet supply as test-only, and enable reserve-backed issuance only after the migration block.

A production launch should use the fresh zero-supply genesis. No destructive reset has been performed yet.

## Implementation update

The protocol now includes:

- fixed integer quote RPC: `mna_quote` (`2 USDC = 1 MNA`);
- signed wallet operations: `mna_swap_prepare`, `mna_swap_broadcast`, and `mna_swap_status`;
- consensus-visible reserve ledger RPC: `mna_reserve_status`;
- USDC collateral accounting for the approved Ethereum Sepolia and Solana devnet USDC asset IDs;
- a separate `IntertrainUsdcReserve` Ethereum contract.

The Sepolia reserve contract is deployed at:

`0xab4056bCb0369897d6D5Ca1A13f670f76C75ef3e`

It uses Circle Sepolia USDC `0x1c7D4B196Cb0C7B01d743Fbc6116a902379C7238`, relayer `0x286c46f1f17d4C948586D2fAB7F571198405ad4b`, replay-protected deposits/releases, and pause controls. It does not mint MNA directly. Finalized deposits are relayed into Intertrain USDC balances and counted as collateral; the signed wallet swap then converts wrapped USDC to MNA at the fixed rate.

This remains a devnet/testnet implementation and requires independent contract/protocol audit before real funds.
