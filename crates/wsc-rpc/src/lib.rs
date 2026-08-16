use axum::{
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use getrandom::fill;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, HashMap},
    net::SocketAddr,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex,
    },
    time::{Instant, SystemTime, UNIX_EPOCH},
};
use thiserror::Error;
use tokio::net::TcpListener;
use wsc_bridge::{
    EthereumBridgeConfig, EthereumRpcClient, EthereumUsdcBridgeConfig, SolanaBridgeConfig,
    SolanaRpcClient,
};
use wsc_core::{
    Address, AssetId, AssetOperation, AssetOperationKind, Block, Hash, MnaReserveOperation,
    MnaReserveOperationKind, MnaSwapKind, MnaSwapOperation, ProgramOperation, ProgramOperationKind,
    PublicKey, Signature, TokenDefinition, TokenOperation, TokenOperationKind, Transaction,
    UnsignedMnaSwapOperation, UnsignedTokenOperation, UnsignedTransaction, CHAIN_ID,
    MNA_USDC_DENOMINATOR,
};
use wsc_crypto::{
    address_from_public_key, block_header_id, mna_reserve_operation_id, mna_swap_operation_id,
    token_id_from_operation, token_operation_id, transaction_id, KeyPair,
};
use wsc_network::{NetworkConfig, NetworkMetrics};
use wsc_node::{Node, NodeError};
use wsc_program::{compile_rust_source, ProgramPackage};

const LOGIN_TTL_SECONDS: u64 = 300;
const MAX_PENDING_CHALLENGES: usize = 10_000;
const MAX_RPC_REQUESTS_PER_SECOND: u64 = 1_000;

#[derive(Clone)]
pub struct RpcContext {
    pub node: Arc<Mutex<Node>>,
    challenges: Arc<Mutex<HashMap<String, LoginChallenge>>>,
    programs: Arc<Mutex<ProgramRegistry>>,
    pub metrics: Arc<Metrics>,
    pub network_metrics: Arc<NetworkMetrics>,
}

impl RpcContext {
    pub fn new(node: Node) -> Arc<Self> {
        Arc::new(Self {
            node: Arc::new(Mutex::new(node)),
            challenges: Arc::new(Mutex::new(HashMap::new())),
            programs: Arc::new(Mutex::new(ProgramRegistry::load_default())),
            metrics: Arc::new(Metrics::default()),
            network_metrics: Arc::new(NetworkMetrics::default()),
        })
    }
}

pub struct Metrics {
    pub rpc_requests: AtomicU64,
    pub rpc_errors: AtomicU64,
    pub transactions_broadcast: AtomicU64,
    pub blocks_produced: AtomicU64,
    started_at: Instant,
    rate_window: Mutex<(Instant, u64)>,
}

impl Default for Metrics {
    fn default() -> Self {
        Self {
            rpc_requests: AtomicU64::new(0),
            rpc_errors: AtomicU64::new(0),
            transactions_broadcast: AtomicU64::new(0),
            blocks_produced: AtomicU64::new(0),
            started_at: Instant::now(),
            rate_window: Mutex::new((Instant::now(), 0)),
        }
    }
}

impl Metrics {
    fn allow_request(&self) -> bool {
        let Ok(mut window) = self.rate_window.lock() else {
            return false;
        };
        if window.0.elapsed() >= std::time::Duration::from_secs(1) {
            *window = (Instant::now(), 0);
        }
        if window.1 >= MAX_RPC_REQUESTS_PER_SECOND {
            return false;
        }
        window.1 += 1;
        true
    }
}

#[derive(Clone, Default)]
struct ProgramRegistry {
    uploads: BTreeMap<String, ProgramPackage>,
}

impl ProgramRegistry {
    fn load_default() -> Self {
        Self::default()
    }
}

#[derive(Debug, Deserialize)]
struct ProgramPackageParams {
    package_base64: String,
}

#[derive(Debug, Deserialize)]
struct ItBuildParams {
    language: String,
    source: String,
    #[serde(default = "default_program_name")]
    name: String,
}

#[derive(Debug, Deserialize)]
struct ProgramDeployParams {
    package_base64: String,
    public_key: String,
    signature: String,
    nonce: u64,
    fee: u128,
}

#[derive(Debug, Deserialize)]
struct ProgramCallParams {
    program_id: String,
    #[serde(default = "default_gas_limit")]
    gas_limit: u64,
    public_key: String,
    signature: String,
    nonce: u64,
    fee: u128,
}

#[derive(Debug, Deserialize)]
struct ProgramStorageGetParams {
    program_id: String,
    key: String,
}

#[derive(Debug, Deserialize)]
struct ProgramStorageSetParams {
    program_id: String,
    key: String,
    value: String,
    public_key: String,
    signature: String,
    nonce: u64,
    fee: u128,
}

#[derive(Debug, Deserialize)]
struct ProgramCloseParams {
    program_id: String,
    public_key: String,
    signature: String,
    nonce: u64,
    fee: u128,
}

fn default_program_name() -> String {
    "program".into()
}
fn default_gas_limit() -> u64 {
    1_000_000
}

#[derive(Clone, Debug)]
struct LoginChallenge {
    chain_id: String,
    address: Address,
    domain: String,
    issued_at: u64,
    expires_at: u64,
}

#[derive(Debug, Error)]
pub enum RpcError {
    #[error("invalid RPC bind address: {0}")]
    BindAddress(String),
    #[error("server error: {0}")]
    Server(String),
}

#[derive(Debug, Deserialize)]
struct RpcRequest {
    jsonrpc: String,
    id: Value,
    method: String,
    #[serde(default)]
    params: Value,
}

#[derive(Debug, Serialize)]
struct RpcResponse {
    jsonrpc: &'static str,
    id: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<RpcErrorObject>,
}

#[derive(Debug, Serialize)]
struct RpcErrorObject {
    code: i32,
    message: String,
}

#[derive(Debug, Deserialize)]
struct HashParams {
    hash: String,
}

#[derive(Debug, Deserialize)]
struct HeightParams {
    height: u64,
}

#[derive(Debug, Deserialize)]
struct AddressParams {
    address: String,
}

#[derive(Debug, Deserialize)]
struct FaucetParams {
    address: String,
    amount: String,
}

#[derive(Debug, Deserialize)]
struct BridgeOperationParams {
    operator_token: String,
    operation_id: String,
    asset_id: String,
    address: String,
    #[serde(default)]
    destination: String,
    amount: String,
    #[serde(default)]
    external_transaction: String,
    #[serde(default)]
    memo: String,
}

#[derive(Debug, Deserialize)]
struct MnaSwapOperationParams {
    operation: RpcMnaSwapOperationInput,
}

#[derive(Debug, Deserialize)]
struct MnaSwapPrepareParams {
    unsigned: RpcUnsignedMnaSwapInput,
}

#[derive(Debug, Deserialize)]
struct RpcMnaSwapOperationInput {
    unsigned: RpcUnsignedMnaSwapInput,
    signature: String,
}

#[derive(Debug, Deserialize)]
struct RpcUnsignedMnaSwapInput {
    version: u8,
    chain_id: String,
    nonce: u64,
    from: String,
    kind: MnaSwapKind,
    collateral_asset: String,
    amount_usdc: String,
    amount_mna: String,
    fee: String,
    public_key: String,
    #[serde(default)]
    memo: String,
}

#[derive(Debug, Deserialize)]
struct MnaReserveOperationParams {
    operator_token: String,
    collateral_asset: String,
    address: String,
    amount_usdc: String,
    amount_mna: String,
    #[serde(default)]
    collateral_amount: String,
    #[serde(default)]
    oracle_price_usd_micro_per_sol: String,
    #[serde(default)]
    oracle_timestamp: u64,
    #[serde(default)]
    fee_mna: String,
    external_transaction: String,
    #[serde(default)]
    destination: String,
    #[serde(default)]
    memo: String,
}

#[derive(Debug, Deserialize)]
struct BroadcastParams {
    transaction: RpcTransactionInput,
}

#[derive(Debug, Deserialize)]
struct TokenOperationParams {
    operation: RpcTokenOperationInput,
}

#[derive(Debug, Deserialize)]
struct TokenPrepareParams {
    unsigned: RpcUnsignedTokenOperationInput,
}

#[derive(Debug, Deserialize)]
struct RpcTokenOperationInput {
    unsigned: RpcUnsignedTokenOperationInput,
    signature: String,
}

#[derive(Debug, Deserialize)]
struct RpcUnsignedTokenOperationInput {
    version: u8,
    chain_id: String,
    nonce: u64,
    from: String,
    kind: TokenOperationKind,
    token_id: String,
    #[serde(default)]
    to: Option<String>,
    amount: String,
    fee: String,
    public_key: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    symbol: String,
    #[serde(default)]
    decimals: u8,
    #[serde(default)]
    max_supply: Option<String>,
    #[serde(default)]
    mint_authority: Option<String>,
    #[serde(default)]
    burn_authority: Option<String>,
    #[serde(default)]
    freeze_authority: Option<String>,
    #[serde(default)]
    metadata_uri: String,
    #[serde(default)]
    metadata_hash: String,
    #[serde(default)]
    memo: String,
}

#[derive(Debug, Deserialize)]
struct ChallengeParams {
    address: String,
    domain: String,
}

