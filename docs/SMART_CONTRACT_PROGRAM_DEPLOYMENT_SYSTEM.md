# Intertrain Smart-Contract Program Deployment System

**Status:** Implemented on `worldstreet-devnet-1` as consensus-replicated chain state

**Updated:** 16 August 2026
**Scope:** Native Intertrain `.it` programs, not the external Ethereum/Solana bridge contracts

## 1. Executive summary

Intertrain's smart-program system builds restricted Rust source into a versioned `.it` WebAssembly package, verifies the package, and submits signed deploy, call, storage, and close operations to the blockchain. These operations are no longer stored in an RPC-local registry. They are admitted to the node's pending-operation pool, included in blocks, covered by the block transaction root, deterministically replayed by every validator, included in the state root, and saved in the normal durable chain-state snapshot.

Deployment and calls pay on-chain MNA fees. The signer must provide the current account nonce and a maximum fee. Accepted operations debit the signer, advance its nonce, and credit the consensus fee pool. Program packages, ownership, storage, receipts, and close tombstones persist after RPC, node, or container restarts because they are part of the same state snapshot as accounts and token state.

This is a real consensus integration, but still a devnet MVP. The bundled compiler only accepts a tiny Rust bootstrap form, and the runtime does not yet expose general contract arguments, program-controlled storage, events, asset transfers, or cross-program calls.

## 2. Delivered behavior

| Capability | Current behavior |
|---|---|
| Build | Restricted Rust is compiled into deterministic WASM and an `.it` package. |
| Verify/upload | Package integrity and WASM structure are checked; upload-only cache is temporary. |
| Deploy | Signed, fee-paying operation enters a block and creates a consensus program record. |
| Call | Signed, fee-paying operation executes deterministically during block application. |
| Receipt | Consensus state records status, return bytes, gas limit/used, fee paid, and error. |
| Storage | Owner-authorized bounded key/value writes are consensus operations. |
| Close | Owner-authorized close removes the program and creates a consensus tombstone. |
| Replication | Imported blocks replay the same operation and must produce the declared state root. |
| Restart persistence | The standard chain store restores programs and receipts from `StateSnapshot`. |
| Fees | Deploy charges by package byte; call charges by fuel used; fees enter the chain fee pool. |

## 3. Architecture

```text
Restricted Rust source
        |
        | it build / it_build
        v
Versioned .it package
  manifest + WASM + SHA-256 code hash
        |
        | local/RPC verification
        v
Signed ProgramOperation
  kind + program ID + nonce + max fee
  package/gas/storage fields as applicable
        |
        | node admission
        v
Pending program-operation pool
        |
        | scheduled proposer builds block
        v
Block.program_operations
  operation IDs included in transaction_root
        |
        | every validator deterministically applies
        v
Consensus State
  programs + receipts + storage + tombstones
  account debit + nonce + fee pool
        |
        | state_root and Store::commit
        v
Durable replicated state snapshot
```

### Code ownership

- `crates/wsc-core`: consensus operation, program record, receipt, and block data types.
- `crates/wsc-crypto`: canonical domain-separated program-operation ID.
- `crates/wsc-program`: `.it` codec, WASM verifier, restricted compiler, and fuel-metered runtime.
- `crates/wsc-state`: signature, nonce, fee, lifecycle, execution, and state-transition rules.
- `crates/wsc-node`: pending operation admission, block production, import replay, roots, and commits.
- `crates/wsc-storage`: block and state-snapshot persistence.
- `crates/wsc-rpc`: client build/submission/query API; it no longer owns durable program state.
- `crates/wsc-it`: local build, verify, and run CLI.

## 4. Consensus operation model

`ProgramOperation` contains:

- version and chain ID;
- operation kind: `Deploy`, `Call`, `StorageSet`, or `Close`;
- payer account nonce and maximum fee;
- program ID;
- `.it` package bytes for deployment;
- gas limit for calls;
- key/value for storage writes;
- Ed25519 public key and signature.

The operation ID is the domain-separated SHA-256 hash of the canonical operation encoding:

```text
SHA256("MNA/program-operation/v1" || canonical_program_operation)
```

The full signed operation is carried in `Block.program_operations`. Its operation ID is included alongside transaction and other operation IDs when computing the block transaction root. During import, a validator revalidates and reapplies each program operation, recomputes both transaction and state roots, and rejects a mismatch.

