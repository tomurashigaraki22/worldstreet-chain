use argon2::Argon2;
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use chacha20poly1305::{
    aead::{Aead, KeyInit},
    Key, XChaCha20Poly1305, XNonce,
};
use getrandom::fill;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use wsc_core::{Address, PublicKey, Signature, CHAIN_ID};
use wsc_crypto::{derive_wallet_key, generate_mnemonic, mnemonic_to_seed, CryptoError, KeyPair};
use zeroize::Zeroize;

const KEY_BYTES: usize = 32;
const SALT_BYTES: usize = 16;
const NONCE_BYTES: usize = 24;
const KEYSTORE_VERSION: u8 = 1;
const DERIVATION_PATH: &str = "m/44'/9999'/0'/0'/0'";

#[derive(Debug, Error)]
pub enum WalletError {
    #[error("crypto error: {0}")]
    Crypto(#[from] CryptoError),
    #[error("random generation failed: {0}")]
    Random(String),
    #[error("keystore serialization failed: {0}")]
    Serialization(#[from] serde_json::Error),
    #[error("invalid keystore version")]
    UnsupportedVersion,
    #[error("invalid keystore field")]
    InvalidField,
    #[error("wrong password or corrupted keystore")]
    DecryptionFailed,
}

pub struct Wallet {
    key_pair: KeyPair,
    address: Address,
    public_key: PublicKey,
}

impl Wallet {
    pub fn from_key_pair(key_pair: KeyPair) -> Self {
        let public_key = key_pair.public_key();
        let address = key_pair.address();
        Self {
            key_pair,
            address,
            public_key,
        }
    }

    pub fn create() -> Result<(Self, String), WalletError> {
        let mnemonic = generate_mnemonic()?;
        let seed = mnemonic_to_seed(&mnemonic, "")?;
        let wallet = Self::from_seed(&seed)?;
        Ok((wallet, mnemonic))
    }

    pub fn restore(mnemonic: &str, passphrase: &str) -> Result<Self, WalletError> {
        let seed = mnemonic_to_seed(mnemonic, passphrase)?;
        Self::from_seed(&seed)
    }

    fn from_seed(seed: &[u8; 64]) -> Result<Self, WalletError> {
        let key_pair = derive_wallet_key(seed)?;
        Ok(Self::from_key_pair(key_pair))
    }

    pub fn address(&self) -> Address {
        self.address
    }

    pub fn public_key(&self) -> PublicKey {
        self.public_key
    }

    pub fn sign(&self, message: &[u8]) -> Signature {
        self.key_pair.sign(message)
    }

    pub fn private_key_bytes(&self) -> [u8; 32] {
        self.key_pair.secret_bytes()
    }

    pub fn chain_id(&self) -> &'static str {
        CHAIN_ID
    }

    pub fn save_encrypted(&self, password: &str) -> Result<EncryptedKeystore, WalletError> {
        EncryptedKeystore::encrypt(self, password)
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct EncryptedKeystore {
    pub version: u8,
    pub chain_id: String,
    pub derivation_path: String,
    pub address: String,
    pub public_key: String,
    pub kdf: KdfMetadata,
    pub cipher: CipherMetadata,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct KdfMetadata {
    pub algorithm: String,
    pub salt: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CipherMetadata {
    pub algorithm: String,
    pub nonce: String,
    pub ciphertext: String,
}

impl EncryptedKeystore {
    pub fn encrypt(wallet: &Wallet, password: &str) -> Result<Self, WalletError> {
        let mut salt = [0u8; SALT_BYTES];
        let mut nonce = [0u8; NONCE_BYTES];
        fill(&mut salt).map_err(|error| WalletError::Random(error.to_string()))?;
        fill(&mut nonce).map_err(|error| WalletError::Random(error.to_string()))?;

        let key = derive_encryption_key(password, &salt)?;
        let mut payload = wallet.private_key_bytes();
        let ciphertext = {
            let cipher = XChaCha20Poly1305::new(Key::from_slice(&key));
            cipher
                .encrypt(XNonce::from_slice(&nonce), payload.as_ref())
                .map_err(|_| WalletError::DecryptionFailed)?
        };
        let mut key = key;
        key.zeroize();
        payload.zeroize();

        let result = Self {
            version: KEYSTORE_VERSION,
            chain_id: CHAIN_ID.to_owned(),
            derivation_path: DERIVATION_PATH.to_owned(),
            address: wallet.address().to_string(),
            public_key: hex::encode(wallet.public_key().0),
            kdf: KdfMetadata {
                algorithm: "argon2id".to_owned(),
                salt: BASE64.encode(salt),
            },
            cipher: CipherMetadata {
                algorithm: "xchacha20-poly1305".to_owned(),
                nonce: BASE64.encode(nonce),
                ciphertext: BASE64.encode(ciphertext),
            },
        };

        salt.zeroize();
        nonce.zeroize();
        Ok(result)
    }

    pub fn decrypt(&self, password: &str) -> Result<Wallet, WalletError> {
        if self.version != KEYSTORE_VERSION
            || self.chain_id != CHAIN_ID
            || self.derivation_path != DERIVATION_PATH
            || self.kdf.algorithm != "argon2id"
            || self.cipher.algorithm != "xchacha20-poly1305"
        {
            return Err(WalletError::UnsupportedVersion);
        }

        let salt = decode_fixed::<SALT_BYTES>(&self.kdf.salt)?;
        let nonce = decode_fixed::<NONCE_BYTES>(&self.cipher.nonce)?;
        let ciphertext = BASE64
            .decode(&self.cipher.ciphertext)
            .map_err(|_| WalletError::InvalidField)?;
        let key = derive_encryption_key(password, &salt)?;
        let decrypt_result = {
            let cipher = XChaCha20Poly1305::new(Key::from_slice(&key));
            cipher.decrypt(XNonce::from_slice(&nonce), ciphertext.as_ref())
        };
        let mut key = key;
        key.zeroize();
        let mut plaintext = decrypt_result.map_err(|_| WalletError::DecryptionFailed)?;

        if plaintext.len() != KEY_BYTES {
            plaintext.zeroize();
            return Err(WalletError::InvalidField);
        }

        let mut secret = [0u8; KEY_BYTES];
        secret.copy_from_slice(&plaintext);
        plaintext.zeroize();

        let key_pair = KeyPair::from_secret_bytes(secret);
        let wallet = Wallet::from_key_pair(key_pair);

        if wallet.address().to_string() != self.address
            || hex::encode(wallet.public_key().0) != self.public_key
        {
            return Err(WalletError::DecryptionFailed);
        }

        Ok(wallet)
    }

    pub fn to_json_pretty(&self) -> Result<String, WalletError> {
        Ok(serde_json::to_string_pretty(self)?)
    }

    pub fn from_json(value: &str) -> Result<Self, WalletError> {
        Ok(serde_json::from_str(value)?)
    }
}

fn derive_encryption_key(password: &str, salt: &[u8]) -> Result<[u8; KEY_BYTES], WalletError> {
    let mut key = [0u8; KEY_BYTES];
    Argon2::default()
        .hash_password_into(password.as_bytes(), salt, &mut key)
        .map_err(|_| WalletError::DecryptionFailed)?;
    Ok(key)
}

fn decode_fixed<const N: usize>(value: &str) -> Result<[u8; N], WalletError> {
    let bytes = BASE64
        .decode(value)
        .map_err(|_| WalletError::InvalidField)?;
    if bytes.len() != N {
        return Err(WalletError::InvalidField);
    }
    let mut result = [0u8; N];
    result.copy_from_slice(&bytes);
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wallet_can_encrypt_and_decrypt() {
        let (wallet, _) = Wallet::create().unwrap();
        let keystore = wallet
            .save_encrypted("correct horse battery staple")
            .unwrap();
        let restored = keystore.decrypt("correct horse battery staple").unwrap();
        assert_eq!(wallet.address(), restored.address());
        assert_eq!(wallet.public_key(), restored.public_key());
    }

    #[test]
    fn wrong_password_is_rejected() {
        let (wallet, _) = Wallet::create().unwrap();
        let keystore = wallet.save_encrypted("right").unwrap();
        assert!(matches!(
            keystore.decrypt("wrong"),
            Err(WalletError::DecryptionFailed)
        ));
    }

    #[test]
    fn signed_message_verifies() {
        let (wallet, _) = Wallet::create().unwrap();
        let signature = wallet.sign(b"login challenge");
        assert!(wsc_crypto::KeyPair::verify(
            &wallet.public_key(),
            b"login challenge",
            &signature
        ));
    }

    #[test]
    fn keystore_json_round_trip_preserves_identity() {
        let (wallet, _) = Wallet::create().unwrap();
        let keystore = wallet.save_encrypted("password").unwrap();
        let json = keystore.to_json_pretty().unwrap();
        let parsed = EncryptedKeystore::from_json(&json).unwrap();
        let restored = parsed.decrypt("password").unwrap();
        assert_eq!(wallet.address(), restored.address());
        assert_eq!(wallet.public_key(), restored.public_key());
    }
}
