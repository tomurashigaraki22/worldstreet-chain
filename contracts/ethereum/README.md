# Ethereum reserve contracts

`IntertrainWethBridge.sol` handles the existing testnet WETH/ETH lane.
`IntertrainUsdcReserve.sol` is a separate USDC escrow for reserve accounting.
It emits finalized deposit/release events but never mints MNA itself. The
Intertrain relayer submits those events to the consensus-visible reserve ledger.

Deploy only to Sepolia/devnet until audited. The Circle Sepolia USDC token is
`0x1c7D4B196Cb0C7B01d743Fbc6116a902379C7238`.
