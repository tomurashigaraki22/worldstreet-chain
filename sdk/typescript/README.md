# Worldstreet Chain TypeScript SDK

This package is intentionally crypto-library agnostic. Wallet implementations should sign the exact login message or transaction signing bytes, then pass the resulting byte arrays/hex values to the SDK.

```ts
import { RpcClient, WorldstreetClient } from "@worldstreet/wsc-sdk";

const client = new WorldstreetClient(new RpcClient("http://127.0.0.1:26657/rpc"));
const chain = await client.chainInfo();
const account = await client.account("mna1...");
```

Amounts are represented as strings at API boundaries so MNA and future wrapped assets can be handled without JavaScript number precision loss.
