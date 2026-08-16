import { ed25519 } from "@noble/curves/ed25519.js";
import { hmac } from "@noble/hashes/hmac.js";
import { sha256 } from "@noble/hashes/sha2.js";
import { sha512 } from "@noble/hashes/sha2.js";
import { bech32m } from "@scure/base";
import { generateMnemonic, mnemonicToSeedSync } from "@scure/bip39";
import { wordlist } from "@scure/bip39/wordlists/english";

export const CHAIN_ID = "worldstreet-devnet-1";
export const MNA_DECIMALS = 6;
export const USDC_DECIMALS = 6;
export const SOL_DECIMALS = 9;
export const MNA_ASSET = "worldstreet:MNA:native";
export const ETH_SEPOLIA_USDC = "0x1c7D4B196Cb0C7B01d743Fbc6116a902379C7238";
export const ETH_USDC_RESERVE = "0xab4056bCb0369897d6D5Ca1A13f670f76C75ef3e";
export const SOLANA_USDC_MINT = "4zMMC9srt5Ri5X14GAgXhaHii3GnPAEERYPJgZJDncDU";
export const SOLANA_PROGRAM = "FyAuUc2pPkz1nt2vR27R6NfE3Lgb4Z69sjUoPSeU7PCw";

export type SwapKind = "MintMna" | "RedeemMna";
export type JsonRpcResult<T> = { jsonrpc: "2.0"; id: string | number; result?: T; error?: { code: number; message: string } };

export class RpcError extends Error {
  constructor(public readonly code: number, message: string) { super(message); this.name = "RpcError"; }
}

export interface RpcClientOptions { rpcUrl: string; chainId?: string; fetchImpl?: typeof fetch; }

export class IntertrainRpc {
  readonly chainId: string;
  private readonly fetchImpl: typeof fetch;
  constructor(private readonly options: RpcClientOptions) {
    this.chainId = options.chainId ?? CHAIN_ID;
    this.fetchImpl = options.fetchImpl ?? fetch;
  }
  async call<T>(method: string, params: unknown = {}): Promise<T> {
    const response = await this.fetchImpl(this.options.rpcUrl, {
      method: "POST", headers: { "content-type": "application/json" },
      body: JSON.stringify({ jsonrpc: "2.0", id: crypto.randomUUID(), method, params })
    });
    const body = await response.json() as JsonRpcResult<T>;
    if (body.error) throw new RpcError(body.error.code, body.error.message);
    if (body.result === undefined) throw new RpcError(-32000, `RPC returned no result for ${method}`);
    return body.result;
  }
  chainInfo<T = unknown>() { return this.call<T>("chain_info"); }
  assets<T = unknown>() { return this.call<T>("asset_list"); }
  bridges<T = unknown>() { return this.call<T>("bridge_status"); }
  reserveStatus<T = unknown>() { return this.call<T>("mna_reserve_status"); }
  account<T = AccountResult>(address: string) { return this.call<T>("account_get", { address }); }
  quote<T = MnaQuote>(amountUsdc: bigint | string) { return this.call<T>("mna_quote", { amount_usdc: String(amountUsdc) }); }
  transactionStatus<T = unknown>(hash: string) { return this.call<T>("transaction_status", { hash }); }
  swapStatus<T = unknown>(hash: string) { return this.call<T>("mna_swap_status", { hash }); }
  tokenStatus<T = unknown>(hash: string) { return this.call<T>("token_operation_status", { hash }); }
  faucet<T = unknown>(address: string, amountMna: bigint | string = 100_000_000n) { return this.call<T>("devnet_faucet", { address, amount: String(amountMna) }); }
  async waitFor<T>(lookup: () => Promise<T & { status?: string }>, options: { timeoutMs?: number; intervalMs?: number } = {}): Promise<T> {
    const timeout = options.timeoutMs ?? 120_000, interval = options.intervalMs ?? 2_000, started = Date.now();
    while (Date.now() - started < timeout) {
      const result = await lookup();
      if (result.status === "confirmed" || result.status === "failed" || result.status === "reverted") return result;
      await new Promise(resolve => setTimeout(resolve, interval));
    }
    throw new Error("Timed out waiting for Intertrain finality");
  }
}

