# Worldstreet Chain VPS deployment and test runbook

This runbook deploys the current experimental Worldstreet Chain MVP on a fresh Ubuntu/Debian VPS using Docker.
Do not use this release with real-value assets.


### Low-cost Solana WSOL custody mode

The devnet WSOL path does not deploy a custom Solana/Anchor program. Create the root-only vault and write its public configuration before starting Docker:

```bash
bash /root/worldstreet-chain/ops/setup-wsol-vault.sh
docker compose --env-file devnet/.env -f devnet/docker-compose.yml -f devnet/docker-compose.vps.yml up -d --build
systemctl daemon-reload
systemctl restart worldstreet-relayer.service
```

Use `docs/WSOL_CUSTODY_MODE.md` for the memo-tagged deposit format and PowerShell example. The vault keypair is `/root/.config/solana/intertrain-wsol-vault.json` and must remain mode `0600`; do not copy it to the browser or repository.

## 1. VPS requirements and Docker

Recommended minimum for the four-node devnet: 2 vCPU, 4 GB RAM, 20 GB free disk, and outbound HTTPS access
to GitHub, crates.io, and npm during the first build.

~~~bash
sudo apt-get update
sudo apt-get install -y git curl jq ca-certificates docker.io docker-compose-plugin
sudo systemctl enable --now docker
sudo usermod -aG docker "$USER"
~~~

Log out and back in after adding the user to the docker group, then verify:

~~~bash
docker version
docker compose version
~~~

If the Compose plugin is unavailable in the VPS distribution, install Docker Engine and the Compose plugin
from Docker's official installation instructions before continuing.

## 2. Clone the repository

~~~bash
mkdir -p ~/src
cd ~/src
git clone https://github.com/tomurashigaraki22/worldstreet-chain.git
cd worldstreet-chain
git checkout main
git rev-parse --short HEAD
~~~

Record the commit printed by git rev-parse. Every test and deployment should be tied to an explicit commit.

## 3. Validate configuration

~~~bash
docker compose config --quiet
docker compose -f devnet/docker-compose.yml config --quiet
git status --short
~~~

Both Compose commands must exit with code 0. Do not deploy with uncommitted local modifications unless
the change is intentional and documented.

## 4. Rust workspace gates

Run from the repository root:

~~~bash
docker compose run --rm wsc cargo fmt --all -- --check
docker compose run --rm wsc cargo test --workspace
docker compose run --rm wsc cargo test --workspace --release
docker compose run --rm wsc cargo clippy --workspace --all-targets --all-features -- -D warnings
docker compose run --rm wsc cargo build --workspace --release
~~~

These tests cover protocol encoding, Ed25519 signing, wallet encryption, MNA state transitions, storage
recovery, block import/finality, consensus votes, RPC login messages, and network framing.

## 5. Security and fuzz checks

The base development container does not preinstall audit tools:

~~~bash
docker compose run --rm wsc sh -lc \
  'cargo install cargo-audit cargo-deny --locked && \
   cargo generate-lockfile && \
   cargo audit && \
   cargo deny check'
~~~

The fuzz package is excluded from the normal workspace. On a dedicated test machine:

~~~bash
cargo install cargo-fuzz
cargo fuzz run core_decode -- -max_total_time=60
cargo fuzz run address_parse -- -max_total_time=60
cargo fuzz run keystore_json -- -max_total_time=60
~~~

Never run fuzzing against production keys or live node data.

## 6. TypeScript SDK check

~~~bash
cd sdk/typescript
npm install
npm run typecheck
cd ../..
~~~

Amounts are strings at the SDK/RPC boundary to preserve MNA and future wrapped-token precision.

## 7. Standalone image smoke test

~~~bash
docker build -t worldstreet-chain:dev .
docker run --rm worldstreet-chain:dev version
~~~

The version command must print the wsc version and MANNA (MNA).

## 8. Start the four-node devnet

