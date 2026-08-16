# Intertrain frontend wallet implementation guide

Audience: frontend engineers building a new wallet or replacing the current static client.
Network: Intertrain devnet, protocol chain ID worldstreet-devnet-1.
Client brand: Intertrain / WorldstreetGold.
Status: devnet implementation handoff; protocol and bridge lanes are not audited for production funds.

This guide describes client-side responsibilities. It must not contain VPS relayer credentials, contract-owner keys, operator tokens, or private reserve keys. The supported integration path is the SDK; the later protocol sections are for SDK maintainers, auditors, and advanced clients only.

## 0. Use the SDK instead of protocol internals

For a normal frontend, start with the local package `@watchupltd/intertrain-wallet-sdk` in `packages/intertrain-wallet-sdk`. It wraps JSON-RPC envelopes, amount parsing, wallet/address derivation, Ed25519 transfer signing, MNA swap preparation/broadcast, browser vault encryption, MetaMask Sepolia USDC deposits, and Solana instruction data. The frontend should call SDK methods and keep the protocol encoding details out of React components. Do not copy the raw encoding sections into feature components.

See [packages/intertrain-wallet-sdk/README.md](../packages/intertrain-wallet-sdk/README.md). The package is currently a local devnet package; publish @watchupltd/intertrain-wallet-sdk to your registry or consume it with a file dependency when starting a separate frontend.

~~~ts
const rpc = new IntertrainRpc({ rpcUrl: "https://rpc-worldstreet.watchup.space/rpc" });
const vault = await BrowserVault.create("local-vault-password");
const record = vault.createWallet("Main wallet");
const wallet = LocalIntertrainWallet.fromMnemonic(vault.revealMnemonic(record.id));
await wallet.transfer(rpc, { to, amountMna: parseUnits("10.5", 6) });
~~~

The SDK deliberately excludes operator-only reserve methods and never sends private keys to the node. A normal frontend should stop at this abstraction layer; only the SDK package itself should need protocol codec details.

## 1. Public services and configuration

Use environment variables rather than scattering URLs through components:

~~~text
INTERTRAIN_RPC_URL=https://rpc-worldstreet.watchup.space/rpc
INTERTRAIN_HEALTH_URL=https://rpc-worldstreet.watchup.space/healthz
INTERTRAIN_EXPLORER_URL=https://wsc.watchup.space
INTERTRAIN_CHAIN_ID=worldstreet-devnet-1
INTERTRAIN_CLIENT_DOMAIN=your-frontend.example
SOLANA_DEVNET_RPC=https://api.devnet.solana.com
~~~

The current wallet is published at https://dev-wallets.watchup.space. A new frontend can use the same RPC methods.

### Asset constants

~~~text
Native MNA asset: worldstreet:MNA:native
MNA decimals: 6
Ethereum Sepolia Circle USDC: 0x1c7D4B196Cb0C7B01d743Fbc6116a902379C7238
Ethereum reserve contract: 0xab4056bCb0369897d6D5Ca1A13f670f76C75ef3e
Solana devnet Circle USDC mint: 4zMMC9srt5Ri5X14GAgXhaHii3GnPAEERYPJgZJDncDU
Solana bridge program: FyAuUc2pPkz1nt2vR27R6NfE3Lgb4Z69sjUoPSeU7PCw
Solana WSOL mint: So11111111111111111111111111111111111111112
~~~

Check bridge_status and asset_list at runtime; constants are only initial configuration.

## 2. Recommended application architecture

~~~mermaid
flowchart LR
  UI[Wallet UI] --> Store[Wallet state store]
  Store --> Vault[Encrypted browser vault]
  Store --> Signer[Local signing adapter]
  Store --> RPC[Intertrain JSON-RPC]
  Signer --> Solana[Solana devnet RPC]
  MetaMask[Injected EVM provider] --> Sepolia[Ethereum Sepolia]
  Sepolia --> Reserve[USDC reserve contract]
  Solana --> Program[Intertrain bridge program]
  RPC --> Relayer[VPS relayer and finality accounting]
