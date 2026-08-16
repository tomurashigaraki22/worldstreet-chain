# Devnet USDC setup for Intertrain

The selected test stablecoin is Circle testnet USDC on Ethereum Sepolia. Circle publishes the Sepolia token contract as:

```text
0x1c7D4B196Cb0C7B01d743Fbc6116a902379C7238
```

Use the [Circle Testnet Faucet](https://faucet.circle.com/) to request test USDC and a separate Sepolia ETH faucet for gas. Testnet USDC has no real monetary value.

## Important: where to send it

Do **not** send USDC to the existing WETH bridge, the Sepolia deployer/relayer address, or the Solana WSOL vault. The dedicated USDC reserve contract is active at `0xab4056bCb0369897d6D5Ca1A13f670f76C75ef3e`. Approve it for Circle Sepolia USDC, then call its `deposit` method; never send USDC directly to the deployer/relayer address.

## Planned user flow

1. Claim Sepolia ETH and Circle test USDC.
2. Open the Intertrain wallet and request a USDC→MNA quote.
3. Approve the published USDC bridge contract for the quoted amount.
4. Submit the deposit with a replay-protected deposit ID and Intertrain destination.
5. Wait for the configured Sepolia confirmations.
6. The relayer credits the approved wrapped-USDC balance after finality; the signed wallet swap then mints MNA only when reserve and issuance checks pass.

The public status endpoint exposes the USDC token, bridge contract, reserve quote/settlement state, and whether the path is enabled.

## Current status

- USDC token identity: configured in the example environment.
- Ethereum Sepolia USDC reserve contract: deployed and enabled at `0xab4056bCb0369897d6D5Ca1A13f670f76C75ef3e`.
- USDC→MNA minting: implemented at the fixed devnet rate `2 USDC = 1 MNA`; it remains collateral-gated until USDC has been deposited.
- wSOL: enabled on Solana devnet program mode; the wallet exposes native SOL→wSOL deposits.

## Solana devnet USDC (SPL) — enabled

Solana devnet uses Circle's official USDC mint `4zMMC9srt5Ri5X14GAgXhaHii3GnPAEERYPJgZJDncDU`. The Intertrain combined Solana program accepts this mint only and deposits into vault token account `H9q1t5qw2gbwD9RoUSJp4wNiqWMAyr4zmwz53NmM9bG8`. Obtain test USDC from Circle's devnet tooling, then use the browser wallet's **Deposit Solana USDC** action (or `intertrain-wsol-client lock-spl`); the VPS relayer detects the finalized structured event and submits the matching wrapped-asset mint. Approved USDC deposits are recorded in the reserve ledger and can then be swapped for MNA at the fixed rate.

## Ethereum Sepolia reserve contract

The dedicated Intertrain USDC reserve contract is deployed at:

`0xab4056bCb0369897d6D5Ca1A13f670f76C75ef3e`

It is separate from the WETH bridge. Approve Circle Sepolia USDC to this contract before calling `deposit`; the destination string must be an Intertrain `mna1...` address. The relayer watches finalized `Deposit` events and credits the matching Intertrain USDC asset. Never send mainnet USDC to this testnet contract.
