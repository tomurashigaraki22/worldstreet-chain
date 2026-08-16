use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use thiserror::Error;
use wsc_core::{
    canonical_encode, Address, Amount, AssetOperation, AssetOperationKind, GenesisConfig, Hash,
    MnaReserveLedger, MnaReserveOperation, MnaReserveOperationKind, MnaSwapKind, MnaSwapOperation,
    ProgramOperation, ProgramOperationKind, ProgramReceiptRecord, ProgramRecord, TokenDefinition,
    TokenOperation, TokenOperationKind, Transaction, MNA_USDC_DENOMINATOR,
};
use wsc_crypto::{
    address_from_public_key, mna_reserve_operation_id, mna_swap_operation_id, program_operation_id,
    sha256_domain, token_id_from_operation, token_operation_id, transaction_id, KeyPair,
};
use wsc_program::{execute, ProgramPackage};

pub const MAX_MEMO_BYTES: usize = 256;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Account {
    pub balance: Amount,
    pub nonce: u64,
}

impl Default for Account {
    fn default() -> Self {
        Self {
            balance: 0,
            nonce: 0,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StateSnapshot {
    pub chain_id: String,
    pub fee_minimum: Amount,
    pub fee_pool: Amount,
    pub accounts: BTreeMap<Address, Account>,
    #[serde(default)]
    pub asset_balances: BTreeMap<Address, BTreeMap<String, Amount>>,
    #[serde(default)]
    pub processed_asset_operations: BTreeSet<Hash>,
    #[serde(default)]
    pub asset_operation_records: BTreeMap<Hash, AssetOperation>,
    #[serde(default)]
    pub token_definitions: BTreeMap<Hash, TokenDefinition>,
    #[serde(default)]
    pub processed_token_operations: BTreeSet<Hash>,
    #[serde(default)]
    pub token_operation_records: BTreeMap<Hash, TokenOperation>,
    #[serde(default)]
    pub frozen_token_accounts: BTreeSet<(Hash, Address)>,
    #[serde(default)]
    pub mna_reserve_ledger: MnaReserveLedger,
    #[serde(default)]
    pub processed_mna_swap_operations: BTreeSet<Hash>,
    #[serde(default)]
    pub mna_swap_operation_records: BTreeMap<Hash, MnaSwapOperation>,
    #[serde(default)]
    pub processed_mna_reserve_operations: BTreeSet<Hash>,
    #[serde(default)]
    pub mna_reserve_operation_records: BTreeMap<Hash, MnaReserveOperation>,
    #[serde(default)]
    pub programs: BTreeMap<String, ProgramRecord>,
    #[serde(default)]
    pub program_receipts: BTreeMap<Hash, ProgramReceiptRecord>,
    #[serde(default)]
    pub closed_programs: BTreeMap<String, u64>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct LegacyStateSnapshot {
    chain_id: String,
    fee_minimum: Amount,
    fee_pool: Amount,
    accounts: BTreeMap<Address, Account>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct LegacyAssetStateSnapshot {
    chain_id: String,
    fee_minimum: Amount,
    fee_pool: Amount,
    accounts: BTreeMap<Address, Account>,
    asset_balances: BTreeMap<Address, BTreeMap<String, Amount>>,
    processed_asset_operations: BTreeSet<Hash>,
    asset_operation_records: BTreeMap<Hash, AssetOperation>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct PreProgramStateSnapshot {
    chain_id: String,
    fee_minimum: Amount,
    fee_pool: Amount,
    accounts: BTreeMap<Address, Account>,
    asset_balances: BTreeMap<Address, BTreeMap<String, Amount>>,
    processed_asset_operations: BTreeSet<Hash>,
    asset_operation_records: BTreeMap<Hash, AssetOperation>,
    token_definitions: BTreeMap<Hash, TokenDefinition>,
    processed_token_operations: BTreeSet<Hash>,
    token_operation_records: BTreeMap<Hash, TokenOperation>,
    frozen_token_accounts: BTreeSet<(Hash, Address)>,
    mna_reserve_ledger: MnaReserveLedger,
    processed_mna_swap_operations: BTreeSet<Hash>,
    mna_swap_operation_records: BTreeMap<Hash, MnaSwapOperation>,
    processed_mna_reserve_operations: BTreeSet<Hash>,
    mna_reserve_operation_records: BTreeMap<Hash, MnaReserveOperation>,
}

#[derive(Clone, Debug)]
pub struct State {
    chain_id: String,
    fee_minimum: Amount,
    fee_pool: Amount,
    accounts: BTreeMap<Address, Account>,
    asset_balances: BTreeMap<Address, BTreeMap<String, Amount>>,
    processed_asset_operations: BTreeSet<Hash>,
    asset_operation_records: BTreeMap<Hash, AssetOperation>,
    token_definitions: BTreeMap<Hash, TokenDefinition>,
    processed_token_operations: BTreeSet<Hash>,
    token_operation_records: BTreeMap<Hash, TokenOperation>,
    frozen_token_accounts: BTreeSet<(Hash, Address)>,
    mna_reserve_ledger: MnaReserveLedger,
    processed_mna_swap_operations: BTreeSet<Hash>,
    mna_swap_operation_records: BTreeMap<Hash, MnaSwapOperation>,
    processed_mna_reserve_operations: BTreeSet<Hash>,
    mna_reserve_operation_records: BTreeMap<Hash, MnaReserveOperation>,
    programs: BTreeMap<String, ProgramRecord>,
    program_receipts: BTreeMap<Hash, ProgramReceiptRecord>,
    closed_programs: BTreeMap<String, u64>,
}

#[derive(Debug, Error)]
pub enum StateError {
    #[error("genesis allocation total does not equal initial supply")]
    GenesisSupplyMismatch,
    #[error("duplicate genesis allocation")]
    DuplicateGenesisAllocation,
    #[error("unsupported transaction version")]
    UnsupportedTransactionVersion,
    #[error("transaction chain ID does not match state")]
    WrongChainId,
    #[error("sender public key does not derive the sender address")]
    AddressMismatch,
    #[error("invalid transaction signature")]
    InvalidSignature,
    #[error("transfer amount must be greater than zero")]
    ZeroAmount,
    #[error("transaction memo is too long")]
    MemoTooLong,
    #[error("fee is below the configured minimum")]
    FeeTooLow,
    #[error("transaction nonce does not match account nonce")]
    NonceMismatch { expected: u64, actual: u64 },
    #[error("sender account does not have enough balance")]
    InsufficientBalance,
    #[error("balance arithmetic overflow")]
    BalanceOverflow,
    #[error("account nonce overflow")]
    NonceOverflow,
    #[error("fee-pool arithmetic overflow")]
    FeePoolOverflow,
    #[error("unsupported asset operation version")]
    UnsupportedAssetOperationVersion,
    #[error("asset operation amount must be greater than zero")]
    ZeroAssetOperationAmount,
    #[error("asset operation is not a wrapped asset")]
    InvalidAssetOperationAsset,
    #[error("asset balance is insufficient")]
    InsufficientAssetBalance,
    #[error("asset balance arithmetic overflow")]
    AssetBalanceOverflow,
    #[error("unsupported token operation version")]
    UnsupportedTokenOperationVersion,
    #[error("token operation chain ID does not match state")]
    TokenWrongChainId,
    #[error("token operation address mismatch")]
    TokenAddressMismatch,
    #[error("invalid token operation signature")]
    InvalidTokenSignature,
    #[error("token operation memo is too long")]
    TokenMemoTooLong,
    #[error("token operation fee is below the configured minimum")]
    TokenFeeTooLow,
    #[error("token operation nonce does not match account nonce")]
    TokenNonceMismatch { expected: u64, actual: u64 },
    #[error("token operation sender cannot pay the fee")]
    TokenInsufficientFeeBalance,
    #[error("token operation amount must be greater than zero")]
    TokenZeroAmount,
    #[error("token name or symbol is invalid")]
    InvalidTokenMetadata,
    #[error("token already exists")]
    TokenAlreadyExists,
    #[error("token does not exist")]
    TokenNotFound,
    #[error("token authority is not authorized")]
    TokenUnauthorized,
    #[error("token supply cap exceeded")]
    TokenSupplyCapExceeded,
    #[error("token balance is insufficient")]
    TokenInsufficientBalance,
    #[error("token balance arithmetic overflow")]
    TokenBalanceOverflow,
    #[error("token account is frozen")]
    TokenAccountFrozen,
    #[error("token is paused")]
    TokenPaused,
    #[error("MNA reserve operation is paused")]
    MnaReservePaused,
    #[error("MNA reserve operation has an invalid version")]
    InvalidMnaReserveVersion,
    #[error("MNA swap operation has an invalid version")]
    InvalidMnaSwapVersion,
    #[error("MNA swap operation chain ID does not match state")]
    MnaSwapWrongChainId,
    #[error("MNA swap operation address mismatch")]
    MnaSwapAddressMismatch,
    #[error("invalid MNA swap signature")]
    InvalidMnaSwapSignature,
    #[error("MNA swap fee is below the configured minimum")]
    MnaSwapFeeTooLow,
    #[error("MNA swap nonce does not match account nonce")]
    MnaSwapNonceMismatch { expected: u64, actual: u64 },
    #[error("MNA swap sender cannot pay the fee")]
    MnaSwapInsufficientFeeBalance,
    #[error("MNA swap collateral must be supported USDC")]
    InvalidMnaCollateral,
    #[error("MNA swap amount must be exactly convertible at 2 USDC = 1 MNA")]
    InvalidMnaConversion,
    #[error("MNA swap balance is insufficient")]
    MnaSwapInsufficientBalance,
    #[error("MNA reserve collateral is insufficient")]
    MnaReserveInsufficient,
    #[error("token operation requires a recipient")]
    TokenRecipientRequired,
    #[error("token operation forbids a recipient")]
    TokenRecipientForbidden,
    #[error("token operation has an invalid token ID")]
    InvalidTokenId,
    #[error("state encoding failed: {0}")]
    Encoding(String),
    #[error("transaction hashing failed: {0}")]
    TransactionHashing(String),
    #[error("unsupported program operation version")]
    UnsupportedProgramOperationVersion,
    #[error("program operation chain ID does not match state")]
    ProgramWrongChainId,
    #[error("invalid program operation: {0}")]
    InvalidProgramOperation(String),
    #[error("invalid program owner signature")]
    InvalidProgramSignature,
    #[error("program does not exist")]
    ProgramNotFound,
    #[error("program owner is not authorized")]
    ProgramUnauthorized,
    #[error("program already exists or was closed")]
    ProgramAlreadyExists,
}

impl State {
    pub fn from_genesis(genesis: &GenesisConfig) -> Result<Self, StateError> {
        let mut accounts = BTreeMap::new();
        let mut total = 0u128;

        for allocation in &genesis.allocations {
            if accounts.contains_key(&allocation.address) {
                return Err(StateError::DuplicateGenesisAllocation);
            }
            total = total
                .checked_add(allocation.balance)
                .ok_or(StateError::GenesisSupplyMismatch)?;
            accounts.insert(
                allocation.address,
                Account {
                    balance: allocation.balance,
                    nonce: 0,
                },
            );
        }

        if total != genesis.initial_supply {
            return Err(StateError::GenesisSupplyMismatch);
        }

        Ok(Self {
            chain_id: genesis.chain_id.clone(),
            fee_minimum: genesis.fee_minimum,
            fee_pool: 0,
            accounts,
            asset_balances: BTreeMap::new(),
            processed_asset_operations: BTreeSet::new(),
            asset_operation_records: BTreeMap::new(),
            token_definitions: BTreeMap::new(),
            processed_token_operations: BTreeSet::new(),
            token_operation_records: BTreeMap::new(),
            frozen_token_accounts: BTreeSet::new(),
            mna_reserve_ledger: MnaReserveLedger::default(),
            processed_mna_swap_operations: BTreeSet::new(),
            mna_swap_operation_records: BTreeMap::new(),
            processed_mna_reserve_operations: BTreeSet::new(),
            mna_reserve_operation_records: BTreeMap::new(),
            programs: BTreeMap::new(),
            program_receipts: BTreeMap::new(),
            closed_programs: BTreeMap::new(),
        })
    }

    pub fn from_snapshot(snapshot: StateSnapshot) -> Self {
        Self {
            chain_id: snapshot.chain_id,
            fee_minimum: snapshot.fee_minimum,
            fee_pool: snapshot.fee_pool,
            accounts: snapshot.accounts,
            asset_balances: snapshot.asset_balances,
            processed_asset_operations: snapshot.processed_asset_operations,
            asset_operation_records: snapshot.asset_operation_records,
            token_definitions: snapshot.token_definitions,
            processed_token_operations: snapshot.processed_token_operations,
            token_operation_records: snapshot.token_operation_records,
            frozen_token_accounts: snapshot.frozen_token_accounts,
            mna_reserve_ledger: snapshot.mna_reserve_ledger,
            processed_mna_swap_operations: snapshot.processed_mna_swap_operations,
            mna_swap_operation_records: snapshot.mna_swap_operation_records,
            processed_mna_reserve_operations: snapshot.processed_mna_reserve_operations,
            mna_reserve_operation_records: snapshot.mna_reserve_operation_records,
            programs: snapshot.programs,
            program_receipts: snapshot.program_receipts,
            closed_programs: snapshot.closed_programs,
        }
    }

    pub fn snapshot(&self) -> StateSnapshot {
        StateSnapshot {
            chain_id: self.chain_id.clone(),
            fee_minimum: self.fee_minimum,
            fee_pool: self.fee_pool,
            accounts: self.accounts.clone(),
            asset_balances: self.asset_balances.clone(),
            processed_asset_operations: self.processed_asset_operations.clone(),
            asset_operation_records: self.asset_operation_records.clone(),
            token_definitions: self.token_definitions.clone(),
            processed_token_operations: self.processed_token_operations.clone(),
            token_operation_records: self.token_operation_records.clone(),
            frozen_token_accounts: self.frozen_token_accounts.clone(),
            mna_reserve_ledger: self.mna_reserve_ledger.clone(),
            processed_mna_swap_operations: self.processed_mna_swap_operations.clone(),
            mna_swap_operation_records: self.mna_swap_operation_records.clone(),
            processed_mna_reserve_operations: self.processed_mna_reserve_operations.clone(),
            mna_reserve_operation_records: self.mna_reserve_operation_records.clone(),
            programs: self.programs.clone(),
            program_receipts: self.program_receipts.clone(),
            closed_programs: self.closed_programs.clone(),
        }
    }

    pub fn chain_id(&self) -> &str {
        &self.chain_id
    }

    pub fn fee_minimum(&self) -> Amount {
        self.fee_minimum
    }

    pub fn fee_pool(&self) -> Amount {
        self.fee_pool
    }

    pub fn balance_of(&self, address: &Address) -> Amount {
        self.accounts
            .get(address)
            .map(|account| account.balance)
            .unwrap_or(0)
    }

    pub fn nonce_of(&self, address: &Address) -> u64 {
        self.accounts
            .get(address)
            .map(|account| account.nonce)
            .unwrap_or(0)
    }

    pub fn accounts(&self) -> &BTreeMap<Address, Account> {
        &self.accounts
    }

    pub fn asset_balance_of(&self, address: &Address, asset_id: &str) -> Amount {
        self.asset_balances
            .get(address)
            .and_then(|assets| assets.get(asset_id).copied())
            .unwrap_or(0)
    }

    pub fn asset_balances_of(&self, address: &Address) -> BTreeMap<String, Amount> {
        self.asset_balances
            .get(address)
            .cloned()
            .unwrap_or_default()
    }

    pub fn asset_balances(&self) -> &BTreeMap<Address, BTreeMap<String, Amount>> {
        &self.asset_balances
    }

    pub fn processed_asset_operation(&self, operation_id: &Hash) -> bool {
        self.processed_asset_operations.contains(operation_id)
    }

    pub fn asset_operation_records(&self) -> &BTreeMap<Hash, AssetOperation> {
        &self.asset_operation_records
    }

    pub fn token_definitions(&self) -> &BTreeMap<Hash, TokenDefinition> {
        &self.token_definitions
    }

    pub fn token_definition(&self, token_id: &Hash) -> Option<&TokenDefinition> {
        self.token_definitions.get(token_id)
    }

    pub fn token_balance_of(&self, address: &Address, token_id: &Hash) -> Amount {
        let contract = format!("token:{token_id}");
        self.asset_balances
            .get(address)
            .and_then(|assets| {
                assets.iter().find_map(|(key, balance)| {
                    (key.starts_with("intertrain:") && key.ends_with(&contract)).then_some(*balance)
                })
            })
            .unwrap_or(0)
    }

    pub fn token_balance(&self, address: &Address, token: &TokenDefinition) -> Amount {
        self.asset_balances
            .get(address)
            .and_then(|assets| {
                assets
                    .get(
                        &wsc_core::AssetId::custom(token.token_id, &token.symbol, token.decimals)
                            .canonical_key(),
                    )
                    .copied()
            })
            .unwrap_or(0)
    }

    pub fn token_operation_records(&self) -> &BTreeMap<Hash, TokenOperation> {
        &self.token_operation_records
    }

    pub fn processed_token_operation(&self, operation_id: &Hash) -> bool {
        self.processed_token_operations.contains(operation_id)
    }

    pub fn is_token_account_frozen(&self, token_id: &Hash, address: &Address) -> bool {
        self.frozen_token_accounts.contains(&(*token_id, *address))
    }

    pub fn mna_reserve_ledger(&self) -> &MnaReserveLedger {
        &self.mna_reserve_ledger
    }

    pub fn mna_swap_operation_records(&self) -> &BTreeMap<Hash, MnaSwapOperation> {
        &self.mna_swap_operation_records
    }

    pub fn mna_reserve_operation_records(&self) -> &BTreeMap<Hash, MnaReserveOperation> {
        &self.mna_reserve_operation_records
    }

    pub fn is_supported_usdc(asset: &wsc_core::AssetId) -> bool {
        asset.namespace == "ethereum"
            && asset.symbol == "USDC"
            && asset.decimals == 6
            && asset.contract.as_deref().is_some_and(|c| {
                c.eq_ignore_ascii_case("sepolia:0x1c7D4B196Cb0C7B01d743Fbc6116a902379C7238")
            })
            || asset.namespace == "solana"
                && asset.symbol == "USDC"
                && asset.decimals == 6
                && asset.contract.as_deref()
                    == Some("devnet:4zMMC9srt5Ri5X14GAgXhaHii3GnPAEERYPJgZJDncDU")
    }

    /// Native SOL collateral is represented as a wrapped asset identity so it
    /// cannot be confused with Intertrain's native MNA. Amounts are lamports.
    pub fn is_supported_sol(asset: &wsc_core::AssetId) -> bool {
        asset.namespace == "solana"
            && asset.symbol == "SOL"
            && asset.decimals == 9
            && asset.contract.as_deref() == Some("devnet:native")
    }

    pub fn current_reserve_usd(&self) -> Amount {
        self.mna_reserve_ledger
            .total_verified_deposits_usdc
            .saturating_sub(self.mna_reserve_ledger.total_released_usdc)
            .saturating_add(
                self.mna_reserve_ledger
                    .total_verified_sol_usd
                    .saturating_sub(self.mna_reserve_ledger.total_released_sol_usd),
            )
    }

    pub fn validate_mna_reserve_operation(
        &self,
        operation: &MnaReserveOperation,
    ) -> Result<Hash, StateError> {
        if operation.version != 1 {
            return Err(StateError::InvalidMnaReserveVersion);
        }
        if self.mna_reserve_ledger.paused {
            return Err(StateError::MnaReservePaused);
        }
        let is_sol = Self::is_supported_sol(&operation.collateral_asset);
        if !is_sol && !Self::is_supported_usdc(&operation.collateral_asset) {
            return Err(StateError::InvalidMnaCollateral);
        }
        if operation.amount_usdc == 0
            || operation.amount_usdc % MNA_USDC_DENOMINATOR != 0
            || operation.amount_mna == 0
            || operation.fee_mna > operation.amount_usdc / MNA_USDC_DENOMINATOR
            || operation.amount_mna + operation.fee_mna
                != operation.amount_usdc / MNA_USDC_DENOMINATOR
        {
            return Err(StateError::InvalidMnaConversion);
        }
        if is_sol {
            if operation.collateral_amount == 0
                || operation.oracle_price_usd_micro_per_sol == 0
                || operation.oracle_timestamp == 0
            {
                return Err(StateError::InvalidMnaConversion);
            }
            let usd = operation
                .collateral_amount
                .checked_mul(operation.oracle_price_usd_micro_per_sol)
                .ok_or(StateError::BalanceOverflow)?
                / 1_000_000_000u128;
            if usd != operation.amount_usdc {
                return Err(StateError::InvalidMnaConversion);
            }
        } else if operation.collateral_amount != 0
            || operation.oracle_price_usd_micro_per_sol != 0
            || operation.oracle_timestamp != 0
            || operation.fee_mna != 0
        {
            return Err(StateError::InvalidMnaConversion);
        }
        if operation.external_transaction.trim().is_empty() {
            return Err(StateError::InvalidMnaReserveVersion);
        }
        let id = mna_reserve_operation_id(operation)
            .map_err(|e| StateError::TransactionHashing(e.to_string()))?;
        if id != operation.operation_id {
            return Err(StateError::InvalidMnaReserveVersion);
        }
        Ok(id)
    }

    pub fn apply_mna_reserve_operation(
        &mut self,
        operation: &MnaReserveOperation,
    ) -> Result<bool, StateError> {
        let id = self.validate_mna_reserve_operation(operation)?;
        if !self.processed_mna_reserve_operations.insert(id) {
            return Ok(false);
        }
        let mut next = self.clone();
        let is_sol = Self::is_supported_sol(&operation.collateral_asset);
        match operation.kind {
            MnaReserveOperationKind::VerifyDeposit => {
                if is_sol {
                    next.mna_reserve_ledger.total_verified_sol_lamports = next
                        .mna_reserve_ledger
                        .total_verified_sol_lamports
                        .checked_add(operation.collateral_amount)
                        .ok_or(StateError::BalanceOverflow)?;
                    next.mna_reserve_ledger.total_verified_sol_usd = next
                        .mna_reserve_ledger
                        .total_verified_sol_usd
                        .checked_add(operation.amount_usdc)
                        .ok_or(StateError::BalanceOverflow)?;
                    let acct = next.accounts.entry(operation.address).or_default();
                    acct.balance = acct
                        .balance
                        .checked_add(operation.amount_mna)
                        .ok_or(StateError::BalanceOverflow)?;
                    next.mna_reserve_ledger.reserve_backed_mna_minted = next
                        .mna_reserve_ledger
                        .reserve_backed_mna_minted
                        .checked_add(operation.amount_mna)
                        .ok_or(StateError::BalanceOverflow)?;
                } else {
                    next.mna_reserve_ledger.total_verified_deposits_usdc = next
                        .mna_reserve_ledger
                        .total_verified_deposits_usdc
                        .checked_add(operation.amount_usdc)
                        .ok_or(StateError::BalanceOverflow)?;
                }
            }
            MnaReserveOperationKind::Release => {
                if is_sol {
                    let available = next
                        .mna_reserve_ledger
                        .total_verified_sol_usd
                        .saturating_sub(next.mna_reserve_ledger.total_released_sol_usd);
                    if available < operation.amount_usdc {
                        return Err(StateError::MnaReserveInsufficient);
                    }
                    next.mna_reserve_ledger.total_released_sol_usd = next
                        .mna_reserve_ledger
                        .total_released_sol_usd
                        .checked_add(operation.amount_usdc)
                        .ok_or(StateError::BalanceOverflow)?;
                    next.mna_reserve_ledger.total_released_sol_lamports = next
                        .mna_reserve_ledger
                        .total_released_sol_lamports
                        .checked_add(operation.collateral_amount)
                        .ok_or(StateError::BalanceOverflow)?;
                } else {
                    let available = next
                        .mna_reserve_ledger
                        .total_verified_deposits_usdc
                        .saturating_sub(next.mna_reserve_ledger.total_released_usdc);
                    if available < operation.amount_usdc {
                        return Err(StateError::MnaReserveInsufficient);
                    }
                    next.mna_reserve_ledger.total_released_usdc = next
                        .mna_reserve_ledger
                        .total_released_usdc
                        .checked_add(operation.amount_usdc)
                        .ok_or(StateError::BalanceOverflow)?;
                }
            }
        }
        if next.current_reserve_usd()
            < next
                .mna_reserve_ledger
                .reserve_backed_mna_minted
                .saturating_mul(MNA_USDC_DENOMINATOR)
        {
            return Err(StateError::MnaReserveInsufficient);
        }
        next.mna_reserve_operation_records
            .insert(id, operation.clone());
        *self = next;
        Ok(true)
    }

    pub fn validate_mna_swap(&self, operation: &MnaSwapOperation) -> Result<Hash, StateError> {
        let unsigned = &operation.unsigned;
        if unsigned.version != 1 {
            return Err(StateError::InvalidMnaSwapVersion);
        }
        if unsigned.chain_id != self.chain_id {
            return Err(StateError::MnaSwapWrongChainId);
        }
        if address_from_public_key(&unsigned.public_key) != unsigned.from {
            return Err(StateError::MnaSwapAddressMismatch);
        }
        if !KeyPair::verify(
            &unsigned.public_key,
            &operation
                .signing_bytes()
                .map_err(|e| StateError::Encoding(e.to_string()))?,
            &operation.signature,
        ) {
            return Err(StateError::InvalidMnaSwapSignature);
        }
        if unsigned.fee < self.fee_minimum {
            return Err(StateError::MnaSwapFeeTooLow);
        }
        let account = self
            .accounts
            .get(&unsigned.from)
            .ok_or(StateError::MnaSwapInsufficientFeeBalance)?;
        if account.nonce != unsigned.nonce {
            return Err(StateError::MnaSwapNonceMismatch {
                expected: account.nonce,
                actual: unsigned.nonce,
            });
        }
        if account.balance < unsigned.fee {
            return Err(StateError::MnaSwapInsufficientFeeBalance);
        }
        if !Self::is_supported_usdc(&unsigned.collateral_asset) {
            return Err(StateError::InvalidMnaCollateral);
        }
        if unsigned.amount_usdc == 0
            || unsigned.amount_usdc % MNA_USDC_DENOMINATOR != 0
            || unsigned.amount_mna != unsigned.amount_usdc / MNA_USDC_DENOMINATOR
        {
            return Err(StateError::InvalidMnaConversion);
        }
        let id = mna_swap_operation_id(operation)
            .map_err(|e| StateError::TransactionHashing(e.to_string()))?;
        Ok(id)
    }

    pub fn apply_mna_swap(&mut self, operation: &MnaSwapOperation) -> Result<bool, StateError> {
        let id = self.validate_mna_swap(operation)?;
        if !self.processed_mna_swap_operations.insert(id) {
            return Ok(false);
        }
        let mut next = self.clone();
        let key = operation.unsigned.collateral_asset.canonical_key();
        let asset_balance = next
            .asset_balances
            .entry(operation.unsigned.from)
            .or_default();
        let current_usdc = asset_balance.get(&key).copied().unwrap_or(0);
        let account = next
            .accounts
            .get(&operation.unsigned.from)
            .cloned()
            .unwrap_or_default();
        let new_balance = account
            .balance
            .checked_sub(operation.unsigned.fee)
            .ok_or(StateError::MnaSwapInsufficientFeeBalance)?;
        let new_nonce = account
            .nonce
            .checked_add(1)
            .ok_or(StateError::NonceOverflow)?;
        match operation.unsigned.kind {
            MnaSwapKind::MintMna => {
                if current_usdc < operation.unsigned.amount_usdc {
                    return Err(StateError::MnaSwapInsufficientBalance);
                }
                asset_balance.insert(key, current_usdc - operation.unsigned.amount_usdc);
                let acct = next.accounts.entry(operation.unsigned.from).or_default();
                acct.balance = new_balance
                    .checked_add(operation.unsigned.amount_mna)
                    .ok_or(StateError::BalanceOverflow)?;
                acct.nonce = new_nonce;
                let new_backed = next
                    .mna_reserve_ledger
                    .reserve_backed_mna_minted
                    .checked_add(operation.unsigned.amount_mna)
                    .ok_or(StateError::BalanceOverflow)?;
                let reserves = next.current_reserve_usd();
                if reserves < new_backed.saturating_mul(MNA_USDC_DENOMINATOR) {
                    return Err(StateError::MnaReserveInsufficient);
                }
                next.mna_reserve_ledger.reserve_backed_mna_minted = new_backed;
            }
            MnaSwapKind::RedeemMna => {
                let acct = next.accounts.entry(operation.unsigned.from).or_default();
                if acct.balance < operation.unsigned.amount_mna + operation.unsigned.fee {
                    return Err(StateError::MnaSwapInsufficientBalance);
                }
                acct.balance = acct
                    .balance
                    .checked_sub(operation.unsigned.amount_mna + operation.unsigned.fee)
                    .ok_or(StateError::MnaSwapInsufficientBalance)?;
                acct.nonce = new_nonce;
                asset_balance.insert(
                    key,
                    current_usdc
                        .checked_add(operation.unsigned.amount_usdc)
                        .ok_or(StateError::AssetBalanceOverflow)?,
                );
                next.mna_reserve_ledger.reserve_backed_mna_minted = next
                    .mna_reserve_ledger
                    .reserve_backed_mna_minted
                    .checked_sub(operation.unsigned.amount_mna)
                    .ok_or(StateError::MnaReserveInsufficient)?;
                next.mna_reserve_ledger.total_redeemed_mna = next
                    .mna_reserve_ledger
                    .total_redeemed_mna
                    .checked_add(operation.unsigned.amount_mna)
                    .ok_or(StateError::BalanceOverflow)?;
            }
        }
        next.mna_swap_operation_records
            .insert(id, operation.clone());
        *self = next;
        Ok(true)
    }

    pub fn apply_asset_operation(
        &mut self,
        operation: &AssetOperation,
    ) -> Result<bool, StateError> {
        if operation.version != 1 {
            return Err(StateError::UnsupportedAssetOperationVersion);
        }
        if operation.amount == 0 {
            return Err(StateError::ZeroAssetOperationAmount);
        }
        if operation.asset_id.namespace == "worldstreet" || operation.asset_id.contract.is_none() {
            return Err(StateError::InvalidAssetOperationAsset);
        }
        if !self
            .processed_asset_operations
            .insert(operation.operation_id)
        {
            return Ok(false);
        }
        self.asset_operation_records
            .insert(operation.operation_id, operation.clone());
        let key = operation.asset_id.canonical_key();
        if Self::is_supported_usdc(&operation.asset_id) {
            match operation.kind {
                AssetOperationKind::Mint => {
                    self.mna_reserve_ledger.total_verified_deposits_usdc = self
                        .mna_reserve_ledger
                        .total_verified_deposits_usdc
                        .checked_add(operation.amount)
                        .ok_or(StateError::BalanceOverflow)?;
                }
                AssetOperationKind::Burn => {
                    self.mna_reserve_ledger.total_released_usdc = self
                        .mna_reserve_ledger
                        .total_released_usdc
                        .checked_add(operation.amount)
                        .ok_or(StateError::BalanceOverflow)?;
                }
            }
        }
        let balances = self.asset_balances.entry(operation.address).or_default();
        match operation.kind {
            AssetOperationKind::Mint => {
                let current = balances.get(&key).copied().unwrap_or(0);
                balances.insert(
                    key,
                    current
                        .checked_add(operation.amount)
                        .ok_or(StateError::AssetBalanceOverflow)?,
                );
            }
            AssetOperationKind::Burn => {
                let current = balances.get(&key).copied().unwrap_or(0);
                balances.insert(
                    key,
                    current
                        .checked_sub(operation.amount)
                        .ok_or(StateError::InsufficientAssetBalance)?,
                );
            }
        }
        Ok(true)
    }

    pub fn validate_token_operation(&self, operation: &TokenOperation) -> Result<Hash, StateError> {
        let operation_id = token_operation_id(operation)
            .map_err(|error| StateError::TransactionHashing(error.to_string()))?;
        let unsigned = &operation.unsigned;
        if unsigned.version != 1 {
            return Err(StateError::UnsupportedTokenOperationVersion);
        }
        if unsigned.chain_id != self.chain_id {
            return Err(StateError::TokenWrongChainId);
        }
        if address_from_public_key(&unsigned.public_key) != unsigned.from {
            return Err(StateError::TokenAddressMismatch);
        }
        if !KeyPair::verify(
            &unsigned.public_key,
            &operation
                .signing_bytes()
                .map_err(|error| StateError::Encoding(error.to_string()))?,
            &operation.signature,
        ) {
            return Err(StateError::InvalidTokenSignature);
        }
        if unsigned.memo.len() > MAX_MEMO_BYTES {
            return Err(StateError::TokenMemoTooLong);
        }
        if unsigned.fee < self.fee_minimum {
            return Err(StateError::TokenFeeTooLow);
        }
        let sender = self
            .accounts
            .get(&unsigned.from)
            .ok_or(StateError::TokenInsufficientFeeBalance)?;
        if sender.nonce != unsigned.nonce {
            return Err(StateError::TokenNonceMismatch {
                expected: sender.nonce,
                actual: unsigned.nonce,
            });
        }
        if sender.balance < unsigned.fee {
            return Err(StateError::TokenInsufficientFeeBalance);
        }
        match unsigned.kind {
            TokenOperationKind::Create => {
                if unsigned.token_id != Hash::ZERO
                    || unsigned.name.is_empty()
                    || unsigned.name.len() > 64
                    || unsigned.symbol.is_empty()
                    || unsigned.symbol.len() > 16
                    || !unsigned
                        .symbol
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric())
                    || unsigned.decimals > 18
                    || unsigned.to.is_some()
                    || unsigned.max_supply == Some(0)
                    || unsigned.max_supply.is_some_and(|cap| unsigned.amount > cap)
                {
                    return Err(StateError::InvalidTokenMetadata);
                }
            }
            TokenOperationKind::Transfer | TokenOperationKind::Mint => {
                if unsigned.token_id == Hash::ZERO || unsigned.to.is_none() || unsigned.amount == 0
                {
                    return Err(StateError::TokenRecipientRequired);
                }
                if unsigned.to == Some(unsigned.from) {
                    return Err(StateError::TokenRecipientRequired);
                }
            }
            TokenOperationKind::Burn => {
                if unsigned.token_id == Hash::ZERO || unsigned.to.is_some() || unsigned.amount == 0
                {
                    return Err(StateError::TokenRecipientForbidden);
                }
            }
            TokenOperationKind::SetAuthorities
            | TokenOperationKind::Freeze
            | TokenOperationKind::Unfreeze
            | TokenOperationKind::Pause
            | TokenOperationKind::Unpause
            | TokenOperationKind::UpdateMetadata => {
                if unsigned.token_id == Hash::ZERO {
                    return Err(StateError::InvalidTokenId);
                }
                if matches!(
                    unsigned.kind,
                    TokenOperationKind::Freeze | TokenOperationKind::Unfreeze
                ) && unsigned.to.is_none()
                {
                    return Err(StateError::TokenRecipientRequired);
                }
                if !matches!(
                    unsigned.kind,
                    TokenOperationKind::Freeze | TokenOperationKind::Unfreeze
                ) && unsigned.to.is_some()
                {
                    return Err(StateError::TokenRecipientForbidden);
                }
            }
        }
        Ok(operation_id)
    }

    pub fn apply_token_operation(
        &mut self,
        operation: &TokenOperation,
        expected_operation_id: Hash,
    ) -> Result<bool, StateError> {
        let actual_operation_id = token_operation_id(operation)
            .map_err(|error| StateError::TransactionHashing(error.to_string()))?;
        if actual_operation_id != expected_operation_id {
            return Err(StateError::InvalidTokenId);
        }
        if self
            .processed_token_operations
            .contains(&expected_operation_id)
        {
            return Ok(false);
        }
        self.validate_token_operation(operation)?;
        let mut next = self.clone();
        next.apply_token_operation_mut(operation, expected_operation_id)?;
        *self = next;
        Ok(true)
    }

    pub fn programs(&self) -> &BTreeMap<String, ProgramRecord> {
        &self.programs
    }

    pub fn program_receipts(&self) -> &BTreeMap<Hash, ProgramReceiptRecord> {
        &self.program_receipts
    }

    pub fn closed_programs(&self) -> &BTreeMap<String, u64> {
        &self.closed_programs
    }

    pub fn validate_program_operation(
        &self,
        operation: &ProgramOperation,
    ) -> Result<(), StateError> {
        let mut next = self.clone();
        next.apply_program_operation_mut(operation, 0).map(|_| ())
    }

    pub fn apply_program_operation(
        &mut self,
        operation: &ProgramOperation,
        block_height: u64,
    ) -> Result<bool, StateError> {
        let operation_id = program_operation_id(operation)
            .map_err(|e| StateError::InvalidProgramOperation(e.to_string()))?;
        if self.program_receipts.contains_key(&operation_id) {
            return Ok(false);
        }
        let mut next = self.clone();
        next.apply_program_operation_mut(operation, block_height)?;
        *self = next;
        Ok(true)
    }

    fn apply_program_operation_mut(
        &mut self,
        operation: &ProgramOperation,
        block_height: u64,
    ) -> Result<Hash, StateError> {
        if operation.version != 1 {
            return Err(StateError::UnsupportedProgramOperationVersion);
        }
        if operation.chain_id != self.chain_id {
            return Err(StateError::ProgramWrongChainId);
        }
        let operation_id = program_operation_id(operation)
            .map_err(|e| StateError::InvalidProgramOperation(e.to_string()))?;
        let owner = address_from_public_key(&operation.public_key);
        let action = match operation.kind {
            ProgramOperationKind::Deploy => "deploy",
            ProgramOperationKind::Call => "call",
            ProgramOperationKind::StorageSet => "storage_set",
            ProgramOperationKind::Close => "close",
        };
        let mut message = format!(
            "Intertrain Program Authorization\nAction: {action}\nChain ID: {}\nProgram ID: {}\nOwner: {owner}\nNonce: {}\nFee: {}",
            operation.chain_id, operation.program_id, operation.nonce, operation.fee
        );
        if operation.kind == ProgramOperationKind::Call {
            message.push_str(&format!("\nGas Limit: {}", operation.gas_limit));
        }
        if operation.kind == ProgramOperationKind::StorageSet {
            message.push_str(&format!(
                "\nKey: {}\nValue: {}",
                operation.key, operation.value
            ));
        }
        if !KeyPair::verify(
            &operation.public_key,
            message.as_bytes(),
            &operation.signature,
        ) {
            return Err(StateError::InvalidProgramSignature);
        }
        let account = self.accounts.entry(owner).or_default();
        if operation.nonce != account.nonce {
            return Err(StateError::NonceMismatch {
                expected: account.nonce,
                actual: operation.nonce,
            });
        }
        let required_max_fee = match operation.kind {
            ProgramOperationKind::Deploy => self
                .fee_minimum
                .checked_add(operation.package.len() as u128),
            ProgramOperationKind::Call => self.fee_minimum.checked_add(operation.gas_limit as u128),
            ProgramOperationKind::StorageSet | ProgramOperationKind::Close => {
                Some(self.fee_minimum)
            }
        }
        .ok_or(StateError::FeePoolOverflow)?;
        if operation.fee < required_max_fee {
            return Err(StateError::FeeTooLow);
        }
        if account.balance < required_max_fee {
            return Err(StateError::InsufficientBalance);
        }

        let (status, return_data, gas_used, error) = match operation.kind {
            ProgramOperationKind::Deploy => {
                if self.programs.contains_key(&operation.program_id)
                    || self.closed_programs.contains_key(&operation.program_id)
                {
                    return Err(StateError::ProgramAlreadyExists);
                }
                let package = ProgramPackage::decode(&operation.package)
                    .map_err(|e| StateError::InvalidProgramOperation(e.to_string()))?;
                if package.program_id() != operation.program_id {
                    return Err(StateError::InvalidProgramOperation(
                        "program ID does not match package".into(),
                    ));
                }
                self.programs.insert(
                    operation.program_id.clone(),
                    ProgramRecord {
                        package: operation.package.clone(),
                        creator: owner,
                        deployed_at_height: block_height,
                        storage: BTreeMap::new(),
                    },
                );
                ("confirmed".into(), vec![], 0, None)
            }
            ProgramOperationKind::Call => {
                if operation.gas_limit == 0 {
                    return Err(StateError::InvalidProgramOperation(
                        "gas limit must be positive".into(),
                    ));
                }
                let record = self
                    .programs
                    .get(&operation.program_id)
                    .ok_or(StateError::ProgramNotFound)?;
                let package = ProgramPackage::decode(&record.package)
                    .map_err(|e| StateError::InvalidProgramOperation(e.to_string()))?;
                match execute(&package, operation.gas_limit) {
                    Ok((data, used)) => ("success".into(), data, used, None),
                    Err(e) => (
                        "failed".into(),
                        vec![],
                        operation.gas_limit,
                        Some(e.to_string()),
                    ),
                }
            }
            ProgramOperationKind::StorageSet => {
                if operation.key.len() > 128 || operation.value.len() > 4096 {
                    return Err(StateError::InvalidProgramOperation(
                        "storage key/value exceeds limits".into(),
                    ));
                }
                let record = self
                    .programs
                    .get_mut(&operation.program_id)
                    .ok_or(StateError::ProgramNotFound)?;
                if record.creator != owner {
                    return Err(StateError::ProgramUnauthorized);
                }
                record
                    .storage
                    .insert(operation.key.clone(), operation.value.clone());
                ("confirmed".into(), vec![], 0, None)
            }
            ProgramOperationKind::Close => {
                let record = self
                    .programs
                    .get(&operation.program_id)
                    .ok_or(StateError::ProgramNotFound)?;
                if record.creator != owner {
                    return Err(StateError::ProgramUnauthorized);
                }
                self.programs.remove(&operation.program_id);
                self.closed_programs
                    .insert(operation.program_id.clone(), block_height);
                ("closed".into(), vec![], 0, None)
            }
        };
        let fee_paid = match operation.kind {
            ProgramOperationKind::Deploy => required_max_fee,
            ProgramOperationKind::Call => self
                .fee_minimum
                .checked_add(gas_used as u128)
                .ok_or(StateError::FeePoolOverflow)?,
            ProgramOperationKind::StorageSet | ProgramOperationKind::Close => self.fee_minimum,
        };
        let account = self
            .accounts
            .get_mut(&owner)
            .expect("program payer account exists");
        account.balance -= fee_paid;
        account.nonce = account
            .nonce
            .checked_add(1)
            .ok_or(StateError::NonceOverflow)?;
        self.fee_pool = self
            .fee_pool
            .checked_add(fee_paid)
            .ok_or(StateError::FeePoolOverflow)?;
        self.program_receipts.insert(
            operation_id,
            ProgramReceiptRecord {
                operation_id,
                program_id: operation.program_id.clone(),
                kind: operation.kind,
                status,
                return_data,
                gas_used,
                gas_limit: operation.gas_limit,
                fee_paid,
                error,
            },
        );
        Ok(operation_id)
    }

    fn apply_token_operation_mut(
        &mut self,
        operation: &TokenOperation,
        operation_id: Hash,
    ) -> Result<(), StateError> {
        let unsigned = &operation.unsigned;
        let sender = self
            .accounts
            .get(&unsigned.from)
            .cloned()
            .ok_or(StateError::TokenInsufficientFeeBalance)?;
        let sender_balance = sender
            .balance
            .checked_sub(unsigned.fee)
            .ok_or(StateError::TokenInsufficientFeeBalance)?;
        let sender_nonce = sender
            .nonce
            .checked_add(1)
            .ok_or(StateError::NonceOverflow)?;
        self.accounts.insert(
            unsigned.from,
            Account {
                balance: sender_balance,
                nonce: sender_nonce,
            },
        );
        self.fee_pool = self
            .fee_pool
            .checked_add(unsigned.fee)
            .ok_or(StateError::FeePoolOverflow)?;

        match unsigned.kind {
            TokenOperationKind::Create => {
                let token_id = token_id_from_operation(operation_id);
                if self.token_definitions.contains_key(&token_id) {
                    return Err(StateError::TokenAlreadyExists);
                }
                let definition = TokenDefinition {
                    token_id,
                    creator: unsigned.from,
                    name: unsigned.name.clone(),
                    symbol: unsigned.symbol.clone(),
                    decimals: unsigned.decimals,
                    total_supply: unsigned.amount,
                    max_supply: unsigned.max_supply,
                    mint_authority: unsigned.mint_authority,
                    burn_authority: unsigned.burn_authority,
                    freeze_authority: unsigned.freeze_authority,
                    metadata_uri: unsigned.metadata_uri.clone(),
                    metadata_hash: unsigned.metadata_hash,
                    paused: false,
                };
                self.token_definitions.insert(token_id, definition.clone());
                if unsigned.amount > 0 {
                    self.adjust_token_balance(&definition, unsigned.from, unsigned.amount, true)?;
                }
            }
            TokenOperationKind::Transfer => {
                let definition = self
                    .token_definitions
                    .get(&unsigned.token_id)
                    .cloned()
                    .ok_or(StateError::TokenNotFound)?;
                let recipient = unsigned.to.ok_or(StateError::TokenRecipientRequired)?;
                self.ensure_token_transferable(&definition, unsigned.from, recipient)?;
                self.adjust_token_balance(&definition, unsigned.from, unsigned.amount, false)?;
                self.adjust_token_balance(&definition, recipient, unsigned.amount, true)?;
            }
            TokenOperationKind::Mint => {
                let mut definition = self
                    .token_definitions
                    .get(&unsigned.token_id)
                    .cloned()
                    .ok_or(StateError::TokenNotFound)?;
                if definition.mint_authority != Some(unsigned.from) {
                    return Err(StateError::TokenUnauthorized);
                }
                let recipient = unsigned.to.ok_or(StateError::TokenRecipientRequired)?;
                if definition.paused
                    || self.is_token_account_frozen(&definition.token_id, &recipient)
                {
                    return Err(if definition.paused {
                        StateError::TokenPaused
                    } else {
                        StateError::TokenAccountFrozen
                    });
                }
                let total = definition
                    .total_supply
                    .checked_add(unsigned.amount)
                    .ok_or(StateError::TokenSupplyCapExceeded)?;
                if definition.max_supply.is_some_and(|cap| total > cap) {
                    return Err(StateError::TokenSupplyCapExceeded);
                }
                definition.total_supply = total;
                self.adjust_token_balance(&definition, recipient, unsigned.amount, true)?;
                self.token_definitions
                    .insert(definition.token_id, definition);
            }
            TokenOperationKind::Burn => {
                let mut definition = self
                    .token_definitions
                    .get(&unsigned.token_id)
                    .cloned()
                    .ok_or(StateError::TokenNotFound)?;
                self.ensure_token_transferable(&definition, unsigned.from, unsigned.from)?;
                self.adjust_token_balance(&definition, unsigned.from, unsigned.amount, false)?;
                definition.total_supply = definition
                    .total_supply
                    .checked_sub(unsigned.amount)
                    .ok_or(StateError::TokenInsufficientBalance)?;
                self.token_definitions
                    .insert(definition.token_id, definition);
            }
            TokenOperationKind::SetAuthorities => {
                let mut definition = self
                    .token_definitions
                    .get(&unsigned.token_id)
                    .cloned()
                    .ok_or(StateError::TokenNotFound)?;
                if definition.creator != unsigned.from {
                    return Err(StateError::TokenUnauthorized);
                }
                definition.mint_authority = unsigned.mint_authority;
                definition.burn_authority = unsigned.burn_authority;
                definition.freeze_authority = unsigned.freeze_authority;
                self.token_definitions
                    .insert(definition.token_id, definition);
            }
            TokenOperationKind::Freeze | TokenOperationKind::Unfreeze => {
                let definition = self
                    .token_definitions
                    .get(&unsigned.token_id)
                    .cloned()
                    .ok_or(StateError::TokenNotFound)?;
                if definition.freeze_authority != Some(unsigned.from) {
                    return Err(StateError::TokenUnauthorized);
                }
                let recipient = unsigned.to.ok_or(StateError::TokenRecipientRequired)?;
                if unsigned.kind == TokenOperationKind::Freeze {
                    self.frozen_token_accounts
                        .insert((definition.token_id, recipient));
                } else {
                    self.frozen_token_accounts
                        .remove(&(definition.token_id, recipient));
                }
            }
            TokenOperationKind::Pause | TokenOperationKind::Unpause => {
                let mut definition = self
                    .token_definitions
                    .get(&unsigned.token_id)
                    .cloned()
                    .ok_or(StateError::TokenNotFound)?;
                if definition.creator != unsigned.from {
                    return Err(StateError::TokenUnauthorized);
                }
                definition.paused = unsigned.kind == TokenOperationKind::Pause;
                self.token_definitions
                    .insert(definition.token_id, definition);
            }
            TokenOperationKind::UpdateMetadata => {
                let mut definition = self
                    .token_definitions
                    .get(&unsigned.token_id)
                    .cloned()
                    .ok_or(StateError::TokenNotFound)?;
                if definition.creator != unsigned.from || unsigned.metadata_uri.len() > 256 {
                    return Err(StateError::TokenUnauthorized);
                }
                definition.metadata_uri = unsigned.metadata_uri.clone();
                definition.metadata_hash = unsigned.metadata_hash;
                self.token_definitions
                    .insert(definition.token_id, definition);
            }
        }
        self.processed_token_operations.insert(operation_id);
        self.token_operation_records
            .insert(operation_id, operation.clone());
        Ok(())
    }

    fn token_asset_key(&self, definition: &TokenDefinition) -> String {
        wsc_core::AssetId::custom(definition.token_id, &definition.symbol, definition.decimals)
            .canonical_key()
    }

    fn adjust_token_balance(
        &mut self,
        definition: &TokenDefinition,
        address: Address,
        amount: Amount,
        add: bool,
    ) -> Result<(), StateError> {
        let key = self.token_asset_key(definition);
        let balances = self.asset_balances.entry(address).or_default();
        let current = balances.get(&key).copied().unwrap_or(0);
        let next = if add {
            current
                .checked_add(amount)
                .ok_or(StateError::TokenBalanceOverflow)?
        } else {
            current
                .checked_sub(amount)
                .ok_or(StateError::TokenInsufficientBalance)?
        };
        balances.insert(key, next);
        Ok(())
    }

    fn ensure_token_transferable(
        &self,
        definition: &TokenDefinition,
        from: Address,
        to: Address,
    ) -> Result<(), StateError> {
        if definition.paused {
            return Err(StateError::TokenPaused);
        }
        if self.is_token_account_frozen(&definition.token_id, &from)
            || self.is_token_account_frozen(&definition.token_id, &to)
        {
            return Err(StateError::TokenAccountFrozen);
        }
        Ok(())
    }

    pub fn credit_devnet(&mut self, address: Address, amount: Amount) -> Result<(), StateError> {
        let account = self.accounts.entry(address).or_default();
        account.balance = account
            .balance
            .checked_add(amount)
            .ok_or(StateError::BalanceOverflow)?;
        Ok(())
    }

    pub fn state_root(&self) -> Result<Hash, StateError> {
        let bytes = if self.programs.is_empty()
            && self.program_receipts.is_empty()
            && self.closed_programs.is_empty()
        {
            if self.token_definitions.is_empty()
                && self.processed_token_operations.is_empty()
                && self.token_operation_records.is_empty()
                && self.frozen_token_accounts.is_empty()
            {
                if self.asset_balances.is_empty()
                    && self.processed_asset_operations.is_empty()
                    && self.asset_operation_records.is_empty()
                {
                    canonical_encode(&LegacyStateSnapshot {
                        chain_id: self.chain_id.clone(),
                        fee_minimum: self.fee_minimum,
                        fee_pool: self.fee_pool,
                        accounts: self.accounts.clone(),
                    })
                } else {
                    canonical_encode(&LegacyAssetStateSnapshot {
                        chain_id: self.chain_id.clone(),
                        fee_minimum: self.fee_minimum,
                        fee_pool: self.fee_pool,
                        accounts: self.accounts.clone(),
                        asset_balances: self.asset_balances.clone(),
                        processed_asset_operations: self.processed_asset_operations.clone(),
                        asset_operation_records: self.asset_operation_records.clone(),
                    })
                }
            } else {
                canonical_encode(&PreProgramStateSnapshot {
                    chain_id: self.chain_id.clone(),
                    fee_minimum: self.fee_minimum,
                    fee_pool: self.fee_pool,
                    accounts: self.accounts.clone(),
                    asset_balances: self.asset_balances.clone(),
                    processed_asset_operations: self.processed_asset_operations.clone(),
                    asset_operation_records: self.asset_operation_records.clone(),
                    token_definitions: self.token_definitions.clone(),
                    processed_token_operations: self.processed_token_operations.clone(),
                    token_operation_records: self.token_operation_records.clone(),
                    frozen_token_accounts: self.frozen_token_accounts.clone(),
                    mna_reserve_ledger: self.mna_reserve_ledger.clone(),
                    processed_mna_swap_operations: self.processed_mna_swap_operations.clone(),
                    mna_swap_operation_records: self.mna_swap_operation_records.clone(),
                    processed_mna_reserve_operations: self.processed_mna_reserve_operations.clone(),
                    mna_reserve_operation_records: self.mna_reserve_operation_records.clone(),
                })
            }
        } else {
            canonical_encode(&self.snapshot())
        }
        .map_err(|error| StateError::Encoding(error.to_string()))?;
        Ok(Hash(sha256_domain(b"MNA/state/v1", &bytes)))
    }

    pub fn validate_transaction(&self, transaction: &Transaction) -> Result<Hash, StateError> {
        let unsigned = &transaction.unsigned;
        if unsigned.version != 1 {
            return Err(StateError::UnsupportedTransactionVersion);
        }
        if unsigned.chain_id != self.chain_id {
            return Err(StateError::WrongChainId);
        }
        if address_from_public_key(&unsigned.public_key) != unsigned.from {
            return Err(StateError::AddressMismatch);
        }
        if !KeyPair::verify(
            &unsigned.public_key,
            &transaction
                .signing_bytes()
                .map_err(|error| StateError::Encoding(error.to_string()))?,
            &transaction.signature,
        ) {
            return Err(StateError::InvalidSignature);
        }
        if unsigned.amount == 0 {
            return Err(StateError::ZeroAmount);
        }
        if unsigned.memo.len() > MAX_MEMO_BYTES {
            return Err(StateError::MemoTooLong);
        }
        if unsigned.fee < self.fee_minimum {
            return Err(StateError::FeeTooLow);
        }

        let faucet = self.chain_id.contains("devnet")
            && unsigned.public_key == wsc_crypto::KeyPair::devnet_faucet().public_key();
        let sender = self.accounts.get(&unsigned.from);
        if !faucet && sender.is_none() {
            return Err(StateError::InsufficientBalance);
        }
        let total = unsigned
            .amount
            .checked_add(unsigned.fee)
            .ok_or(StateError::BalanceOverflow)?;
        if !faucet && sender.expect("non-faucet sender exists").balance < total {
            return Err(StateError::InsufficientBalance);
        }
        if let Some(sender) = sender {
            let expected_nonce = sender.nonce;
            if unsigned.nonce != expected_nonce {
                return Err(StateError::NonceMismatch {
                    expected: expected_nonce,
                    actual: unsigned.nonce,
                });
            }
        }

        transaction_id(transaction)
            .map_err(|error| StateError::TransactionHashing(error.to_string()))
    }

    pub fn apply_transaction(&mut self, transaction: &Transaction) -> Result<Hash, StateError> {
        let tx_id = self.validate_transaction(transaction)?;
        let unsigned = &transaction.unsigned;
        let total = unsigned
            .amount
            .checked_add(unsigned.fee)
            .ok_or(StateError::BalanceOverflow)?;
        let faucet = self.chain_id.contains("devnet")
            && unsigned.public_key == wsc_crypto::KeyPair::devnet_faucet().public_key();
        let sender = self
            .accounts
            .get(&unsigned.from)
            .cloned()
            .unwrap_or_default();
        let receiver = self.accounts.get(&unsigned.to).cloned().unwrap_or_default();
        if faucet {
            let new_receiver_balance = receiver
                .balance
                .checked_add(unsigned.amount)
                .ok_or(StateError::BalanceOverflow)?;
            self.accounts.insert(
                unsigned.to,
                Account {
                    balance: new_receiver_balance,
                    nonce: receiver.nonce,
                },
            );
            self.fee_pool = self
                .fee_pool
                .checked_add(unsigned.fee)
                .ok_or(StateError::FeePoolOverflow)?;
            return Ok(tx_id);
        }

        let new_sender_balance = sender
            .balance
            .checked_sub(total)
            .ok_or(StateError::InsufficientBalance)?;
        let new_sender_nonce = sender
            .nonce
            .checked_add(1)
            .ok_or(StateError::NonceOverflow)?;
        let new_fee_pool = self
            .fee_pool
            .checked_add(unsigned.fee)
            .ok_or(StateError::FeePoolOverflow)?;

        if unsigned.from == unsigned.to {
            self.accounts.insert(
                unsigned.from,
                Account {
                    balance: new_sender_balance
                        .checked_add(unsigned.amount)
                        .ok_or(StateError::BalanceOverflow)?,
                    nonce: new_sender_nonce,
                },
            );
        } else {
            let new_receiver_balance = receiver
                .balance
                .checked_add(unsigned.amount)
                .ok_or(StateError::BalanceOverflow)?;
            self.accounts.insert(
                unsigned.from,
                Account {
                    balance: new_sender_balance,
                    nonce: new_sender_nonce,
                },
            );
            self.accounts.insert(
                unsigned.to,
                Account {
                    balance: new_receiver_balance,
                    nonce: receiver.nonce,
                },
            );
        }
        self.fee_pool = new_fee_pool;
        Ok(tx_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wsc_core::{
        Address, GenesisAllocation, TokenOperation, TokenOperationKind, UnsignedTokenOperation,
        UnsignedTransaction,
    };
    use wsc_crypto::KeyPair;

    fn genesis(address: Address, balance: Amount) -> GenesisConfig {
        GenesisConfig {
            version: 1,
            chain_id: "worldstreet-devnet-1".to_owned(),
            genesis_time: 1,
            block_time_ms: 2000,
            initial_supply: balance,
            fee_minimum: 1,
            validators: vec![],
            allocations: vec![GenesisAllocation { address, balance }],
            assets: vec![],
        }
    }

    #[test]
    fn valid_transfer_updates_balances_and_nonces() {
        let sender = KeyPair::generate().unwrap();
        let receiver = KeyPair::generate().unwrap();
        let mut state = State::from_genesis(&genesis(sender.address(), 100)).unwrap();
        let unsigned = UnsignedTransaction {
            version: 1,
            chain_id: state.chain_id().to_owned(),
            nonce: 0,
            from: sender.address(),
            to: receiver.address(),
            amount: 25,
            fee: 1,
            public_key: sender.public_key(),
            memo: String::new(),
        };
        let signature = sender.sign(&unsigned.signing_bytes().unwrap());
        let transaction = Transaction {
            unsigned,
            signature,
        };

        state.apply_transaction(&transaction).unwrap();

        assert_eq!(state.balance_of(&sender.address()), 74);
        assert_eq!(state.balance_of(&receiver.address()), 25);
        assert_eq!(state.nonce_of(&sender.address()), 1);
        assert_eq!(state.fee_pool(), 1);
    }

    #[test]
    fn invalid_signature_does_not_mutate_state() {
        let sender = KeyPair::generate().unwrap();
        let receiver = KeyPair::generate().unwrap();
        let mut state = State::from_genesis(&genesis(sender.address(), 100)).unwrap();
        let unsigned = UnsignedTransaction {
            version: 1,
            chain_id: state.chain_id().to_owned(),
            nonce: 0,
            from: sender.address(),
            to: receiver.address(),
            amount: 25,
            fee: 1,
            public_key: sender.public_key(),
            memo: String::new(),
        };
        let transaction = Transaction {
            unsigned,
            signature: KeyPair::generate().unwrap().sign(b"wrong"),
        };

        assert!(matches!(
            state.apply_transaction(&transaction),
            Err(StateError::InvalidSignature)
        ));
        assert_eq!(state.balance_of(&sender.address()), 100);
        assert_eq!(state.nonce_of(&sender.address()), 0);
    }

    #[test]
    fn wrapped_asset_mint_burn_is_idempotent_and_balanced() {
        let owner = KeyPair::generate().unwrap();
        let mut state = State::from_genesis(&genesis(owner.address(), 0)).unwrap();
        let asset = wsc_core::AssetId::wrapped("ethereum", "WETH", "sepolia:bridge", 18);
        let mint = AssetOperation {
            version: 1,
            operation_id: Hash([7; 32]),
            kind: AssetOperationKind::Mint,
            asset_id: asset.clone(),
            address: owner.address(),
            destination: owner.address().to_string(),
            amount: 10,
            external_transaction: "deposit".to_owned(),
            memo: String::new(),
        };
        assert!(state.apply_asset_operation(&mint).unwrap());
        assert!(!state.apply_asset_operation(&mint).unwrap());
        assert_eq!(
            state.asset_balance_of(&owner.address(), &asset.canonical_key()),
            10
        );
        let burn = AssetOperation {
            operation_id: Hash([8; 32]),
            kind: AssetOperationKind::Burn,
            amount: 4,
            ..mint
        };
        assert!(state.apply_asset_operation(&burn).unwrap());
        assert_eq!(
            state.asset_balance_of(&owner.address(), &asset.canonical_key()),
            6
        );
        assert_eq!(state.asset_operation_records().len(), 2);
    }

    #[test]
    fn native_token_create_transfer_and_idempotency() {
        let creator = KeyPair::generate().unwrap();
        let recipient = KeyPair::generate().unwrap();
        let mut state = State::from_genesis(&genesis(creator.address(), 100)).unwrap();
        let create_unsigned = UnsignedTokenOperation {
            version: 1,
            chain_id: state.chain_id().to_owned(),
            nonce: 0,
            from: creator.address(),
            kind: TokenOperationKind::Create,
            token_id: Hash::ZERO,
            to: None,
            amount: 50,
            fee: 1,
            public_key: creator.public_key(),
            name: "Example Token".to_owned(),
            symbol: "EXT".to_owned(),
            decimals: 6,
            max_supply: Some(100),
            mint_authority: Some(creator.address()),
            burn_authority: None,
            freeze_authority: Some(creator.address()),
            metadata_uri: String::new(),
            metadata_hash: Hash::ZERO,
            memo: String::new(),
        };
        let create = TokenOperation {
            unsigned: create_unsigned.clone(),
            signature: creator.sign(&create_unsigned.signing_bytes().unwrap()),
        };
        let create_id = token_operation_id(&create).unwrap();
        assert!(state.apply_token_operation(&create, create_id).unwrap());
        assert!(!state.apply_token_operation(&create, create_id).unwrap());
        let token_id = token_id_from_operation(create_id);
        let definition = state.token_definition(&token_id).unwrap().clone();
        assert_eq!(definition.total_supply, 50);
        assert_eq!(state.token_balance(&creator.address(), &definition), 50);

        let transfer_unsigned = UnsignedTokenOperation {
            version: 1,
            chain_id: state.chain_id().to_owned(),
            nonce: 1,
            from: creator.address(),
            kind: TokenOperationKind::Transfer,
            token_id,
            to: Some(recipient.address()),
            amount: 10,
            fee: 1,
            public_key: creator.public_key(),
            name: String::new(),
            symbol: String::new(),
            decimals: 0,
            max_supply: None,
            mint_authority: None,
            burn_authority: None,
            freeze_authority: None,
            metadata_uri: String::new(),
            metadata_hash: Hash::ZERO,
            memo: String::new(),
        };
        let transfer = TokenOperation {
            unsigned: transfer_unsigned.clone(),
            signature: creator.sign(&transfer_unsigned.signing_bytes().unwrap()),
        };
        let transfer_id = token_operation_id(&transfer).unwrap();
        assert!(state.apply_token_operation(&transfer, transfer_id).unwrap());
        assert_eq!(state.token_balance(&creator.address(), &definition), 40);
        assert_eq!(state.token_balance(&recipient.address(), &definition), 10);
        assert_eq!(state.balance_of(&creator.address()), 98);
        assert_eq!(state.nonce_of(&creator.address()), 2);
        assert!(state.state_root().is_ok());
    }

    #[test]
    fn genesis_supply_is_checked() {
        let key = KeyPair::generate().unwrap();
        let mut config = genesis(key.address(), 100);
        config.allocations[0].balance = 99;
        assert!(matches!(
            State::from_genesis(&config),
            Err(StateError::GenesisSupplyMismatch)
        ));
    }

    fn signed_program_operation(
        key: &KeyPair,
        kind: ProgramOperationKind,
        program_id: &str,
        package: Vec<u8>,
        gas_limit: u64,
        nonce: u64,
        fee: u128,
    ) -> ProgramOperation {
        let action = match kind {
            ProgramOperationKind::Deploy => "deploy",
            ProgramOperationKind::Call => "call",
            ProgramOperationKind::StorageSet => "storage_set",
            ProgramOperationKind::Close => "close",
        };
        let mut message = format!(
            "Intertrain Program Authorization\nAction: {action}\nChain ID: worldstreet-devnet-1\nProgram ID: {program_id}\nOwner: {}\nNonce: {nonce}\nFee: {fee}",
            key.address()
        );
        if kind == ProgramOperationKind::Call {
            message.push_str(&format!("\nGas Limit: {gas_limit}"));
        }
        ProgramOperation {
            version: 1,
            chain_id: "worldstreet-devnet-1".into(),
            kind,
            nonce,
            fee,
            program_id: program_id.into(),
            package,
            gas_limit,
            key: String::new(),
            value: String::new(),
            public_key: key.public_key(),
            signature: key.sign(message.as_bytes()),
        }
    }

    #[test]
    fn program_deploy_and_call_are_consensus_state_with_fees() {
        let owner = KeyPair::generate().unwrap();
        let mut state = State::from_genesis(&genesis(owner.address(), 1_000_000)).unwrap();
        let package = wsc_program::compile_rust_source("demo", "fn main() -> i32 { 7 }").unwrap();
        let program_id = package.program_id();
        let package_bytes = package.encode().unwrap();
        let deploy_fee = 1 + package_bytes.len() as u128;
        let deploy = signed_program_operation(
            &owner,
            ProgramOperationKind::Deploy,
            &program_id,
            package_bytes,
            0,
            0,
            deploy_fee,
        );
        let root_before = state.state_root().unwrap();
        assert!(state.apply_program_operation(&deploy, 1).unwrap());
        assert_ne!(root_before, state.state_root().unwrap());
        assert_eq!(state.programs()[&program_id].deployed_at_height, 1);
        assert_eq!(state.fee_pool(), deploy_fee);

        let call = signed_program_operation(
            &owner,
            ProgramOperationKind::Call,
            &program_id,
            vec![],
            100_000,
            1,
            100_001,
        );
        assert!(state.apply_program_operation(&call, 2).unwrap());
        let call_id = program_operation_id(&call).unwrap();
        let receipt = &state.program_receipts()[&call_id];
        assert_eq!(
            i32::from_le_bytes(receipt.return_data.clone().try_into().unwrap()),
            7
        );
        assert_eq!(receipt.fee_paid, 1 + receipt.gas_used as u128);
        assert_eq!(state.nonce_of(&owner.address()), 2);
        assert_eq!(
            State::from_snapshot(state.snapshot()).state_root().unwrap(),
            state.state_root().unwrap()
        );
    }
}