~~~

Suggested modules:

~~~text
src/
  wallet/vault.ts       browser encryption and lock/unlock
  wallet/intertrain.ts mnemonic -> Ed25519 key/address
  wallet/solana.ts     mnemonic -> Solana key and bridge instructions
  wallet/evm.ts        MetaMask and Sepolia deposit calls
  rpc/client.ts        JSON-RPC transport
  rpc/schemas.ts       runtime response validation
  transactions/build.ts unsigned payload construction
  transactions/sign.ts canonical bytes and Ed25519 signatures
  transactions/tracker.ts pending/confirmed polling
  features/            balances, transfer, swaps, bridges, tokens, login
~~~

Keep only a selected wallet ID in the application store. Derive private keys just before signing and clear sensitive values after use.

## 3. Browser vault and wallet creation (SDK path)

First-use flow through the SDK:

~~~ts
import { BrowserVault, LocalIntertrainWallet } from "@watchupltd/intertrain-wallet-sdk";

const vault = await BrowserVault.create(vaultPassword);
const record = vault.createWallet("Main wallet");
const wallet = LocalIntertrainWallet.fromMnemonic(vault.revealMnemonic(record.id));
const encryptedRecord = await vault.export();
// Store encryptedRecord, never the mnemonic.
~~~

The SDK handles the following details internally:

1. Ask for a strong vault password. It encrypts browser storage; it is not a blockchain password.
2. Generate a 24-word BIP-39 English mnemonic.
3. Store wallet metadata only inside encrypted ciphertext.
4. Encrypt with Web Crypto AES-GCM using a random 12-byte IV and salt.
5. Derive the AES key with PBKDF2-SHA-256 using at least 210,000 iterations, or an audited stronger KDF.
6. Store only version, salt, IV, and ciphertext in IndexedDB or localStorage.
7. Require recovery-phrase backup confirmation before enabling sends.

Inner plaintext example:

~~~json
{
  "version": 1,
  "wallets": [{
    "id": "uuid",
    "name": "Main wallet",
    "mnemonic": "twelve or twenty-four words",
    "created_at": "2026-08-16T00:00:00Z"
  }],
  "active_wallet_id": "uuid"
}
~~~

The current static client uses storage key wsc-dev-wallet-vault-v1. A replacement frontend may use IndexedDB, but it must support an explicit restore/migration path.

### Intertrain key and address derivation

For compatibility with existing wallets:

1. seed = BIP39 mnemonicToSeedSync(mnemonic, empty passphrase).
2. SLIP-0010 Ed25519 path [44, 9999, 0, 0, 0], hardened at every level.
3. public_key = Ed25519 public key.
4. digest = SHA-256("MNA/address/v1" || public_key).
5. Address payload = version byte 0x01 plus the first 20 digest bytes.
6. Encode payload as Bech32m with HRP mna.

Do not use Ethereum secp256k1 derivation for an Intertrain account. MetaMask is a separate EVM account.

### Solana derivation

The current wallet derives a Solana devnet account from the same mnemonic using SLIP-0010 Ed25519 path [501, 0, 0, 0]. Display this public key separately from the mna1 address.

## 4. JSON-RPC client

~~~ts
export async function rpc<T>(method: string, params: unknown = {}): Promise<T> {
  const response = await fetch(INTERTRAIN_RPC_URL, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ jsonrpc: "2.0", id: crypto.randomUUID(), method, params })
  });
  const body = await response.json();
  if (body.error) throw new RpcError(body.error.code, body.error.message);
  return body.result as T;
}
~~~

Validate responses at runtime. Amounts, hashes, balances, nonces, and signatures are strings; use bigint or decimal libraries, never JavaScript number for transaction values.

At app bootstrap call chain_info, asset_list, bridge_status, and mna_reserve_status in parallel. For the active wallet call account_get(address), which returns native MNA balance, nonce, and an assets map.

## 5. Units and display rules

