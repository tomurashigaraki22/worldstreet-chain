use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    net::SocketAddr,
    sync::{atomic::{AtomicU64, Ordering}, Arc, Mutex},
};
use thiserror::Error;
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    time::{timeout, Duration},
};
use wsc_consensus::{ValidatorSet, Vote, VoteSet};
use wsc_core::{canonical_decode, canonical_encode, Block, Hash, Transaction};
use wsc_crypto::KeyPair;
use wsc_node::{Node, NodeError};

const MAX_FRAME_BYTES: usize = 8 * 1024 * 1024;
const FRAME_READ_TIMEOUT: Duration = Duration::from_secs(3);

#[derive(Clone, Debug)]
pub struct NetworkConfig {
    pub chain_id: String,
    pub listen_addr: SocketAddr,
    pub peers: Vec<SocketAddr>,
    pub node_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum NetworkMessage {
    Hello {
        chain_id: String,
        genesis_hash: Hash,
        height: u64,
        node_id: String,
    },
    GetBlocks { from: u64, to: u64 },
    Blocks { blocks: Vec<Block> },
    GetMempool,
    Transactions { transactions: Vec<Transaction> },
    Transaction { transaction: Transaction },
    Vote { vote: Vote },
}

pub struct NetworkMetrics {
    pub peer_connections: AtomicU64,
    pub blocks_imported: AtomicU64,
    pub transactions_relayed: AtomicU64,
    pub votes_received: AtomicU64,
}

impl Default for NetworkMetrics {
    fn default() -> Self {
        Self {
            peer_connections: AtomicU64::new(0),
            blocks_imported: AtomicU64::new(0),
            transactions_relayed: AtomicU64::new(0),
            votes_received: AtomicU64::new(0),
        }
    }
}

#[derive(Debug, Error)]
pub enum NetworkError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("encoding error: {0}")]
    Encoding(String),
    #[error("invalid frame length")]
    InvalidFrameLength,
    #[error("peer frame read timed out")]
    Timeout,
    #[error("peer handshake failed: {0}")]
    Handshake(String),
    #[error("node error: {0}")]
    Node(String),
}

pub async fn run(node: Arc<Mutex<Node>>, config: NetworkConfig) -> Result<(), NetworkError> {
    run_with_validator(node, config, Arc::new(NetworkMetrics::default()), None).await
}

pub async fn run_with_metrics(
    node: Arc<Mutex<Node>>,
    config: NetworkConfig,
    metrics: Arc<NetworkMetrics>,
) -> Result<(), NetworkError> {
    run_with_validator(node, config, metrics, None).await
}

pub async fn run_with_validator(
    node: Arc<Mutex<Node>>,
    config: NetworkConfig,
    metrics: Arc<NetworkMetrics>,
    validator: Option<Arc<KeyPair>>,
) -> Result<(), NetworkError> {
    let listener = TcpListener::bind(config.listen_addr).await?;
    let vote_sets = Arc::new(Mutex::new(HashMap::<(u64, Hash, u64), VoteSet>::new()));
    for peer in config.peers.iter().copied() {
        let node = Arc::clone(&node);
        let config = config.clone();
        let vote_sets = Arc::clone(&vote_sets);
        let metrics = Arc::clone(&metrics);
        let validator = validator.clone();
        tokio::spawn(async move {
            loop {
                if let Ok(stream) = TcpStream::connect(peer).await {
                    let _ = serve_connection(
                        stream,
                        Arc::clone(&node),
                        config.clone(),
                        Arc::clone(&vote_sets),
                        Arc::clone(&metrics),
                        validator.clone(),
                        true,
                    ).await;
                }
                tokio::time::sleep(Duration::from_secs(2)).await;
            }
        });
    }
    loop {
        let (stream, _) = listener.accept().await?;
        let node = Arc::clone(&node);
        let config = config.clone();
        let vote_sets = Arc::clone(&vote_sets);
        let metrics = Arc::clone(&metrics);
        let validator = validator.clone();
        tokio::spawn(async move {
            let _ = serve_connection(stream, node, config, vote_sets, metrics, validator, false).await;
        });
    }
}

