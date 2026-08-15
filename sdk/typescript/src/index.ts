export interface AssetId {
  namespace: string;
  symbol: string;
  contract: string | null;
  decimals: number;
}

export interface AccountResponse {
  address: string;
  asset: "MNA";
  balance: string;
  nonce: number;
}

export interface ChainInfo {
  chain_id: string;
  native_asset: { name: "MANNA"; symbol: "MNA"; decimals: 6 };
  genesis_hash: string;
  latest_height: number;
  latest_hash: string;
  finalized_height: number;
  finalized_hash: string;
}

export interface Transaction {
  unsigned: {
    version: number;
    chain_id: string;
    nonce: number;
    from: string;
    to: string;
    amount: string;
    fee: string;
    public_key: string;
    memo: string;
  };
  signature: string;
}

export interface BlockResponse {
  hash: string;
  height: number;
  chain_id: string;
  parent_hash: string;
  timestamp: number;
  transaction_root: string;
  state_root: string;
  proposer: string | null;
  proposer_signature: string | null;
  transactions: Transaction[];
}

export interface LoginChallenge {
  nonce: string;
  message: string;
  issued_at: number;
  expires_at: number;
}

export interface LoginVerification {
  authenticated: boolean;
  address: string;
  session_token: string;
}

export interface TransferInput {
  chainId?: string;
  nonce: number;
  from: string;
  to: string;
  amount: string | number;
  fee: string | number;
  publicKeyHex: string;
  memo?: string;
}

export interface RpcClientOptions {
  url: string;
  fetch?: typeof globalThis.fetch;
  headers?: Record<string, string>;
}

interface RpcResponse<T> {
  jsonrpc: "2.0";
  id: number;
  result?: T;
  error?: { code: number; message: string };
}

export class RpcError extends Error {
  readonly code: number;

  constructor(code: number, message: string) {
    super(message);
    this.name = "RpcError";
    this.code = code;
  }
}

export class RpcClient {
  private readonly endpoint: string;
  private readonly requestFetch: typeof globalThis.fetch;
  private readonly headers: Record<string, string>;
  private nextId = 1;

  constructor(options: RpcClientOptions | string) {
    const normalized = typeof options === "string" ? { url: options } : options;
    this.endpoint = normalized.url;
    this.requestFetch = normalized.fetch ?? globalThis.fetch.bind(globalThis);
    this.headers = { "content-type": "application/json", ...normalized.headers };
  }

  async call<T>(method: string, params: unknown = {}): Promise<T> {
    const response = await this.requestFetch(this.endpoint, {
      method: "POST",
      headers: this.headers,
      body: JSON.stringify({ jsonrpc: "2.0", id: this.nextId++, method, params })
    });
    if (!response.ok) {
      throw new Error(`Worldstreet RPC HTTP ${response.status}`);
    }
    const payload = (await response.json()) as RpcResponse<T>;
    if (payload.error) {
      throw new RpcError(payload.error.code, payload.error.message);
    }
    return payload.result as T;
  }
}

export class WorldstreetClient {
  constructor(readonly rpc: RpcClient) {}

  chainInfo(): Promise<ChainInfo> {
    return this.rpc.call("chain_info");
  }

  latestBlock(): Promise<BlockResponse> {
    return this.rpc.call("block_latest");
  }

  block(hash: string): Promise<BlockResponse> {
    return this.rpc.call("block_get", { hash });
  }

  blockAtHeight(height: number): Promise<BlockResponse> {
    return this.rpc.call("block_get_by_height", { height });
  }

  account(address: string): Promise<AccountResponse> {
    return this.rpc.call("account_get", { address });
  }

  broadcast(transaction: Transaction): Promise<{ hash: string; status: "accepted" }> {
    return this.rpc.call("transaction_broadcast", { transaction });
  }

  transaction(hash: string): Promise<{ hash: string; transaction: Transaction }> {
    return this.rpc.call("transaction_get", { hash });
  }

  createLoginChallenge(address: string, domain: string): Promise<LoginChallenge> {
    return this.rpc.call("auth_challenge", { address, domain });
  }

  verifyLogin(input: {
    address: string;
    domain: string;
    nonce: string;
    publicKeyHex: string;
    signatureHex: string;
  }): Promise<LoginVerification> {
    return this.rpc.call("auth_verify", {
      address: input.address,
      domain: input.domain,
      nonce: input.nonce,
      public_key: input.publicKeyHex,
      signature: input.signatureHex
    });
  }
}

export function buildUnsignedTransfer(input: TransferInput): Transaction["unsigned"] {
  return {
    version: 1,
    chain_id: input.chainId ?? "worldstreet-devnet-1",
    nonce: input.nonce,
    from: input.from,
    to: input.to,
    amount: String(input.amount),
    fee: String(input.fee),
    public_key: input.publicKeyHex,
    memo: input.memo ?? ""
  };
}

export function formatLoginMessage(input: {
  domain: string;
  chainId?: string;
  address: string;
  nonce: string;
  issuedAt: number;
  expiresAt: number;
}): string {
  return [
    "Worldstreet Chain Login",
    "",
    `Domain: ${input.domain}`,
    `Chain ID: ${input.chainId ?? "worldstreet-devnet-1"}`,
    `Address: ${input.address}`,
    `Nonce: ${input.nonce}`,
    `Issued At: ${input.issuedAt}`,
    `Expires At: ${input.expiresAt}`
  ].join("\n");
}
