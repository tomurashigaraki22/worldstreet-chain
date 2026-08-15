# Worldstreet Chain (WSC)

## Comprehensive MVP implementation plan

**Status:** Implemented through Phase 12; the network remains experimental and requires independent review before real-value use.  
**Repository:** Greenfield  
**Target:** An experimental/devnet blockchain supporting wallets, signed native-token transfers, multiple nodes, validator finality, persistence, RPC, and wallet-signature login.

## 1. MVP outcome

The MVP is complete when a developer can:

1. Generate or restore a wallet.
2. Obtain a chain address using the mna prefix.
3. Fund the address on a local devnet.
4. Sign a transfer without exposing the private key to a node.
5. Broadcast the signed transaction.
6. Observe the transaction finalized in a block.
7. Query the resulting balances from multiple nodes.
8. Restart a node and recover the same chain state.
9. Start a new node and synchronize the chain.
10. Authenticate to an application by signing a login challenge.

The network is experimental. It must not be presented as mainnet-ready until the protocol, cryptography, consensus, wallet, and networking code have been independently reviewed.

## 2. MVP scope

### Included

- Native MANNA (MNA) asset.
- Account-based balances and nonces.
- Ed25519 signatures.
- BIP-39 recovery phrases.
- SLIP-0010 hardened Ed25519 derivation.
- Bech32m chain addresses with the mna prefix.
- Encrypted local keystore.
- Signed transfer transactions.
- Mempool and block production.
- Static Proof-of-Authority validators.
- Two-thirds validator finality.
- Peer-to-peer transaction, block, and vote propagation.
- Block synchronization.
- Embedded persistent storage.
- JSON-RPC API.
- Rust CLI.
- TypeScript client SDK.
- Devnet faucet.
- Wallet-signature login challenges.
- Unit, integration, property, fuzz, and end-to-end tests.

### Excluded

- Smart contracts.
- Staking, delegation, slashing, or validator rewards.
- Permissionless validator admission.
- Dynamic validator-set changes.
- Bridges, NFTs, privacy, governance, or light clients.
- Mobile or hardware wallets.
- Exchange integrations.
- Production custody.
- Mainnet tokenomics.

## 3. Protocol decisions

Freeze these decisions in versioned documents before implementation.

| Area | Decision |
|---|---|
| Chain name | Worldstreet Chain |
| Initial chain ID | worldstreet-devnet-1 |
| Native asset | MANNA (MNA) |
| Internal denomination | Integer microMNA units |
| State model | Account with balance and nonce |
| Signature | Ed25519 |
| Mnemonic | BIP-39 |
| HD derivation | SLIP-0010 hardened Ed25519 |
| Provisional path | m/44'/9999'/0'/0'/0' |
| Address | Bech32m with chain HRP mna |
| Hash | SHA-256 with WSC domain separation |
| Serialization | Canonical CBOR or specified deterministic binary encoding |
| Consensus | Static PoA with two-thirds finality |
| Block interval | Approximately 2 seconds on devnet |
| Fees | Fixed minimum fee |
| Smart contracts | Not supported |

Before public testnet, replace the provisional coin type and chain ID with values intentionally reserved for the project.

## 4. Repository layout

~~~text
worldstreet-chain/
├── Cargo.toml
├── crates/
│   ├── wsc-core/
│   ├── wsc-crypto/
│   ├── wsc-wallet/
│   ├── wsc-state/
│   ├── wsc-storage/
│   ├── wsc-consensus/
│   ├── wsc-network/
│   ├── wsc-rpc/
│   ├── wsc-node/
│   └── wsc-cli/
├── sdk/typescript/
├── tests/
├── fuzz/
├── devnet/
├── docs/
├── scripts/
└── .github/workflows/
~~~

### Crate boundaries

- wsc-core: protocol types, canonical encoding, IDs, validation errors.
- wsc-crypto: key generation, mnemonic derivation, signatures, hashes, addresses.
- wsc-wallet: encrypted keystores, restoration, local signing.
- wsc-state: account state and deterministic transaction execution.
- wsc-storage: blocks, transactions, state, indexes, snapshots.
- wsc-consensus: proposers, votes, quorum, finality.
- wsc-network: libp2p peers, gossip, status, block sync.
- wsc-rpc: JSON-RPC request handling and public query methods.
- wsc-node: runtime orchestration, mempool, block production, metrics.
- wsc-cli: node, wallet, transaction, query, and devnet commands.

