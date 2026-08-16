# Legacy WSOL custody fallback

The active devnet wSOL lane now uses the hardened native Rust program documented in `contracts/solana/wsol-bridge-program/README.md` and configured by `ops/deploy-wsol-solana-program.sh`. This document describes the retained custody fallback only. It is not the active VPS mode.

To deliberately switch a separate devnet back to custody mode, run as root:

```bash
bash /root/worldstreet-chain/ops/setup-wsol-vault.sh
systemctl restart worldstreet-relayer.service
```

Custody mode uses a normal root-owned vault and memo deposits. It is inexpensive but trusted: the VPS key controls the SOL reserve. Never use this fallback with mainnet funds.
