# Intertrain wSOL bridge program

This is the deployed devnet-only native Rust program for Intertrain wSOL. It has no Anchor dependency. The program binds state to the authority and vault PDAs, records each deposit/release ID in a replay PDA, checks account ownership and amounts, and emits structured logs consumed by the VPS relayer.

The deployment is experimental and not independently audited. It must not be used with mainnet assets. Production requirements include multisig/HSM authority, pause/rate limits, reserve accounting, key rotation, monitoring, and an independent audit.
