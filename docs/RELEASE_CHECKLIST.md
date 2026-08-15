# Internal devnet release checklist

## Protocol and code

- [ ] Confirm chain ID, address HRP, MANNA/MNA decimals, fee minimum, and genesis hash.
- [ ] Review all genesis validators and replace deterministic devnet keys.
- [ ] Run `cargo fmt --all -- --check`.
- [ ] Run workspace Clippy with `-D warnings`.
- [ ] Run debug and release workspace tests.
- [ ] Run `cargo audit` and `cargo deny check`.
- [ ] Run the fuzz targets for a bounded time in CI.

## Devnet

- [ ] Start the four-node Compose devnet from a clean directory.
- [ ] Verify all `/healthz` endpoints and compare `/metrics` across nodes.
- [ ] Verify block height, state roots, and finalized hashes converge.
- [ ] Create an MNA wallet, use the faucet, broadcast a signed transfer, and verify the recipient balance.
- [ ] Exercise login challenge success, expiry, wrong domain, wrong address, invalid signature, and replay.
- [ ] Restart one node and verify storage recovery and peer synchronization.
- [ ] Restore a node from a backup and verify its genesis/latest roots.

## Security and operations

- [ ] Restrict ports 26656/26657 and admin access with the VPS firewall.
- [ ] Do not commit `devnet/data`, `validators.env`, keystores, or production secrets.
- [ ] Store production validator keys in an external secret manager or HSM.
- [ ] Configure log rotation and metrics retention.
- [ ] Publish experimental-network warnings and incident contacts.
- [ ] Explicitly reject real-value deployment until an independent security review is complete.
