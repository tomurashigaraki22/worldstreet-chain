use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    fs,
    path::PathBuf,
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use thiserror::Error;
use wsc_consensus::{proposer_for_height, ConsensusError, ValidatorSet, VoteSet};
use wsc_core::{Address, Block, BlockHeader, GenesisConfig, Hash, PublicKey, Transaction, Validator, CHAIN_ID};
use wsc_crypto::{block_header_id, merkle_root, transaction_id, KeyPair};
use wsc_state::{State, StateError};
use wsc_storage::{StorageError, Store};

const NODE_CONFIG_FILE: &str = "node.json";
const GENESIS_FILE: &str = "genesis.json";
const DATA_DIR_NAME: &str = "db";
pub const MAX_MEMPOOL_TRANSACTIONS: usize = 10_000;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NodeConfig {
    pub chain_id: String,
    pub block_time_ms: u64,
    pub data_dir: PathBuf,
    pub genesis_path: PathBuf,
}

impl NodeConfig {
    pub fn for_data_dir(data_dir: impl Into<PathBuf>) -> Self {
        let data_dir = data_dir.into();
        Self {
            chain_id: CHAIN_ID.to_owned(),
            block_time_ms: 2000,
            genesis_path: data_dir.join(GENESIS_FILE),
            data_dir,
        }
    }

    pub fn config_path(&self) -> PathBuf {
        self.data_dir.join(NODE_CONFIG_FILE)
    }

    pub fn save(&self) -> Result<(), NodeError> {
        fs::create_dir_all(&self.data_dir)?;
        let json = serde_json::to_string_pretty(self)?;
        fs::write(self.config_path(), format!("{json}\n"))?;
        Ok(())
    }

    pub fn load(data_dir: impl Into<PathBuf>) -> Result<Self, NodeError> {
        let data_dir = data_dir.into();
        let value = fs::read_to_string(data_dir.join(NODE_CONFIG_FILE))?;
        Ok(serde_json::from_str(&value)?)
    }
}