The init service creates one shared genesis and four node directories. Its validator keys are deterministic
development-only keys for repeatability and must never be reused elsewhere.

~~~bash
docker compose --env-file devnet/.env -f devnet/docker-compose.yml -f devnet/docker-compose.vps.yml up -d --build
docker compose -f devnet/docker-compose.yml ps
docker compose -f devnet/docker-compose.yml logs --tail=100 init
docker compose -f devnet/docker-compose.yml logs --tail=100 node1
~~~

Expected services are init, node1, node2, node3, and node4. The init service should exit 0; the four node
services should remain running.

| Node | RPC host port | Peer diagnostic port |
|---|---:|---:|
| node1 | 26657 | 26656 |
| node2 | 26658 | 26757 |
| node3 | 26659 | 26758 |
| node4 | 26660 | 26759 |

Container-to-container peer traffic uses port 26656.

## 9. Health, convergence, metrics, and finality

~~~bash
for port in 26657 26658 26659 26660; do
  echo "RPC $port"
  curl --fail --silent "http://127.0.0.1:${port}/healthz" | jq .
done
~~~

Query every node:

~~~bash
for port in 26657 26658 26659 26660; do
  curl --fail --silent "http://127.0.0.1:${port}/rpc" \
    -H 'content-type: application/json' \
    -d '{"jsonrpc":"2.0","id":1,"method":"chain_info","params":{}}' \
    | jq --arg port "$port" '.result + {rpc_port: $port}'
done
~~~

All nodes should eventually report the same genesis_hash, latest_hash, and finalized_hash. The height
should increase as round-robin validators produce signed blocks.

~~~bash
for attempt in $(seq 1 30); do
  value=$(curl --fail --silent http://127.0.0.1:26657/rpc \
    -H 'content-type: application/json' \
    -d '{"jsonrpc":"2.0","id":1,"method":"chain_info","params":{}}' \
    | jq -r '.result.finalized_height')
  echo "finalized_height=$value"
  [ "$value" -ge 1 ] && break
  sleep 2
done
~~~

Inspect Prometheus-compatible metrics:

~~~bash
curl --fail --silent http://127.0.0.1:26657/metrics
~~~

Confirm wsc_latest_height, wsc_finalized_height, wsc_peer_connections_total,
wsc_blocks_imported_total, and wsc_votes_received_total are present.

## 10. Wallet and MNA faucet smoke test

Create a disposable test wallet without saving its mnemonic in shared logs:

~~~bash
docker compose -f devnet/docker-compose.yml exec -T node1 sh -lc \
  'WSC_WALLET_PASSWORD="dev-only-password" cargo run --package wsc -- \
   wallet create --keystore /tmp/alice.keystore.json --json'
~~~

Copy the returned address into ALICE_ADDRESS, then fund it:

~~~bash
export ALICE_ADDRESS='mna1-replace-with-the-created-address'
docker compose -f devnet/docker-compose.yml exec -T node1 cargo run --package wsc -- \
  node faucet --data-dir /app/devnet/data/node-1 \
  --address "$ALICE_ADDRESS" --amount 100000000
~~~

Query the balance through all RPC nodes:

~~~bash
for port in 26657 26658 26659 26660; do
  curl --fail --silent "http://127.0.0.1:${port}/rpc" \
    -H 'content-type: application/json' \
    -d "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"account_get\",\"params\":{\"address\":\"${ALICE_ADDRESS}\"}}" \
    | jq --arg port "$port" '.result + {rpc_port: $port}'
done
~~~

The balance should converge to 100000000 microMNA. RPC accepts only fully signed transactions; it never
accepts seed phrases or private keys.

## 11. Login challenge test

~~~bash
curl --fail --silent http://127.0.0.1:26657/rpc \
  -H 'content-type: application/json' \
  -d "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"auth_challenge\",\"params\":{\"address\":\"${ALICE_ADDRESS}\",\"domain\":\"vps-test.example\"}}" \
  | tee /tmp/wsc-login-challenge.json | jq .