| Asset | Base unit | Decimals | Example |
|---|---:|---:|---|
| MNA | microMNA | 6 | 10.5 MNA = 10500000 |
| USDC | microUSDC | 6 | 2 USDC = 2000000 |
| SOL/WSOL | lamports | 9 | 0.1 SOL = 100000000 |

Validate decimals and bounds before signing. The reserve policy is integer-exact:

~~~text
amount_mna = amount_usdc / 2
required_reserve_usdc = reserve_backed_mna_minted * 2
~~~

USDC amounts that cannot divide evenly by two micro-units are rejected.

## 6. Native MNA transfer flow (SDK path)

Use the SDK for normal transfers:

~~~ts
const result = await wallet.transfer(rpc, {
  to: recipientAddress,
  amountMna: parseUnits("10.5", 6),
  feeMna: 1n,
  memo: "frontend transfer"
});
const final = await rpc.waitFor(() => rpc.transactionStatus(result.hash));
~~~

The SDK reads the sender nonce, constructs the unsigned operation, encodes canonical signing bytes, signs locally, and broadcasts.

### Advanced protocol detail

Read account_get for the sender and use the returned nonce. Never guess a nonce after an error.

Unsigned payload:

~~~json
{
  "version": 1,
  "chain_id": "worldstreet-devnet-1",
  "nonce": 0,
  "from": "mna1...sender",
  "to": "mna1...recipient",
  "amount": "10500000",
  "fee": "1",
  "public_key": "64 hex characters",
  "memo": "optional memo"
}
~~~

Canonical signing bytes are Postcard serialization of the Rust UnsignedTransaction struct, not JSON or a concatenated string. A production frontend should use a reviewed WASM/TypeScript codec generated from the protocol crate. Do not hand-roll Postcard encoding inside a UI component.

Sign the exact bytes with Ed25519 and send transaction_broadcast:

~~~json
{
  "transaction": {
    "unsigned": {
      "version": 1,
      "chain_id": "worldstreet-devnet-1",
      "nonce": 0,
      "from": "mna1...",
      "to": "mna1...",
      "amount": "10500000",
      "fee": "1",
      "public_key": "...",
      "memo": "..."
    },
    "signature": "128 hex characters"
  }
}
~~~

Save the returned hash immediately and poll transaction_status until confirmed.

## 7. MNA reserve swap (SDK path)

Use the SDK for the complete quote/sign/broadcast path:

~~~ts
const quote = await rpc.quote(parseUnits("10", 6));
const queued = await wallet.swapMna(rpc, {
  kind: "MintMna",
  collateralAsset: "ethereum:USDC:sepolia:0x1c7D4B196Cb0C7B01d743Fbc6116a902379C7238",
  amountUsdc: parseUnits("10", 6),
  feeMna: 1n
});
const final = await rpc.waitFor(() => rpc.swapStatus(queued.operation_id));
~~~

The SDK handles nonce reads, mna_swap_prepare, canonical signing bytes, local Ed25519 signing, and mna_swap_broadcast.

### Quote and reserve state

Call mna_quote with an integer micro-USDC string:

~~~json
{"amount_usdc":"10000000"}
~~~

A 10 USDC quote returns 5000000 microMNA and policy fields usdc_per_mna = 2, mna_per_usdc = 0.5, price_usdc = 2.000000.

Call mna_reserve_status before enabling the swap action:

~~~json
{
  "rate": "2 USDC = 1 MNA",
  "paused": false,
  "current_reserves_usdc": "0",
  "reserve_backed_mna_minted": "0",
  "required_reserve_usdc": "0",
  "surplus_usdc": "0",
  "collateralized": true
}
~~~

At zero balances, collateralized = true is only a vacuous invariant. It does not mean that the reserve is funded.

### Prepare, sign, broadcast

Build the unsigned operation with the current account nonce:

