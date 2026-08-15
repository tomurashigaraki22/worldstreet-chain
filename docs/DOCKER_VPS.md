# Docker and VPS verification

## Requirements

- Docker Engine with Compose support.
- A Linux VPS with outbound access to crates.io during the first build.
- At least 2 GB RAM recommended for the initial Rust dependency build.
- At least 5 GB free disk space for the build cache.

## Verify the Docker engine

~~~bash
docker version
docker compose version
~~~

If Docker commands fail with a daemon or engine connection error, start Docker Engine/Desktop before running the project commands.

## Build and test

~~~bash
docker compose run --rm wsc cargo test --workspace
docker compose run --rm wsc cargo fmt --all -- --check
docker compose run --rm wsc cargo clippy --workspace --all-targets --all-features -- -D warnings
docker compose run --rm wsc cargo build --workspace --release
~~~

The Phase 7–10 checks additionally cover the RPC, peer, consensus, and SDK layers:

~~~bash
docker compose run --rm wsc cargo test -p wsc-rpc -p wsc-network -p wsc-consensus -p wsc-node
docker compose run --rm wsc cargo run --package wsc -- node init --data-dir /app/.wsc/node
docker compose run --rm --service-ports wsc cargo run --package wsc -- \
  node start --data-dir /app/.wsc/node --rpc-bind 0.0.0.0:26657
~~~

In a second terminal, query `http://127.0.0.1:26657/rpc` with the `chain_info` method. For peer tests,
run `node network` on two VPS processes using the same genesis file and different `--node-id` values,
then confirm that mismatched chain IDs or genesis hashes are rejected. The TypeScript SDK can be checked
with `cd sdk/typescript && npm install && npm run typecheck`.

The Compose service mounts the repository and persists Cargo caches in named volumes. This makes repeated VPS runs faster without installing Rust on the host.

Compose exposes TCP ports `26657` (RPC) and `26656` (devnet peer transport). Restrict these ports with
the VPS firewall; do not expose the MVP RPC or peer transport broadly on the public internet.

## Phase 11–12 devnet and release checks

~~~bash
docker compose -f devnet/docker-compose.yml up --build
curl http://127.0.0.1:26657/healthz
curl http://127.0.0.1:26657/metrics
docker compose run --rm wsc cargo audit
docker compose run --rm wsc cargo deny check
~~~

The four-node Compose project uses deterministic development-only validator keys, generates one shared
genesis, starts signed block producers, and connects all peers over the internal Compose network. Never
copy its validator secrets to a public testnet. Use `scripts/backup-node.sh` on Linux or
`scripts/backup-node.ps1` on Windows before maintenance, and test restoration on a separate data directory.

## Build the runtime image

~~~bash
docker build -t worldstreet-chain:dev .
docker run --rm worldstreet-chain:dev version
~~~

The runtime image contains the wsc binary and runs as the unprivileged wsc user.

## Wallet test in a container

Use a mounted directory for test keystores:

~~~bash
mkdir -p .wsc-test
docker run --rm \
  -e WSC_WALLET_PASSWORD='dev-only-password' \
  -v "$PWD/.wsc-test:/wallets" \
  worldstreet-chain:dev \
  wallet create --keystore /wallets/alice.keystore.json --json
~~~

The mnemonic printed during creation is recovery material. Do not save it in CI logs or commit it to the repository.

## VPS release discipline

For every VPS test:

1. Pull the intended repository revision.
2. Review the chain ID and genesis configuration.
3. Build the image from that revision.
4. Run the complete workspace test suite.
5. Run the CLI smoke test.
6. Keep test keystores outside the repository.
7. Delete devnet keys before reusing the VPS for real work.
8. Do not use mainnet funds or production secrets with this MVP.