export interface AccountResult { address: string; asset: "MNA"; balance: string; nonce: number; assets: Record<string, string>; }
export interface MnaQuote { amount_usdc: string; amount_mna: string; usdc_per_mna: string; mna_per_usdc: string; price_usdc: string; decimals: number; }

function bytesToHex(bytes: Uint8Array): string { return [...bytes].map(value => value.toString(16).padStart(2, "0")).join(""); }
function hexToBytes(value: string): Uint8Array { const clean = value.replace(/^0x/, ""); if (clean.length % 2) throw new Error("hex string has odd length"); return Uint8Array.from(clean.match(/../g) ?? [], pair => Number.parseInt(pair, 16)); }
function concat(...parts: Uint8Array[]): Uint8Array { const result = new Uint8Array(parts.reduce((size, part) => size + part.length, 0)); let offset = 0; for (const part of parts) { result.set(part, offset); offset += part.length; } return result; }
function utf8(value: string): Uint8Array { return new TextEncoder().encode(value); }
function varint(value: bigint | number): Uint8Array { let n = BigInt(value), result: number[] = []; do { let byte = Number(n & 0x7fn); n >>= 7n; result.push(n === 0n ? byte : byte | 0x80); } while (n !== 0n); return Uint8Array.from(result); }
function postcardString(value: string): Uint8Array { const data = utf8(value); return concat(varint(data.length), data); }
function bytes32(value: Uint8Array): Uint8Array { if (value.length !== 32) throw new Error("expected 32 bytes"); return value; }

export function parseUnits(value: string, decimals: number): bigint {
  const normalized = value.trim();
  if (!/^\d+(\.\d+)?$/.test(normalized)) throw new Error("Amount must be a positive decimal");
  const [whole, fraction = ""] = normalized.split(".");
  if (fraction.length > decimals) throw new Error(`Amount has more than ${decimals} decimals`);
  return BigInt(whole) * 10n ** BigInt(decimals) + BigInt((fraction + "0".repeat(decimals)).slice(0, decimals) || "0");
}
export function formatUnits(value: bigint | string, decimals: number, maxFraction = decimals): string {
  const n = BigInt(value), base = 10n ** BigInt(decimals), whole = n / base, fraction = (n % base).toString().padStart(decimals, "0").slice(0, maxFraction).replace(/0+$/, "");
  return fraction ? `${whole}.${fraction}` : whole.toString();
}

function deriveEd25519Secret(mnemonic: string, path: number[]): Uint8Array {
  const seed = mnemonicToSeedSync(mnemonic, ""); let key = hmac(sha512, utf8("ed25519 seed"), seed).slice(0, 32); let chain = hmac(sha512, utf8("ed25519 seed"), seed).slice(32);
  for (const component of path) { const index = component + 0x80000000; const input = concat(new Uint8Array([0]), key, new Uint8Array([index >>> 24, index >>> 16 & 255, index >>> 8 & 255, index & 255])); const next = hmac(sha512, chain, input); key = next.slice(0, 32); chain = next.slice(32); }
  return key;
}
export function intertrainAddress(publicKey: Uint8Array): string {
  const digest = sha256(concat(utf8("MNA/address/v1"), publicKey)); return bech32m.encode("mna", bech32m.toWords(concat(new Uint8Array([1]), digest.slice(0, 20))), 90);
}
export function decodeIntertrainAddress(address: string): Uint8Array {
  const decoded = bech32m.decode(address as `${string}1${string}`, 90); if (decoded.prefix !== "mna") throw new Error("Invalid Intertrain address prefix"); const data = bech32m.fromWords(decoded.words); if (data.length !== 21 || data[0] !== 1) throw new Error("Invalid Intertrain address payload"); return data;
}

function assetFields(canonical: string, decimals: number): { namespace: string; symbol: string; contract: string | null; decimals: number } {
  const first = canonical.indexOf(":"), second = canonical.indexOf(":", first + 1); if (first < 1 || second < 0) throw new Error("Invalid canonical asset ID");
  return { namespace: canonical.slice(0, first), symbol: canonical.slice(first + 1, second), contract: canonical.slice(second + 1) === "native" ? null : canonical.slice(second + 1), decimals };
}

