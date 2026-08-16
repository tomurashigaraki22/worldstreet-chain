# Intertrain wSOL bridge (Solana devnet)

The active devnet path is the hardened native-Rust program mode. It uses Solana canonical WSOL mint `So11111111111111111111111111111111111111112`, an authority-bound PDA state account, a program-owned PDA vault, one replay PDA per deposit/release, and the VPS relayer. It is devnet-only and has not been independently audited.

Current deployment:

- Program: `FyAuUc2pPkz1nt2vR27R6NfE3Lgb4Z69sjUoPSeU7PCw`
- Authority/deployer: `EQQvAukwEiwiXShm93HTSLC2vTftMnPzeRxUD4LGd6rv`
- State PDA: `76UrihHJQAH88E6JQXMUFDkXM3PhL1Vf56od4XUdV29t`
- Vault PDA: `829dNAZ1DfpsQKxaw9gqcuxfMRyFsCg6chtKCajQeaGe`
- RPC: `https://api.devnet.solana.com`

`contracts/solana/wsol-bridge-client` builds `intertrain-wsol-client` with `init`, `lock`, `release`, and `pdas` commands. `ops/wsc-relayer.py` watches finalized vault transactions, validates program log records, calls Intertrain idempotent minting, and submits program release instructions for Intertrain burns.

The vault is funded with a 1 SOL devnet reserve. Do not send mainnet SOL or real user funds to these devnet addresses. Before production, add multisig/HSM authority, rate limits, pause controls, reserve accounting, independent audit, and a separately managed production deployment.

## Token scope

This deployment handles native SOL lock/release plus the allowlisted Circle devnet USDC SPL lane. The canonical WSOL mint remains metadata identity for the Intertrain asset; native SOL is the wSOL deposit endpoint. The program does not handle Token-2022 or arbitrary SPL mints, and it does not create or control any external mint.

## Solana devnet USDC (SPL Token)

The fresh combined deployment also accepts Circle's official Solana devnet USDC mint only:

- Mint: `4zMMC9srt5Ri5X14GAgXhaHii3GnPAEERYPJgZJDncDU`
- Vault token account: `H9q1t5qw2gbwD9RoUSJp4wNiqWMAyr4zmwz53NmM9bG8`
- Asset ID: `solana:USDC:devnet:4zMMC9srt5Ri5X14GAgXhaHii3GnPAEERYPJgZJDncDU`

Use `lock-spl` and `release-spl` from the client. This is devnet-only, has zero USDC liquidity until a devnet faucet deposit is made, and is not a Token-2022 or arbitrary-SPL bridge.