Keep core protocol crates independent from databases, HTTP servers, and wallet-file concerns.

## 5. Cryptographic specification

Use established standards:

- BIP-39: https://github.com/bitcoin/bips/blob/master/bip-0039.mediawiki
- SLIP-0010: https://github.com/satoshilabs/slips/blob/master/slip-0010.md
- RFC 8032 Ed25519: https://www.rfc-editor.org/rfc/rfc8032

### Key derivation

Use this provisional path:

~~~text
m/44'/9999'/account'/change'/address_index'
~~~

Default MVP path:

~~~text
m/44'/9999'/0'/0'/0'
~~~

Because Ed25519 derivation is hardened, reject unsupported non-hardened derivation rather than silently producing incompatible keys.

### Address derivation

Define one canonical algorithm:

~~~text
payload = SHA256("WSC/address/v1" || public_key)[0..20]
address = Bech32m("wsc", version || payload)
~~~

Reject wrong HRP, bad checksum, bad length, mixed case, and unknown versions.

### Keystore

Use:

- Argon2id or an equivalent memory-hard password KDF.
- XChaCha20-Poly1305 or AES-256-GCM authenticated encryption.
- Random salt and nonce.
- Versioned keystore format.
- Atomic file writes.
- Restrictive file permissions.
- Zeroization of temporary secret material where supported.

The plaintext mnemonic must not be written automatically. Private keys must never be sent to nodes, RPC servers, login services, or logs.

The wallet password encrypts the local keystore. It is separate from the optional BIP-39 passphrase, which changes the derived wallet.

## 6. Transaction and state specification

### Transfer transaction

~~~json
{
  "version": 1,
  "chain_id": "worldstreet-devnet-1",
  "type": "transfer",
  "nonce": 0,
  "from": "mna1...",
  "to": "mna1...",
  "amount": 1000000,
  "fee": 10,
  "public_key": "...",
  "memo": "",
  "signature": "..."
}
~~~

Sign every field except signature:

~~~text
sign_bytes = canonical_encode(transaction_without_signature)
signature = Ed25519.sign(private_key, sign_bytes)
tx_id = SHA256("WSC/tx/v1" || sign_bytes)
~~~

### Account

~~~text
Account {
    address: Address,
    balance: u128,
    nonce: u64
}
~~~

Use integer arithmetic only. Reject overflow, underflow, zero amounts, negative values, and amounts that exceed configured limits.

### State transition

~~~text
sender.balance -= amount + fee
sender.nonce += 1
receiver.balance += amount
fee_pool += fee
~~~

Apply the transition atomically. Invalid transactions must leave state unchanged.

### Validation order

1. Decode.
2. Check version.
3. Check chain ID.
4. Check field sizes and limits.
5. Decode addresses and public key.
6. Verify public key derives the sender address.
7. Verify signature.
8. Verify nonce.
9. Verify amount and fee.
10. Verify sender balance.
11. Reject duplicates.
12. Admit to mempool or block execution.

## 7. Blocks, genesis, and storage

### Block header

~~~json
{
  "version": 1,
  "chain_id": "worldstreet-devnet-1",
  "height": 1,
  "parent_hash": "...",
  "timestamp": 1760000000,
  "proposer": "...",
  "transaction_root": "...",
  "state_root": "..."
}
~~~

The block hash is the domain-separated hash of canonical header bytes. The proposer signature must be checked against the validator selected for the height and round.

### Genesis

~~~json
{
  "version": 1,
  "chain_id": "worldstreet-devnet-1",
  "genesis_time": 1760000000,
  "block_time_ms": 2000,
  "initial_supply": 1000000000000,
  "fee_minimum": 1,
  "validators": [],
  "allocations": [],
  "genesis_hash": "..."
}
~~~

Nodes must reject a mismatched chain ID or genesis hash.

### Storage keys

~~~text
block/hash/<hash>
block/height/<height>
tx/id/<tx-id>
account/address/<address>
consensus/finalized-height
consensus/latest-height
consensus/validator-set
meta/genesis-hash
meta/chain-id
~~~

Block commits must atomically update block storage, height indexes, transaction indexes, account changes, and finalized-height metadata.

