use bip39::Mnemonic;
use ed25519_dalek::{Signer, Verifier};
use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256, Sha512};
use thiserror::Error;
use wsc_core::{
    canonical_encode, Address, BlockHeader, Hash, MnaReserveOperation, MnaSwapOperation,
    ProgramOperation, PublicKey, Signature, TokenOperation, Transaction,
};
use zeroize::Zeroize;

type HmacSha512 = Hmac<Sha512>;

const SLIP10_ED25519_SEED: &[u8] = b"ed25519 seed";
const ADDRESS_DOMAIN: &[u8] = b"MNA/address/v1";

#[derive(Debug, Error)]
pub enum CryptoError {
    #[error("secure random generation failed: {0}")]
    Random(String),
    #[error("invalid mnemonic: {0}")]
    Mnemonic(String),
    #[error("invalid derivation path")]
    DerivationPath,
    #[error("invalid child derivation index")]
    DerivationIndex,
    #[error("invalid private key")]
    PrivateKey,
    #[error("invalid public key")]
    PublicKey,
    #[error("invalid signature")]
    Signature,
    #[error("canonical encoding failed: {0}")]
    Encoding(String),
}

pub const DEVNET_FAUCET_SECRET: [u8; 32] = [0xf0; 32];

pub struct KeyPair {
    signing_key: ed25519_dalek::SigningKey,
}

impl KeyPair {
    pub fn devnet_faucet() -> Self {
        Self::from_secret_bytes(DEVNET_FAUCET_SECRET)
    }

    pub fn generate() -> Result<Self, CryptoError> {
        let mut secret = [0u8; 32];
        getrandom::fill(&mut secret).map_err(|error| CryptoError::Random(error.to_string()))?;
        let key_pair = Self::from_secret_bytes(secret);
        secret.zeroize();
        Ok(key_pair)
    }

    pub fn from_secret_bytes(mut secret: [u8; 32]) -> Self {
        let signing_key = ed25519_dalek::SigningKey::from_bytes(&secret);
        secret.zeroize();
        Self { signing_key }
    }

    pub fn secret_bytes(&self) -> [u8; 32] {
        self.signing_key.to_bytes()
    }

    pub fn public_key(&self) -> PublicKey {
        PublicKey(self.signing_key.verifying_key().to_bytes())
    }

    pub fn address(&self) -> Address {
        address_from_public_key(&self.public_key())
    }

    pub fn sign(&self, message: &[u8]) -> Signature {
        Signature(self.signing_key.sign(message).to_bytes())
    }

    pub fn verify(public_key: &PublicKey, message: &[u8], signature: &Signature) -> bool {
        let verifying_key = match ed25519_dalek::VerifyingKey::from_bytes(&public_key.0) {
            Ok(key) => key,
            Err(_) => return false,
        };
        let signature = ed25519_dalek::Signature::from_bytes(&signature.0);
        verifying_key.verify(message, &signature).is_ok()
    }
}

impl Drop for KeyPair {
    fn drop(&mut self) {
        let mut secret = self.signing_key.to_bytes();
        secret.zeroize();
    }
}

pub fn generate_mnemonic() -> Result<String, CryptoError> {
    let mut entropy = [0u8; 32];
    getrandom::fill(&mut entropy).map_err(|error| CryptoError::Random(error.to_string()))?;
    let mnemonic = Mnemonic::from_entropy(&entropy)
        .map_err(|error| CryptoError::Mnemonic(error.to_string()))?;
    entropy.zeroize();
    Ok(mnemonic.to_string())
}

pub fn mnemonic_to_seed(mnemonic: &str, passphrase: &str) -> Result<[u8; 64], CryptoError> {
    let mnemonic =
        Mnemonic::parse(mnemonic).map_err(|error| CryptoError::Mnemonic(error.to_string()))?;
    Ok(mnemonic.to_seed(passphrase))
}

pub fn derive_wallet_key(seed: &[u8; 64]) -> Result<KeyPair, CryptoError> {
    let path = [44u32, 9999u32, 0u32, 0u32, 0u32];
    let secret = derive_hardened_path(seed, &path)?;
    Ok(KeyPair::from_secret_bytes(secret))
}

pub fn derive_hardened_path(seed: &[u8; 64], path: &[u32]) -> Result<[u8; 32], CryptoError> {
    let mut mac =
        HmacSha512::new_from_slice(SLIP10_ED25519_SEED).map_err(|_| CryptoError::DerivationPath)?;
    mac.update(seed);
    let master = mac.finalize().into_bytes();

    let mut key = [0u8; 32];
    let mut chain_code = [0u8; 32];
    key.copy_from_slice(&master[..32]);
    chain_code.copy_from_slice(&master[32..]);

    for index in path {
        if *index >= 0x8000_0000 {
            return Err(CryptoError::DerivationIndex);
        }
        let child_index = index | 0x8000_0000;
        let mut child_mac =
            HmacSha512::new_from_slice(&chain_code).map_err(|_| CryptoError::DerivationPath)?;
        child_mac.update(&[0]);
        child_mac.update(&key);
        child_mac.update(&child_index.to_be_bytes());
        let child = child_mac.finalize().into_bytes();
        key.copy_from_slice(&child[..32]);
        chain_code.copy_from_slice(&child[32..]);
    }

    chain_code.zeroize();
    Ok(key)
}