#[derive(Debug, Deserialize)]
struct VerifyParams {
    address: String,
    domain: String,
    nonce: String,
    public_key: String,
    signature: String,
}

#[derive(Debug, Serialize)]
struct BlockView {
    hash: String,
    height: u64,
    chain_id: String,
    parent_hash: String,
    timestamp: u64,
    transaction_root: String,
    state_root: String,
    proposer: Option<String>,
    proposer_signature: Option<String>,
    transactions: Vec<RpcTransactionView>,
    asset_operations: Vec<Value>,
    token_operations: Vec<Value>,
    mna_swap_operations: Vec<Value>,
    mna_reserve_operations: Vec<Value>,
    program_operations: Vec<Value>,
}

#[derive(Debug, Deserialize)]
struct RpcTransactionInput {
    unsigned: RpcUnsignedTransactionInput,
    signature: String,
}

#[derive(Debug, Deserialize)]
struct RpcUnsignedTransactionInput {
    version: u8,
    chain_id: String,
    nonce: u64,
    from: String,
    to: String,
    amount: String,
    fee: String,
    public_key: String,
    memo: String,
}

#[derive(Debug, Serialize)]
struct RpcTransactionView {
    unsigned: RpcUnsignedTransactionView,
    signature: String,
}

#[derive(Debug, Serialize)]
struct RpcUnsignedTransactionView {
    version: u8,
    chain_id: String,
    nonce: u64,
    from: String,
    to: String,
    amount: String,
    fee: String,
    public_key: String,
    memo: String,
}

impl RpcTransactionInput {
    fn into_core(self) -> Result<Transaction, String> {
        let from = self
            .unsigned
            .from
            .parse::<Address>()
            .map_err(|error| format!("invalid from address: {error}"))?;
        let to = self
            .unsigned
            .to
            .parse::<Address>()
            .map_err(|error| format!("invalid to address: {error}"))?;
        let public_key = decode_fixed::<32>(&self.unsigned.public_key, "public_key")?;
        let signature = decode_fixed::<64>(&self.signature, "signature")?;
        let amount = self
            .unsigned
            .amount
            .parse::<u128>()
            .map_err(|_| "amount must be an unsigned integer string".to_owned())?;
        let fee = self
            .unsigned
            .fee
            .parse::<u128>()
            .map_err(|_| "fee must be an unsigned integer string".to_owned())?;
        Ok(Transaction {
            unsigned: UnsignedTransaction {
                version: self.unsigned.version,
                chain_id: self.unsigned.chain_id,
                nonce: self.unsigned.nonce,
                from,
                to,
                amount,
                fee,
                public_key: PublicKey(public_key),
                memo: self.unsigned.memo,
            },
            signature: Signature(signature),
        })
    }
}

impl RpcUnsignedMnaSwapInput {
    fn into_core(self) -> Result<UnsignedMnaSwapOperation, String> {
        Ok(UnsignedMnaSwapOperation {
            version: self.version,
            chain_id: self.chain_id,
            nonce: self.nonce,
            from: self
                .from
                .parse()
                .map_err(|e| format!("invalid from address: {e}"))?,
            kind: self.kind,
            collateral_asset: parse_asset_id(&self.collateral_asset).map_err(|(_, e)| e)?,
            amount_usdc: self
                .amount_usdc
                .parse()
                .map_err(|_| "amount_usdc must be an unsigned integer string".to_owned())?,
            amount_mna: self
                .amount_mna
                .parse()
                .map_err(|_| "amount_mna must be an unsigned integer string".to_owned())?,
            fee: self
                .fee
                .parse()
                .map_err(|_| "fee must be an unsigned integer string".to_owned())?,
            public_key: PublicKey(decode_fixed::<32>(&self.public_key, "public_key")?),
            memo: self.memo,
        })
    }
}

impl RpcMnaSwapOperationInput {
    fn into_core(self) -> Result<MnaSwapOperation, String> {
        Ok(MnaSwapOperation {
            unsigned: self.unsigned.into_core()?,
            signature: Signature(decode_fixed::<64>(&self.signature, "signature")?),
        })
    }
}

impl RpcUnsignedTokenOperationInput {
    fn into_core(self) -> Result<UnsignedTokenOperation, String> {
        let parse_optional_address =
            |value: Option<String>, field: &str| -> Result<Option<Address>, String> {
                value
                    .filter(|value| !value.is_empty())
                    .map(|value| {
                        value
                            .parse::<Address>()
                            .map_err(|error| format!("invalid {field}: {error}"))
                    })
                    .transpose()
            };
        Ok(UnsignedTokenOperation {
            version: self.version,
            chain_id: self.chain_id,
            nonce: self.nonce,
            from: self
                .from
                .parse()
                .map_err(|error| format!("invalid from address: {error}"))?,
            kind: self.kind,
            token_id: parse_hash_string(&self.token_id, "token_id")?,
            to: self
                .to
                .filter(|value| !value.is_empty())
                .map(|value| {
                    value
                        .parse()
                        .map_err(|error| format!("invalid to address: {error}"))
                })
                .transpose()?,
            amount: self
                .amount
                .parse()
                .map_err(|_| "amount must be an unsigned integer string".to_owned())?,
            fee: self
                .fee
                .parse()
                .map_err(|_| "fee must be an unsigned integer string".to_owned())?,
            public_key: PublicKey(decode_fixed::<32>(&self.public_key, "public_key")?),
            name: self.name,
            symbol: self.symbol,
            decimals: self.decimals,
            max_supply: self
                .max_supply
                .filter(|value| !value.is_empty())
                .map(|value| {
                    value
                        .parse()
                        .map_err(|_| "max_supply must be an unsigned integer string".to_owned())
                })
                .transpose()?,
            mint_authority: parse_optional_address(self.mint_authority, "mint_authority")?,
            burn_authority: parse_optional_address(self.burn_authority, "burn_authority")?,
            freeze_authority: parse_optional_address(self.freeze_authority, "freeze_authority")?,
            metadata_uri: self.metadata_uri,
            metadata_hash: if self.metadata_hash.is_empty() {
                Hash::ZERO
            } else {
                parse_hash_string(&self.metadata_hash, "metadata_hash")?
            },
            memo: self.memo,
        })
    }
}

impl RpcTokenOperationInput {
    fn into_core(self) -> Result<TokenOperation, String> {
        Ok(TokenOperation {
            unsigned: self.unsigned.into_core()?,
            signature: Signature(decode_fixed::<64>(&self.signature, "signature")?),
        })
    }
}

fn token_operation_unsigned_view(unsigned: &UnsignedTokenOperation) -> Value {
    json!({
        "version": unsigned.version,
        "chain_id": unsigned.chain_id,
        "nonce": unsigned.nonce,
        "from": unsigned.from.to_string(),
        "kind": format!("{:?}", unsigned.kind).to_ascii_lowercase(),
        "token_id": unsigned.token_id.to_string(),
        "to": unsigned.to.map(|address| address.to_string()),
        "amount": unsigned.amount.to_string(),
        "fee": unsigned.fee.to_string(),
        "public_key": hex::encode(unsigned.public_key.0),
        "name": unsigned.name,
        "symbol": unsigned.symbol,
        "decimals": unsigned.decimals,
        "max_supply": unsigned.max_supply.map(|amount| amount.to_string()),
        "mint_authority": unsigned.mint_authority.map(|address| address.to_string()),
        "burn_authority": unsigned.burn_authority.map(|address| address.to_string()),
        "freeze_authority": unsigned.freeze_authority.map(|address| address.to_string()),
        "metadata_uri": unsigned.metadata_uri,
        "metadata_hash": unsigned.metadata_hash.to_string(),
        "memo": unsigned.memo,
    })
}

fn token_operation_view(operation_id: Hash, operation: &TokenOperation) -> Value {
    json!({
        "operation_id": operation_id.to_string(),
        "unsigned": token_operation_unsigned_view(&operation.unsigned),
        "signature": hex::encode(operation.signature.0),
    })
}

fn token_creation_operation_id(node: &Node, token_id: Hash) -> Option<Hash> {
    node.state()
        .token_operation_records()
        .iter()
        .find_map(|(operation_id, operation)| {
            (operation.unsigned.kind == TokenOperationKind::Create
                && token_id_from_operation(*operation_id) == token_id)
                .then_some(*operation_id)
        })
}

fn token_definition_view(definition: &TokenDefinition) -> Value {
    json!({
        "token_id": definition.token_id.to_string(),
        "id": AssetId::custom(definition.token_id, &definition.symbol, definition.decimals).canonical_key(),
        "creator": definition.creator.to_string(),
        "name": definition.name,
        "symbol": definition.symbol,
        "decimals": definition.decimals,
        "total_supply": definition.total_supply.to_string(),
        "max_supply": definition.max_supply.map(|amount| amount.to_string()),
        "mint_authority": definition.mint_authority.map(|address| address.to_string()),
        "burn_authority": definition.burn_authority.map(|address| address.to_string()),
        "freeze_authority": definition.freeze_authority.map(|address| address.to_string()),
        "metadata_uri": definition.metadata_uri,
        "metadata_hash": definition.metadata_hash.to_string(),
        "paused": definition.paused,
    })
}

