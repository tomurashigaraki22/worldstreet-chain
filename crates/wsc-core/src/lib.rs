use bech32::{primitives::decode::CheckedHrpstring, Bech32m, Hrp};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_big_array::BigArray;
use std::{collections::BTreeMap, fmt, str::FromStr};
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

impl AssetId {
    pub fn canonical_key(&self) -> String {
        format!(
            "{}:{}:{}",
            self.namespace,
            self.symbol,
            self.contract.as_deref().unwrap_or("native")
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssetDefinition {
    pub id: AssetId,
    pub display_name: String,
    pub wrapped: bool,
    #[serde(default)]
    pub enabled: bool,
}

impl AssetDefinition {
    pub fn weth_ethereum(reference: impl Into<String>) -> Self {
        Self {
            id: AssetId::wrapped("ethereum", "WETH", reference, 18),
            display_name: "Wrapped Ether".to_owned(),
            wrapped: true,
            enabled: false,
        }
    }

    pub fn wsol_solana(reference: impl Into<String>) -> Self {
        Self {
            id: AssetId::wrapped("solana", "WSOL", reference, 9),
            display_name: "Wrapped SOL".to_owned(),
            wrapped: true,
            enabled: false,
        }
    }
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
    #[serde(default)]
    pub assets: Vec<AssetDefinition>,
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AssetOperationKind {
    Mint,
    Burn,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssetOperation {
    pub version: u8,
    pub operation_id: Hash,
    pub kind: AssetOperationKind,
    pub asset_id: AssetId,
    pub address: Address,
    #[serde(default)]
    pub destination: String,
    pub amount: Amount,
    pub external_transaction: String,
    pub memo: String,
}

/// Reserve-backed MNA conversion rate: two USDC base units represent one MNA
/// base unit. Both assets use six decimals, so conversion is integer-exact.
pub const MNA_USDC_NUMERATOR: Amount = 1;
pub const MNA_USDC_DENOMINATOR: Amount = 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum MnaSwapKind {
    MintMna,
    RedeemMna,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnsignedMnaSwapOperation {
    pub version: u8,
    pub chain_id: String,
    pub nonce: Nonce,
    pub from: Address,
    pub kind: MnaSwapKind,
    pub collateral_asset: AssetId,
    pub amount_usdc: Amount,
    pub amount_mna: Amount,
    pub fee: Amount,
    pub public_key: PublicKey,
    #[serde(default)]
    pub memo: String,
}

impl UnsignedMnaSwapOperation {
    pub fn signing_bytes(&self) -> Result<Vec<u8>, EncodingError> {
        canonical_encode(self)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MnaSwapOperation {
    pub unsigned: UnsignedMnaSwapOperation,
    pub signature: Signature,
}

impl MnaSwapOperation {
    pub fn signing_bytes(&self) -> Result<Vec<u8>, EncodingError> {
        self.unsigned.signing_bytes()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum MnaReserveOperationKind {
    VerifyDeposit,
    Release,
}

/// Operator-submitted external reserve accounting. It never authorizes
/// arbitrary minting: the relayer must supply a finalized external tx/log ID,
/// the exact 2:1 conversion, and a configured USDC asset identity.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MnaReserveOperation {
    pub version: u8,
    pub operation_id: Hash,
    pub kind: MnaReserveOperationKind,
    pub collateral_asset: AssetId,
    pub address: Address,
    pub amount_usdc: Amount,
    pub amount_mna: Amount,
    /// External collateral amount in the smallest unit (lamports for SOL).
    #[serde(default)]
    pub collateral_amount: Amount,
    /// Oracle snapshot in micro-USD per SOL; zero for USDC operations.
    #[serde(default)]
    pub oracle_price_usd_micro_per_sol: Amount,
    #[serde(default)]
    pub oracle_timestamp: u64,
    /// Direct SOL lane fee retained in MNA base units.
    #[serde(default)]
    pub fee_mna: Amount,
    #[serde(default)]
    pub destination: String,
    pub external_transaction: String,
    #[serde(default)]
    pub memo: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MnaReserveLedger {
    pub total_verified_deposits_usdc: Amount,
    pub total_released_usdc: Amount,
    pub reserve_backed_mna_minted: Amount,
    pub total_redeemed_mna: Amount,
    #[serde(default)]
    pub total_verified_sol_lamports: Amount,
    #[serde(default)]
    pub total_released_sol_lamports: Amount,
    #[serde(default)]
    pub total_verified_sol_usd: Amount,
    #[serde(default)]
    pub total_released_sol_usd: Amount,
    pub paused: bool,
}

impl Default for MnaReserveLedger {
    fn default() -> Self {
        Self {
            total_verified_deposits_usdc: 0,
            total_released_usdc: 0,
            reserve_backed_mna_minted: 0,
            total_redeemed_mna: 0,
            total_verified_sol_lamports: 0,
            total_released_sol_lamports: 0,
            total_verified_sol_usd: 0,
            total_released_sol_usd: 0,
            paused: false,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TokenOperationKind {
    Create,
    Transfer,
    Mint,
    Burn,
    SetAuthorities,
    Freeze,
    Unfreeze,
    Pause,
    Unpause,
    UpdateMetadata,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnsignedTokenOperation {
    pub version: u8,
    pub chain_id: String,
    pub nonce: Nonce,
    pub from: Address,
    pub kind: TokenOperationKind,
    /// Hash::ZERO for Create; the canonical token ID for all other operations.
    pub token_id: Hash,
    pub to: Option<Address>,
    pub amount: Amount,
    pub fee: Amount,
    pub public_key: PublicKey,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub symbol: String,
    #[serde(default)]
    pub decimals: u8,
    #[serde(default)]
    pub max_supply: Option<Amount>,
    #[serde(default)]
    pub mint_authority: Option<Address>,
    #[serde(default)]
    pub burn_authority: Option<Address>,
    #[serde(default)]
    pub freeze_authority: Option<Address>,
    #[serde(default)]
    pub metadata_uri: String,
    #[serde(default)]
    pub metadata_hash: Hash,
    #[serde(default)]
    pub memo: String,
}

impl UnsignedTokenOperation {
    pub fn signing_bytes(&self) -> Result<Vec<u8>, EncodingError> {
        canonical_encode(self)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenOperation {
    pub unsigned: UnsignedTokenOperation,
    pub signature: Signature,
}

impl TokenOperation {
    pub fn signing_bytes(&self) -> Result<Vec<u8>, EncodingError> {
        self.unsigned.signing_bytes()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenDefinition {
    pub token_id: Hash,
    pub creator: Address,
    pub name: String,
    pub symbol: String,
    pub decimals: u8,
    pub total_supply: Amount,
    pub max_supply: Option<Amount>,
    pub mint_authority: Option<Address>,
    pub burn_authority: Option<Address>,
    pub freeze_authority: Option<Address>,
    pub metadata_uri: String,
    pub metadata_hash: Hash,
    pub paused: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProgramOperationKind {
    Deploy,
    Call,
    StorageSet,
    Close,
}

/// A consensus-carried Intertrain program operation. Signatures use the
/// domain-separated program authorization message enforced by `wsc-state`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProgramOperation {
    pub version: u8,
    pub chain_id: String,
    pub kind: ProgramOperationKind,
    pub nonce: u64,
    pub fee: Amount,
    pub program_id: String,
    #[serde(default)]
    pub package: Vec<u8>,
    #[serde(default)]
    pub gas_limit: u64,
    #[serde(default)]
    pub key: String,
    #[serde(default)]
    pub value: String,
    pub public_key: PublicKey,
    pub signature: Signature,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProgramRecord {
    pub package: Vec<u8>,
    pub creator: Address,
    pub deployed_at_height: u64,
    pub storage: BTreeMap<String, String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProgramReceiptRecord {
    pub operation_id: Hash,
    pub program_id: String,
    pub kind: ProgramOperationKind,
    pub status: String,
    pub return_data: Vec<u8>,
    pub gas_used: u64,
    pub gas_limit: u64,
    pub fee_paid: Amount,
    pub error: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Block {
    pub header: BlockHeader,
    pub transactions: Vec<Transaction>,
    #[serde(default)]
    pub asset_operations: Vec<AssetOperation>,
    #[serde(default)]
    pub token_operations: Vec<TokenOperation>,
    #[serde(default)]
    pub mna_swap_operations: Vec<MnaSwapOperation>,
    #[serde(default)]
    pub mna_reserve_operations: Vec<MnaReserveOperation>,
    #[serde(default)]
    pub program_operations: Vec<ProgramOperation>,
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

    pub fn custom(token_id: Hash, symbol: impl Into<String>, decimals: u8) -> Self {
        Self {
            namespace: "intertrain".to_owned(),
            symbol: symbol.into(),
            contract: Some(format!("token:{token_id}")),
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

#[derive(Clone, Copy, Default, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
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
        f.debug_tuple("PublicKey")
            .field(&hex_string(&self.0))
            .finish()
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Signature(#[serde(with = "BigArray")] pub [u8; 64]);

impl fmt::Debug for Signature {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("Signature")
            .field(&hex_string(&self.0))
            .finish()
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
        let checked =
            CheckedHrpstring::new::<Bech32m>(value).map_err(|_| AddressError::InvalidEncoding)?;
        if checked.hrp().as_str() != ADDRESS_HRP {
            return Err(AddressError::WrongPrefix);
        }
        let data: Vec<u8> = checked.byte_iter().collect();
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
        let wrong =
            bech32::encode::<Bech32m>(Hrp::parse("wsc").unwrap(), address.as_bytes()).unwrap();
        assert_eq!(
            Address::from_bech32m(&wrong),
            Err(AddressError::WrongPrefix)
        );
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
