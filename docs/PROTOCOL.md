# Worldstreet Chain protocol foundation

## Network identity

- Chain name: Worldstreet Chain
- Initial chain ID: worldstreet-devnet-1
- Native asset: MANNA
- Native ticker: MNA
- Base unit: microMNA
- Base-unit scale: 1 MNA = 1,000,000 microMNA
- Address HRP: mna
- Address format: Bech32m
- Address namespace: chain-wide, not asset-specific

The address is chain-wide so MANNA and future wrapped assets such as wrapped ETH can use the same wallet identity. The protocol already defines an explicit `AssetId`; the current transfer state machine activates only native MNA balances.

## Cryptography

- Signature algorithm: Ed25519
- Mnemonic standard: BIP-39
- Derivation standard: SLIP-0010 Ed25519 hardened derivation
- Provisional derivation path: m/44'/9999'/0'/0'/0'
- Address hash: SHA-256("MNA/address/v1" || public_key), first 20 bytes
- Transaction/block hashing: domain-separated SHA-256
- Deterministic encoding: Postcard field-order encoding for structs and enums; maps are not protocol values

The final coin type and public chain ID must be selected before a public testnet.

## Asset extensibility

The wallet key and address are not tied to MANNA. Future state types should use:

~~~text
AssetId {
    namespace: String,
    symbol: String,
    contract: Option<String>,
    decimals: u8
}
~~~

The native asset is represented by a reserved AssetId:

~~~text
namespace = "worldstreet"
symbol = "MNA"
contract = None
decimals = 6
~~~

Wrapped ETH and other wrapped assets must be introduced through explicit state transitions and verified bridge/deposit rules. They must not be created by making wallet addresses asset-specific.

## RPC and login

The MVP JSON-RPC endpoint is mounted at `/rpc` and uses JSON-RPC 2.0. Read methods expose chain, block,
transaction, account, validator, mempool, and finality data. `transaction_broadcast` accepts a fully signed
transaction and performs the same signature, nonce, fee, balance, and chain-ID checks as block execution.

Wallet login is a one-time challenge flow. A client requests `auth_challenge`, signs the returned exact
message with the wallet key, then submits the signature to `auth_verify`. The node checks that the public key
derives the requested `mna1...` address, enforces expiry, and consumes the nonce to prevent replay.

## Peer and finality MVP

The `wsc-network` crate provides a length-prefixed TCP transport for devnet peers. Each connection begins with
chain ID and genesis-hash validation, then supports block-range requests, block import, transaction relay,
and vote messages. `wsc-consensus` signs votes with Ed25519 and treats at least two-thirds of the configured
validator set as quorum. A node persists the finalized height/hash but does not yet implement production-grade
leader rotation, slashing, dynamic validator sets, encrypted peer transport, or a bridge for wrapped assets.

## Security boundary

- Wallets create and hold private keys.
- Nodes receive public keys and signed payloads only.
- Application login uses one-time signed challenges.
- No seed phrase or private key is accepted by an RPC endpoint.
- This code is experimental and must not hold real-value assets until reviewed.