~~~json
{
  "version": 1,
  "chain_id": "worldstreet-devnet-1",
  "nonce": 0,
  "from": "mna1...",
  "kind": "MintMna",
  "collateral_asset": "ethereum:USDC:sepolia:0x1c7D4B196Cb0C7B01d743Fbc6116a902379C7238",
  "amount_usdc": "10000000",
  "amount_mna": "5000000",
  "fee": "1",
  "public_key": "64 hex characters",
  "memo": "frontend reserve swap"
}
~~~

Call mna_swap_prepare({unsigned}), decode the returned signing_bytes hex, and sign those exact bytes locally. Then call mna_swap_broadcast({operation:{unsigned,signature}}) and poll mna_swap_status({hash:operation_id}).

The operation ID is deterministic from the unsigned operation. Save it in local activity history. The browser must never call mna_reserve_mint or mna_reserve_release; those methods require the operator token and are relayer-only.

## 8. Ethereum Sepolia USDC deposit (SDK path)

Use the SDK helper with the injected MetaMask provider:

~~~ts
import { EthereumSepoliaUsdc, parseUnits } from "@watchupltd/intertrain-wallet-sdk";
const lane = new EthereumSepoliaUsdc();
const hashes = await lane.deposit(window.ethereum, parseUnits("10", 6), wallet.address);
~~~

The helper validates Sepolia, sends approval, waits for its receipt, and submits the reserve deposit. The application then polls the external receipt and Intertrain account state.

### Advanced protocol detail

Ethereum deposits use MetaMask or another injected EVM provider. The EVM account is separate from the Intertrain Ed25519 wallet.

User flow:

1. Request Sepolia ETH and Circle Sepolia USDC from faucets.
2. Connect MetaMask and verify chain ID 0xaa36a7.
3. Display the token contract, reserve contract, amount, and Intertrain destination for review.
4. Send ERC-20 approve(reserve_contract, amount).
5. Wait for the approval receipt.
6. Generate a random 32-byte deposit ID.
7. Call deposit(bytes32 depositId, uint256 amount, string destination) on the reserve contract.
8. Wait for the receipt and display the Ethereum hash.
9. Wait for relayer confirmations, then refresh account_get and mna_reserve_status.
10. Offer the MNA swap only once the Intertrain USDC balance is visible.

The contract is separate from WETH. It emits deposit evidence and never mints MNA directly.

Safety rules:

- Never import or request the MetaMask private key.
- Never send USDC to the relayer/deployer address.
- Verify chain ID, token contract, reserve contract, amount, and destination.
- A wallet prompt is not a mined transaction; wait for the receipt.
- Treat a reverted receipt as failed, not pending.

## 9. Solana devnet deposits (SDK data helpers)

Use the SDK data helpers, then submit the returned bytes with @solana/web3.js and @solana/spl-token against https://api.devnet.solana.com.

### Program PDAs

The SDK exposes the deployed PDA seed names in solanaPdaSeeds. The deployed program uses:

~~~text
state PDA: findProgramAddress(["intertrain-wsol-state"], program)
vault PDA: findProgramAddress(["intertrain-wsol-vault"], program)
~~~

Do not use the shorter state or vault seeds; they do not match the deployed program.

### Native SOL to wSOL

Use the SDK to produce instruction data:

~~~ts
import { nativeSolDepositData } from "@watchupltd/intertrain-wallet-sdk";
const data = nativeSolDepositData(amountLamports, crypto.getRandomValues(new Uint8Array(32)), wallet.address);
~~~

Instruction tag 1 data:

~~~text
[tag: u8 = 1]
[amount: u64 little-endian lamports]
[deposit_id: 32 bytes]
[destination_length: u16 little-endian]
[destination: ASCII bytes]
~~~

Accounts in order:

~~~text
depositor signer+writable
vault PDA writable
state PDA readonly
replay PDA writable, seed ["deposit", deposit_id]
System Program readonly
~~~

The user signs the Solana transaction. The relayer observes the finalized structured event and mints matching WSOL on Intertrain.

### Direct SOL to MNA (devnet lane)