pub fn address_from_public_key(public_key: &PublicKey) -> Address {
    let mut hasher = Sha256::new();
    hasher.update(ADDRESS_DOMAIN);
    hasher.update(public_key.0);
    let digest = hasher.finalize();
    let mut hash = [0u8; 20];
    hash.copy_from_slice(&digest[..20]);
    Address::from_hash(hash)
}

pub fn sha256_domain(domain: &[u8], bytes: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(domain);
    hasher.update(bytes);
    hasher.finalize().into()
}

pub fn transaction_id(transaction: &Transaction) -> Result<Hash, CryptoError> {
    let bytes = transaction
        .signing_bytes()
        .map_err(|error| CryptoError::Encoding(error.to_string()))?;
    Ok(Hash(sha256_domain(b"MNA/tx/v1", &bytes)))
}

pub fn token_operation_id(operation: &TokenOperation) -> Result<Hash, CryptoError> {
    let bytes = operation
        .signing_bytes()
        .map_err(|error| CryptoError::Encoding(error.to_string()))?;
    Ok(Hash(sha256_domain(b"MNA/token-operation/v1", &bytes)))
}

pub fn token_id_from_operation(operation_id: Hash) -> Hash {
    Hash(sha256_domain(b"MNA/token-id/v1", operation_id.as_bytes()))
}

pub fn mna_swap_operation_id(operation: &MnaSwapOperation) -> Result<Hash, CryptoError> {
    let bytes = operation
        .signing_bytes()
        .map_err(|error| CryptoError::Encoding(error.to_string()))?;
    Ok(Hash(sha256_domain(b"MNA/mna-swap/v1", &bytes)))
}

pub fn mna_reserve_operation_id(operation: &MnaReserveOperation) -> Result<Hash, CryptoError> {
    let bytes =
        canonical_encode(operation).map_err(|error| CryptoError::Encoding(error.to_string()))?;
    Ok(Hash(sha256_domain(b"MNA/mna-reserve/v1", &bytes)))
}

pub fn program_operation_id(operation: &ProgramOperation) -> Result<Hash, CryptoError> {
    let bytes =
        canonical_encode(operation).map_err(|error| CryptoError::Encoding(error.to_string()))?;
    Ok(Hash(sha256_domain(b"MNA/program-operation/v1", &bytes)))
}

pub fn block_header_id(header: &BlockHeader) -> Result<Hash, CryptoError> {
    let bytes =
        canonical_encode(header).map_err(|error| CryptoError::Encoding(error.to_string()))?;
    Ok(Hash(sha256_domain(b"MNA/block/v1", &bytes)))
}

pub fn merkle_root(leaves: &[Hash]) -> Hash {
    if leaves.is_empty() {
        return Hash(sha256_domain(b"MNA/tx-root/v1", b""));
    }

    let mut layer = leaves.to_vec();
    while layer.len() > 1 {
        let mut next = Vec::with_capacity(layer.len().div_ceil(2));
        for pair in layer.chunks(2) {
            let right = pair.get(1).unwrap_or(&pair[0]);
            let mut bytes = Vec::with_capacity(64);
            bytes.extend_from_slice(pair[0].as_bytes());
            bytes.extend_from_slice(right.as_bytes());
            next.push(Hash(sha256_domain(b"MNA/merkle/v1", &bytes)));
        }
        layer = next;
    }
    layer[0]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_key_signs_and_verifies() {
        let key_pair = KeyPair::generate().unwrap();
        let message = b"worldstreet";
        let signature = key_pair.sign(message);
        assert!(KeyPair::verify(&key_pair.public_key(), message, &signature));
        assert!(!KeyPair::verify(
            &key_pair.public_key(),
            b"tampered",
            &signature
        ));
    }

    #[test]
    fn mnemonic_restores_the_same_wallet() {
        let phrase = generate_mnemonic().unwrap();
        let seed = mnemonic_to_seed(&phrase, "").unwrap();
        let key_a = derive_wallet_key(&seed).unwrap();
        let key_b = derive_wallet_key(&seed).unwrap();
        assert_eq!(key_a.public_key(), key_b.public_key());
        assert_eq!(key_a.address(), key_b.address());
    }

    #[test]
    fn domain_hash_is_deterministic() {
        assert_eq!(
            sha256_domain(b"test", b"value"),
            sha256_domain(b"test", b"value")
        );
    }
}
