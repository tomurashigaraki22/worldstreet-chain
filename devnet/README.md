# Four-node Worldstreet Chain devnet

This Compose project creates four local validator nodes from one deterministic genesis. The validator
secrets are intentionally fixed for repeatable development only and must never be reused for a public
testnet or any network holding value.

From the repository root:

```bash
docker compose -f devnet/docker-compose.yml up --build
```

RPC endpoints are exposed on `localhost:26657` through `localhost:26660`. Peer transport uses container
port `26656`. Query health and metrics with:

```bash
curl http://127.0.0.1:26657/healthz
curl http://127.0.0.1:26657/metrics
```

The init service writes generated data under `devnet/data`. Reset the devnet with:

```bash
docker compose -f devnet/docker-compose.yml down -v
Remove-Item -Recurse -Force devnet/data
```

The node processes currently use a static validator set, deterministic round-robin proposer selection, and
two-thirds signed-vote finality. Dynamic validator sets, slashing, encrypted peer transport, and production
key custody are not included. This devnet is for protocol and integration testing, not real funds.
