# Intertrain program platform — Rust MVP

Updated 2026-08-16. The client-facing chain name is **Intertrain**; the current network is `worldstreet-devnet-1`.

## What is implemented

The workspace now contains:

- `wsc-program`: versioned `.it` package encoding, SHA-256 code identity, structural WASM validation, and a deterministic `wasmi` runtime with fuel (gas) metering.
- `wsc-it`: a small command-line tool with `build`, `verify`, and `run` commands.
- `wsc-rpc`: `it_build`, `it_verify`, `program_upload`, `program_deploy`, `program_close`, `program_get`, `program_list`, `program_call`, `program_receipt`, `program_storage_get`, and `program_storage_set`.

A deployment or call is now a signed consensus operation carried in blocks, included in the transaction root, executed by every validator, and committed in the consensus state root and durable state snapshot. Receipts, bounded key/value storage, close tombstones, and program packages therefore replicate with the chain. Deployment and calls pay on-chain MNA fees and advance the payer's account nonce. Only the program owner can mutate storage or close a deployment.

## Rust frontend scope in this release

Only the Rust frontend is enabled. It accepts the deterministic bootstrap form:

```rust
fn main() -> i32 { 7 }
// or
fn main() -> i32 { return 7; }
```

The compiler currently lowers a signed `i32` literal into a small WASM module. It is deliberately **not** a general `rustc` replacement yet. Python and JavaScript are rejected by both the CLI and RPC until their sandboxed frontends are implemented.

## `.it` package

The package is a binary envelope containing:

1. `ITPK` magic and format version;
2. manifest JSON (name, language, compiler version, VM version, entrypoint, capabilities, ABI);
3. WASM bytes;
4. the 64-character SHA-256 code hash.

Limits are 2 MiB per package and 1 MiB for WASM. Imports are denied except `env::host_log`; reference types, threads, SIMD, and bulk-memory features are disabled. The default entrypoint is `main` with signature `() -> i32`.

## CLI

```bash
printf 'fn main() -> i32 { 7 }\n' > main.rs
cargo run -p wsc-it -- build --language rust --source main.rs --out main.it --name demo
cargo run -p wsc-it -- verify main.it
cargo run -p wsc-it -- run main.it --gas 100000
```

The build output prints the program ID and code hash. Keep the `.it` file and source together for reproducibility.

## RPC examples

Endpoint: `https://rpc-worldstreet.watchup.space/rpc`.

Build (the source is sent as a JSON string):

```json
{"jsonrpc":"2.0","id":1,"method":"it_build","params":{"language":"rust","name":"demo","source":"fn main() -> i32 { 7 }"}}
```

For Windows PowerShell, use `Invoke-RestMethod` (no Unix `curl` quoting is required):

```powershell
$body = @{ jsonrpc = "2.0"; id = 1; method = "it_build"; params = @{ language = "rust"; name = "demo"; source = "fn main() -> i32 { 7 }" } } | ConvertTo-Json -Depth 5
$result = Invoke-RestMethod -Uri "https://rpc-worldstreet.watchup.space/rpc" -Method Post -ContentType "application/json" -Body $body
$result.result.package_base64
```

The response contains `package_base64`. Upload/verify it with `program_upload` (or `it_verify`), deploy it with:

```json
{"jsonrpc":"2.0","id":2,"method":"program_deploy","params":{"public_key":"<32-byte public key hex>","signature":"<signature hex>","nonce":0,"fee":1000,"package_base64":"<value from it_build>"}}
```

The owner is identified cryptographically. The node derives the Intertrain address from `public_key`, then verifies an Ed25519 signature over this exact UTF-8 message (with real values substituted):

```text
Intertrain Program Authorization
Action: deploy | storage_set | close
Chain ID: worldstreet-devnet-1
Program ID: <program id>
Owner: <address derived from public key>
Nonce: <current account nonce>
Fee: <maximum fee in MNA base units>
```

For `call`, append `\nGas Limit: <gas limit>`. For `storage_set`, append `\nKey: <key>\nValue: <value>`. The stored deployment owner is the derived address; a different key cannot close or mutate it.

The minimum deploy fee is `fee_minimum + package_bytes`. The maximum call fee is `fee_minimum + gas_limit`; the chain charges `fee_minimum + gas_used` and leaves unused maximum fee in the payer account. Storage and close currently charge `fee_minimum`.

Then call and query the receipt:

```json
{"jsonrpc":"2.0","id":3,"method":"program_call","params":{"program_id":"<program id>","gas_limit":100000,"public_key":"<caller public key hex>","signature":"<signature hex>","nonce":1,"fee":100001}}
{"jsonrpc":"2.0","id":4,"method":"program_receipt","params":{"operation_id":"<operation id>"}}
```

Only the owner may close a deployment:

```json
{"jsonrpc":"2.0","id":8,"method":"program_close","params":{"program_id":"<program id>","public_key":"<owner public key hex>","signature":"<signature hex>","nonce":3,"fee":1}}
```

Storage is currently explicit and bounded:

```json
{"jsonrpc":"2.0","id":5,"method":"program_storage_set","params":{"program_id":"<program id>","public_key":"<owner public key hex>","signature":"<signature hex>","nonce":2,"fee":1,"key":"counter","value":"1"}}
{"jsonrpc":"2.0","id":6,"method":"program_storage_get","params":{"program_id":"<program id>","key":"counter"}}
```

## Important devnet limitation

Uploaded-but-not-deployed packages remain temporary, but deployed programs, calls, receipts, storage, and closures are consensus state. RPC mutation methods return `pending`; their effects become visible after block inclusion. The platform remains devnet-only because it still lacks account locks, ABI-encoded arguments/results, program-controlled storage host calls, call-depth limits, a general Rust SDK, resource/rent limits, and an independent audit. No real-value program should be deployed against it yet.