## 5. Authorization

The node derives the Intertrain address from the submitted Ed25519 public key. Clients sign this exact UTF-8 base message:

```text
Intertrain Program Authorization
Action: <deploy|call|storage_set|close>
Chain ID: worldstreet-devnet-1
Program ID: <program ID>
Owner: <address derived from public key>
Nonce: <current payer account nonce>
Fee: <maximum fee in MNA base units>
```

For a call, append:

```text
Gas Limit: <gas limit>
```

For a storage write, append:

```text
Key: <exact key>
Value: <exact value>
```

Deploy and call are signed by the payer. Storage and close additionally require that the derived signer address equals the program's recorded creator. Binding the chain ID, action, program ID, owner, nonce, fee, and action-specific fields prevents cross-chain, cross-action, and ordinary replay.

## 6. Fees and nonces

All four lifecycle operations advance the payer's account nonce. The submitted `fee` is a maximum authorization.

Current deterministic fee rules, in MNA base units:

```text
deploy required/paid = chain fee_minimum + encoded package bytes
call required max    = chain fee_minimum + gas_limit
call paid            = chain fee_minimum + gas_used
storage_set paid     = chain fee_minimum
close paid           = chain fee_minimum
```

The call admission rule reserves the ability to pay the maximum gas. After deterministic execution, only actual gas is charged on success. A failed runtime call records full `gas_limit` as used and therefore charges the maximum. Paid fees are added to the consensus `fee_pool`; unused call maximum remains in the account.

## 7. Deployment flow

### Build locally

```bash
printf 'fn main() -> i32 { 7 }\n' > main.rs
cargo run -p wsc-it -- build --language rust --source main.rs --out demo.it --name demo
cargo run -p wsc-it -- verify demo.it
cargo run -p wsc-it -- run demo.it --gas 100000
```

