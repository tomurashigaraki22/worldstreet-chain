# @watchupltd/intertrain-wallet-sdk

A small browser-facing TypeScript SDK for Intertrain devnet wallet integration. It is currently maintained as a local package in this repository, ready to publish to npm under the @watchupltd scope later. It hides RPC envelopes, amount conversion, Intertrain address derivation, Ed25519 signing, MNA swaps, local vault encryption, MetaMask Sepolia USDC deposits, and Solana bridge instruction encoding (including distinct native SOL→wSOL tag 1 and direct SOL→MNA tag 5 helpers).

## Install

```bash
npm install @watchupltd/intertrain-wallet-sdk @noble/curves @noble/hashes @scure/base @scure/bip39

# Before publishing, use the local package path instead:
# npm install ../worldstreet-chain/packages/intertrain-wallet-sdk @noble/curves @noble/hashes @scure/base @scure/bip39
```

For Solana transaction submission, install the peer dependencies `@solana/web3.js` and `@solana/spl-token`.

## Basic usage

```ts
import { BrowserVault, IntertrainRpc, LocalIntertrainWallet, parseUnits } from "@watchupltd/intertrain-wallet-sdk";

const rpc = new IntertrainRpc({
  rpcUrl: "https://rpc-worldstreet.watchup.space/rpc"
});
const vault = await BrowserVault.create("a-long-local-password");
const record = vault.createWallet("Main wallet");
const wallet = LocalIntertrainWallet.fromMnemonic(vault.revealMnemonic(record.id));

await rpc.faucet(wallet.address);
const transfer = await wallet.transfer(rpc, {
  to: "mna1...recipient",
  amountMna: parseUnits("10.5", 6),
  feeMna: 1n
});
const swap = await wallet.swapMna(rpc, {
  kind: "MintMna",
  collateralAsset: "ethereum:USDC:sepolia:0x1c7D4B196Cb0C7B01d743Fbc6116a902379C7238",
  amountUsdc: parseUnits("10", 6)
});
const encrypted = await vault.export();
```

The SDK returns operation hashes; the application still polls the corresponding status method and displays pending/confirmed states.

## Design boundary

The SDK is a devnet convenience layer, not a custody service. It never sends Intertrain private keys to the RPC server. `EthereumSepoliaUsdc` expects an injected MetaMask-style provider and never imports an EVM private key. Solana helpers produce canonical instruction data; the application submits those instructions through `@solana/web3.js`.

The SDK deliberately does not expose operator-only reserve RPC methods. Before production, add independent tests for the codec, key derivation, browser vault, and external bridge transactions.