function encodeTransfer(value: { version: number; chain_id: string; nonce: number; from: string; to: string; amount: bigint; fee: bigint; public_key: Uint8Array; memo: string }): Uint8Array {
  return concat(new Uint8Array([value.version]), postcardString(value.chain_id), varint(value.nonce), decodeIntertrainAddress(value.from), decodeIntertrainAddress(value.to), varint(value.amount), varint(value.fee), value.public_key, postcardString(value.memo));
}
function encodeSwap(value: { version: number; chain_id: string; nonce: number; from: string; kind: SwapKind; collateral_asset: string; amount_usdc: bigint; amount_mna: bigint; fee: bigint; public_key: Uint8Array; memo: string }): Uint8Array {
  const asset = assetFields(value.collateral_asset, USDC_DECIMALS);
  return concat(new Uint8Array([value.version]), postcardString(value.chain_id), varint(value.nonce), decodeIntertrainAddress(value.from), new Uint8Array([value.kind === "MintMna" ? 0 : 1]), postcardString(asset.namespace), postcardString(asset.symbol), asset.contract === null ? new Uint8Array([0]) : concat(new Uint8Array([1]), postcardString(asset.contract)), new Uint8Array([asset.decimals]), varint(value.amount_usdc), varint(value.amount_mna), varint(value.fee), value.public_key, postcardString(value.memo));
}

export interface TransferInput { to: string; amountMna: string | bigint; feeMna?: string | bigint; memo?: string; }
export interface SwapInput { kind: SwapKind; collateralAsset: string; amountUsdc: string | bigint; feeMna?: string | bigint; memo?: string; }

export class LocalIntertrainWallet {
  private readonly secretKey: Uint8Array;
  readonly publicKey: Uint8Array;
  readonly address: string;
  private constructor(secret: Uint8Array) { this.secretKey = secret; this.publicKey = ed25519.getPublicKey(secret); this.address = intertrainAddress(this.publicKey); }
  static create(): { wallet: LocalIntertrainWallet; mnemonic: string } { const mnemonic = generateMnemonic(wordlist, 256); return { mnemonic, wallet: LocalIntertrainWallet.fromMnemonic(mnemonic) }; }
  static fromMnemonic(mnemonic: string): LocalIntertrainWallet { if (!mnemonic.trim()) throw new Error("Mnemonic is required"); return new LocalIntertrainWallet(deriveEd25519Secret(mnemonic, [44, 9999, 0, 0, 0])); }
  sign(message: Uint8Array): string { return bytesToHex(ed25519.sign(message, this.secretKey)); }
  async transfer(rpc: IntertrainRpc, input: TransferInput) {
    const account = await rpc.account(this.address), unsigned = { version: 1, chain_id: rpc.chainId, nonce: account.nonce, from: this.address, to: input.to, amount: BigInt(input.amountMna), fee: BigInt(input.feeMna ?? 1), public_key: this.publicKey, memo: input.memo ?? "" };
    const signature = this.sign(encodeTransfer(unsigned));
    return rpc.call<{ hash: string; status: string }>("transaction_broadcast", { transaction: { unsigned: { ...unsigned, amount: unsigned.amount.toString(), fee: unsigned.fee.toString(), public_key: bytesToHex(unsigned.public_key) }, signature } });
  }
  async swapMna(rpc: IntertrainRpc, input: SwapInput) {
    const account = await rpc.account(this.address), amountUsdc = BigInt(input.amountUsdc), amountMna = amountUsdc / 2n;
    if (amountUsdc <= 0n || amountUsdc % 2n !== 0n) throw new Error("USDC amount must convert exactly at 2:1");
    const unsigned = { version: 1, chain_id: rpc.chainId, nonce: account.nonce, from: this.address, kind: input.kind, collateral_asset: input.collateralAsset, amount_usdc: amountUsdc, amount_mna: amountMna, fee: BigInt(input.feeMna ?? 1), public_key: this.publicKey, memo: input.memo ?? "" };
    const prepared = await rpc.call<{ operation_id: string; signing_bytes: string }>("mna_swap_prepare", { unsigned: { ...unsigned, amount_usdc: amountUsdc.toString(), amount_mna: amountMna.toString(), fee: unsigned.fee.toString(), public_key: bytesToHex(this.publicKey) } });
    return rpc.call<{ operation_id: string; status: string }>("mna_swap_broadcast", { operation: { unsigned: { ...unsigned, amount_usdc: amountUsdc.toString(), amount_mna: amountMna.toString(), fee: unsigned.fee.toString(), public_key: bytesToHex(this.publicKey) }, signature: this.sign(hexToBytes(prepared.signing_bytes)) } });
  }
  async authenticate(rpc: IntertrainRpc, domain: string) {
    const challenge = await rpc.call<{ nonce: string; message: string }>("auth_challenge", { address: this.address, domain });
    return rpc.call<{ session_token: string }>("auth_verify", { address: this.address, domain, nonce: challenge.nonce, public_key: bytesToHex(this.publicKey), signature: this.sign(utf8(challenge.message)) });
  }
}