fn transaction_view(transaction: &Transaction) -> RpcTransactionView {
    RpcTransactionView {
        unsigned: RpcUnsignedTransactionView {
            version: transaction.unsigned.version,
            chain_id: transaction.unsigned.chain_id.clone(),
            nonce: transaction.unsigned.nonce,
            from: transaction.unsigned.from.to_string(),
            to: transaction.unsigned.to.to_string(),
            amount: transaction.unsigned.amount.to_string(),
            fee: transaction.unsigned.fee.to_string(),
            public_key: hex::encode(transaction.unsigned.public_key.0),
            memo: transaction.unsigned.memo.clone(),
        },
        signature: hex::encode(transaction.signature.0),
    }
}

pub fn router(context: Arc<RpcContext>) -> Router {
    Router::new()
        .route("/rpc", post(handle_rpc))
        .route("/healthz", get(handle_health))
        .route("/metrics", get(handle_metrics))
        .layer(axum::extract::DefaultBodyLimit::max(1024 * 1024))
        .with_state(context)
}

pub fn run(node: Node, bind: SocketAddr) -> Result<(), RpcError> {
    run_with_proposer(node, bind, None)
}

pub fn run_with_proposer(
    node: Node,
    bind: SocketAddr,
    proposer: Option<KeyPair>,
) -> Result<(), RpcError> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|error| RpcError::Server(error.to_string()))?;
    runtime.block_on(run_async_internal(node, bind, None, proposer))
}

pub async fn run_async(node: Node, bind: SocketAddr) -> Result<(), RpcError> {
    run_async_internal(node, bind, None, None).await
}

pub fn run_with_network(
    node: Node,
    bind: SocketAddr,
    network: NetworkConfig,
    proposer: Option<KeyPair>,
) -> Result<(), RpcError> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|error| RpcError::Server(error.to_string()))?;
    runtime.block_on(run_async_internal(node, bind, Some(network), proposer))
}

pub async fn run_async_with_network(
    node: Node,
    bind: SocketAddr,
    network: NetworkConfig,
    proposer: Option<KeyPair>,
) -> Result<(), RpcError> {
    run_async_internal(node, bind, Some(network), proposer).await
}

async fn run_async_internal(
    node: Node,
    bind: SocketAddr,
    network: Option<NetworkConfig>,
    proposer: Option<KeyPair>,
) -> Result<(), RpcError> {
    let context = RpcContext::new(node);
    let proposer = proposer.map(Arc::new);
    let p2p_enabled = network.is_some();
    if let Some(network) = network {
        let network_node = Arc::clone(&context.node);
        let network_metrics = Arc::clone(&context.network_metrics);
        let validator = proposer.clone();
        tokio::spawn(async move {
            if let Err(error) =
                wsc_network::run_with_validator(network_node, network, network_metrics, validator)
                    .await
            {
                eprintln!(
                    "{}",
                    json!({"event":"network_stopped", "error":error.to_string()})
                );
            }
        });
    }
    let producer_context = Arc::clone(&context);
    let block_time_ms = producer_context
        .node
        .lock()
        .map_err(|_| RpcError::Server("node lock poisoned".to_owned()))?
        .config()
        .block_time_ms;
    eprintln!(
        "{}",
        json!({
            "event": "node_started",
            "rpc_bind": bind.to_string(),
            "p2p_enabled": p2p_enabled,
            "block_time_ms": block_time_ms,
        })
    );
    tokio::spawn(async move {
        let delay = tokio::time::Duration::from_millis(block_time_ms.max(1));
        loop {
            tokio::time::sleep(delay).await;
            if let Ok(mut node) = producer_context.node.lock() {
                if let Some(proposer) = proposer.as_deref() {
                    if !node.is_expected_proposer(&proposer.public_key()) {
                        continue;
                    }
                }
                let result = node.produce_block_with_proposer(proposer.as_deref());
                if result.is_ok() {
                    producer_context
                        .metrics
                        .blocks_produced
                        .fetch_add(1, Ordering::Relaxed);
                } else if let Err(error) = result {
                    eprintln!(
                        "{}",
                        json!({"event":"block_production_failed", "error":error.to_string()})
                    );
                }
            }
        }
    });

    let listener = TcpListener::bind(bind)
        .await
        .map_err(|error| RpcError::Server(error.to_string()))?;
    axum::serve(listener, router(context))
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await
        .map_err(|error| RpcError::Server(error.to_string()))
}

async fn handle_health(State(context): State<Arc<RpcContext>>) -> impl IntoResponse {
    match context.node.lock() {
        Ok(node) => (
            StatusCode::OK,
            Json(json!({
                "status": "ok",
                "chain_id": node.config().chain_id,
                "height": node.latest_block().header.height,
                "finalized_height": node.finalized_height().unwrap_or(0),
            })),
        ),
        Err(_) => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"status":"unavailable"})),
        ),
    }
}

async fn handle_metrics(State(context): State<Arc<RpcContext>>) -> impl IntoResponse {
    let (height, finalized_height) = context
        .node
        .lock()
        .map(|node| {
            (
                node.latest_block().header.height,
                node.finalized_height().unwrap_or(0),
            )
        })
        .unwrap_or((0, 0));
    let uptime = context.metrics.started_at.elapsed().as_secs();
    let body = format!(
        "# TYPE wsc_rpc_requests_total counter\nwsc_rpc_requests_total {}\n# TYPE wsc_rpc_errors_total counter\nwsc_rpc_errors_total {}\n# TYPE wsc_transactions_broadcast_total counter\nwsc_transactions_broadcast_total {}\n# TYPE wsc_blocks_produced_total counter\nwsc_blocks_produced_total {}\n# TYPE wsc_peer_connections_total counter\nwsc_peer_connections_total {}\n# TYPE wsc_blocks_imported_total counter\nwsc_blocks_imported_total {}\n# TYPE wsc_transactions_relayed_total counter\nwsc_transactions_relayed_total {}\n# TYPE wsc_votes_received_total counter\nwsc_votes_received_total {}\n# TYPE wsc_latest_height gauge\nwsc_latest_height {}\n# TYPE wsc_finalized_height gauge\nwsc_finalized_height {}\n# TYPE wsc_process_uptime_seconds gauge\nwsc_process_uptime_seconds {}\n",
        context.metrics.rpc_requests.load(Ordering::Relaxed),
        context.metrics.rpc_errors.load(Ordering::Relaxed),
        context.metrics.transactions_broadcast.load(Ordering::Relaxed),
        context.metrics.blocks_produced.load(Ordering::Relaxed),
        context.network_metrics.peer_connections.load(Ordering::Relaxed),
        context.network_metrics.blocks_imported.load(Ordering::Relaxed),
        context.network_metrics.transactions_relayed.load(Ordering::Relaxed),
        context.network_metrics.votes_received.load(Ordering::Relaxed),
        height,
        finalized_height,
        uptime,
    );
    (
        StatusCode::OK,
        [("content-type", "text/plain; version=0.0.4")],
        body,
    )
}

async fn handle_rpc(
    State(context): State<Arc<RpcContext>>,
    Json(request): Json<RpcRequest>,
) -> Json<RpcResponse> {
    if !context.metrics.allow_request() {
        context.metrics.rpc_errors.fetch_add(1, Ordering::Relaxed);
        return Json(error_response(
            request.id,
            -32029,
            "RPC rate limit exceeded",
        ));
    }
    context.metrics.rpc_requests.fetch_add(1, Ordering::Relaxed);
    let response = if request.jsonrpc != "2.0" {
        error_response(request.id, -32600, "jsonrpc must be 2.0")
    } else {
        dispatch(&context, &request.method, request.params)
            .await
            .map_or_else(
                |(code, message)| {
                    context.metrics.rpc_errors.fetch_add(1, Ordering::Relaxed);
                    error_response(request.id.clone(), code, &message)
                },
                |result| RpcResponse {
                    jsonrpc: "2.0",
                    id: request.id.clone(),
                    result: Some(result),
                    error: None,
                },
            )
    };
    Json(response)
}

fn it_build(params: Value) -> Result<Value, (i32, String)> {
    let p: ItBuildParams = parse_params(params)?;
    if p.language.to_ascii_lowercase() != "rust" {
        return Err((
            -32602,
            "only Rust is enabled; Python and JavaScript frontends are planned".into(),
        ));
    }
    let package = compile_rust_source(p.name, &p.source).map_err(|e| (-32602, e.to_string()))?;
    let bytes = package.encode().map_err(|e| (-32602, e.to_string()))?;
    Ok(json!({
        "status": "built",
        "program_id": package.program_id(),
        "code_hash": package.code_hash,
        "package_base64": BASE64.encode(bytes),
        "manifest": package.manifest,
    }))
}

fn decode_program_package(encoded: &str) -> Result<ProgramPackage, (i32, String)> {
    let bytes = BASE64
        .decode(encoded)
        .map_err(|e| (-32602, format!("package_base64 is invalid: {e}")))?;
    ProgramPackage::decode(&bytes).map_err(|e| (-32602, e.to_string()))
}

fn program_view(record: &wsc_core::ProgramRecord) -> Value {
    let package = ProgramPackage::decode(&record.package).ok();
    json!({
        "program_id": package.as_ref().map(ProgramPackage::program_id),
        "code_hash": package.as_ref().map(|p| p.code_hash.clone()),
        "manifest": package.as_ref().map(|p| p.manifest.clone()),
        "creator": record.creator,
        "deployed_at_height": record.deployed_at_height,
        "wasm_bytes": package.as_ref().map(|p| p.wasm.len()),
        "storage_keys": record.storage.len(),
    })
}