### Build by RPC

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "it_build",
  "params": {
    "language": "rust",
    "name": "demo",
    "source": "fn main() -> i32 { 7 }"
  }
}
```

The result contains `program_id`, `code_hash`, `manifest`, and `package_base64`.

### Deploy by RPC

```json
{
  "jsonrpc": "2.0",
  "id": 2,
  "method": "program_deploy",
  "params": {
    "package_base64": "<package>",
    "public_key": "<32-byte hex public key>",
    "signature": "<64-byte hex signature>",
    "nonce": 0,
    "fee": 1000
  }
}
```

The response is `pending` with an operation ID. “Pending” is deliberate: deployment is not confirmed until a proposer includes it in a block. After block inclusion, `program_get` returns the consensus record and `program_receipt` returns the confirmed receipt.

Deployment validation checks:

1. operation version and chain ID;
2. signature and derived payer address;
3. exact account nonce;
4. maximum fee and available balance;
5. `.it` package decoding, size, hash, and WASM rules;
6. package-derived program ID equality;
7. absence of an active program or prior close tombstone.

## 8. Call flow

```json
{
  "jsonrpc": "2.0",
  "id": 3,
  "method": "program_call",
  "params": {
    "program_id": "<program ID>",
    "gas_limit": 100000,
    "public_key": "<caller public key hex>",
    "signature": "<caller signature hex>",
    "nonce": 1,
    "fee": 100001
  }
}
```

The call is executed while validators apply the block, not inside the RPC handler. Each validator decodes the consensus-stored package, re-verifies it, creates a `wasmi` instance, grants the gas limit as fuel, and invokes the configured `() -> i32` entrypoint. The receipt is part of consensus state.

Receipt query:

```json
{
  "jsonrpc": "2.0",
  "id": 4,
  "method": "program_receipt",
  "params": { "operation_id": "<32-byte operation ID hex>" }
}
```

The confirmed receipt contains operation/program IDs, operation kind, status, little-endian return bytes as hex, gas used/limit, fee paid, and an optional error.

## 9. Storage and close

Storage reads are public consensus queries:

```json
{"jsonrpc":"2.0","id":5,"method":"program_storage_get","params":{"program_id":"<ID>","key":"counter"}}
```

Writes are owner-signed consensus operations. Keys are limited to 128 bytes and values to 4,096 bytes:

```json
{"jsonrpc":"2.0","id":6,"method":"program_storage_set","params":{"program_id":"<ID>","key":"counter","value":"1","public_key":"<hex>","signature":"<hex>","nonce":2,"fee":1}}
```

Close is also owner-signed and fee-paying:

```json
{"jsonrpc":"2.0","id":7,"method":"program_close","params":{"program_id":"<ID>","public_key":"<hex>","signature":"<hex>","nonce":3,"fee":1}}
```

Closing removes the active program and its storage, records a receipt, and creates a height-stamped consensus tombstone. The same content-derived program ID cannot be redeployed.

## 10. Persistence and replication guarantees

The following fields are serialized in `StateSnapshot` and committed by the standard chain store:

- `programs`: package bytes, creator, deployment height, and storage;
- `program_receipts`: deploy/call/storage/close outcomes and fees;
- `closed_programs`: program ID to close height.

These fields participate in `State::state_root`. A program operation therefore changes the state root committed to the block header. `Node::open` restores the latest snapshot and verifies that its recomputed root equals the latest block's state root. A mismatched or corrupted snapshot is rejected as corrupt chain data.

Replication occurs through normal block propagation/import. There is no `WSC_PROGRAM_REGISTRY_FILE` and no per-validator JSON program database. The only temporary RPC state is the optional upload cache before deployment.

Tests cover:

- program deployment changing the state root;
- on-chain deployment fee and account nonce accounting;
- deterministic call execution and actual-gas fee accounting;
- snapshot round-trip preserving the identical state root;
- one validator producing a deployment block and another importing it to the identical state root and program record;
- the existing node store restart path for committed state.

## 11. `.it` package and runtime

The package uses `ITPK` magic, format version 1, manifest length, WASM length, reserved bytes, manifest JSON, WASM, and a 64-character SHA-256 code hash.

Limits and restrictions:

- package maximum: 2 MiB;
- WASM maximum: 1 MiB;
- only allowed import: `env::host_log`;
- reference types, threads, SIMD, and bulk memory disabled;
- current entrypoint: zero arguments returning `i32`;
- code execution is fuel-metered.

The program ID is `it1` plus the first 40 hexadecimal characters of the WASM SHA-256 hash. Identity currently depends on WASM bytes, not the manifest or deployer.

The compiler bundled in this MVP accepts only:

```rust
fn main() -> i32 { 7 }
```

or the equivalent explicit `return`. This is not general Rust support.

## 12. RPC summary

| Method | Result source | Authorization |
|---|---|---|
| `it_build` | RPC computation | None |
| `it_verify` / `program_upload` / `contract_upload` | Temporary RPC cache | None |
| `program_deploy` | Pending consensus operation | Signed payer, nonce, max fee |
| `program_call` | Pending consensus operation | Signed payer, nonce, max fee |
| `program_storage_set` | Pending consensus operation | Signed owner, nonce, max fee |
| `program_close` | Pending consensus operation | Signed owner, nonce, max fee |
| `program_get` / `program_list` | Consensus state | None |
| `program_receipt` | Consensus state | None |
| `program_storage_get` | Consensus state | None |

## 13. Remaining production gates

The consensus persistence issue is resolved. Before real-value activation, the platform still needs:

1. a stable versioned host ABI with caller, inputs, program-controlled account storage, events, and asset APIs;
2. explicit memory, stack, recursion, aggregate storage, receipt-retention, and program-count limits;
3. account locks, reentrancy policy, cross-program call rules, and atomic child-call behavior;
4. a pinned reproducible `no_std` Rust SDK and source verification;
5. deployment/call preparation and fee-quote APIs for safer client signing;
6. wallet and explorer support for pending versus confirmed operations and verified source;
7. fuzzing, VM differential tests, crash/recovery tests, multi-platform consensus replay, and load tests;
8. independent audit of the codec, VM, signatures, state transitions, fees, and upgrade policy;
9. governance decisions for upgrades, pause controls, storage rent, pruning, and mainnet activation.

## 14. Leadership summary

> Intertrain smart-program deployments and calls now execute through blockchain consensus rather than an RPC-local registry. Signed, fee-paying operations are included in blocks, replayed by every validator, committed into transaction and state roots, and restored from the durable chain snapshot after restart. This resolves the persistence and multi-validator consistency gap. The remaining work is productization and security hardening of the restricted devnet runtime before real-value use.
