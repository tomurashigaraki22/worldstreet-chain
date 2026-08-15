use bech32::{Bech32m, Hrp};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::{fmt, str::FromStr};
use thiserror::Error;

pub const CHAIN_ID: &str = "worldstreet-devnet-1";
pub const NATIVE_ASSET_NAME: &str = "MANNA";
pub const NATIVE_ASSET_SYMBOL: &str = "MNA";
pub const NATIVE_ASSET_DECIMALS: u8 = 6;
pub const ADDRESS_HRP: &str = "mna";
pub const ADDRESS_VERSION: u8 = 1;

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AssetId {
    pub namespace: String,
    pub symbol: String,
    pub contract: Option<String>,
    pub decimals: u8,
}

pub type Amount = u128;
pub type Nonce = u64;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GenesisAllocation {
    pub address: Address,
    pub balance: Amount,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Validator {
    pub name: String,
    pub public_key: PublicKey,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GenesisConfig {
    pub version: u8,
    pub chain_id: String,
    pub genesis_time: u64,
    pub block_time_ms: u64,
    pub initial_supply: Amount,
    pub fee_minimum: Amount,
    pub validators: Vec<Validator>,
    pub allocations: Vec<GenesisAllocation>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnsignedTransaction {
    pub version: u8,
    pub chain_id: String,
    pub nonce: Nonce,
    pub from: Address,
    pub to: Address,
    pub amount: Amount,
    pub fee: Amount,
    pub public_key: PublicKey,
    pub memo: String,
}

impl UnsignedTransaction {
    pub fn signing_bytes(&self) -> Result<Vec<u8>, EncodingError> {
        canonical_encode(self)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Transaction {
    pub unsigned: UnsignedTransaction,
    pub signature: Signature,
}

impl Transaction {
    pub fn signing_bytes(&self) -> Result<Vec<u8>, EncodingError> {
        self.unsigned.signing_bytes()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlockHeader {
    pub version: u8,
    pub chain_id: String,
    pub height: u64,
    pub parent_hash: Hash,
    pub timestamp: u64,
    pub transaction_root: Hash,
    pub state_root: Hash,
    pub proposer: Option<PublicKey>,
    pub proposer_signature: Option<Signature>,
}

impl BlockHeader {
    pub fn signing_bytes(&self) -> Result<Vec<u8>, EncodingError> {
        let mut unsigned = self.clone();
        unsigned.proposer_signature = None;
        canonical_encode(&unsigned)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Block {
    pub header: BlockHeader,
    pub transactions: Vec<Transaction>,
}

impl AssetId {
    pub fn native() -> Self {
        Self {
            namespace: "worldstreet".to_owned(),
            symbol: NATIVE_ASSET_SYMBOL.to_owned(),
            contract: None,
            decimals: NATIVE_ASSET_DECIMALS,
        }
    }

    pub fn wrapped(
        namespace: impl Into<String>,
        symbol: impl Into<String>,
        contract: impl Into<String>,
        decimals: u8,
    ) -> Self {
        Self {
            namespace: namespace.into(),
            symbol: symbol.into(),
            contract: Some(contract.into()),
            decimals,
        }
    }
}

#[derive(Debug, Error)]
pub enum EncodingError {
    #[error("canonical encoding failed: {0}")]
    Encode(String),
    #[error("canonical decoding failed: {0}")]
    Decode(String),
}

pub fn canonical_encode<T: Serialize>(value: &T) -> Result<Vec<u8>, EncodingError> {
    postcard::to_allocvec(value).map_err(|error| EncodingError::Encode(error.to_string()))
}

pub fn canonical_decode<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, EncodingError> {
    postcard::from_bytes(bytes).map_err(|error| EncodingError::Decode(error.to_string()))
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Hash(pub [u8; 32]);

impl Hash {
    pub const ZERO: Self = Self([0; 32]);

    pub const fn new(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl fmt::Debug for Hash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("Hash").field(&hex_string(&self.0)).finish()
    }
}

impl fmt::Display for Hash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&hex_string(&self.0))
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct PublicKey(pub [u8; 32]);

impl fmt::Debug for PublicKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("PublicKey").field(&hex_string(&self.0)).finish()
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Signature(pub [u8; 64]);

impl fmt::Debug for Signature {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("Signature").field(&hex_string(&self.0)).finish()
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Address(pub [u8; 21]);

#[derive(Debug, Error, PartialEq, Eq)]
pub enum AddressError {
    #[error("invalid Bech32m address")]
    InvalidEncoding,
    #[error("address has the wrong human-readable prefix")]
    WrongPrefix,
    #[error("address payload must be 21 bytes")]
    WrongLength,
    #[error("unsupported address version")]
    UnsupportedVersion,
}

impl Address {
    pub const fn from_hash(hash: [u8; 20]) -> Self {
        let mut bytes = [0u8; 21];
        bytes[0] = ADDRESS_VERSION;
        let mut i = 0;
        while i < 20 {
            bytes[i + 1] = hash[i];
            i += 1;
        }
        Self(bytes)
    }

    pub const fn as_bytes(&self) -> &[u8; 21] {
        &self.0
    }

    pub fn to_bech32m(self) -> String {
        let hrp = Hrp::parse(ADDRESS_HRP).expect("static address HRP is valid");
        bech32::encode::<Bech32m>(hrp, &self.0).expect("address encoding cannot fail")
    }

    pub fn from_bech32m(value: &str) -> Result<Self, AddressError> {
        let (hrp, data) = bech32::decode_bech32m(value).map_err(|_| AddressError::InvalidEncoding)?;
        if hrp.as_str() != ADDRESS_HRP {
            return Err(AddressError::WrongPrefix);
        }
        if data.len() != 21 {
            return Err(AddressError::WrongLength);
        }
        if data[0] != ADDRESS_VERSION {
            return Err(AddressError::UnsupportedVersion);
        }
        let mut bytes = [0u8; 21];
        bytes.copy_from_slice(&data);
        Ok(Self(bytes))
    }
}

impl fmt::Debug for Address {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("Address").field(&self.to_bech32m()).finish()
    }
}

impl fmt::Display for Address {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_bech32m())
    }
}

impl FromStr for Address {
    type Err = AddressError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::from_bech32m(value)
    }
}

fn hex_string(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn address_round_trip() {
        let address = Address::from_hash([7; 20]);
        let encoded = address.to_bech32m();
        assert_eq!(Address::from_bech32m(&encoded).unwrap(), address);
        assert!(encoded.starts_with("mna1"));
    }

    #[test]
    fn rejects_wrong_prefix() {
        let address = Address::from_hash([7; 20]);
        let wrong = bech32::encode::<Bech32m>(
            Hrp::parse("wsc").unwrap(),
            address.as_bytes(),
        )
        .unwrap();
        assert_eq!(Address::from_bech32m(&wrong), Err(AddressError::WrongPrefix));
    }

    #[test]
    fn canonical_encoding_is_stable() {
        let first = canonical_encode(&(1u32, 2u32, 3u32)).unwrap();
        let second = canonical_encode(&(1u32, 2u32, 3u32)).unwrap();
        assert_eq!(first, second);
    }

    #[test]
    fn asset_identity_is_not_address_specific() {
        let native = AssetId::native();
        let wrapped_eth = AssetId::wrapped("ethereum", "WETH", "0xbridge", 18);
        assert_eq!(native.symbol, "MNA");
        assert_eq!(wrapped_eth.symbol, "WETH");
        assert_ne!(native, wrapped_eth);
    }
}
