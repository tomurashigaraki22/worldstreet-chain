use axum::{
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use getrandom::fill;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    net::SocketAddr,
    sync::{atomic::{AtomicU64, Ordering}, Arc, Mutex},
    time::{Instant, SystemTime, UNIX_EPOCH},
};
use thiserror::Error;
use tokio::net::TcpListener;
use wsc_core::{Address, Block, Hash, PublicKey, Signature, Transaction, UnsignedTransaction, CHAIN_ID};
use wsc_crypto::{address_from_public_key, block_header_id, KeyPair};
use wsc_network::{NetworkConfig, NetworkMetrics};
use wsc_node::{Node, NodeError};

const LOGIN_TTL_SECONDS: u64 = 300;
const MAX_PENDING_CHALLENGES: usize = 10_000;
const MAX_RPC_REQUESTS_PER_SECOND: u64 = 1_000;

#[derive(Clone)]
pub struct RpcContext {
    pub node: Arc<Mutex<Node>>,
    challenges: Arc<Mutex<HashMap<String, LoginChallenge>>>,
    pub metrics: Arc<Metrics>,
    pub network_metrics: Arc<NetworkMetrics>,
}

impl RpcContext {
    pub fn new(node: Node) -> Arc<Self> {
        Arc::new(Self {
            node: Arc::new(Mutex::new(node)),
            challenges: Arc::new(Mutex::new(HashMap::new())),
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
        let Ok(mut window) = self.rate_window.lock() else { return false; };
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
struct BroadcastParams {
    transaction: RpcTransactionInput,
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
        let from = self.unsigned.from.parse::<Address>().map_err(|error| format!("invalid from address: {error}"))?;
        let to = self.unsigned.to.parse::<Address>().map_err(|error| format!("invalid to address: {error}"))?;
        let public_key = decode_fixed::<32>(&self.unsigned.public_key, "public_key")?;
        let signature = decode_fixed::<64>(&self.signature, "signature")?;
        let amount = self.unsigned.amount.parse::<u128>().map_err(|_| "amount must be an unsigned integer string".to_owned())?;
        let fee = self.unsigned.fee.parse::<u128>().map_err(|_| "fee must be an unsigned integer string".to_owned())?;
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

pub fn run_with_proposer(node: Node, bind: SocketAddr, proposer: Option<KeyPair>) -> Result<(), RpcError> {
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
            if let Err(error) = wsc_network::run_with_validator(network_node, network, network_metrics, validator).await {
                eprintln!("{}", json!({"event":"network_stopped", "error":error.to_string()}));
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
                if let Some(proposer) = proposer.as_deref()
                    && !node.is_expected_proposer(&proposer.public_key())
                {
                    continue;
                }
                let result = node.produce_block_with_proposer(proposer.as_deref());
                if result.is_ok() {
                    producer_context.metrics.blocks_produced.fetch_add(1, Ordering::Relaxed);
                } else if let Err(error) = result {
                    eprintln!("{}", json!({"event":"block_production_failed", "error":error.to_string()}));
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
        Err(_) => (StatusCode::SERVICE_UNAVAILABLE, Json(json!({"status":"unavailable"}))),
    }
}

async fn handle_metrics(State(context): State<Arc<RpcContext>>) -> impl IntoResponse {
    let (height, finalized_height) = context
        .node
        .lock()
        .map(|node| (node.latest_block().header.height, node.finalized_height().unwrap_or(0)))
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
    (StatusCode::OK, [("content-type", "text/plain; version=0.0.4")], body)
}

async fn handle_rpc(State(context): State<Arc<RpcContext>>, Json(request): Json<RpcRequest>) -> Json<RpcResponse> {
    if !context.metrics.allow_request() {
        context.metrics.rpc_errors.fetch_add(1, Ordering::Relaxed);
        return Json(error_response(request.id, -32029, "RPC rate limit exceeded"));
    }
    context.metrics.rpc_requests.fetch_add(1, Ordering::Relaxed);
    let response = if request.jsonrpc != "2.0" {
        error_response(request.id, -32600, "jsonrpc must be 2.0")
    } else {
        dispatch(&context, &request.method, request.params).await
            .map_or_else(|(code, message)| {
                context.metrics.rpc_errors.fetch_add(1, Ordering::Relaxed);
                error_response(request.id, code, &message)
            }, |result| RpcResponse {
                jsonrpc: "2.0",
                id: request.id,
                result: Some(result),
                error: None,
            })
    };
    Json(response)
}

async fn dispatch(context: &RpcContext, method: &str, params: Value) -> Result<Value, (i32, String)> {
    match method {
        "node_status" | "chain_info" => chain_info(context),
        "block_latest" => with_node(context, |node| block_view(node.latest_block()).map_err(view_error)),
        "block_get" => {
            let params: HashParams = parse_params(params)?;
            let hash = parse_hash(&params.hash)?;
            with_node(context, |node| node.get_block(hash).and_then(|block| block_view(&block).map_err(view_error)))
        }
        "block_get_by_height" => {
            let params: HeightParams = parse_params(params)?;
            with_node(context, |node| node.get_block_by_height(params.height).and_then(|block| block_view(&block).map_err(view_error)))
        }
        "transaction_get" => {
            let params: HashParams = parse_params(params)?;
            let hash = parse_hash(&params.hash)?;
            with_node(context, |node| {
                node.get_transaction(hash).map(|transaction| json!({
                    "hash": hash.to_string(),
                    "transaction": transaction_view(&transaction),
                }))
            })
        }
        "transaction_broadcast" => {
            let params: BroadcastParams = parse_params(params)?;
            with_node_mut(context, |node| {
                let transaction = params.transaction.into_core().map_err(|error| NodeError::Crypto(error))?;
                node.submit_transaction(transaction).map(|hash| {
                    context.metrics.transactions_broadcast.fetch_add(1, Ordering::Relaxed);
                    json!({
                    "hash": hash.to_string(),
                    "status": "accepted",
                    })
                })
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
    let chain_id = context.node.lock().map_err(|_| (-32000, "node lock poisoned".to_owned()))?.config().chain_id.clone();
    let challenge = LoginChallenge { chain_id: chain_id.clone(), address, domain: params.domain.clone(), issued_at, expires_at };
    let message = format_login_message_for_chain(&chain_id, &params.domain, address, &nonce, issued_at, expires_at);
    let mut challenges = context.challenges.lock().map_err(|_| (-32000, "challenge lock poisoned".to_owned()))?;
    if challenges.len() >= MAX_PENDING_CHALLENGES {
        return Err((-32029, "too many pending login challenges".to_owned()));
    }
    challenges.insert(nonce.clone(), challenge);
    Ok(json!({ "nonce": nonce, "message": message, "issued_at": issued_at, "expires_at": expires_at }))
}

fn verify_challenge(context: &RpcContext, params: Value) -> Result<Value, (i32, String)> {
    let params: VerifyParams = parse_params(params)?;
    let address = parse_address(&params.address)?;
    let public_key_bytes = hex::decode(&params.public_key).map_err(|_| (-32602, "public_key must be hex".to_owned()))?;
    let signature_bytes = hex::decode(&params.signature).map_err(|_| (-32602, "signature must be hex".to_owned()))?;
    if public_key_bytes.len() != 32 || signature_bytes.len() != 64 {
        return Err((-32602, "public_key or signature has the wrong length".to_owned()));
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
    let challenge = context.challenges.lock().map_err(|_| (-32000, "challenge lock poisoned".to_owned()))?.get(&params.nonce).cloned()
        .ok_or((-32004, "challenge not found or already used".to_owned()))?;
    if challenge.address != address || challenge.domain != params.domain || now_seconds() > challenge.expires_at {
        return Err((-32004, "challenge is invalid or expired".to_owned()));
    }
    let message = format_login_message_for_chain(&challenge.chain_id, &params.domain, address, &params.nonce, challenge.issued_at, challenge.expires_at);
    if !KeyPair::verify(&public_key, message.as_bytes(), &signature) {
        return Err((-32001, "invalid login signature".to_owned()));
    }
    context.challenges.lock().map_err(|_| (-32000, "challenge lock poisoned".to_owned()))?.remove(&params.nonce);
    let mut token_bytes = [0u8; 32];
    fill(&mut token_bytes).map_err(|error| (-32000, error.to_string()))?;
    Ok(json!({ "authenticated": true, "address": address.to_string(), "session_token": hex::encode(token_bytes) }))
}

pub fn format_login_message(domain: &str, address: Address, nonce: &str, issued_at: u64, expires_at: u64) -> String {
    format_login_message_for_chain(CHAIN_ID, domain, address, nonce, issued_at, expires_at)
}

fn format_login_message_for_chain(chain_id: &str, domain: &str, address: Address, nonce: &str, issued_at: u64, expires_at: u64) -> String {
    format!("Worldstreet Chain Login\n\nDomain: {domain}\nChain ID: {chain_id}\nAddress: {address}\nNonce: {nonce}\nIssued At: {issued_at}\nExpires At: {expires_at}")
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
        proposer_signature: block.header.proposer_signature.map(|signature| hex::encode(signature.0)),
        transactions: block.transactions.iter().map(transaction_view).collect(),
    }))
}

fn with_node<F>(context: &RpcContext, operation: F) -> Result<Value, (i32, String)>
where
    F: FnOnce(&Node) -> Result<Value, NodeError>,
{
    let node = context.node.lock().map_err(|_| (-32000, "node lock poisoned".to_owned()))?;
    operation(&node).map_err(node_error)
}

fn with_node_mut<F>(context: &RpcContext, operation: F) -> Result<Value, (i32, String)>
where
    F: FnOnce(&mut Node) -> Result<Value, NodeError>,
{
    let mut node = context.node.lock().map_err(|_| (-32000, "node lock poisoned".to_owned()))?;
    operation(&mut node).map_err(node_error)
}

fn parse_params<T: for<'de> Deserialize<'de>>(params: Value) -> Result<T, (i32, String)> {
    serde_json::from_value(params).map_err(|error| (-32602, format!("invalid params: {error}")))
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
    value.parse().map_err(|error| (-32602, format!("invalid address: {error}")))
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
    RpcResponse { jsonrpc: "2.0", id, result: None, error: Some(RpcErrorObject { code, message: message.to_owned() }) }
}

fn now_seconds() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;
    use wsc_crypto::KeyPair;

    #[test]
    fn login_message_is_deterministic_and_contains_chain_binding() {
        let key = KeyPair::generate().unwrap();
        let message = format_login_message("wallet.example", key.address(), "abcd", 10, 20);
        assert!(message.contains("Worldstreet Chain Login"));
        assert!(message.contains("Chain ID: worldstreet-devnet-1"));
        assert!(message.contains(&key.address().to_string()));
    }
}