First call `sol_mna_quote` through `IntertrainRpc` with the lamport amount. Display the returned oracle price, gross MNA, bounded fee, net MNA, and quote expiry. Only after the user confirms should the wallet sign a tag-5 instruction:

~~~ts
import { nativeSolMnaDepositData, solanaPdaSeeds } from "@watchupltd/intertrain-wallet-sdk";
const data = nativeSolMnaDepositData(amountLamports, crypto.getRandomValues(new Uint8Array(32)), wallet.address);
~~~

The accounts and replay PDA are identical to native wSOL, but tag 5 emits `INTERTRAIN_SOL_MNA_DEPOSIT`. The relayer obtains a fresh Pyth SOL/USD snapshot, verifies finality and stale-price limits, and submits the reserve operation. Do not display this as an instant swap: the Solana transaction and the Intertrain credit are separate statuses. The lane remains disabled until the upgraded program is deployed and the relayer activation flag is enabled.

### SPL USDC deposit

Use the SDK to produce instruction data:

~~~ts
import { splUsdcDepositData } from "@watchupltd/intertrain-wallet-sdk";
const data = splUsdcDepositData(amountMicroUsdc, crypto.getRandomValues(new Uint8Array(32)), wallet.address);
~~~

Instruction tag 3 data:

~~~text
[tag: u8 = 3]
[amount: u64 little-endian micro-USDC]
[deposit_id: 32 bytes]
[destination_length: u16 little-endian]
[destination: ASCII bytes]
~~~

Accounts in order:

~~~text
depositor signer+writable
source SPL token account writable
approved USDC mint readonly
vault USDC token account writable
vault PDA readonly
state PDA readonly
replay PDA writable, seed ["spl-deposit", deposit_id]
SPL Token program readonly
System Program readonly
~~~

Create the associated token account if needed. Only the configured Circle devnet USDC mint is accepted; arbitrary SPL and Token-2022 mints are rejected.

## 10. Custom native tokens (SDK/RPC path)

The token feature is protocol-native and does not execute arbitrary smart-contract bytecode. The frontend may expose Create, Transfer, Mint, Burn, and Freeze according to the token authorities. The separate Rust-only `.it` program MVP is documented in `PROGRAM_PLATFORM_MVP.md`; it is not yet a wallet token-contract lane.

Use token_operation_prepare, token_operation_broadcast, token_operation_status, token_get, and token_balance. The prepare response contains canonical signing_bytes; sign those bytes locally. Never derive a token ID in the UI: use the token_id returned by token_operation_prepare.

## 11. Authentication (Sign in wallet)

Authentication is not a transfer and does not spend funds:

1. auth_challenge({address, domain}) returns a nonce and exact message.
2. Sign the exact UTF-8 message with the selected Intertrain Ed25519 key.
3. auth_verify({address, domain, nonce, public_key, signature}) returns a session token.
4. Keep the token in memory or an application session; do not place private keys in it.

Bind the domain to the frontend origin and show the domain in the confirmation UI.

## 12. Transaction history and status UX

Maintain local history records such as:

~~~json
{
  "id": "local UUID",
  "operation_id": "64-hex hash",
  "kind": "transfer | mna-swap | token | ethereum-deposit | solana-deposit",
  "wallet_address": "mna1...",
  "created_at": "2026-08-16T00:00:00Z",
  "external_hash": "optional Ethereum/Solana hash"
}
~~~

Use these labels:

~~~text
queued/submitted -> Pending
confirmed        -> Confirmed
not_found        -> Unknown / not found yet
reverted/failed  -> Failed
~~~

Intertrain transfers use transaction_status. MNA swaps use mna_swap_status. Tokens use token_operation_status. External Ethereum and Solana hashes must be tracked separately from the Intertrain operation ID.

Never delete a pending record after one not_found result. Retry reads with backoff and keep the external hash visible.

## 13. UI component contract

A new frontend should include:

