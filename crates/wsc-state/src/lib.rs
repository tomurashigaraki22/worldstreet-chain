use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use thiserror::Error;
use wsc_core::{
    canonical_encode, Address, Amount, GenesisConfig, Hash, Transaction,
};
use wsc_crypto::{address_from_public_key, sha256_domain, transaction_id, KeyPair};

pub const MAX_MEMO_BYTES: usize = 256;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Account {
    pub balance: Amount,
    pub nonce: u64,
}

impl Default for Account {
    fn default() -> Self {
        Self { balance: 0, nonce: 0 }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StateSnapshot {
    pub chain_id: String,
    pub fee_minimum: Amount,
    pub fee_pool: Amount,
    pub accounts: BTreeMap<Address, Account>,
}

#[derive(Clone, Debug)]
pub struct State {
    chain_id: String,
    fee_minimum: Amount,
    fee_pool: Amount,
    accounts: BTreeMap<Address, Account>,
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
    #[error("state encoding failed: {0}")]
    Encoding(String),
    #[error("transaction hashing failed: {0}")]
    TransactionHashing(String),
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
        })
    }

    pub fn from_snapshot(snapshot: StateSnapshot) -> Self {
        Self {
            chain_id: snapshot.chain_id,
            fee_minimum: snapshot.fee_minimum,
            fee_pool: snapshot.fee_pool,
            accounts: snapshot.accounts,
        }
    }

    pub fn snapshot(&self) -> StateSnapshot {
        StateSnapshot {
            chain_id: self.chain_id.clone(),
            fee_minimum: self.fee_minimum,
            fee_pool: self.fee_pool,
            accounts: self.accounts.clone(),
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
        self.accounts.get(address).map(|account| account.balance).unwrap_or(0)
    }

    pub fn nonce_of(&self, address: &Address) -> u64 {
        self.accounts.get(address).map(|account| account.nonce).unwrap_or(0)
    }

    pub fn accounts(&self) -> &BTreeMap<Address, Account> {
        &self.accounts
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
        let bytes = canonical_encode(&self.snapshot())
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
        if !KeyPair::verify(&unsigned.public_key, &transaction.signing_bytes().map_err(|error| StateError::Encoding(error.to_string()))?, &transaction.signature) {
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

        let sender = self.accounts.get(&unsigned.from).ok_or(StateError::InsufficientBalance)?;
        let total = unsigned
            .amount
            .checked_add(unsigned.fee)
            .ok_or(StateError::BalanceOverflow)?;
        if sender.balance < total {
            return Err(StateError::InsufficientBalance);
        }
        let expected_nonce = sender.nonce;
        if unsigned.nonce != expected_nonce {
            return Err(StateError::NonceMismatch {
                expected: expected_nonce,
                actual: unsigned.nonce,
            });
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
        let sender = self
            .accounts
            .get(&unsigned.from)
            .cloned()
            .ok_or(StateError::InsufficientBalance)?;
        let receiver = self.accounts.get(&unsigned.to).cloned().unwrap_or_default();

        let new_sender_balance = sender
            .balance
            .checked_sub(total)
            .ok_or(StateError::InsufficientBalance)?;
        let new_sender_nonce = sender.nonce.checked_add(1).ok_or(StateError::NonceOverflow)?;
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
    use wsc_core::{Address, GenesisAllocation, UnsignedTransaction};
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
        let transaction = Transaction { unsigned, signature };

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
    fn genesis_supply_is_checked() {
        let key = KeyPair::generate().unwrap();
        let mut config = genesis(key.address(), 100);
        config.allocations[0].balance = 99;
        assert!(matches!(
            State::from_genesis(&config),
            Err(StateError::GenesisSupplyMismatch)
        ));
    }
}