#[derive(Debug, Error)]
pub enum NodeError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("state error: {0}")]
    State(#[from] StateError),
    #[error("storage error: {0}")]
    Storage(#[from] StorageError),
    #[error("crypto error: {0}")]
    Crypto(String),
    #[error("node configuration chain ID does not match genesis")]
    ConfigChainIdMismatch,
    #[error("mempool already contains this transaction")]
    DuplicateTransaction,
    #[error("mempool capacity has been reached")]
    MempoolFull,
    #[error("node is not the scheduled proposer")]
    NotProposer,
    #[error("persisted chain data is inconsistent: {0}")]
    CorruptChain(String),
    #[error("devnet faucet is disabled on this chain")]
    FaucetDisabled,
    #[error("invalid imported block: {0}")]
    InvalidBlock(String),
    #[error("consensus error: {0}")]
    Consensus(String),
}

#[derive(Default)]
pub struct Mempool {
    transactions: Vec<Transaction>,
    ids: BTreeSet<Hash>,
}

impl Mempool {
    pub fn len(&self) -> usize {
        self.transactions.len()
    }

    pub fn admit(&mut self, state: &State, transaction: Transaction) -> Result<Hash, NodeError> {
        if self.transactions.len() >= MAX_MEMPOOL_TRANSACTIONS {
            return Err(NodeError::MempoolFull);
        }
        let id = state.validate_transaction(&transaction)?;
        if !self.ids.insert(id) {
            return Err(NodeError::DuplicateTransaction);
        }
        self.transactions.push(transaction);
        Ok(id)
    }

    fn drain(&mut self) -> Vec<Transaction> {
        self.ids.clear();
        std::mem::take(&mut self.transactions)
    }
}

pub struct Node {
    config: NodeConfig,
    genesis: GenesisConfig,
    store: Store,
    state: State,
    latest_block: Block,
    mempool: Mempool,
}

impl Node {
    pub fn init(data_dir: impl Into<PathBuf>, chain_id: impl Into<String>) -> Result<NodeConfig, NodeError> {
        let data_dir = data_dir.into();
        let chain_id = chain_id.into();
        let config = NodeConfig {
            chain_id: chain_id.clone(),
            block_time_ms: 2000,
            genesis_path: data_dir.join(GENESIS_FILE),
            data_dir: data_dir.clone(),
        };
        let genesis = GenesisConfig {
            version: 1,
            chain_id,
            genesis_time: now_seconds(),
            block_time_ms: config.block_time_ms,
            initial_supply: 0,
            fee_minimum: 1,
            validators: vec![],
            allocations: vec![],
        };
        fs::create_dir_all(&data_dir)?;
        let genesis_json = serde_json::to_string_pretty(&genesis)?;
        fs::write(&config.genesis_path, format!("{genesis_json}\n"))?;
        config.save()?;
        Ok(config)
    }

    pub fn open(config: NodeConfig) -> Result<Self, NodeError> {
        let genesis_value = fs::read_to_string(&config.genesis_path)?;
        let genesis: GenesisConfig = serde_json::from_str(&genesis_value)?;
        if genesis.chain_id != config.chain_id {
            return Err(NodeError::ConfigChainIdMismatch);
        }
        let state = State::from_genesis(&genesis)?;
        let store = Store::open(config.data_dir.join(DATA_DIR_NAME))?;

        let genesis_block = Self::make_genesis_block(&genesis, &state)?;
        store.ensure_genesis(&genesis, &genesis_block, &state.snapshot())?;

        let latest_block = store.latest_block()?;
        let latest_state = store.latest_state()?;
        if latest_block.header.chain_id != genesis.chain_id {
            return Err(NodeError::CorruptChain("latest block has the wrong chain ID".to_owned()));
        }
        let restored_state = State::from_snapshot(latest_state);
        let restored_root = restored_state
            .state_root()
            .map_err(|error| NodeError::CorruptChain(error.to_string()))?;
        if restored_root != latest_block.header.state_root {
            return Err(NodeError::CorruptChain("latest state root does not match latest block".to_owned()));
        }
        Ok(Self {
            config,
            genesis,
            store,
            state: restored_state,
            latest_block,
            mempool: Mempool::default(),
        })
    }

    pub fn config(&self) -> &NodeConfig {
        &self.config
    }

    pub fn genesis(&self) -> &GenesisConfig {
        &self.genesis
    }

    pub fn latest_block(&self) -> &Block {
        &self.latest_block
    }

    pub fn state(&self) -> &State {
        &self.state
    }

    pub fn mempool_len(&self) -> usize {
        self.mempool.len()
    }

    pub fn mempool_transactions(&self) -> Vec<Transaction> {
        self.mempool.transactions.clone()
    }

    pub fn is_expected_proposer(&self, public_key: &PublicKey) -> bool {
        if self.genesis.validators.is_empty() {
            return true;
        }
        proposer_for_height(
            &self.genesis.validators,
            self.latest_block.header.height.saturating_add(1),
            0,
        )
        .map(|validator| validator.public_key == *public_key)
        .unwrap_or(false)
    }

    pub fn latest_hash(&self) -> Result<Hash, NodeError> {
        block_id_for(&self.latest_block)
    }

    pub fn genesis_hash(&self) -> Result<Hash, NodeError> {
        self.store.genesis_hash().map_err(NodeError::from)
    }

    pub fn finalized_height(&self) -> Result<u64, NodeError> {
        self.store.finalized_height().map_err(NodeError::from)
    }

    pub fn finalized_hash(&self) -> Result<Hash, NodeError> {
        self.store.finalized_hash().map_err(NodeError::from)
    }

    pub fn get_block(&self, hash: Hash) -> Result<Block, NodeError> {
        self.store.get_block(hash).map_err(NodeError::from)
    }

    pub fn get_block_by_height(&self, height: u64) -> Result<Block, NodeError> {
        self.store.get_block_by_height(height).map_err(NodeError::from)
    }

    pub fn get_transaction(&self, hash: Hash) -> Result<Transaction, NodeError> {
        self.store.get_transaction(hash).map_err(NodeError::from)
    }

    pub fn account(&self, address: &Address) -> (u128, u64) {
        (self.state.balance_of(address), self.state.nonce_of(address))
    }

    pub fn validators(&self) -> &[Validator] {
        &self.genesis.validators
    }

    pub fn submit_transaction(&mut self, transaction: Transaction) -> Result<Hash, NodeError> {
        self.mempool.admit(&self.state, transaction)
    }

    pub fn produce_block(&mut self) -> Result<Block, NodeError> {
        self.produce_block_with_proposer(None)
    }

    pub fn produce_block_with_proposer(&mut self, proposer: Option<&KeyPair>) -> Result<Block, NodeError> {
        if let Some(proposer) = proposer {
            if !self.is_expected_proposer(&proposer.public_key()) {
                return Err(NodeError::NotProposer);
            }
        }
        let pending = self.mempool.drain();
        let mut next_state = self.state.clone();
        let mut accepted = Vec::with_capacity(pending.len());

        for transaction in pending {
            if next_state.apply_transaction(&transaction).is_ok() {
                accepted.push(transaction);
            }
        }

        self.commit_block(next_state, accepted, proposer)
    }

    pub fn faucet(&mut self, address: wsc_core::Address, amount: u128) -> Result<Block, NodeError> {
        if !self.genesis.chain_id.contains("devnet") {
            return Err(NodeError::FaucetDisabled);
        }
        let mut next_state = self.state.clone();
        next_state.credit_devnet(address, amount)?;
        self.commit_block(next_state, vec![], None)
    }

    pub fn import_block(&mut self, block: Block) -> Result<Hash, NodeError> {
        if block.header.version != 1 {
            return Err(NodeError::InvalidBlock("unsupported block version".to_owned()));
        }
        let block_hash = block_id_for(&block)?;
        if self.store.get_block(block_hash).is_ok() {
            return Ok(block_hash);
        }
        if block.header.chain_id != self.genesis.chain_id {
            return Err(NodeError::InvalidBlock("wrong chain ID".to_owned()));
        }
        let expected_height = self.latest_block.header.height + 1;
        if block.header.height != expected_height {
            return Err(NodeError::InvalidBlock(format!(
                "expected height {expected_height}, got {}",
                block.header.height
            )));
        }
        let expected_parent = self.latest_hash()?;
        if block.header.parent_hash != expected_parent {
            return Err(NodeError::InvalidBlock("wrong parent hash".to_owned()));
        }
        match (block.header.proposer, block.header.proposer_signature) {
            (Some(proposer), Some(signature)) => {
                if !self.genesis.validators.iter().any(|validator| validator.public_key == proposer) {
                    return Err(NodeError::InvalidBlock("unknown block proposer".to_owned()));
                }
                let expected = proposer_for_height(&self.genesis.validators, block.header.height, 0)
                    .map(|validator| validator.public_key == proposer)
                    .unwrap_or(false);
                if !expected {
                    return Err(NodeError::InvalidBlock("block proposer is not scheduled for this height".to_owned()));
                }
                let signing_bytes = block
                    .header
                    .signing_bytes()
                    .map_err(|error| NodeError::Crypto(error.to_string()))?;
                if !KeyPair::verify(&proposer, &signing_bytes, &signature) {
                    return Err(NodeError::InvalidBlock("invalid proposer signature".to_owned()));
                }
            }
            (None, None) if self.genesis.validators.is_empty() => {}
            (None, None) => return Err(NodeError::InvalidBlock("validator block requires proposer signature".to_owned())),
            _ => return Err(NodeError::InvalidBlock("proposer and signature must be paired".to_owned())),
        }

        let mut next_state = self.state.clone();
        let mut ids = Vec::with_capacity(block.transactions.len());
        for transaction in &block.transactions {
            ids.push(next_state.apply_transaction(transaction)?);
        }
        if merkle_root(&ids) != block.header.transaction_root {
            return Err(NodeError::InvalidBlock("transaction root mismatch".to_owned()));
        }
        if next_state.state_root()? != block.header.state_root {
            return Err(NodeError::InvalidBlock("state root mismatch".to_owned()));
        }
        self.store.commit(block_hash, &block, &next_state.snapshot())?;
        self.state = next_state;
        self.latest_block = block;
        Ok(block_hash)
    }

    pub fn finalize_votes(&self, vote_set: &VoteSet) -> Result<u64, NodeError> {
        let validator_set = ValidatorSet::new(self.genesis.validators.clone());
        let latest_hash = self.latest_hash()?;
        if vote_set.height != self.latest_block.header.height {
            return Err(NodeError::InvalidBlock("finality vote height is not latest".to_owned()));
        }
        if vote_set.block_hash != latest_hash {
            return Err(NodeError::InvalidBlock("finality votes target another block".to_owned()));
        }
        if !vote_set.is_quorum(&validator_set) {
            return Err(NodeError::Consensus(ConsensusError::NoQuorum.to_string()));
        }
        for vote in vote_set.votes() {
            validator_set
                .validate_vote(vote, &self.genesis.chain_id, vote_set.height, vote_set.round, latest_hash)
                .map_err(|error| NodeError::Consensus(error.to_string()))?;
        }
        self.store.mark_finalized(vote_set.height, latest_hash)?;
        Ok(vote_set.height)
    }

    fn commit_block(&mut self, next_state: State, transactions: Vec<Transaction>, proposer: Option<&KeyPair>) -> Result<Block, NodeError> {
        let ids = transactions
            .iter()
            .map(transaction_id)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| NodeError::Crypto(error.to_string()))?;
        let mut header = BlockHeader {
            version: 1,
            chain_id: self.genesis.chain_id.clone(),
            height: self.latest_block.header.height + 1,
            parent_hash: block_header_id(&self.latest_block.header)
                .map_err(|error| NodeError::Crypto(error.to_string()))?,
            timestamp: now_seconds(),
            transaction_root: merkle_root(&ids),
            state_root: next_state.state_root()?,
            proposer: proposer.map(|key| key.public_key()),
            proposer_signature: None,
        };
        if let Some(proposer) = proposer {
            let signing_bytes = header
                .signing_bytes()
                .map_err(|error| NodeError::Crypto(error.to_string()))?;
            header.proposer_signature = Some(proposer.sign(&signing_bytes));
        }
        let block = Block {
            header,
            transactions,
        };
        let block_hash = block_header_id(&block.header)
            .map_err(|error| NodeError::Crypto(error.to_string()))?;
        self.store.commit(block_hash, &block, &next_state.snapshot())?;
        self.state = next_state;
        self.latest_block = block.clone();
        Ok(block)
    }

    pub fn run(&mut self, once: bool) -> Result<(), NodeError> {
        self.run_with_proposer(once, None)
    }

    pub fn run_with_proposer(&mut self, once: bool, proposer: Option<&KeyPair>) -> Result<(), NodeError> {
        if once {
            self.produce_block_with_proposer(proposer)?;
            return Ok(());
        }

        let delay = Duration::from_millis(self.config.block_time_ms);
        loop {
            self.produce_block_with_proposer(proposer)?;
            thread::sleep(delay);
        }
    }

    fn make_genesis_block(genesis: &GenesisConfig, state: &State) -> Result<Block, NodeError> {
        Ok(Block {
            header: BlockHeader {
                version: 1,
                chain_id: genesis.chain_id.clone(),
                height: 0,
                parent_hash: Hash::ZERO,
                timestamp: genesis.genesis_time,
                transaction_root: merkle_root(&[]),
                state_root: state.state_root()?,
                proposer: None,
                proposer_signature: None,
            },
            transactions: vec![],
        })
    }
}