~~~

Use the wallet's Ed25519 key to sign the exact returned message, then call auth_verify with address, domain,
nonce, public-key hex, and signature hex. Test valid signature, changed domain, changed address, changed
chain ID, expired challenge, reused nonce, and invalid signature.

## 12. Backup and restore

~~~bash
mkdir -p ~/wsc-backups
chmod +x scripts/backup-node.sh
scripts/backup-node.sh devnet/data/node-1 \
  "$HOME/wsc-backups/node-1-$(date -u +%Y%m%dT%H%M%SZ).tar.gz"
~~~

Restore only into a new directory:

~~~bash
mkdir -p /tmp/wsc-restore-test
tar -xzf ~/wsc-backups/<backup-file>.tar.gz -C /tmp/wsc-restore-test
find /tmp/wsc-restore-test -maxdepth 3 -type f | head
~~~

Backups must include configuration, genesis, and database. Keep validator secrets in a separate secret
management system.

## 13. Firewall and exposure

For a private devnet, allow SSH and peer traffic only from the validator network. Expose RPC only to trusted
application IPs:

~~~bash
sudo ufw allow OpenSSH
sudo ufw allow from <PRIVATE_VALIDATOR_CIDR> to any port 26656 proto tcp
sudo ufw allow from <PRIVATE_VALIDATOR_CIDR> to any port 26757 proto tcp
sudo ufw allow from <PRIVATE_VALIDATOR_CIDR> to any port 26758 proto tcp
sudo ufw allow from <PRIVATE_VALIDATOR_CIDR> to any port 26759 proto tcp
sudo ufw allow from <TRUSTED_APP_CIDR> to any port 26657 proto tcp
sudo ufw enable
sudo ufw status verbose
~~~

Do not expose all RPC, peer, or metrics ports publicly. The MVP peer transport does not yet provide
production-grade authenticated encryption or peer reputation controls.

## 14. Logs, shutdown, and upgrade

~~~bash
docker compose -f devnet/docker-compose.yml logs -f node1 node2 node3 node4
docker compose -f devnet/docker-compose.yml stop
~~~

To remove containers and volumes:

~~~bash
docker compose -f devnet/docker-compose.yml down -v
~~~

Upgrade procedure:

~~~bash
docker compose -f devnet/docker-compose.yml down
scripts/backup-node.sh devnet/data/node-1 \
  "$HOME/wsc-backups/pre-upgrade-node-1-$(date -u +%Y%m%dT%H%M%SZ).tar.gz"
git fetch origin
git checkout main
git pull --ff-only origin main
docker compose config --quiet
docker compose -f devnet/docker-compose.yml config --quiet
docker compose run --rm wsc cargo test --workspace
docker compose -f devnet/docker-compose.yml up -d --build
~~~

Do not overwrite an existing genesis or database with a newly generated devnet without confirming that the
chain data is disposable.

## 15. Troubleshooting

Docker daemon:

~~~bash
sudo systemctl status docker
sudo systemctl restart docker
docker version
~~~

If init says the devnet already exists, back it up first, then remove only disposable data:

~~~bash
docker compose -f devnet/docker-compose.yml down -v
rm -rf -- devnet/data
docker compose -f devnet/docker-compose.yml up -d --build
~~~

If heights do not converge:

~~~bash
docker compose -f devnet/docker-compose.yml logs --tail=200 node1 node2 node3 node4
curl -s http://127.0.0.1:26657/metrics
~~~

Check shared genesis, container DNS, firewall rules, and port conflicts. Never bypass chain/genesis mismatch
errors by copying databases between different networks.

When a test fails, keep the commit hash, full command, Docker version, Rust image output, and relevant logs.
Re-run from a clean checkout before changing deployment files.

## 16. Final release warning

This is an experimental MVP. Static validator membership, TCP peer transport, storage crash atomicity,
wrapped-asset bridge logic, production key custody, and independent security review remain blockers for any
real-value deployment.

