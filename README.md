# Worldstreet Chain

Early implementation of the Worldstreet Chain wallet and protocol foundation.

Current status:

- Phase 1: Rust workspace, CI, Docker/VPS workflow.
- Phase 2: deterministic encoding, Ed25519, BIP-39, SLIP-0010, MNA addresses.
- Phase 3: encrypted wallet keystores and wallet CLI.
- Phase 4: native MANNA transfer state machine and deterministic state roots.
- Phase 5: genesis, blocks, sled-backed persistence, indexes, and restart recovery.
- Phase 6: single-node mempool, block production, node CLI, and Docker smoke mode.
- Phase 7: JSON-RPC queries, transaction broadcast, signed block import, and wallet login challenges.
- Phase 8: framed TCP peer transport with chain/genesis handshakes and block synchronization.
- Phase 9: validator vote signatures, quorum checks, and persisted finality markers.
- Phase 10: dependency-light TypeScript SDK for RPC, transfers, and login flows.
- Phase 11: deterministic four-node Docker devnet, validator startup, health endpoints, metrics, and release tooling.
- Phase 12: bounded inputs, key zeroization improvements, fuzz targets, dependency audits, threat model, backups, and release checklist.

## Native asset

- Name: MANNA
- Ticker: MNA
- Base unit: microMNA
- 1 MNA = 1,000,000 microMNA

Wallet addresses are chain-wide and use the mna1... Bech32m prefix. Future wrapped ETH and other wrapped assets will use the same wallet address and an explicit AssetId in the state layer.

## Docker development

The local workstation does not need Rust installed.

~~~bash
docker compose run --rm wsc cargo test --workspace
docker compose run --rm wsc cargo fmt --all -- --check
docker compose run --rm wsc cargo clippy --workspace --all-targets --all-features -- -D warnings
docker compose run --rm wsc cargo build --workspace --release
~~~

Build the standalone CLI image:

~~~bash
docker build -t worldstreet-chain:dev .
docker run --rm worldstreet-chain:dev version
~~~

For a VPS, install Docker and run the same commands from the repository root. The first build downloads the Rust toolchain dependencies and may take several minutes.

## Wallet CLI

Create a wallet:

~~~bash
WSC_WALLET_PASSWORD='use-a-local-dev-password' \
docker compose run --rm wsc cargo run --package wsc -- wallet create --json
~~~

Use an explicit keystore path for repeatable tests:

~~~bash
WSC_WALLET_PASSWORD='use-a-local-dev-password' \
docker compose run --rm wsc cargo run --package wsc -- \
  wallet create --keystore /tmp/alice.keystore.json --json
~~~

Restore a wallet from a file:

~~~bash
WSC_WALLET_PASSWORD='use-a-local-dev-password' \
docker compose run --rm wsc cargo run --package wsc -- \
  wallet restore --keystore /tmp/alice-restored.keystore.json --mnemonic-file /tmp/alice.mnemonic
~~~

If the original wallet used a BIP-39 passphrase, provide it through an environment variable with
`--mnemonic-passphrase-env`. The passphrase is separate from the keystore password.

Sign a message:

~~~bash
printf 'Worldstreet Chain login' | \
WSC_WALLET_PASSWORD='use-a-local-dev-password' \
docker compose run --rm -T wsc cargo run --package wsc -- \
  wallet sign-message --keystore /tmp/alice.keystore.json
~~~

Do not use real recovery phrases or real-value passwords in shell history. The password environment variable is provided for CI/devnet automation only.

## Single-node devnet

Initialize a node:

~~~bash
docker compose run --rm wsc cargo run --package wsc -- node init --data-dir /app/.wsc/node
~~~

Produce one block and exit, which is useful for CI or VPS smoke tests:

~~~bash
docker compose run --rm wsc cargo run --package wsc -- node start --data-dir /app/.wsc/node --once
docker compose run --rm wsc cargo run --package wsc -- node status --data-dir /app/.wsc/node
~~~

Fund an address on devnet only:

~~~bash
docker compose run --rm wsc cargo run --package wsc -- node faucet \
  --data-dir /app/.wsc/node --address mna1... --amount 100000000
~~~

Run continuously:

~~~bash
docker compose run --rm --service-ports wsc cargo run --package wsc -- \
  node start --data-dir /app/.wsc/node --rpc-bind 0.0.0.0:26657
~~~

The RPC endpoint is `POST /rpc`. Example read-only request:

~~~bash
curl -s http://127.0.0.1:26657/rpc \
  -H 'content-type: application/json' \
  -d '{"jsonrpc":"2.0","id":1,"method":"chain_info","params":{}}'
~~~

Available MVP methods include `chain_info`, `block_latest`, `block_get`, `block_get_by_height`,
`transaction_get`, `transaction_broadcast`, `account_get`, `mempool_status`, `validator_set`,
`finality_status`, `auth_challenge`, and `auth_verify`. RPC accepts signed transactions only;
seed phrases and private keys never cross the endpoint.

The TCP transport is exposed as the `wsc-network` crate for devnet integration. It uses length-prefixed
canonical messages and rejects peers with a different chain ID or genesis hash. It is intentionally a
small MVP transport; production discovery, peer scoring, encryption, and DoS controls remain required.

Run RPC and peer transport together for a validator process:

~~~bash
docker compose run --rm --service-ports wsc cargo run --package wsc -- \
  node start --data-dir /app/.wsc/node --rpc-bind 0.0.0.0:26657 \
  --p2p-bind 0.0.0.0:26656 --peer 10.0.0.12:26656 --node-id validator-a
~~~

## TypeScript SDK

~~~bash
cd sdk/typescript
npm install
npm run typecheck
~~~

The SDK uses string amounts at JSON boundaries, preserving precision for MNA and future wrapped assets.
It does not store keys or choose a browser/mobile crypto implementation; applications should provide their
own audited signer and send only public keys and signatures to the chain.

## Four-node devnet and operations

~~~bash
docker compose -f devnet/docker-compose.yml up --build
curl http://127.0.0.1:26657/healthz
curl http://127.0.0.1:26657/metrics
~~~

The devnet uses deterministic validator secrets solely for repeatable local testing. Generated state is
under `devnet/data` and is ignored by Git. Review [THREAT_MODEL.md](docs/THREAT_MODEL.md) and
[RELEASE_CHECKLIST.md](docs/RELEASE_CHECKLIST.md) before any VPS deployment.

For the complete fresh-VPS deployment, test, backup, firewall, upgrade, and troubleshooting procedure, see
[VPS_DEPLOYMENT_AND_TEST_RUNBOOK.md](docs/VPS_DEPLOYMENT_AND_TEST_RUNBOOK.md).

## Standards

- BIP-39: https://github.com/bitcoin/bips/blob/master/bip-0039.mediawiki
- SLIP-0010: https://github.com/satoshilabs/slips/blob/master/slip-0010.md
- Ed25519 RFC 8032: https://www.rfc-editor.org/rfc/rfc8032
- Bech32m: https://github.com/bitcoin/bips/blob/master/bip-0350.mediawiki