pub fn transaction_id_for(transaction: &Transaction) -> Result<Hash, NodeError> {
    transaction_id(transaction).map_err(|error| NodeError::Crypto(error.to_string()))
}

pub fn block_id_for(block: &Block) -> Result<Hash, NodeError> {
    block_header_id(&block.header).map_err(|error| NodeError::Crypto(error.to_string()))
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
    use std::time::{SystemTime, UNIX_EPOCH};
    use wsc_consensus::{ValidatorSet, Vote, VoteSet};
    use wsc_core::{GenesisConfig, GenesisAllocation, UnsignedTransaction};
    use wsc_crypto::KeyPair;

    fn temp_path() -> PathBuf {
        let value = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("wsc-node-{value}"))
    }

    #[test]
    fn node_initializes_and_produces_a_persistent_block() {
        let path = temp_path();
        let config = Node::init(&path, CHAIN_ID).unwrap();
        let mut node = Node::open(config.clone()).unwrap();
        assert_eq!(node.latest_block().header.height, 0);
        let block = node.produce_block().unwrap();
        assert_eq!(block.header.height, 1);

        let reopened = Node::open(config).unwrap();
        assert_eq!(reopened.latest_block().header.height, 1);
        assert_eq!(reopened.latest_block().header.parent_hash, block.header.parent_hash);
        std::fs::remove_dir_all(path).ok();
    }

    #[test]
    fn node_admits_and_finalizes_a_signed_transfer() {
        let path = temp_path();
        let config = Node::init(&path, CHAIN_ID).unwrap();
        let mut genesis: GenesisConfig =
            serde_json::from_str(&std::fs::read_to_string(&config.genesis_path).unwrap()).unwrap();
        let sender = KeyPair::generate().unwrap();
        let receiver = KeyPair::generate().unwrap();
        genesis.initial_supply = 100;
        genesis.allocations = vec![GenesisAllocation {
            address: sender.address(),
            balance: 100,
        }];
        std::fs::write(
            &config.genesis_path,
            serde_json::to_string_pretty(&genesis).unwrap(),
        )
        .unwrap();

        let mut node = Node::open(config).unwrap();
        let unsigned = UnsignedTransaction {
            version: 1,
            chain_id: CHAIN_ID.to_owned(),
            nonce: 0,
            from: sender.address(),
            to: receiver.address(),
            amount: 10,
            fee: 1,
            public_key: sender.public_key(),
            memo: String::new(),
        };
        let transaction = Transaction {
            signature: sender.sign(&unsigned.signing_bytes().unwrap()),
            unsigned,
        };
        node.submit_transaction(transaction).unwrap();
        let block = node.produce_block().unwrap();
        assert_eq!(block.transactions.len(), 1);
        assert_eq!(node.state().balance_of(&receiver.address()), 10);
        std::fs::remove_dir_all(path).ok();
    }

    #[test]
    fn signed_block_import_and_finality_are_verified() {
        let source_path = temp_path();
        let target_path = temp_path();
        let source_config = Node::init(&source_path, CHAIN_ID).unwrap();
        let target_config = Node::init(&target_path, CHAIN_ID).unwrap();
        let validator_key = KeyPair::generate().unwrap();
        let validator = Validator { name: "devnet-validator".to_owned(), public_key: validator_key.public_key() };
        let mut genesis: GenesisConfig = serde_json::from_str(
            &std::fs::read_to_string(&source_config.genesis_path).unwrap(),
        ).unwrap();
        genesis.validators = vec![validator.clone()];
        let genesis_json = serde_json::to_string_pretty(&genesis).unwrap();
        std::fs::write(&source_config.genesis_path, &genesis_json).unwrap();
        std::fs::write(&target_config.genesis_path, &genesis_json).unwrap();

        let mut source = Node::open(source_config).unwrap();
        let mut target = Node::open(target_config).unwrap();
        let block = source.produce_block_with_proposer(Some(&validator_key)).unwrap();
        let block_hash = block_id_for(&block).unwrap();
        assert_eq!(target.import_block(block).unwrap(), block_hash);

        let set = ValidatorSet::new([validator]);
        let mut votes = VoteSet::new(CHAIN_ID, 1, 0, block_hash);
        let vote = Vote::sign(CHAIN_ID, 1, 0, block_hash, &validator_key).unwrap();
        assert!(votes.record(&set, vote).unwrap());
        assert_eq!(target.finalize_votes(&votes).unwrap(), 1);
        assert_eq!(target.finalized_hash().unwrap(), block_hash);

        std::fs::remove_dir_all(source_path).ok();
        std::fs::remove_dir_all(target_path).ok();
    }
}