export interface EncryptedVaultRecord { version: 1; salt: string; iv: string; ciphertext: string; }
export interface VaultWallet { id: string; name: string; mnemonic: string; created_at: string; }
function base64(bytes: Uint8Array): string { return btoa(String.fromCharCode(...bytes)); }
function fromBase64(value: string): Uint8Array { return Uint8Array.from(atob(value), char => char.charCodeAt(0)); }
function arrayBuffer(value: Uint8Array): ArrayBuffer { return (value.buffer as ArrayBuffer).slice(value.byteOffset, value.byteOffset + value.byteLength); }
async function vaultKey(password: string, salt: Uint8Array, usage: KeyUsage[]): Promise<CryptoKey> { const base = await crypto.subtle.importKey("raw", arrayBuffer(utf8(password)), "PBKDF2", false, ["deriveKey"]); return crypto.subtle.deriveKey({ name: "PBKDF2", salt: arrayBuffer(salt), iterations: 210000, hash: "SHA-256" }, base, { name: "AES-GCM", length: 256 }, false, usage); }

export class BrowserVault {
  private constructor(private readonly password: string, private data: { version: 1; wallets: VaultWallet[] }) {}
  static async create(password: string): Promise<BrowserVault> { if (password.length < 8) throw new Error("Vault password must be at least 8 characters"); return new BrowserVault(password, { version: 1, wallets: [] }); }
  static async unlock(record: EncryptedVaultRecord, password: string): Promise<BrowserVault> { const key = await vaultKey(password, fromBase64(record.salt), ["decrypt"]); const plaintext = await crypto.subtle.decrypt({ name: "AES-GCM", iv: arrayBuffer(fromBase64(record.iv)) }, key, arrayBuffer(fromBase64(record.ciphertext))); return new BrowserVault(password, JSON.parse(new TextDecoder().decode(plaintext))); }
  async export(): Promise<EncryptedVaultRecord> { const salt = crypto.getRandomValues(new Uint8Array(16)), iv = crypto.getRandomValues(new Uint8Array(12)), key = await vaultKey(this.password, salt, ["encrypt"]); const ciphertext = new Uint8Array(await crypto.subtle.encrypt({ name: "AES-GCM", iv: arrayBuffer(iv) }, key, arrayBuffer(new TextEncoder().encode(JSON.stringify(this.data))))); return { version: 1, salt: base64(salt), iv: base64(iv), ciphertext: base64(ciphertext) }; }
  list(): ReadonlyArray<Omit<VaultWallet, "mnemonic">> { return this.data.wallets.map(({ mnemonic, ...safe }) => safe); }
  createWallet(name: string): VaultWallet { const { mnemonic } = LocalIntertrainWallet.create(); const wallet = { id: crypto.randomUUID(), name, mnemonic, created_at: new Date().toISOString() }; this.data.wallets.push(wallet); return wallet; }
  restoreWallet(name: string, mnemonic: string): VaultWallet { const wallet = { id: crypto.randomUUID(), name, mnemonic, created_at: new Date().toISOString() }; LocalIntertrainWallet.fromMnemonic(mnemonic); this.data.wallets.push(wallet); return wallet; }
  revealMnemonic(id: string): string { const wallet = this.data.wallets.find(item => item.id === id); if (!wallet) throw new Error("Wallet not found"); return wallet.mnemonic; }
}