fn program_operation(
    kind: ProgramOperationKind,
    program_id: String,
    package: Vec<u8>,
    gas_limit: u64,
    key: String,
    value: String,
    public_key: &str,
    signature: &str,
    nonce: u64,
    fee: u128,
) -> Result<ProgramOperation, (i32, String)> {
    Ok(ProgramOperation {
        version: 1,
        chain_id: CHAIN_ID.to_owned(),
        kind,
        nonce,
        fee,
        program_id,
        package,
        gas_limit,
        key,
        value,
        public_key: PublicKey(
            decode_fixed::<32>(public_key, "public_key").map_err(|e| (-32602, e))?,
        ),
        signature: Signature(decode_fixed::<64>(signature, "signature").map_err(|e| (-32602, e))?),
    })
}

fn program_upload(context: &RpcContext, params: Value) -> Result<Value, (i32, String)> {
    let p: ProgramPackageParams = parse_params(params)?;
    let package = decode_program_package(&p.package_base64)?;
    let id = package.program_id();
    let mut registry = context
        .programs
        .lock()
        .map_err(|_| (-32000, "program registry lock poisoned".into()))?;
    registry.uploads.insert(id.clone(), package.clone());
    Ok(
        json!({"status":"verified", "program_id":id, "code_hash":package.code_hash, "manifest":package.manifest, "wasm_bytes":package.wasm.len()}),
    )
}

fn program_deploy(context: &RpcContext, params: Value) -> Result<Value, (i32, String)> {
    let p: ProgramDeployParams = parse_params(params)?;
    let package = decode_program_package(&p.package_base64)?;
    let id = package.program_id();
    let bytes = package.encode().map_err(|e| (-32602, e.to_string()))?;
    let operation = program_operation(
        ProgramOperationKind::Deploy,
        id.clone(),
        bytes,
        0,
        String::new(),
        String::new(),
        &p.public_key,
        &p.signature,
        p.nonce,
        p.fee,
    )?;
    with_node_mut(context, |node| {
        let operation_id = node.submit_program_operation(operation)?;
        Ok(json!({"status":"pending", "program_id":id, "operation_id":operation_id.to_string()}))
    })
}

fn program_get(context: &RpcContext, params: Value) -> Result<Value, (i32, String)> {
    #[derive(Deserialize)]
    struct P {
        program_id: String,
    }
    let p: P = parse_params(params)?;
    let node = context
        .node
        .lock()
        .map_err(|_| (-32000, "node lock poisoned".into()))?;
    let record = node
        .state()
        .programs()
        .get(&p.program_id)
        .ok_or((-32004, "program not found".into()))?;
    Ok(program_view(record))
}

fn program_list(context: &RpcContext) -> Result<Value, (i32, String)> {
    let node = context
        .node
        .lock()
        .map_err(|_| (-32000, "node lock poisoned".into()))?;
    Ok(json!({"programs": node.state().programs().values().map(program_view).collect::<Vec<_>>() }))
}

fn program_call(context: &RpcContext, params: Value) -> Result<Value, (i32, String)> {
    let p: ProgramCallParams = parse_params(params)?;
    if p.gas_limit == 0 {
        return Err((-32602, "gas_limit must be positive".into()));
    }
    let id = p.program_id.clone();
    let operation = program_operation(
        ProgramOperationKind::Call,
        id.clone(),
        vec![],
        p.gas_limit,
        String::new(),
        String::new(),
        &p.public_key,
        &p.signature,
        p.nonce,
        p.fee,
    )?;
    with_node_mut(context, |node| {
        let operation_id = node.submit_program_operation(operation)?;
        Ok(json!({"status":"pending", "program_id":id, "operation_id":operation_id.to_string()}))
    })
}

fn program_receipt(context: &RpcContext, params: Value) -> Result<Value, (i32, String)> {
    #[derive(Deserialize)]
    struct P {
        operation_id: String,
    }
    let p: P = parse_params(params)?;
    let id = parse_hash(&p.operation_id)?;
    let node = context
        .node
        .lock()
        .map_err(|_| (-32000, "node lock poisoned".into()))?;
    let receipt = node
        .state()
        .program_receipts()
        .get(&id)
        .ok_or((-32004, "program receipt not found".into()))?;
    Ok(json!({
        "operation_id": receipt.operation_id.to_string(), "program_id": receipt.program_id,
        "kind": receipt.kind, "status": receipt.status,
        "return_data_hex": hex::encode(&receipt.return_data), "gas_used": receipt.gas_used,
        "gas_limit": receipt.gas_limit, "fee_paid": receipt.fee_paid.to_string(), "error": receipt.error
    }))
}

fn program_storage_get(context: &RpcContext, params: Value) -> Result<Value, (i32, String)> {
    let p: ProgramStorageGetParams = parse_params(params)?;
    let node = context
        .node
        .lock()
        .map_err(|_| (-32000, "node lock poisoned".into()))?;
    let record = node
        .state()
        .programs()
        .get(&p.program_id)
        .ok_or((-32004, "program not found".into()))?;
    Ok(json!({"program_id":p.program_id, "key":p.key, "value":record.storage.get(&p.key)}))
}

fn program_storage_set(context: &RpcContext, params: Value) -> Result<Value, (i32, String)> {
    let p: ProgramStorageSetParams = parse_params(params)?;
    if p.key.len() > 128 || p.value.len() > 4096 {
        return Err((-32602, "storage key/value exceeds MVP limits".into()));
    }
    let id = p.program_id.clone();
    let key = p.key.clone();
    let operation = program_operation(
        ProgramOperationKind::StorageSet,
        id.clone(),
        vec![],
        0,
        p.key,
        p.value,
        &p.public_key,
        &p.signature,
        p.nonce,
        p.fee,
    )?;
    with_node_mut(context, |node| {
        let operation_id = node.submit_program_operation(operation)?;
        Ok(
            json!({"status":"pending", "program_id":id, "key":key, "operation_id":operation_id.to_string()}),
        )
    })
}

fn program_close(context: &RpcContext, params: Value) -> Result<Value, (i32, String)> {
    let p: ProgramCloseParams = parse_params(params)?;
    let id = p.program_id.clone();
    let operation = program_operation(
        ProgramOperationKind::Close,
        id.clone(),
        vec![],
        0,
        String::new(),
        String::new(),
        &p.public_key,
        &p.signature,
        p.nonce,
        p.fee,
    )?;
    with_node_mut(context, |node| {
        let operation_id = node.submit_program_operation(operation)?;
        Ok(json!({"status":"pending", "program_id":id, "operation_id":operation_id.to_string()}))
    })
}

