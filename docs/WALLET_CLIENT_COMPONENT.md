# Intertrain browser wallet component

**Live client:** [dev-wallets.watchup.space](https://dev-wallets.watchup.space)  
**Network:** `worldstreet-devnet-1` (client brand: **Intertrain**)  
**Status:** devnet-only, browser-local signing, not audited for production funds.

## What is shipped

The wallet is a single static browser client. It supports:

- an encrypted local vault protected by a password;
- multiple Intertrain wallets in one browser;
- local Ed25519 signing for MNA transfers, native-token operations, login challenges, and MNA swaps;
- MNA faucet funding on devnet;
- balances, nonces, asset registry, and operation status;
- fixed-rate USDC↔MNA reserve swaps;
- Solana devnet SOL→wSOL deposits;
- Solana devnet Circle USDC SPL deposits;
- MetaMask-based Ethereum Sepolia USDC approval and reserve-contract deposits;
- transaction/operation lookup with pending/confirmed/not-found details;
- a WorldstreetGold-style dark/gold presentation.

Private keys and recovery phrases are not sent to the VPS. The server receives only public addresses and signed payloads.

## Create and use a wallet

1. Open the live client and choose a strong vault password (the password encrypts the browser vault; it is not the blockchain transaction password).
2. Choose **Create wallet**, name it, and save the displayed recovery phrase offline.
3. Create additional wallets from the same vault when testing transfers.
4. Use **Fund with faucet** for devnet MNA.
5. Use **Sign in wallet** only when an application needs to prove control of the selected wallet; it creates a domain-bound challenge signature and does not move funds.
6. Lock the vault when leaving the browser.

The vault is stored in browser `localStorage` as AES-GCM ciphertext derived with PBKDF2. Clearing site data, losing the password, or losing the recovery phrase can make the local copy unrecoverable. For real funds, use an audited wallet and hardware-backed key management instead.

## MNA reserve swap flow

The current devnet policy is fixed: `2 USDC = 1 MNA`, with 6 decimal micro-units. The client calls:

1. `mna_quote` to display the conversion.
2. `account_get` to read the wallet nonce.
3. `mna_swap_prepare` to obtain canonical signing bytes.
4. Local Ed25519 signing in the browser.
5. `mna_swap_broadcast` to queue the signed operation.
6. `mna_swap_status` and `mna_reserve_status` to show finality and reserve state.

The swap is deliberately collateral-gated. The reserve must have enough approved USDC for the resulting MNA (`required reserve = minted MNA × 2`). With the current empty devnet reserve, a mint attempt correctly fails instead of creating unbacked MNA. Redemption converts native MNA into the approved wrapped-USDC asset; external USDC release additionally requires a funded reserve and relayer release operation.

Approved USDC lanes:

- Ethereum Sepolia Circle USDC: `ethereum:USDC:sepolia:0x1c7D4B196Cb0C7B01d743Fbc6116a902379C7238`.
- Solana devnet Circle USDC: `solana:USDC:devnet:4zMMC9srt5Ri5X14GAgXhaHii3GnPAEERYPJgZJDncDU`.

## Ethereum Sepolia component

The **Ethereum Sepolia USDC reserve** panel uses MetaMask (or another injected EVM provider). It switches/validates chain `0xaa36a7`, sends an ERC-20 `approve` transaction to the reserve contract, then calls `deposit(bytes32,uint256,string)` with a random replay ID and the current Intertrain destination. The Intertrain browser key is never used for Ethereum and MetaMask never sends its private key to the wallet server. Wait for the contract transaction to confirm, then wait for the relayer's configured confirmations before expecting the Intertrain USDC balance.

## Solana devnet component

The browser derives a Solana devnet account from the same recovery phrase using the SLIP-0010 path `[501,0,0,0]`, then signs directly against `https://api.devnet.solana.com`.

### SOL → wSOL

**Deposit SOL → wSOL** builds the deployed program's native lock instruction (tag `1`), transfers SOL to the program vault, and writes a random replay-protected deposit ID plus the Intertrain destination. The VPS relayer observes the finalized program event and mints the matching WSOL asset. The relation is 1 SOL locked = 1 WSOL represented on Intertrain, less any explicitly displayed network fee.

### SPL USDC

**Deposit Solana USDC** derives/creates the source associated token account, then calls the program's SPL lock instruction (tag `3`) for the approved Circle devnet mint. The relayer observes the finalized event, submits the idempotent Intertrain asset mint, and records the deposit as reserve collateral. The wallet needs devnet SOL for rent/transaction fees and Circle devnet USDC for the amount being deposited.

## DEX and SOL/USDC

A Solana DEX aggregator such as [Jupiter quote API](https://developers.jup.ag/docs/swap/v1/get-quote) and its [swap API](https://developers.jup.ag/docs/api-reference/swap/v1/swap) can route SOL/USDC when a network has usable pools and a supported route. That is separate from bridging: a DEX swap changes SOL into USDC on Solana, while the Intertrain bridge deposits the resulting USDC into the reserve lane.

The current wallet intentionally does not promise a devnet DEX route. Devnet liquidity and aggregator support are not reliable; users should use the Circle faucet for devnet USDC. A future DEX adapter should quote first, show price impact/slippage, require user approval, execute the Solana swap, then offer the USDC deposit. It must never treat raw SOL as USDC without a verified on-chain swap.

## Minimal RPC integration

A separate client can reproduce the reserve flow with JSON-RPC at `https://rpc-worldstreet.watchup.space/rpc`:

- `mna_quote({amount_usdc})`
- `mna_reserve_status({})`
- `mna_swap_prepare({unsigned})`
- `mna_swap_broadcast({operation})`
- `mna_swap_status({hash})`
- `account_get({address})`
- `bridge_status({})`

The client must sign only the exact `signing_bytes` returned by the prepare method, never a hand-written JSON serialization.

## Deployment/update checklist

The published client is `web-wallet/index.html`, served by nginx at `/var/www/dev-wallets/index.html`. After a change:

```bash
cd /root/worldstreet-chain
python3 - <<'PY'
from pathlib import Path
s = Path('web-wallet/index.html').read_text()
Path('/tmp/wallet-check.mjs').write_text(s.split('<script type="module">', 1)[1].split('</script>', 1)[0])
PY
node --check /tmp/wallet-check.mjs
install -m 0644 web-wallet/index.html /var/www/dev-wallets/index.html
nginx -t
systemctl reload nginx
```

Never put the VPS keystore password, relayer private key, or a production reserve key in this static client.

## What remains before real money

- independent review of the Intertrain state/crypto/bridge code;
- multisig or HSM custody instead of one VPS relayer key;
- funded reserve accounting and reconciliation;
- rate/oracle, slippage, limits, pause, and validator fee policy;
- audited Ethereum/Solana contracts and formal replay/idempotency tests;
- a supported mainnet DEX/liquidity provider;
- secure wallet UX review and recovery testing.