- VaultUnlockScreen: password entry, wrong-password state, recovery warning.
- WalletSwitcher: multiple wallets, active address, create/restore, lock.
- BalanceHeader: MNA balance, nonce, chain, connection indicator.
- AssetList: MNA, USDC, WSOL, custom tokens, decimals and origin chain.
- TransferForm: recipient validation, amount and fee preview, local signing confirmation.
- ReserveSwapCard: quote, reserve state, collateral lane, fixed rate, operation ID.
- EthereumDepositCard: MetaMask connection, contract verification, approval and receipt.
- SolanaDepositCard: derived address, SOL/USDC balances, SOL and SPL-USDC actions.
- TokenStudio: create and manage native custom tokens.
- ActivityTable: status, hashes, detail drawer, explorer links, refresh.
- LoginButton: domain-bound challenge signing labelled authentication only.

Every signing action must show network, asset, amount, fee, destination, and exact external contract/program address.

## 14. Error handling

| Condition | UI response |
|---|---|
| -32600 malformed request | Wallet update required; invalid request. |
| -32601 method missing | This node does not support this feature yet. |
| -32602 invalid parameters | Show the field-level validation message. |
| Signature or nonce error | Refresh account and ask the user to retry. |
| Insufficient balance | Show required amount plus fee. |
| Reserve insufficient | USDC reserve is not funded for this swap. |
| Paused or disabled lane | Disable the action and show bridge_status reason. |
| External receipt reverted | Show external hash and do not credit the user. |
| Relayer delay | Keep pending and show confirmations/last update. |

Do not automatically retry a user signature request. Retry only read calls and idempotent status checks.

## 15. DEX integration boundary

A DEX swap is not the same as a bridge deposit:

~~~text
SOL --Solana DEX route--> Solana USDC --bridge deposit--> Intertrain USDC --signed reserve swap--> MNA
~~~

A future adapter may use the [Jupiter quote API](https://developers.jup.ag/docs/swap/v1/get-quote) and [swap API](https://developers.jup.ag/docs/api-reference/swap/v1/swap), but it must verify that a route and liquidity exist on the selected network. Devnet routes are not dependable. Show route, price impact, slippage, and expiry, then treat the Solana swap as a separate activity before offering the USDC bridge deposit.

Never convert SOL to USDC by applying an assumed price in the frontend.

## 16. Testing checklist

### Unit tests

- mnemonic restore produces the same Intertrain address;
- wrong password cannot decrypt the vault;
- address checksum and prefix validation;
- integer parsing at zero, maximum decimals, and overflow boundaries;
- canonical signing bytes match Rust test vectors;
- changing any signed field invalidates the signature;
- activity history survives a page reload.

### Devnet integration tests

- create two wallets and transfer MNA;
- fund MNA with the faucet and inspect its hash;
- create and transfer a custom token;
- connect MetaMask on Sepolia and reject the wrong chain;
- approve and deposit a small Circle Sepolia USDC amount;
- deposit Solana USDC after creating the associated token account;
- deposit native SOL and observe WSOL after relayer finality;
- attempt MNA mint before collateral and verify rejection;
- deposit USDC, refresh balance, then mint at exactly 2:1;
- redeem MNA and verify wrapped-USDC accounting;
- reload while pending and confirm no duplicate submission.

### Before real funds

Require independent review of the protocol codec and address derivation, audited Ethereum/Solana programs, multisig or HSM relayer custody, rate limits and pause procedures, CSP/dependency/XSS review, and phishing-resistant recovery/authentication review.

## 17. Deployment handoff

The frontend is static and can be deployed behind nginx, Vercel, or another static host. Same-origin RPC proxying is preferred.

Minimum security headers:

~~~text
Content-Security-Policy: default-src 'self'; connect-src 'self' https://rpc-worldstreet.watchup.space https://api.devnet.solana.com; img-src 'self' data:; script-src 'self' https://esm.sh; frame-ancestors 'none'
Strict-Transport-Security: max-age=31536000; includeSubDomains
~~~

Pin ESM dependency versions, run a production build, and smoke-test chain_info, asset_list, mna_quote, and mna_reserve_status before publishing. Never ship operator tokens or server environment files to the browser.