async fn serve_connection(
    mut stream: TcpStream,
    node: Arc<Mutex<Node>>,
    config: NetworkConfig,
    vote_sets: Arc<Mutex<HashMap<(u64, Hash, u64), VoteSet>>>,
    metrics: Arc<NetworkMetrics>,
    validator: Option<Arc<KeyPair>>,
    initiator: bool,
) -> Result<(), NetworkError> {
    let hello = local_hello(&node, &config)?;
    if initiator {
        write_message(&mut stream, &NetworkMessage::Hello {
            chain_id: hello.chain_id.clone(),
            genesis_hash: hello.genesis_hash,
            height: hello.height,
            node_id: hello.node_id.clone(),
        }).await?;
        validate_hello(read_message(&mut stream).await?, &hello)?;
        write_message(&mut stream, &NetworkMessage::GetBlocks { from: hello.height.saturating_add(1), to: hello.height.saturating_add(129) }).await?;
    } else {
        validate_hello(read_message(&mut stream).await?, &hello)?;
        write_message(&mut stream, &NetworkMessage::Hello {
            chain_id: hello.chain_id.clone(),
            genesis_hash: hello.genesis_hash,
            height: hello.height,
            node_id: hello.node_id.clone(),
        }).await?;
        write_message(&mut stream, &NetworkMessage::GetBlocks { from: hello.height.saturating_add(1), to: hello.height.saturating_add(129) }).await?;
    }

    metrics.peer_connections.fetch_add(1, Ordering::Relaxed);
    let latest_block = node
        .lock()
        .map_err(|_| NetworkError::Node("node lock poisoned".to_owned()))?
        .latest_block()
        .clone();
    write_message(&mut stream, &NetworkMessage::Blocks { blocks: vec![latest_block] }).await?;
    write_message(&mut stream, &NetworkMessage::GetMempool).await?;

    loop {
        let message = read_message(&mut stream).await?;
        match message {
            NetworkMessage::Hello { .. } => {}
            NetworkMessage::GetBlocks { from, to } => {
                let blocks = collect_blocks(&node, from, to)?;
                write_message(&mut stream, &NetworkMessage::Blocks { blocks }).await?;
            }
            NetworkMessage::GetMempool => {
                let transactions = node
                    .lock()
                    .map_err(|_| NetworkError::Node("node lock poisoned".to_owned()))?
                    .mempool_transactions();
                write_message(&mut stream, &NetworkMessage::Transactions { transactions }).await?;
            }
            NetworkMessage::Blocks { blocks } => {
                let votes = if let Some(validator) = validator.as_deref() {
                    let chain_id = node
                        .lock()
                        .map_err(|_| NetworkError::Node("node lock poisoned".to_owned()))?
                        .config()
                        .chain_id
                        .clone();
                    blocks
                        .iter()
                        .filter_map(|block| {
                            let hash = wsc_node::block_id_for(block).ok()?;
                            Vote::sign(&chain_id, block.header.height, 0, hash, validator).ok()
                        })
                        .collect::<Vec<_>>()
                } else {
                    Vec::new()
                };
                let request_from = if blocks.len() == 128 {
                    blocks.last().map(|block| block.header.height.saturating_add(1))
                } else {
                    None
                };
                for block in blocks {
                    if let Ok(mut node) = node.lock() {
                        node.import_block(block).map_err(node_error)?;
                        metrics.blocks_imported.fetch_add(1, Ordering::Relaxed);
                    }
                }
                if let Some(from) = request_from {
                    write_message(&mut stream, &NetworkMessage::GetBlocks { from, to: from.saturating_add(128) }).await?;
                }
                for vote in votes {
                    write_message(&mut stream, &NetworkMessage::Vote { vote }).await?;
                }
            }
            NetworkMessage::Transactions { transactions } => {
                for transaction in transactions {
                    if let Ok(mut node) = node.lock() {
                        if node.submit_transaction(transaction).is_ok() {
                            metrics.transactions_relayed.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                }
            }
            NetworkMessage::Transaction { transaction } => {
                if let Ok(mut node) = node.lock() {
                    node.submit_transaction(transaction).map_err(node_error)?;
                    metrics.transactions_relayed.fetch_add(1, Ordering::Relaxed);
                }
            }
            NetworkMessage::Vote { vote } => {
                metrics.votes_received.fetch_add(1, Ordering::Relaxed);
                let key = (vote.height, vote.block_hash, vote.round);
                let finalized = {
                    let node = node.lock().map_err(|_| NetworkError::Node("node lock poisoned".to_owned()))?;
                    let validator_set = ValidatorSet::new(node.validators().to_vec());
                    let mut vote_sets = vote_sets.lock().map_err(|_| NetworkError::Node("vote lock poisoned".to_owned()))?;
                    let vote_set = vote_sets.entry(key).or_insert_with(|| VoteSet::new(
                        vote.chain_id.clone(), vote.height, vote.round, vote.block_hash,
                    ));
                    vote_set.record(&validator_set, vote).map_err(|error| NetworkError::Node(error.to_string()))?;
                    vote_set.is_quorum(&validator_set).then(|| vote_set.clone())
                };
                if let Some(vote_set) = finalized {
                    let node = node.lock().map_err(|_| NetworkError::Node("node lock poisoned".to_owned()))?;
                    node.finalize_votes(&vote_set).map_err(node_error)?;
                }
            }
        }
    }
}

fn local_hello(node: &Arc<Mutex<Node>>, config: &NetworkConfig) -> Result<HelloFields, NetworkError> {
    let node = node.lock().map_err(|_| NetworkError::Node("node lock poisoned".to_owned()))?;
    if node.config().chain_id != config.chain_id {
        return Err(NetworkError::Handshake("configured chain ID does not match node".to_owned()));
    }
    Ok(HelloFields {
        chain_id: config.chain_id.clone(),
        genesis_hash: node.genesis_hash().map_err(node_error)?,
        height: node.latest_block().header.height,
        node_id: config.node_id.clone(),
    })
}

#[derive(Clone)]
struct HelloFields {
    chain_id: String,
    genesis_hash: Hash,
    height: u64,
    node_id: String,
}

fn validate_hello(message: NetworkMessage, expected: &HelloFields) -> Result<(), NetworkError> {
    match message {
        NetworkMessage::Hello { chain_id, genesis_hash, .. }
            if chain_id == expected.chain_id && genesis_hash == expected.genesis_hash => Ok(()),
        NetworkMessage::Hello { chain_id, genesis_hash, .. } => Err(NetworkError::Handshake(format!(
            "peer chain mismatch: chain_id={chain_id}, genesis_hash={genesis_hash}"
        ))),
        _ => Err(NetworkError::Handshake("first peer message must be Hello".to_owned())),
    }
}

fn collect_blocks(node: &Arc<Mutex<Node>>, from: u64, to: u64) -> Result<Vec<Block>, NetworkError> {
    if to < from || to.saturating_sub(from) > 128 {
        return Err(NetworkError::Handshake("block range must be ordered and at most 128 blocks".to_owned()));
    }
    let node = node.lock().map_err(|_| NetworkError::Node("node lock poisoned".to_owned()))?;
    let latest = node.latest_block().header.height;
    let end = to.min(latest);
    if from > end {
        return Ok(Vec::new());
    }
    let mut blocks = Vec::new();
    for height in from..=end {
        if let Ok(block) = node.get_block_by_height(height) {
            blocks.push(block);
        }
    }
    Ok(blocks)
}

async fn write_message<W: AsyncWrite + Unpin>(writer: &mut W, message: &NetworkMessage) -> Result<(), NetworkError> {
    let payload = canonical_encode(message).map_err(|error| NetworkError::Encoding(error.to_string()))?;
    if payload.len() > MAX_FRAME_BYTES {
        return Err(NetworkError::InvalidFrameLength);
    }
    let length = u32::try_from(payload.len()).map_err(|_| NetworkError::InvalidFrameLength)?;
    writer.write_all(&length.to_be_bytes()).await?;
    writer.write_all(&payload).await?;
    writer.flush().await?;
    Ok(())
}

async fn read_message<R: AsyncRead + Unpin>(reader: &mut R) -> Result<NetworkMessage, NetworkError> {
    let mut length_bytes = [0u8; 4];
    timeout(FRAME_READ_TIMEOUT, reader.read_exact(&mut length_bytes))
        .await
        .map_err(|_| NetworkError::Timeout)??;
    let length = u32::from_be_bytes(length_bytes) as usize;
    if length == 0 || length > MAX_FRAME_BYTES {
        return Err(NetworkError::InvalidFrameLength);
    }
    let mut payload = vec![0u8; length];
    timeout(FRAME_READ_TIMEOUT, reader.read_exact(&mut payload))
        .await
        .map_err(|_| NetworkError::Timeout)??;
    canonical_decode(&payload).map_err(|error| NetworkError::Encoding(error.to_string()))
}

fn node_error(error: NodeError) -> NetworkError {
    NetworkError::Node(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::duplex;

    #[tokio::test]
    async fn framed_messages_round_trip() {
        let (mut left, mut right) = duplex(4096);
        let message = NetworkMessage::GetBlocks { from: 4, to: 8 };
        let writer_message = message.clone();
        let writer = tokio::spawn(async move { write_message(&mut left, &writer_message).await });
        let received = read_message(&mut right).await.unwrap();
        writer.await.unwrap().unwrap();
        assert_eq!(received, message);
    }
}