## 8. Consensus design

The MVP uses a static validator set from genesis.

Assumptions:

- Validators are authenticated.
- The validator set is static.
- Proposers rotate round-robin.
- A block is final after at least two-thirds of validators vote for it.
- There are no rewards or staking rules.
- There is no validator rotation.
- The consensus is experimental and unaudited.

Flow:

~~~text
select proposer
      ↓
create proposal
      ↓
validators validate
      ↓
broadcast signed votes
      ↓
aggregate votes
      ↓
two-thirds quorum?
   yes       no
   ↓         ↓
finalize   timeout/retry
~~~

Every proposal and vote includes chain ID, height, round, block hash, validator key, message type, and signature.

Reject unknown validators, invalid signatures, wrong heights, wrong rounds, wrong block hashes, duplicate votes, and conflicting finalized blocks.

## 9. P2P networking

Use libp2p with authenticated encrypted connections.

Initial protocols:

~~~text
/wsc/1/status
/wsc/1/get-blocks
/wsc/1/blocks
/wsc/1/transactions
/wsc/1/votes
~~~

Gossip topics:

~~~text
/wsc/tx/v1
/wsc/block/v1
/wsc/vote/v1
~~~

Use separate keys for:

- Node identity and P2P authentication.
- Wallet funds and transaction signing.
- Validator votes.

Synchronization flow:

1. Connect to bootstrap peers.
2. Exchange chain ID, genesis hash, latest height, latest hash, and finalized height.
3. Reject incompatible peers.
4. Request missing block ranges.
5. Validate parent links, signatures, transactions, roots, and finality.
6. Re-execute transactions locally.
7. Compare state roots.
8. Commit validated blocks.

Apply maximum message, transaction, block, and concurrent-request sizes. Rate-limit peers and back off or ban peers that repeatedly send invalid data.

## 10. RPC and CLI

### JSON-RPC methods

~~~text
node_status
chain_info
block_latest
block_get
transaction_get
transaction_broadcast
account_get
mempool_status
validator_set
~~~

Example request:

~~~json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "account_get",
  "params": {
    "address": "mna1..."
  }
}
~~~

RPC requirements:

- Strict schemas.
- Maximum request body.
- Rate limiting.
- Stable error codes.
- Correlation IDs.
- No secret endpoints.
- CORS disabled by default.
- Authentication for validator/admin methods.

### CLI

~~~text
wsc node init
wsc node start
wsc node status
wsc node peers
wsc node export-snapshot
wsc node import-snapshot

wsc wallet create
wsc wallet restore
wsc wallet list
wsc wallet address
wsc wallet show-public-key
wsc wallet sign-message
wsc wallet export-public

wsc tx build-transfer
wsc tx sign
wsc tx broadcast
wsc tx transfer
wsc tx get
wsc tx wait

wsc query account <address>
wsc query block <height-or-hash>
wsc query transaction <tx-id>
wsc query validators
wsc query chain
~~~

Support human-readable output and machine-readable JSON output.

## 11. Wallet-signature login

Wallet login is an application-level challenge and must not require a blockchain transaction.

Flow:

1. Client requests a challenge for an address.
2. Server creates a random one-time nonce.
3. Wallet signs a structured message.
4. Client submits message and signature.
5. Server verifies domain, chain ID, address, nonce, timestamps, and signature.
6. Server creates a session.

Example:

~~~text
Worldstreet Chain Login

Domain: app.example.com
Chain ID: worldstreet-devnet-1
Address: mna1...
Nonce: random-one-time-value
Issued At: 2026-08-15T12:00:00Z
Expires At: 2026-08-15T12:05:00Z
~~~

Reject expired challenges, reused nonces, wrong domains, wrong addresses, wrong chain IDs, and invalid signatures. Never ask a server for seed phrases or private keys.

# 12. Phase-by-phase plan

## Phase 0 — Charter and protocol specification

**Goal:** Freeze the protocol before coding.

### Work

- Confirm asset denomination.
- Confirm chain ID.
- Define transaction, block, genesis, and vote schemas.
- Define canonical serialization.
- Define address derivation and key path.
- Define nonce and fee behavior.
- Define validator finality assumptions.
- Write threat model.
- Record irreversible choices as ADRs.

### Deliverables

