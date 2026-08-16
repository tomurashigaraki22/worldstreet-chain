use serde::{Deserialize, Serialize};
use sled::{Db, Tree};
use std::collections::{BTreeMap, BTreeSet};
use thiserror::Error;
use wsc_core::{
    canonical_decode, canonical_encode, Block, BlockHeader, GenesisConfig, Hash, Transaction,
    CHAIN_ID,
};
use wsc_crypto::{block_header_id, transaction_id};
use wsc_state::StateSnapshot;

const META_CHAIN_ID: &[u8] = b"chain_id";
const META_GENESIS_HASH: &[u8] = b"genesis_hash";
const META_LATEST_HEIGHT: &[u8] = b"latest_height";
const META_LATEST_HASH: &[u8] = b"latest_hash";
const META_FINALIZED_HEIGHT: &[u8] = b"finalized_height";
const META_FINALIZED_HASH: &[u8] = b"finalized_hash";
const STATE_LATEST: &[u8] = b"latest";

#[derive(Clone, Debug, Serialize, Deserialize)]
struct LegacyBlock {
    header: BlockHeader,
    transactions: Vec<Transaction>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct PreProgramBlock {
    header: BlockHeader,
    transactions: Vec<Transaction>,
    asset_operations: Vec<wsc_core::AssetOperation>,
    token_operations: Vec<wsc_core::TokenOperation>,
    mna_swap_operations: Vec<wsc_core::MnaSwapOperation>,
    mna_reserve_operations: Vec<wsc_core::MnaReserveOperation>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct LegacyStateSnapshot {
    chain_id: String,
    fee_minimum: u128,
    fee_pool: u128,
    accounts: BTreeMap<wsc_core::Address, wsc_state::Account>,
}

#[derive(Clone, Debug, Deserialize)]
struct LegacyAssetStateSnapshot {
    chain_id: String,
    fee_minimum: u128,
    fee_pool: u128,
    accounts: BTreeMap<wsc_core::Address, wsc_state::Account>,
    asset_balances: BTreeMap<wsc_core::Address, BTreeMap<String, u128>>,
    processed_asset_operations: BTreeSet<wsc_core::Hash>,
    asset_operation_records: BTreeMap<wsc_core::Hash, wsc_core::AssetOperation>,
}

#[derive(Clone, Debug, Deserialize)]
struct PreProgramStateSnapshot {
    chain_id: String,
    fee_minimum: u128,
    fee_pool: u128,
    accounts: BTreeMap<wsc_core::Address, wsc_state::Account>,
    asset_balances: BTreeMap<wsc_core::Address, BTreeMap<String, u128>>,
    processed_asset_operations: BTreeSet<wsc_core::Hash>,
    asset_operation_records: BTreeMap<wsc_core::Hash, wsc_core::AssetOperation>,
    token_definitions: BTreeMap<wsc_core::Hash, wsc_core::TokenDefinition>,
    processed_token_operations: BTreeSet<wsc_core::Hash>,
    token_operation_records: BTreeMap<wsc_core::Hash, wsc_core::TokenOperation>,
    frozen_token_accounts: BTreeSet<(wsc_core::Hash, wsc_core::Address)>,
    mna_reserve_ledger: wsc_core::MnaReserveLedger,
    processed_mna_swap_operations: BTreeSet<wsc_core::Hash>,
    mna_swap_operation_records: BTreeMap<wsc_core::Hash, wsc_core::MnaSwapOperation>,
    processed_mna_reserve_operations: BTreeSet<wsc_core::Hash>,
    mna_reserve_operation_records: BTreeMap<wsc_core::Hash, wsc_core::MnaReserveOperation>,
}

fn decode_block(value: &[u8]) -> Result<Block, StorageError> {
    canonical_decode(value)
        .or_else(|_| {
            canonical_decode::<PreProgramBlock>(value).map(|legacy| Block {
                header: legacy.header,
                transactions: legacy.transactions,
                asset_operations: legacy.asset_operations,
                token_operations: legacy.token_operations,
                mna_swap_operations: legacy.mna_swap_operations,
                mna_reserve_operations: legacy.mna_reserve_operations,
                program_operations: vec![],
            })
        })
        .or_else(|_| {
            canonical_decode::<LegacyBlock>(value).map(|legacy| Block {
                header: legacy.header,
                transactions: legacy.transactions,
                asset_operations: vec![],
                token_operations: vec![],
                mna_swap_operations: vec![],
                mna_reserve_operations: vec![],
                program_operations: vec![],
            })
        })
        .map_err(|_| StorageError::Corrupt)
}

#[derive(Debug, Error)]
pub enum StorageError {
    #[error("database error: {0}")]
    Database(#[from] sled::Error),
    #[error("encoding error: {0}")]
    Encoding(String),
    #[error("stored value is missing")]
    Missing,
    #[error("stored value is corrupt")]
    Corrupt,
    #[error("chain ID does not match stored chain")]
    ChainIdMismatch,
    #[error("genesis hash does not match stored chain")]
    GenesisMismatch,
}

pub struct Store {
    db: Db,
    blocks: Tree,
    heights: Tree,
    transactions: Tree,
    state: Tree,
    meta: Tree,
}

impl Store {
    pub fn open(path: impl AsRef<std::path::Path>) -> Result<Self, StorageError> {
        let db = sled::open(path)?;
        Ok(Self {
            blocks: db.open_tree("blocks")?,
            heights: db.open_tree("heights")?,
            transactions: db.open_tree("transactions")?,
            state: db.open_tree("state")?,
            meta: db.open_tree("meta")?,
            db,
        })
    }

    pub fn ensure_genesis(
        &self,
        genesis: &GenesisConfig,
        genesis_block: &Block,
        genesis_state: &StateSnapshot,
    ) -> Result<Hash, StorageError> {
        let block_hash = block_header_id(&genesis_block.header)
            .map_err(|error| StorageError::Encoding(error.to_string()))?;
        if let Some(chain_id) = self.meta.get(META_CHAIN_ID)? {
            let stored_chain_id =
                String::from_utf8(chain_id.to_vec()).map_err(|_| StorageError::Corrupt)?;
            if stored_chain_id != genesis.chain_id {
                return Err(StorageError::ChainIdMismatch);
            }
            let stored_hash = self
                .meta
                .get(META_GENESIS_HASH)?
                .ok_or(StorageError::Corrupt)?;
            if stored_hash.as_ref() != &block_hash.as_bytes()[..] {
                return Err(StorageError::GenesisMismatch);
            }
            if self.meta.get(META_FINALIZED_HEIGHT)?.is_none() {
                let zero_height = 0u64.to_be_bytes();
                self.meta.insert(META_FINALIZED_HEIGHT, &zero_height[..])?;
                self.meta
                    .insert(META_FINALIZED_HASH, &block_hash.as_bytes()[..])?;
                self.db.flush()?;
            }
            return Ok(block_hash);
        }

        self.meta
            .insert(META_CHAIN_ID, genesis.chain_id.as_bytes())?;
        self.meta
            .insert(META_GENESIS_HASH, &block_hash.as_bytes()[..])?;
        let zero_height = 0u64.to_be_bytes();
        self.meta.insert(META_FINALIZED_HEIGHT, &zero_height[..])?;
        self.meta
            .insert(META_FINALIZED_HASH, &block_hash.as_bytes()[..])?;
        self.commit(block_hash, genesis_block, genesis_state)?;
        self.db.flush()?;
        Ok(block_hash)
    }

    pub fn chain_id(&self) -> Result<String, StorageError> {
        let value = self.meta.get(META_CHAIN_ID)?.ok_or(StorageError::Missing)?;
        String::from_utf8(value.to_vec()).map_err(|_| StorageError::Corrupt)
    }

    pub fn genesis_hash(&self) -> Result<Hash, StorageError> {
        let value = self
            .meta
            .get(META_GENESIS_HASH)?
            .ok_or(StorageError::Missing)?;
        decode_hash(&value)
    }

    pub fn latest_height(&self) -> Result<u64, StorageError> {
        let value = self
            .meta
            .get(META_LATEST_HEIGHT)?
            .ok_or(StorageError::Missing)?;
        if value.len() != 8 {
            return Err(StorageError::Corrupt);
        }
        let mut bytes = [0u8; 8];
        bytes.copy_from_slice(&value);
        Ok(u64::from_be_bytes(bytes))
    }

    pub fn latest_block(&self) -> Result<Block, StorageError> {
        let hash = self.latest_hash()?;
        self.get_block(hash)
    }

    pub fn latest_hash(&self) -> Result<Hash, StorageError> {
        let value = self
            .meta
            .get(META_LATEST_HASH)?
            .ok_or(StorageError::Missing)?;
        decode_hash(&value)
    }

    pub fn finalized_height(&self) -> Result<u64, StorageError> {
        let value = self
            .meta
            .get(META_FINALIZED_HEIGHT)?
            .ok_or(StorageError::Missing)?;
        if value.len() != 8 {
            return Err(StorageError::Corrupt);
        }
        let mut bytes = [0u8; 8];
        bytes.copy_from_slice(&value);
        Ok(u64::from_be_bytes(bytes))
    }

    pub fn finalized_hash(&self) -> Result<Hash, StorageError> {
        let value = self
            .meta
            .get(META_FINALIZED_HASH)?
            .ok_or(StorageError::Missing)?;
        decode_hash(&value)
    }

    pub fn mark_finalized(&self, height: u64, hash: Hash) -> Result<(), StorageError> {
        let block = self.get_block(hash)?;
        if block.header.height != height {
            return Err(StorageError::Corrupt);
        }
        let current = self.finalized_height()?;
        if height < current {
            return Ok(());
        }
        let height_bytes = height.to_be_bytes();
        self.meta.insert(META_FINALIZED_HEIGHT, &height_bytes[..])?;
        self.meta
            .insert(META_FINALIZED_HASH, &hash.as_bytes()[..])?;
        self.db.flush()?;
        Ok(())
    }

    pub fn get_block(&self, hash: Hash) -> Result<Block, StorageError> {
        let value = self
            .blocks
            .get(&hash.as_bytes()[..])?
            .ok_or(StorageError::Missing)?;
        decode_block(&value)
    }

    pub fn get_block_by_height(&self, height: u64) -> Result<Block, StorageError> {
        let key = height_key(height);
        let value = self
            .heights
            .get(key.as_slice())?
            .ok_or(StorageError::Missing)?;
        let hash = decode_hash(&value)?;
        self.get_block(hash)
    }

    pub fn get_transaction(&self, hash: Hash) -> Result<wsc_core::Transaction, StorageError> {
        let value = self
            .transactions
            .get(&hash.as_bytes()[..])?
            .ok_or(StorageError::Missing)?;
        canonical_decode(&value).map_err(|_| StorageError::Corrupt)
    }

    pub fn latest_state(&self) -> Result<StateSnapshot, StorageError> {
        let value = self.state.get(STATE_LATEST)?.ok_or(StorageError::Missing)?;
        canonical_decode(&value)
            .or_else(|_| {
                canonical_decode::<PreProgramStateSnapshot>(&value).map(|legacy| StateSnapshot {
                    chain_id: legacy.chain_id,
                    fee_minimum: legacy.fee_minimum,
                    fee_pool: legacy.fee_pool,
                    accounts: legacy.accounts,
                    asset_balances: legacy.asset_balances,
                    processed_asset_operations: legacy.processed_asset_operations,
                    asset_operation_records: legacy.asset_operation_records,
                    token_definitions: legacy.token_definitions,
                    processed_token_operations: legacy.processed_token_operations,
                    token_operation_records: legacy.token_operation_records,
                    frozen_token_accounts: legacy.frozen_token_accounts,
                    mna_reserve_ledger: legacy.mna_reserve_ledger,
                    processed_mna_swap_operations: legacy.processed_mna_swap_operations,
                    mna_swap_operation_records: legacy.mna_swap_operation_records,
                    processed_mna_reserve_operations: legacy.processed_mna_reserve_operations,
                    mna_reserve_operation_records: legacy.mna_reserve_operation_records,
                    programs: Default::default(),
                    program_receipts: Default::default(),
                    closed_programs: Default::default(),
                })
            })
            .or_else(|_| {
                canonical_decode::<LegacyAssetStateSnapshot>(&value).map(|legacy| StateSnapshot {
                    chain_id: legacy.chain_id,
                    fee_minimum: legacy.fee_minimum,
                    fee_pool: legacy.fee_pool,
                    accounts: legacy.accounts,
                    asset_balances: legacy.asset_balances,
                    processed_asset_operations: legacy.processed_asset_operations,
                    asset_operation_records: legacy.asset_operation_records,
                    token_definitions: Default::default(),
                    processed_token_operations: Default::default(),
                    token_operation_records: Default::default(),
                    frozen_token_accounts: Default::default(),
                    mna_reserve_ledger: Default::default(),
                    processed_mna_swap_operations: Default::default(),
                    mna_swap_operation_records: Default::default(),
                    processed_mna_reserve_operations: Default::default(),
                    mna_reserve_operation_records: Default::default(),
                    programs: Default::default(),
                    program_receipts: Default::default(),
                    closed_programs: Default::default(),
                })
            })
            .or_else(|_| {
                canonical_decode::<LegacyStateSnapshot>(&value).map(|legacy| StateSnapshot {
                    chain_id: legacy.chain_id,
                    fee_minimum: legacy.fee_minimum,
                    fee_pool: legacy.fee_pool,
                    accounts: legacy.accounts,
                    asset_balances: BTreeMap::new(),
                    processed_asset_operations: Default::default(),
                    asset_operation_records: Default::default(),
                    token_definitions: Default::default(),
                    processed_token_operations: Default::default(),
                    token_operation_records: Default::default(),
                    frozen_token_accounts: Default::default(),
                    mna_reserve_ledger: Default::default(),
                    processed_mna_swap_operations: Default::default(),
                    mna_swap_operation_records: Default::default(),
                    processed_mna_reserve_operations: Default::default(),
                    mna_reserve_operation_records: Default::default(),
                    programs: Default::default(),
                    program_receipts: Default::default(),
                    closed_programs: Default::default(),
                })
            })
            .map_err(|_| StorageError::Corrupt)
    }

    pub fn commit(
        &self,
        block_hash: Hash,
        block: &Block,
        state: &StateSnapshot,
    ) -> Result<(), StorageError> {
        let block_bytes =
            canonical_encode(block).map_err(|error| StorageError::Encoding(error.to_string()))?;
        let state_bytes =
            canonical_encode(state).map_err(|error| StorageError::Encoding(error.to_string()))?;

        self.blocks
            .insert(&block_hash.as_bytes()[..], block_bytes)?;
        let height = height_key(block.header.height);
        self.heights
            .insert(&height[..], &block_hash.as_bytes()[..])?;
        self.state.insert(STATE_LATEST, state_bytes)?;

        for transaction in &block.transactions {
            let tx_hash = transaction_id(transaction)
                .map_err(|error| StorageError::Encoding(error.to_string()))?;
            let tx_bytes = canonical_encode(transaction)
                .map_err(|error| StorageError::Encoding(error.to_string()))?;
            self.transactions
                .insert(&tx_hash.as_bytes()[..], tx_bytes)?;
        }

        let latest_height = block.header.height.to_be_bytes();
        self.meta.insert(META_LATEST_HEIGHT, &latest_height[..])?;
        self.meta
            .insert(META_LATEST_HASH, &block_hash.as_bytes()[..])?;
        self.db.flush()?;
        Ok(())
    }

    pub fn is_initialized(&self) -> Result<bool, StorageError> {
        Ok(self.meta.get(META_CHAIN_ID)?.is_some())
    }

    pub fn expected_chain_id(&self) -> &'static str {
        CHAIN_ID
    }
}

fn height_key(height: u64) -> [u8; 8] {
    height.to_be_bytes()
}

fn decode_hash(value: &[u8]) -> Result<Hash, StorageError> {
    if value.len() != 32 {
        return Err(StorageError::Corrupt);
    }
    let mut bytes = [0u8; 32];
    bytes.copy_from_slice(value);
    Ok(Hash(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};
    use wsc_core::{Block, BlockHeader, GenesisConfig, Hash};
    use wsc_state::State;

    fn temp_path() -> std::path::PathBuf {
        let value = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("wsc-storage-{value}"))
    }

    #[test]
    fn persists_genesis_and_latest_state() {
        let path = temp_path();
        let store = Store::open(&path).unwrap();
        let genesis = GenesisConfig {
            version: 1,
            chain_id: CHAIN_ID.to_owned(),
            genesis_time: 1,
            block_time_ms: 2000,
            initial_supply: 0,
            fee_minimum: 1,
            validators: vec![],
            allocations: vec![],
            assets: vec![],
        };
        let state = State::from_genesis(&genesis).unwrap();
        let block = Block {
            header: BlockHeader {
                version: 1,
                chain_id: CHAIN_ID.to_owned(),
                height: 0,
                parent_hash: Hash::ZERO,
                timestamp: 1,
                transaction_root: Hash::ZERO,
                state_root: state.state_root().unwrap(),
                proposer: None,
                proposer_signature: None,
            },
            transactions: vec![],
            asset_operations: vec![],
            token_operations: vec![],
            mna_swap_operations: vec![],
            mna_reserve_operations: vec![],
            program_operations: vec![],
        };
        let hash = store
            .ensure_genesis(&genesis, &block, &state.snapshot())
            .unwrap();
        assert_eq!(store.latest_height().unwrap(), 0);
        assert_eq!(store.latest_hash().unwrap(), hash);
        assert_eq!(store.latest_block().unwrap(), block);
        assert_eq!(store.latest_state().unwrap(), state.snapshot());
        std::fs::remove_dir_all(path).ok();
    }
}