export interface Eip1193Provider { request(args: { method: string; params?: unknown[] }): Promise<any>; }
function word(value: bigint | number): string { return BigInt(value).toString(16).padStart(64, "0"); }
function evmAddress(value: string): string { const clean = value.replace(/^0x/, ""); if (!/^[0-9a-fA-F]{40}$/.test(clean)) throw new Error("Invalid EVM address"); return clean.toLowerCase().padStart(64, "0"); }
async function waitForEvmReceipt(provider: Eip1193Provider, hash: string, timeoutMs = 120_000): Promise<any> { const started = Date.now(); while (Date.now() - started < timeoutMs) { const receipt = await provider.request({ method: "eth_getTransactionReceipt", params: [hash] }); if (receipt) { if (receipt.status !== "0x1") throw new Error(`Ethereum transaction reverted: ${hash}`); return receipt; } await new Promise(resolve => setTimeout(resolve, 2_000)); } throw new Error(`Timed out waiting for Ethereum receipt: ${hash}`); }

export class EthereumSepoliaUsdc {
  constructor(public readonly token = ETH_SEPOLIA_USDC, public readonly reserve = ETH_USDC_RESERVE) {}
  encodeApprove(amount: bigint): string { return "0x095ea7b3" + evmAddress(this.reserve) + word(amount); }
  encodeDeposit(depositId: Uint8Array, amount: bigint, destination: string): string { const body = utf8(destination), padded = Math.ceil(body.length / 32) * 32; return "0xf3cc5389" + bytesToHex(bytes32(depositId)) + word(amount) + word(96) + word(body.length) + bytesToHex(concat(body, new Uint8Array(padded - body.length))); }
  async deposit(provider: Eip1193Provider, amount: bigint, destination: string): Promise<{ approvalHash: string; depositHash: string }> { const accounts = await provider.request({ method: "eth_requestAccounts" }) as string[]; const chain = await provider.request({ method: "eth_chainId" }); if (chain.toLowerCase() !== "0xaa36a7") throw new Error("Switch wallet to Ethereum Sepolia"); const from = accounts[0]; const approvalHash = await provider.request({ method: "eth_sendTransaction", params: [{ from, to: this.token, data: this.encodeApprove(amount) }] }); await waitForEvmReceipt(provider, approvalHash); const depositHash = await provider.request({ method: "eth_sendTransaction", params: [{ from, to: this.reserve, data: this.encodeDeposit(crypto.getRandomValues(new Uint8Array(32)), amount, destination) }] }); return { approvalHash, depositHash }; }
}

function le64(value: bigint): Uint8Array { const result = new Uint8Array(8); let n = value; for (let index = 0; index < 8; index++) { result[index] = Number(n & 255n); n >>= 8n; } return result; }
export function nativeSolDepositData(amountLamports: bigint, depositId: Uint8Array, destination: string): Uint8Array { const body = utf8(destination); return concat(new Uint8Array([1]), le64(amountLamports), bytes32(depositId), new Uint8Array([body.length & 255, body.length >>> 8]), body); }
/** Build the tag-5 direct SOL collateral instruction. It is deliberately
 * separate from tag 1 so a relayer cannot double-count a deposit as wSOL and MNA. */
export function nativeSolMnaDepositData(amountLamports: bigint, depositId: Uint8Array, destination: string): Uint8Array { const body = utf8(destination); return concat(new Uint8Array([5]), le64(amountLamports), bytes32(depositId), new Uint8Array([body.length & 255, body.length >>> 8]), body); }
export function splUsdcDepositData(amountMicroUsdc: bigint, depositId: Uint8Array, destination: string): Uint8Array { const body = utf8(destination); return concat(new Uint8Array([3]), le64(amountMicroUsdc), bytes32(depositId), new Uint8Array([body.length & 255, body.length >>> 8]), body); }
export const solanaPdaSeeds = { state: "intertrain-wsol-state", vault: "intertrain-wsol-vault", nativeReplay: "deposit", splReplay: "spl-deposit" } as const;