async fn dispatch(
    context: &RpcContext,
    method: &str,
    params: Value,
) -> Result<Value, (i32, String)> {
    match method {
        "it_build" => it_build(params),
        "it_verify" | "program_upload" | "contract_upload" => program_upload(context, params),
        "program_deploy" => program_deploy(context, params),
        "program_close" => program_close(context, params),
        "program_get" => program_get(context, params),
        "program_list" => program_list(context),
        "program_call" => program_call(context, params),
        "program_receipt" => program_receipt(context, params),
        "program_storage_get" => program_storage_get(context, params),
        "program_storage_set" => program_storage_set(context, params),
        "node_status" | "chain_info" => chain_info(context),
        "bridge_status" => bridge_status().await,
        "bridge_mint" => bridge_asset_operation(context, params, AssetOperationKind::Mint),
        "bridge_burn" => bridge_asset_operation(context, params, AssetOperationKind::Burn),
        "bridge_operation_status" => bridge_operation_status(context, params),
        "bridge_operations_pending" => with_node(context, |node| {
            Ok(json!({
                "operations": node.pending_asset_operations().iter().map(asset_operation_view).collect::<Vec<_>>()
            }))
        }),
        "bridge_operations_recent" => with_node(context, |node| {
            Ok(json!({
                "operations": node.state().asset_operation_records().values().rev().take(1000).map(asset_operation_view).collect::<Vec<_>>()
            }))
        }),
        "mna_quote" => {
            #[derive(Debug, Deserialize)]
            struct QuoteParams {
                amount_usdc: String,
            }
            let p: QuoteParams = parse_params(params)?;
            let usdc: u128 = p.amount_usdc.parse().map_err(|_| {
                (
                    -32602,
                    "amount_usdc must be an unsigned integer string".to_owned(),
                )
            })?;
            if usdc == 0 || usdc % MNA_USDC_DENOMINATOR != 0 {
                return Err((
                    -32602,
                    "amount_usdc must be an even integer number of micro-USDC".to_owned(),
                ));
            }
            Ok(
                json!({"amount_usdc": usdc.to_string(), "amount_mna": (usdc / MNA_USDC_DENOMINATOR).to_string(), "usdc_per_mna": "2", "mna_per_usdc": "0.5", "price_usdc": "2.000000", "decimals": 6}),
            )
        }
        "mna_reserve_status" => with_node(context, |node| {
            let ledger = node.state().mna_reserve_ledger();
            let reserves_usdc = ledger
                .total_verified_deposits_usdc
                .saturating_sub(ledger.total_released_usdc);
            let reserves_sol_usd = ledger
                .total_verified_sol_usd
                .saturating_sub(ledger.total_released_sol_usd);
            let reserves = reserves_usdc.saturating_add(reserves_sol_usd);
            let required = ledger
                .reserve_backed_mna_minted
                .saturating_mul(MNA_USDC_DENOMINATOR);
            Ok(json!({
                "rate":"2 USD = 1 MNA", "paused":ledger.paused,
                "total_verified_deposits_usdc":ledger.total_verified_deposits_usdc.to_string(),
                "total_released_usdc":ledger.total_released_usdc.to_string(),
                "current_reserves_usdc":reserves_usdc.to_string(),
                "total_verified_sol_lamports":ledger.total_verified_sol_lamports.to_string(),
                "total_released_sol_lamports":ledger.total_released_sol_lamports.to_string(),
                "current_reserves_sol_usd":reserves_sol_usd.to_string(),
                "current_reserves_total_usd":reserves.to_string(),
                "reserve_backed_mna_minted":ledger.reserve_backed_mna_minted.to_string(),
                "total_redeemed_mna":ledger.total_redeemed_mna.to_string(),
                "required_reserve_usd":required.to_string(),
                "surplus_usd":reserves.saturating_sub(required).to_string(),
                "collateralized":reserves >= required
            }))
        }),
        "sol_mna_quote" => {
            #[derive(Debug, Deserialize)]
            struct SolQuoteParams {
                amount_lamports: String,
            }
            let p: SolQuoteParams = parse_params(params)?;
            let lamports: u128 = p.amount_lamports.parse().map_err(|_| {
                (
                    -32602,
                    "amount_lamports must be an unsigned integer string".to_owned(),
                )
            })?;
            if lamports == 0 {
                return Err((-32602, "amount_lamports must be positive".to_owned()));
            }
            let price: u128 = std::env::var("WSC_SOL_MNA_PRICE_USD_MICRO_PER_SOL")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(0);
            if price == 0 {
                return Err((-32010, "SOL/MNA oracle is not configured".to_owned()));
            }
            let usd = lamports
                .checked_mul(price)
                .ok_or((-32000, "SOL quote overflow".to_owned()))?
                / 1_000_000_000u128;
            let gross = usd / MNA_USDC_DENOMINATOR;
            let fee_bps: u128 = std::env::var("WSC_SOL_MNA_FEE_BPS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(50)
                .clamp(50, 500);
            let fee = gross.saturating_mul(fee_bps) / 10_000;
            let amount_mna = gross.saturating_sub(fee);
            Ok(
                json!({"amount_lamports":lamports.to_string(), "oracle_price_usd_micro_per_sol":price.to_string(), "amount_usd_micro":usd.to_string(), "gross_mna":gross.to_string(), "fee_mna":fee.to_string(), "amount_mna":amount_mna.to_string(), "fee_bps":fee_bps}),
            )
        }
        "mna_swap_prepare" => {
            let p: MnaSwapPrepareParams = parse_params(params)?;
            let unsigned = p.unsigned.into_core().map_err(|e| (-32602, e))?;
            let op = MnaSwapOperation {
                unsigned: unsigned.clone(),
                signature: Signature([0; 64]),
            };
            let id = mna_swap_operation_id(&op).map_err(|e| (-32000, e.to_string()))?;
            Ok(
                json!({"operation_id":id.to_string(), "signing_bytes":hex::encode(unsigned.signing_bytes().map_err(|e| (-32000,e.to_string()))?)}),
            )
        }
        "mna_swap_broadcast" => {
            let p: MnaSwapOperationParams = parse_params(params)?;
            let op = p.operation.into_core().map_err(|e| (-32602, e))?;
            with_node_mut(context, |node| {
                node.submit_mna_swap(op)
                    .map(|id| json!({"operation_id":id.to_string(),"status":"queued"}))
            })
        }
        "mna_swap_status" => {
            let p: HashParams = parse_params(params)?;
            let id = parse_hash(&p.hash)?;
            with_node(context, |node| {
                if let Some(op) = node.state().mna_swap_operation_records().get(&id) {
                    return Ok(
                        json!({"operation_id":id.to_string(),"status":"confirmed","operation":op}),
                    );
                }
                if let Some(op) = node
                    .pending_mna_swap_operations()
                    .into_iter()
                    .find(|op| mna_swap_operation_id(op).ok() == Some(id))
                {
                    return Ok(
                        json!({"operation_id":id.to_string(),"status":"pending","operation":op}),
                    );
                }
                Ok(json!({"operation_id":id.to_string(),"status":"not_found"}))
            })
        }
        "mna_reserve_mint" | "mna_reserve_release" => {
            let p: MnaReserveOperationParams = parse_params(params)?;
            let expected = std::env::var("WSC_BRIDGE_OPERATOR_TOKEN").unwrap_or_default();
            if expected.is_empty() || p.operator_token != expected {
                return Err((-32001, "bridge operator authorization failed".to_owned()));
            }
            let kind = if method.ends_with("mint") {
                MnaReserveOperationKind::VerifyDeposit
            } else {
                MnaReserveOperationKind::Release
            };
            let op0 = MnaReserveOperation {
                version: 1,
                operation_id: Hash::ZERO,
                kind,
                collateral_asset: parse_asset_id(&p.collateral_asset)?,
                address: parse_address(&p.address)?,
                amount_usdc: p
                    .amount_usdc
                    .parse()
                    .map_err(|_| (-32602, "amount_usdc must be unsigned".to_owned()))?,
                amount_mna: p
                    .amount_mna
                    .parse()
                    .map_err(|_| (-32602, "amount_mna must be unsigned".to_owned()))?,
                collateral_amount: p
                    .collateral_amount
                    .parse()
                    .map_err(|_| (-32602, "collateral_amount must be unsigned".to_owned()))?,
                oracle_price_usd_micro_per_sol: p.oracle_price_usd_micro_per_sol.parse().map_err(
                    |_| {
                        (
                            -32602,
                            "oracle_price_usd_micro_per_sol must be unsigned".to_owned(),
                        )
                    },
                )?,
                oracle_timestamp: p.oracle_timestamp,
                fee_mna: p
                    .fee_mna
                    .parse()
                    .map_err(|_| (-32602, "fee_mna must be unsigned".to_owned()))?,
                destination: p.destination,
                external_transaction: p.external_transaction,
                memo: p.memo,
            };
            let id = mna_reserve_operation_id(&op0).map_err(|e| (-32000, e.to_string()))?;
            let mut op = op0;
            op.operation_id = id;
            with_node_mut(context, |node| {
                node.submit_mna_reserve_operation(op)
                    .map(|id| json!({"operation_id":id.to_string(),"status":"queued"}))
            })
        }
        "mna_reserve_status_recent" => with_node(context, |node| {
            Ok(
                json!({"operations":node.state().mna_reserve_operation_records().values().rev().take(1000).collect::<Vec<_>>()}),
            )
        }),
        "token_operation_prepare" => {
            let params: TokenPrepareParams = parse_params(params)?;
            let unsigned = params
                .unsigned
                .into_core()
                .map_err(|error| (-32602, error))?;
            let signing_bytes = unsigned
                .signing_bytes()
                .map_err(|error| (-32000, error.to_string()))?;
            let unsigned_operation = TokenOperation {
                unsigned: unsigned.clone(),
                signature: Signature([0; 64]),
            };
            let operation_id = token_operation_id(&unsigned_operation)
                .map_err(|error| (-32000, error.to_string()))?;
            let token_id = if unsigned.kind == TokenOperationKind::Create {
                token_id_from_operation(operation_id)
            } else {
                unsigned.token_id
            };
            Ok(json!({
                "operation_id": operation_id.to_string(),
                "token_id": token_id.to_string(),
                "signing_bytes": hex::encode(signing_bytes),
                "kind": format!("{:?}", unsigned.kind).to_ascii_lowercase(),
            }))
        }
        "token_operation_broadcast" => {
            let params: TokenOperationParams = parse_params(params)?;
            let operation = params
                .operation
                .into_core()
                .map_err(|error| (-32602, error))?;
            with_node_mut(context, |node| {
                let operation_id = token_operation_id(&operation)
                    .map_err(|error| NodeError::Crypto(error.to_string()))?;
                let token_id = if operation.unsigned.kind == TokenOperationKind::Create {
                    token_id_from_operation(operation_id)
                } else {
                    operation.unsigned.token_id
                };
                node.submit_token_operation(operation).map(|hash| {
                    json!({
                        "operation_id": hash.to_string(),
                        "token_id": token_id.to_string(),
                        "status": "queued",
                    })
                })
            })
        }
        "token_operation_status" => {
            let params: HashParams = parse_params(params)?;
            let operation_id = parse_hash(&params.hash)?;
            with_node(context, |node| {
                if let Some(operation) = node.state().token_operation_records().get(&operation_id) {
                    return Ok(json!({
                        "operation_id": operation_id.to_string(),
                        "status": "confirmed",
                        "operation": token_operation_view(operation_id, operation),
                    }));
                }
                if let Some(operation) = node
                    .pending_token_operations()
                    .into_iter()
                    .find(|operation| token_operation_id(operation).ok() == Some(operation_id))
                {
                    return Ok(json!({
                        "operation_id": operation_id.to_string(),
                        "status": "pending",
                        "operation": token_operation_view(operation_id, &operation),
                    }));
                }
                Ok(json!({ "operation_id": operation_id.to_string(), "status": "not_found" }))
            })
        }
        "token_list" => with_node(context, |node| {
            let tokens = node
                .token_definitions()
                .values()
                .map(|definition| {
                    let mut value = token_definition_view(definition);
                    if let Some(operation_id) =
                        token_creation_operation_id(node, definition.token_id)
                    {
                        value["creation_operation_id"] = json!(operation_id.to_string());
                    }
                    value
                })
                .collect::<Vec<_>>();
            Ok(json!({ "tokens": tokens }))
        }),
        "token_get" => {
            let params: HashParams = parse_params(params)?;
            let token_id = parse_hash(&params.hash)?;
            with_node(context, |node| {
                let definition = node
                    .state()
                    .token_definition(&token_id)
                    .ok_or_else(|| NodeError::Crypto("token not found".to_owned()))?;
                let mut value = token_definition_view(definition);
                if let Some(operation_id) = token_creation_operation_id(node, token_id) {
                    value["creation_operation_id"] = json!(operation_id.to_string());
                }
                Ok(value)
            })
        }
        "token_balance" => {
            #[derive(Debug, Deserialize)]
            struct Params {
                address: String,
                token_id: String,
            }
            let params: Params = parse_params(params)?;
            let address = parse_address(&params.address)?;
            let token_id = parse_hash(&params.token_id)?;
            with_node(context, |node| {
                let definition = node
                    .state()
                    .token_definition(&token_id)
                    .ok_or_else(|| NodeError::Crypto("token not found".to_owned()))?;
                Ok(
                    json!({ "address": address.to_string(), "token_id": token_id.to_string(), "symbol": definition.symbol, "decimals": definition.decimals, "balance": node.token_balance(&address, &token_id).to_string() }),
                )
            })
        }
        "asset_list" => with_node(context, |node| {
            let mut assets = vec![json!({
                "id": "worldstreet:MNA:native",
                "symbol": "MNA",
                "display_name": "MANNA",
                "decimals": 6,
                "wrapped": false,
                "enabled": true,
            })];
            assets.extend(
                node.assets()
                    .iter()
                    .filter(|asset| asset.id.symbol != "WSOL")
                    .map(|asset| {
                        json!({
                            "id": asset.id.canonical_key(),
                            "symbol": asset.id.symbol,
                            "display_name": asset.display_name,
                            "decimals": asset.id.decimals,
                            "origin_chain": asset.id.namespace,
                            "origin_reference": asset.id.contract,
                            "wrapped": asset.wrapped,
                            "enabled": asset.enabled,
                        })
                    }),
            );
            let usdc = EthereumUsdcBridgeConfig::from_env().usdc_definition();
            assets.push(json!({
                "id": usdc.id.canonical_key(),
                "symbol": usdc.id.symbol,
                "display_name": usdc.display_name,
                "decimals": usdc.id.decimals,
                "origin_chain": usdc.id.namespace,
                "origin_reference": usdc.id.contract,
                "wrapped": usdc.wrapped,
                "enabled": usdc.enabled,
            }));
            let solana = SolanaBridgeConfig::from_env().wsol_definition();
            assets.push(json!({
                "id": solana.id.canonical_key(),
                "symbol": solana.id.symbol,
                "display_name": solana.display_name,
                "decimals": solana.id.decimals,
                "origin_chain": solana.id.namespace,
                "origin_reference": solana.id.contract,
                "wrapped": solana.wrapped,
                "enabled": solana.enabled,
            }));
            let solana_usdc = SolanaBridgeConfig::from_env().spl_usdc_definition();
            assets.push(json!({
                "id": solana_usdc.id.canonical_key(),
                "symbol": solana_usdc.id.symbol,
                "display_name": solana_usdc.display_name,
                "decimals": solana_usdc.id.decimals,
                "origin_chain": solana_usdc.id.namespace,
                "origin_reference": solana_usdc.id.contract,
                "wrapped": solana_usdc.wrapped,
                "enabled": solana_usdc.enabled,
            }));
            assets.extend(node.token_definitions().values().map(|definition| {
                let mut value = token_definition_view(definition);
                value["wrapped"] = json!(false);
                value["enabled"] = json!(true);
                value["display_name"] = json!(definition.name);
                value
            }));
            Ok(json!({ "assets": assets }))
        }),
        "block_latest" => with_node(context, |node| {
            block_view(node.latest_block()).map_err(view_error)
        }),
        "block_get" => {
            let params: HashParams = parse_params(params)?;
            let hash = parse_hash(&params.hash)?;
            with_node(context, |node| {
                node.get_block(hash)
                    .and_then(|block| block_view(&block).map_err(view_error))
            })
        }
        "block_get_by_height" => {
            let params: HeightParams = parse_params(params)?;
            with_node(context, |node| {
                node.get_block_by_height(params.height)
                    .and_then(|block| block_view(&block).map_err(view_error))
            })
        }
        "transaction_get" => {
            let params: HashParams = parse_params(params)?;
            let hash = parse_hash(&params.hash)?;
            with_node(context, |node| {
                node.get_transaction(hash).map(|transaction| {
                    json!({
                        "hash": hash.to_string(),
                        "transaction": transaction_view(&transaction),
                    })
                })
            })
        }
        "transaction_status" => {
            let params: HashParams = parse_params(params)?;
            let hash = parse_hash(&params.hash)?;
            with_node(context, |node| {
                if let Ok(transaction) = node.get_transaction(hash) {
                    return Ok(json!({
                        "hash": hash.to_string(),
                        "status": "confirmed",
                        "transaction": transaction_view(&transaction),
                    }));
                }
                for transaction in node.mempool_transactions() {
                    if transaction_id(&transaction).ok() == Some(hash) {
                        return Ok(json!({
                            "hash": hash.to_string(),
                            "status": "pending",
                            "transaction": transaction_view(&transaction),
                        }));
                    }
                }
                Ok(json!({ "hash": hash.to_string(), "status": "not_found" }))
            })
        }
        "transaction_broadcast" => {
            let params: BroadcastParams = parse_params(params)?;
            with_node_mut(context, |node| {
                let transaction = params
                    .transaction
                    .into_core()
                    .map_err(|error| NodeError::Crypto(error))?;
                node.submit_transaction(transaction).map(|hash| {
                    context
                        .metrics
                        .transactions_broadcast
                        .fetch_add(1, Ordering::Relaxed);
                    json!({
                    "hash": hash.to_string(),
                    "status": "accepted",
                    })
                })
            })
        }
        "devnet_faucet" => {
            let params: FaucetParams = parse_params(params)?;
            let address = parse_address(&params.address)?;
            let amount = params.amount.parse::<u128>().map_err(|_| {
                (
                    -32602,
                    "amount must be an unsigned integer string".to_owned(),
                )
            })?;
            if amount == 0 || amount > 1_000_000_000_000 {
                return Err((
                    -32602,
                    "devnet faucet amount must be between 1 and 1000000000000 microMNA".to_owned(),
                ));
            }
            with_node_mut(context, |node| {
                if !node.config().chain_id.contains("devnet") {
                    return Err(NodeError::FaucetDisabled);
                }
                let faucet = KeyPair::devnet_faucet();
                let unsigned = UnsignedTransaction {
                    version: 1,
                    chain_id: node.config().chain_id.clone(),
                    nonce: 0,
                    from: faucet.address(),
                    to: address,
                    amount,
                    fee: 1,
                    public_key: faucet.public_key(),
                    memo: "devnet faucet".to_owned(),
                };
                let signature = faucet.sign(
                    &unsigned
                        .signing_bytes()
                        .map_err(|error| NodeError::Crypto(error.to_string()))?,
                );
                let transaction = Transaction {
                    unsigned,
                    signature,
                };
                let hash = node.submit_transaction(transaction)?;
                Ok(
                    json!({ "status": "queued", "address": address.to_string(), "amount": amount.to_string(), "hash": hash.to_string() }),
                )
            })
        }
        "account_get" => {
            let params: AddressParams = parse_params(params)?;
            let address = parse_address(&params.address)?;
            with_node(context, |node| {
                let (balance, nonce) = node.account(&address);
                Ok(json!({
                    "address": address.to_string(),
                    "asset": "MNA",
                    "balance": balance.to_string(),
                    "nonce": nonce,
                    "assets": node.asset_balances(&address),
                }))
            })
        }
        "mempool_status" => with_node(context, |node| Ok(json!({ "size": node.mempool_len() }))),
        "validator_set" => with_node(context, |node| {
            Ok(json!({
                "validators": node.validators().iter().map(|validator| json!({
                    "name": validator.name.clone(),
                    "public_key": hex::encode(validator.public_key.0),
                })).collect::<Vec<_>>(),
            }))
        }),
        "finality_status" => with_node(context, |node| {
            Ok(json!({
                "height": node.finalized_height()?,
                "hash": node.finalized_hash()?.to_string(),
            }))
        }),
        "auth_challenge" => create_challenge(context, params),
        "auth_verify" => verify_challenge(context, params),
        _ => Err((-32601, format!("method not found: {method}"))),
    }
}

fn bridge_asset_operation(
    context: &RpcContext,
    params: Value,
    kind: AssetOperationKind,
) -> Result<Value, (i32, String)> {
    let params: BridgeOperationParams = parse_params(params)?;
    let expected = std::env::var("WSC_BRIDGE_OPERATOR_TOKEN").unwrap_or_default();
    if expected.is_empty() || params.operator_token != expected {
        return Err((-32001, "bridge operator authorization failed".to_owned()));
    }
    let operation_id = parse_hash(&params.operation_id)?;
    let asset_id = parse_asset_id(&params.asset_id)?;
    let address = parse_address(&params.address)?;
    let amount = params.amount.parse::<u128>().map_err(|_| {
        (
            -32602,
            "amount must be an unsigned integer string".to_owned(),
        )
    })?;
    if amount == 0 {
        return Err((-32602, "amount must be greater than zero".to_owned()));
    }
    let operation = AssetOperation {
        version: 1,
        operation_id,
        kind,
        asset_id,
        address,
        destination: params.destination,
        amount,
        external_transaction: params.external_transaction,
        memo: params.memo,
    };
    with_node_mut(context, |node| {
        node.submit_asset_operation(operation.clone())
            .map(|hash| json!({ "operation_id": hash.to_string(), "status": "queued", "kind": match operation.kind { AssetOperationKind::Mint => "mint", AssetOperationKind::Burn => "burn" } }))
    })
}

fn bridge_operation_status(context: &RpcContext, params: Value) -> Result<Value, (i32, String)> {
    #[derive(Debug, Deserialize)]
    struct Params {
        operation_id: String,
    }
    let params: Params = parse_params(params)?;
    let operation_id = parse_hash(&params.operation_id)?;
    with_node(context, |node| {
        if node.state().processed_asset_operation(&operation_id) {
            return Ok(json!({ "operation_id": operation_id.to_string(), "status": "confirmed" }));
        }
        if let Some(operation) = node
            .pending_asset_operations()
            .into_iter()
            .find(|item| item.operation_id == operation_id)
        {
            return Ok(
                json!({ "operation_id": operation_id.to_string(), "status": "pending", "operation": asset_operation_view(&operation) }),
            );
        }
        Ok(json!({ "operation_id": operation_id.to_string(), "status": "not_found" }))
    })
}

fn parse_asset_id(value: &str) -> Result<AssetId, (i32, String)> {
    let mut parts = value.splitn(3, ':');
    let namespace = parts.next().unwrap_or_default();
    let symbol = parts.next().unwrap_or_default();
    let reference = parts.next().unwrap_or_default();
    if namespace.is_empty()
        || symbol.is_empty()
        || reference.is_empty()
        || namespace == "worldstreet"
    {
        return Err((
            -32602,
            "asset_id must be namespace:symbol:reference for a wrapped asset".to_owned(),
        ));
    }
    let upper = symbol.to_ascii_uppercase();
    let decimals = match upper.as_str() {
        "WETH" => 18,
        "WSOL" | "SOL" => 9,
        "USDC" => 6,
        _ => return Err((-32602, "unsupported external asset symbol".to_owned())),
    };
    Ok(AssetId::wrapped(namespace, symbol, reference, decimals))
}

fn asset_operation_view(operation: &AssetOperation) -> Value {
    json!({
        "operation_id": operation.operation_id.to_string(),
        "kind": match operation.kind { AssetOperationKind::Mint => "mint", AssetOperationKind::Burn => "burn" },
        "asset_id": operation.asset_id.canonical_key(),
        "address": operation.address.to_string(),
        "destination": operation.destination,
        "amount": operation.amount.to_string(),
        "external_transaction": operation.external_transaction,
        "memo": operation.memo,
    })
}

fn chain_info(context: &RpcContext) -> Result<Value, (i32, String)> {
    with_node(context, |node| {
        Ok(json!({
            "chain_id": node.config().chain_id.clone(),
            "native_asset": { "name": "MANNA", "symbol": "MNA", "decimals": 6 },
            "genesis_hash": node.genesis_hash()?.to_string(),
            "latest_height": node.latest_block().header.height,
            "latest_hash": node.latest_hash()?.to_string(),
            "finalized_height": node.finalized_height()?,
            "finalized_hash": node.finalized_hash()?.to_string(),
        }))
    })
}

async fn bridge_status() -> Result<Value, (i32, String)> {
    let ethereum_config = EthereumBridgeConfig::from_env();
    let usdc_config = EthereumUsdcBridgeConfig::from_env();
    let solana_config = SolanaBridgeConfig::from_env();
    let mut result = json!({
        "ethereum": {
            "network": ethereum_config.network,
            "bridge_contract": ethereum_config.bridge_contract,
            "confirmations": ethereum_config.confirmations,
            "enabled": ethereum_config.enabled,
            "rpc_configured": !ethereum_config.rpc_url.is_empty(),
            "deposit_topic_configured": !ethereum_config.deposit_event_topic.is_empty(),
        },
        "usdc": {
            "network": usdc_config.network,
            "token_contract": usdc_config.token_contract,
            "bridge_contract": usdc_config.bridge_contract,
            "confirmations": usdc_config.confirmations,
            "enabled": usdc_config.enabled,
            "token_configured": !usdc_config.token_contract.is_empty(),
            "deposit_topic_configured": !usdc_config.deposit_event_topic.is_empty(),
        },
        "weth": {
            "asset_id": ethereum_config.weth_definition().id.canonical_key(),
            "symbol": "WETH",
            "decimals": 18,
        },
        "usdc_asset": {
            "asset_id": usdc_config.usdc_definition().id.canonical_key(),
            "symbol": "USDC",
            "decimals": 6,
        },
        "solana": {
            "network": solana_config.network,
            "mode": solana_config.mode,
            "bridge_program": solana_config.bridge_program,
            "vault_address": solana_config.vault_address,
            "wsol_mint": solana_config.wsol_mint,
            "spl_usdc_mint": solana_config.spl_usdc_mint,
            "spl_usdc_vault_token_account": solana_config.spl_usdc_vault_token_account,
            "spl_usdc_enabled": solana_config.spl_usdc_enabled,
            "commitment": solana_config.commitment,
            "confirmations": solana_config.confirmations,
            "enabled": solana_config.enabled,
            "rpc_configured": !solana_config.rpc_url.is_empty(),
        },
        "wsol": {
            "asset_id": solana_config.wsol_definition().id.canonical_key(),
            "symbol": "WSOL",
            "decimals": 9,
        },
        "solana_usdc": {
            "asset_id": solana_config.spl_usdc_definition().id.canonical_key(),
            "symbol": "USDC",
            "decimals": 6,
            "enabled": solana_config.spl_usdc_enabled,
        },
    });
    if ethereum_config.enabled {
        let client = EthereumRpcClient::new(ethereum_config.rpc_url);
        let ethereum = result.get_mut("ethereum").ok_or((
            -32000,
            "bridge status response construction failed".to_owned(),
        ))?;
        match (client.chain_id().await, client.block_number().await) {
            (Ok(chain_id), Ok(block_number)) => {
                ethereum["connected"] = json!(true);
                ethereum["chain_id"] = json!(chain_id);
                ethereum["block_number"] = json!(block_number);
            }
            (chain_result, block_result) => {
                ethereum["connected"] = json!(false);
                ethereum["error"] = json!(format!(
                    "chain_id={chain_result:?}; block_number={block_result:?}"
                ));
            }
        }
    } else if let Some(ethereum) = result.get_mut("ethereum") {
        ethereum["connected"] = json!(false);
        ethereum["reason"] = json!("set WSC_ETHEREUM_RPC_URL, WSC_ETHEREUM_BRIDGE_CONTRACT, and WSC_ETHEREUM_DEPOSIT_TOPIC to enable the testnet bridge");
    }
    if solana_config.enabled {
        let client = SolanaRpcClient::new(solana_config.rpc_url);
        let solana = result.get_mut("solana").ok_or((
            -32000,
            "bridge status response construction failed".to_owned(),
        ))?;
        match (
            client.health().await,
            client.slot(&solana_config.commitment).await,
        ) {
            (Ok(health), Ok(slot)) => {
                solana["connected"] = json!(health == "ok");
                solana["health"] = json!(health);
                solana["slot"] = json!(slot);
            }
            (health_result, slot_result) => {
                solana["connected"] = json!(false);
                solana["error"] = json!(format!("health={health_result:?}; slot={slot_result:?}"));
            }
        }
    } else if let Some(solana) = result.get_mut("solana") {
        solana["connected"] = json!(false);
        solana["reason"] = json!("WSOL is disabled until SOL liquidity is funded; set WSC_SOLANA_MODE=custody and configure the vault to reactivate it");
    }
    Ok(result)
}

fn create_challenge(context: &RpcContext, params: Value) -> Result<Value, (i32, String)> {
    let params: ChallengeParams = parse_params(params)?;
    let address = parse_address(&params.address)?;
    if params.domain.trim().is_empty() || params.domain.len() > 255 {
        return Err((-32602, "domain must be 1-255 bytes".to_owned()));
    }
    let mut nonce_bytes = [0u8; 24];
    fill(&mut nonce_bytes).map_err(|error| (-32000, error.to_string()))?;
    let nonce = hex::encode(nonce_bytes);
    let issued_at = now_seconds();
    let expires_at = issued_at + LOGIN_TTL_SECONDS;
    let chain_id = context
        .node
        .lock()
        .map_err(|_| (-32000, "node lock poisoned".to_owned()))?
        .config()
        .chain_id
        .clone();
    let challenge = LoginChallenge {
        chain_id: chain_id.clone(),
        address,
        domain: params.domain.clone(),
        issued_at,
        expires_at,
    };
    let message = format_login_message_for_chain(
        &chain_id,
        &params.domain,
        address,
        &nonce,
        issued_at,
        expires_at,
    );
    let mut challenges = context
        .challenges
        .lock()
        .map_err(|_| (-32000, "challenge lock poisoned".to_owned()))?;
    if challenges.len() >= MAX_PENDING_CHALLENGES {
        return Err((-32029, "too many pending login challenges".to_owned()));
    }
    challenges.insert(nonce.clone(), challenge);
    Ok(
        json!({ "nonce": nonce, "message": message, "issued_at": issued_at, "expires_at": expires_at }),
    )
}

fn verify_challenge(context: &RpcContext, params: Value) -> Result<Value, (i32, String)> {
    let params: VerifyParams = parse_params(params)?;
    let address = parse_address(&params.address)?;
    let public_key_bytes = hex::decode(&params.public_key)
        .map_err(|_| (-32602, "public_key must be hex".to_owned()))?;
    let signature_bytes =
        hex::decode(&params.signature).map_err(|_| (-32602, "signature must be hex".to_owned()))?;
    if public_key_bytes.len() != 32 || signature_bytes.len() != 64 {
        return Err((
            -32602,
            "public_key or signature has the wrong length".to_owned(),
        ));
    }
    let mut public_key = [0u8; 32];
    public_key.copy_from_slice(&public_key_bytes);
    let mut signature = [0u8; 64];
    signature.copy_from_slice(&signature_bytes);
    let public_key = PublicKey(public_key);
    let signature = Signature(signature);
    if address_from_public_key(&public_key) != address {
        return Err((-32001, "public key does not derive address".to_owned()));
    }
    let challenge = context
        .challenges
        .lock()
        .map_err(|_| (-32000, "challenge lock poisoned".to_owned()))?
        .get(&params.nonce)
        .cloned()
        .ok_or((-32004, "challenge not found or already used".to_owned()))?;
    if challenge.address != address
        || challenge.domain != params.domain
        || now_seconds() > challenge.expires_at
    {
        return Err((-32004, "challenge is invalid or expired".to_owned()));
    }
    let message = format_login_message_for_chain(
        &challenge.chain_id,
        &params.domain,
        address,
        &params.nonce,
        challenge.issued_at,
        challenge.expires_at,
    );
    if !KeyPair::verify(&public_key, message.as_bytes(), &signature) {
        return Err((-32001, "invalid login signature".to_owned()));
    }
    context
        .challenges
        .lock()
        .map_err(|_| (-32000, "challenge lock poisoned".to_owned()))?
        .remove(&params.nonce);
    let mut token_bytes = [0u8; 32];
    fill(&mut token_bytes).map_err(|error| (-32000, error.to_string()))?;
    Ok(
        json!({ "authenticated": true, "address": address.to_string(), "session_token": hex::encode(token_bytes) }),
    )
}

pub fn format_login_message(
    domain: &str,
    address: Address,
    nonce: &str,
    issued_at: u64,
    expires_at: u64,
) -> String {
    format_login_message_for_chain(CHAIN_ID, domain, address, nonce, issued_at, expires_at)
}

fn format_login_message_for_chain(
    chain_id: &str,
    domain: &str,
    address: Address,
    nonce: &str,
    issued_at: u64,
    expires_at: u64,
) -> String {
    format!("Intertrain Login\n\nDomain: {domain}\nChain ID: {chain_id}\nAddress: {address}\nNonce: {nonce}\nIssued At: {issued_at}\nExpires At: {expires_at}")
}

fn block_view(block: &Block) -> Result<Value, (i32, String)> {
    let hash = block_header_id(&block.header).map_err(|error| (-32000, error.to_string()))?;
    Ok(json!(BlockView {
        hash: hash.to_string(),
        height: block.header.height,
        chain_id: block.header.chain_id.clone(),
        parent_hash: block.header.parent_hash.to_string(),
        timestamp: block.header.timestamp,
        transaction_root: block.header.transaction_root.to_string(),
        state_root: block.header.state_root.to_string(),
        proposer: block.header.proposer.map(|key| hex::encode(key.0)),
        proposer_signature: block
            .header
            .proposer_signature
            .map(|signature| hex::encode(signature.0)),
        transactions: block.transactions.iter().map(transaction_view).collect(),
        asset_operations: block
            .asset_operations
            .iter()
            .map(asset_operation_view)
            .collect(),
        token_operations: block
            .token_operations
            .iter()
            .map(|operation| {
                let operation_id =
                    token_operation_id(operation).map_err(|error| (-32000, error.to_string()))?;
                Ok(token_operation_view(operation_id, operation))
            })
            .collect::<Result<Vec<_>, (i32, String)>>()?,
        mna_swap_operations: block.mna_swap_operations.iter().map(|op| json!({"operation_id":mna_swap_operation_id(op).map(|id|id.to_string()).unwrap_or_default(),"operation":op})).collect(),
        mna_reserve_operations: block.mna_reserve_operations.iter().map(|op| json!({"operation_id":mna_reserve_operation_id(op).map(|id|id.to_string()).unwrap_or_default(),"operation":op})).collect(),
        program_operations: block.program_operations.iter().map(|op| json!({"operation_id":wsc_crypto::program_operation_id(op).map(|id|id.to_string()).unwrap_or_default(),"operation":op})).collect(),
    }))
}

fn with_node<F>(context: &RpcContext, operation: F) -> Result<Value, (i32, String)>
where
    F: FnOnce(&Node) -> Result<Value, NodeError>,
{
    let node = context
        .node
        .lock()
        .map_err(|_| (-32000, "node lock poisoned".to_owned()))?;
    operation(&node).map_err(node_error)
}

fn with_node_mut<F>(context: &RpcContext, operation: F) -> Result<Value, (i32, String)>
where
    F: FnOnce(&mut Node) -> Result<Value, NodeError>,
{
    let mut node = context
        .node
        .lock()
        .map_err(|_| (-32000, "node lock poisoned".to_owned()))?;
    operation(&mut node).map_err(node_error)
}

fn parse_params<T: for<'de> Deserialize<'de>>(params: Value) -> Result<T, (i32, String)> {
    serde_json::from_value(params).map_err(|error| (-32602, format!("invalid params: {error}")))
}

fn parse_hash_string(value: &str, field: &str) -> Result<Hash, String> {
    let bytes = hex::decode(value).map_err(|_| format!("{field} must be 32-byte hex"))?;
    if bytes.len() != 32 {
        return Err(format!("{field} must be 32-byte hex"));
    }
    let mut output = [0u8; 32];
    output.copy_from_slice(&bytes);
    Ok(Hash(output))
}

fn parse_hash(value: &str) -> Result<Hash, (i32, String)> {
    let bytes = hex::decode(value).map_err(|_| (-32602, "hash must be 32-byte hex".to_owned()))?;
    if bytes.len() != 32 {
        return Err((-32602, "hash must be 32-byte hex".to_owned()));
    }
    let mut output = [0u8; 32];
    output.copy_from_slice(&bytes);
    Ok(Hash(output))
}

fn parse_address(value: &str) -> Result<Address, (i32, String)> {
    value
        .parse()
        .map_err(|error| (-32602, format!("invalid address: {error}")))
}

fn decode_fixed<const N: usize>(value: &str, field: &str) -> Result<[u8; N], String> {
    let bytes = hex::decode(value).map_err(|_| format!("{field} must be hex"))?;
    if bytes.len() != N {
        return Err(format!("{field} must be {N}-byte hex"));
    }
    let mut output = [0u8; N];
    output.copy_from_slice(&bytes);
    Ok(output)
}

fn node_error(error: NodeError) -> (i32, String) {
    let message = error.to_string();
    match error {
        NodeError::Storage(wsc_storage::StorageError::Missing) => (-32004, "not found".to_owned()),
        NodeError::State(_) => (-32010, message),
        NodeError::DuplicateTransaction => (-32011, message),
        NodeError::MempoolFull => (-32029, message),
        _ => (-32000, message),
    }
}

fn view_error((_, message): (i32, String)) -> NodeError {
    NodeError::Crypto(message)
}

fn error_response(id: Value, code: i32, message: &str) -> RpcResponse {
    RpcResponse {
        jsonrpc: "2.0",
        id,
        result: None,
        error: Some(RpcErrorObject {
            code,
            message: message.to_owned(),
        }),
    }
}

fn now_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;
    use wsc_crypto::KeyPair;

    #[test]
    fn login_message_is_deterministic_and_contains_chain_binding() {
        let key = KeyPair::generate().unwrap();
        let message = format_login_message("wallet.example", key.address(), "abcd", 10, 20);
        assert!(message.contains("Intertrain Login"));
        assert!(message.contains("Chain ID: worldstreet-devnet-1"));
        assert!(message.contains(&key.address().to_string()));
    }
}