- docs/PROTOCOL.md
- docs/TRANSACTIONS.md
- docs/GENESIS.md
- docs/CONSENSUS.md
- docs/WALLET.md
- docs/THREAT_MODEL.md
- docs/adr/*

### Exit criteria

- Another developer can implement compatible protocol types from the documents.
- Every hash and signature input is explicit.
- No field depends on language-specific serialization.

## Phase 1 — Workspace and engineering foundation

**Goal:** Create a buildable, testable Rust workspace.

### Work

- Create Cargo workspace and crates.
- Add format, lint, and test configuration.
- Add structured logging and error conventions.
- Add CI.
- Pin and review dependencies.
- Add version command.
- Add release profile.

### Tests and gates

~~~text
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --workspace
cargo test --workspace --release
~~~

### Exit criteria

- Clean checkout builds.
- CI rejects format, lint, and test failures.
- Protocol crates remain independent from storage and networking.

## Phase 2 — Crypto primitives and deterministic encoding

**Goal:** Implement protocol primitives with known vectors.

### Work

- Hash, key, signature, address, and amount types.
- Ed25519 signing and verification.
- BIP-39 generation and validation.
- SLIP-0010 derivation.
- Bech32m address encoding.
- Canonical serialization.
- Domain-separated hashes.
- Sensitive-memory cleanup.

### Tests

- RFC 8032 vectors.
- BIP-39 vectors.
- SLIP-0010 vectors.
- Address round trips.
- Invalid checksums and lengths.
- Wrong HRP.
- Bad keys and signatures.
- Deterministic serialization and hashing.

### Exit criteria

- All known vectors pass.
- Identical inputs produce identical bytes and hashes.
- Malformed input never panics.

## Phase 3 — Wallet and encrypted keystore

**Goal:** Build an offline-capable wallet.

### Work

- Wallet creation and restoration.
- Mnemonic confirmation.
- Address display.
- Public-key export.
- Keystore encryption and unlock.
- Detached-message signing.
- Transaction signing.
- Backup and recovery instructions.

### Tests

- Create/restore address equality.
- Wrong password.
- Tampered ciphertext.
- Unsupported keystore version.
- Signature verification.
- Secret leakage checks.
- File permission checks where supported.

### Exit criteria

- Wallet signs offline.
- Restored wallet reproduces the same key and address.
- Private keys never leave the wallet process.

## Phase 4 — Transaction model and state machine

**Goal:** Implement deterministic native transfers without networking.

### Work

- Accounts, balances, nonces, fees.
- Genesis allocations.
- Transfer validation.
- Atomic state transitions.
- State-root calculation.
- Stable validation errors.

### Tests

- Valid transfer.
- Insufficient balance.
- Wrong nonce and replay.
- Duplicate transaction.
- Wrong chain ID.
- Invalid signature.
- Address/public-key mismatch.
- Overflow, underflow, zero amount.
- Invalid transaction leaves state unchanged.
- State-root determinism.

### Exit criteria

- Same initial state and transaction sequence always produce the same final state.
- Invalid transactions cannot partially mutate state.

## Phase 5 — Genesis, blocks, storage, and recovery

**Goal:** Persist a valid single-node chain.

### Work

- Genesis loading and hash checking.
- Block headers, hashes, roots, and signatures.
- Block validation.
- Embedded database integration.
- Atomic commits.
- Transaction indexing.
- Restart recovery.
- Snapshot export/import.

### Tests

- Genesis stability and mismatch rejection.
- Parent and height validation.
- Root validation.
- Proposer signature validation.
- Restart recovery.
- Commit-boundary crash simulation.
- Snapshot round trip.

### Exit criteria

- A node creates and persists blocks.
- Restart preserves the exact finalized state.
- Snapshot restore preserves height, hashes, and state root.

## Phase 6 — Mempool and single-node runtime

**Goal:** Accept transactions and produce blocks locally.

### Work

- Mempool admission.
- Deduplication.
- Nonce-aware queueing.
- Capacity and transaction-size limits.
- Block assembly.
- Block interval scheduling.
- Devnet-only faucet.
- Configuration loading.
- Graceful shutdown.

### Tests

- Valid and invalid admission.
- Duplicate suppression.
- Capacity limits.
- Nonce handling.
- Eviction policy.
- Block ordering.
- Faucet disabled outside devnet.
- Shutdown/restart.

### Exit criteria

- A signed transfer enters the mempool, is included in a block, and changes balances.
- Transaction and block queries work locally.

## Phase 7 — JSON-RPC and CLI

**Goal:** Make the node and wallet usable.

### Work

- JSON-RPC server.
- Request schemas and error codes.
- Account, block, transaction, node, and mempool queries.
- Transaction broadcasting.
- Node CLI.
- Wallet CLI.
- JSON output mode.
- RPC rate limiting.

### Tests

- Valid and invalid RPC calls.
- Unknown methods.
- Bad parameters.
- Oversized bodies.
- Malformed transaction broadcast.
- Unknown account.
- Block lookup by height and hash.
- Secret isolation.
- CLI exit codes.

### Exit criteria

- A developer can create a wallet, fund it, transfer funds, and query results with the CLI.
- RPC responses are stable for SDK use.

## Phase 8 — P2P networking and synchronization

**Goal:** Connect nodes and synchronize valid chain data.

### Work

- libp2p node identity.
- Encrypted peer connections.
- Bootstrap peers.
- Status exchange.
- Transaction, block, and vote gossip.
- Block-range requests.
- Peer validation.
- Message limits.
- Invalid-peer backoff.

### Tests

- Handshake.
- Wrong chain/genesis rejection.
- Transaction propagation.
- Block propagation.
- Block-range sync.
- Duplicate and out-of-order messages.
- Dropped-message recovery.
- Malformed network messages.
- Peer rate limits.

### Exit criteria

- Three or more nodes exchange transactions.
- A new node synchronizes from genesis.
- Nodes agree on block hashes before consensus finality.

## Phase 9 — Validator consensus and finality

**Goal:** Make multiple validators finalize the same blocks.

### Work

- Static validator set from genesis.
- Validator identity handling.
- Round-robin proposer.
- Proposal messages.
- Signed votes.
- Vote aggregation.
- Two-thirds quorum.
- Timeouts and retry rounds.
- Conflicting-block rejection.
- Finalized-height persistence.

### Tests

- Proposer selection.
- Unknown validator.
- Invalid vote signature.
- Wrong height, round, or block.
- Duplicate votes.
- Insufficient quorum.
- Two-thirds quorum.
- Offline validator.
- Delayed messages.
- Conflicting proposals.
- Restart during consensus.

### Exit criteria

- Four local validators finalize blocks.
- Honest nodes converge on block hashes and state roots.
- Tested failure scenarios do not produce conflicting finality.

## Phase 10 — Wallet login and TypeScript SDK

**Goal:** Let applications authenticate without receiving private keys.

### Work

- Challenge-message format.
- Challenge creation and expiry.
- One-time nonce storage.
- Signature verification.
- Minimal example application.
- TypeScript RPC client.
- TypeScript transaction builder.
- Browser-safe detached-message interface.

### Tests

- Valid signature.
- Expired challenge.
- Reused nonce.
- Wrong domain.
- Wrong chain ID.
- Wrong address.
- Invalid signature.
- Session creation.
- Rust/TypeScript serialization compatibility.

### Exit criteria

- Example application logs in with a wallet signature.
- Server never receives seed phrase or private key.

## Phase 11 — Devnet packaging and observability

**Goal:** Make the network reproducible.

### Work

- Docker images.
- Four-node Docker Compose devnet.
- Deterministic local validator keys.
- Genesis generation scripts.
- Health checks.
- Structured logs.
- Prometheus-compatible metrics.
- Block-height, peer, transaction, and consensus metrics.
- Devnet faucet.
- Troubleshooting guide.

### Exit criteria

~~~bash
docker compose -f devnet/docker-compose.yml up
~~~

starts four connected nodes with a shared genesis, block production, finality, faucet, and queryable RPC.

## Phase 12 — Security hardening and release

**Goal:** Prepare the private devnet/testnet release.

### Work

- Complete threat-model review.
- Fuzz transaction, block, RPC, P2P, and keystore decoders.
- Audit transaction validation and canonical serialization.
- Audit key handling and keystore encryption.
- Run dependency audits.
- Add denial-of-service controls.
- Protect validator/admin RPC methods.
- Complete backup/recovery documentation.
- Complete release checklist.
- Tag internal devnet release.

### Exit criteria

- No known critical or high-severity issues remain.
- CI passes all required checks.
- Devnet works from a clean machine.
- Release documentation calls the network experimental.
- No real-value deployment is encouraged.

## 13. Test strategy

### Unit tests

Cover:

- Mnemonic creation and validation.
- Key derivation.
- Known Ed25519, BIP-39, and SLIP-0010 vectors.
- Address encoding and decoding.
- Canonical serialization.
- Transaction and block IDs.
- Merkle or ordered transaction roots.
- State roots.
- Checked balance arithmetic.
- Every validation error.

### Property tests

Check that:

- Encode/decode round trips preserve values.
- Address encode/decode is reversible.
- Transaction IDs are stable.
- A changed signed field changes the transaction ID.
- Valid signatures verify.
- Any changed signed field invalidates the signature.
- State execution is deterministic.
- Invalid transactions do not mutate state.

### Integration tests

Run real node processes and verify:

- Peer discovery.
- Transaction gossip.
- Block gossip.
- Vote gossip.
- Finality.
- State convergence.
- Restart recovery.
- Snapshot recovery.
- New-node synchronization.
- Duplicate and invalid message rejection.

### Failure and chaos tests

Simulate:

- Process restart during block commit.
- Validator offline.
- Delayed messages.
- Dropped messages.
- Duplicate messages.
- Out-of-order messages.
- Temporary network partition.
- Conflicting proposals.
- Malicious transactions.
- Malformed network input.

### Fuzzing targets

Fuzz:

- Transaction decoding.
- Block decoding.
- Genesis decoding.
- RPC input.
- P2P messages.
- Address parsing.
- Keystore parsing.
- Canonical serialization.

The node must not panic on untrusted input.

### End-to-end wallet test

1. Create Alice.
2. Create Bob.
3. Fund Alice from the devnet faucet.
4. Build and sign a transfer offline.
5. Broadcast it.
6. Wait for finality.
7. Query both balances from multiple nodes.
8. Restore Alice from the mnemonic.
9. Confirm the same address.
10. Sign a second transfer.

### End-to-end login test

Cover valid login, expired challenges, reused nonces, wrong domain, wrong chain ID, wrong address, invalid signatures, and session creation.

## 14. CI gates

Every pull request should run:

~~~text
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --workspace
cargo test --workspace --release
cargo audit
cargo deny check
~~~

Scheduled jobs should include:

- Fuzzing.
- Cross-platform tests.
- Docker devnet smoke test.
- Multi-node integration test.
- Snapshot restore test.
- Dependency update review.

## 15. Security rules

- Never invent cryptographic algorithms.
- Never log seed phrases, private keys, or decrypted keystores.
- Never send private keys to nodes or servers.
- Never use floating-point balances.
- Never hash or sign non-canonical serialization.
- Always bind transactions to a chain ID.
- Always enforce nonces.
- Always check arithmetic overflow and underflow.
- Always validate network data before use.
- Never let malformed network input panic the node.
- Keep node identity keys separate from wallet keys.
- Disable the faucet outside explicit devnet mode.
- Treat consensus as unaudited until reviewed.
- Do not deploy a public mainnet from this MVP.

## 16. Post-MVP roadmap

Only after private devnet testing and independent review:

1. Public testnet with reset procedures.
2. Validator key rotation.
3. Dynamic validator-set governance.
4. Staking, delegation, and slashing design.
5. Fee-market improvements.
6. Light-client proofs.
7. Mobile and browser wallet support.
8. Hardware-wallet support.
9. Smart-contract research and a separate security review.
10. Economic model and token distribution.
11. Mainnet readiness review.

## 17. Dependency order

~~~text
Protocol specification
        ↓
Crypto and canonical encoding
        ↓
Wallet and keystore
        ↓
Transactions and state machine
        ↓
Blocks and storage
        ↓
Single-node runtime
        ↓
RPC and CLI
        ↓
P2P networking
        ↓
Validator consensus
        ↓
Wallet login and SDK
        ↓
Devnet packaging
        ↓
Fuzzing, hardening, and private release
~~~

Start by completing Phase 0, then implement Phases 1–3 before networking. This keeps protocol and wallet-recovery behavior stable before consensus and distributed-systems complexity is introduced.
