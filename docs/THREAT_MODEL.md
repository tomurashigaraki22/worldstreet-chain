# Worldstreet Chain MVP threat model

## Assets

- Wallet seed phrases, private keys, and encrypted keystore files.
- Signed MNA transactions and validator signatures.
- Genesis identity, validator membership, block/state roots, and finality markers.
- Node databases and backups.

## Trust boundaries

1. Wallet process to RPC: only signed payloads and public keys cross this boundary.
2. RPC to node state: all transactions are revalidated before mempool admission and block execution.
3. Peer socket to network decoder: frames are length-limited, time-limited, canonical-decoded, and chain/genesis checked.
4. Filesystem to storage: the node treats persisted roots and chain identity as consistency checks.
5. Validator environment to process: validator private keys are loaded from environment variables and never logged.

## MVP mitigations

- Ed25519 signatures bind transactions, login challenges, votes, and proposer headers.
- BIP-39/SLIP-0010 wallet derivation and Argon2id plus XChaCha20-Poly1305 keystores protect wallet material at rest.
- RPC bodies, mempool entries, login challenges, peer frames, and block ranges have bounded sizes.
- Login challenges are chain/domain/address bound, expire, and are single-use.
- Peer messages require a matching chain ID and genesis hash.
- State, transaction, and block roots are recomputed before commit/import.
- Devnet validator keys are explicitly deterministic and non-production.

## Known residual risks

- Static validator membership, no slashing, and no production-grade leader election.
- TCP peer transport has no authenticated encryption or peer reputation system.
- Storage commit is multi-tree sled writes rather than a crash-tested atomic database transaction.
- RPC session tokens are MVP response artifacts and are not yet a durable authorization system.
- Wrapped ETH and other wrapped assets require bridge contracts, custody controls, replay protection, and audits.

Do not use this MVP with real-value funds.
