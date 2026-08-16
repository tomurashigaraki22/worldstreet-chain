use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use thiserror::Error;
use wsc_core::{AssetDefinition, AssetId};

#[derive(Debug, Error)]
pub enum BridgeError {
    #[error("external chain RPC request failed: {0}")]
    Http(#[from] reqwest::Error),
    #[error("external chain RPC returned an error: {0}")]
    Rpc(String),
    #[error("invalid external chain RPC response: {0}")]
    Response(String),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EthereumBridgeConfig {
    pub network: String,
    pub rpc_url: String,
    pub bridge_contract: String,
    pub deposit_event_topic: String,
    pub confirmations: u64,
    pub enabled: bool,
}

impl EthereumBridgeConfig {
    pub fn from_env() -> Self {
        let rpc_url = std::env::var("WSC_ETHEREUM_RPC_URL").unwrap_or_default();
        let bridge_contract = std::env::var("WSC_ETHEREUM_BRIDGE_CONTRACT").unwrap_or_default();
        let deposit_event_topic = std::env::var("WSC_ETHEREUM_DEPOSIT_TOPIC").unwrap_or_default();
        let confirmations = std::env::var("WSC_ETHEREUM_CONFIRMATIONS")
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(12);
        let network =
            std::env::var("WSC_ETHEREUM_NETWORK").unwrap_or_else(|_| "sepolia".to_owned());
        let enabled =
            !rpc_url.is_empty() && !bridge_contract.is_empty() && !deposit_event_topic.is_empty();
        Self {
            network,
            rpc_url,
            bridge_contract,
            deposit_event_topic,
            confirmations,
            enabled,
        }
    }

    pub fn weth_definition(&self) -> AssetDefinition {
        let reference = if self.bridge_contract.is_empty() {
            format!("{}:bridge-placeholder", self.network)
        } else {
            format!("{}:{}", self.network, self.bridge_contract)
        };
        AssetDefinition {
            id: AssetId::wrapped("ethereum", "WETH", reference, 18),
            display_name: "Wrapped Ether".to_owned(),
            wrapped: true,
            enabled: self.enabled,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EthereumUsdcBridgeConfig {
    pub network: String,
    pub rpc_url: String,
    pub bridge_contract: String,
    pub token_contract: String,
    pub deposit_event_topic: String,
    pub confirmations: u64,
    pub enabled: bool,
}

impl EthereumUsdcBridgeConfig {
    pub fn from_env() -> Self {
        let rpc_url = std::env::var("WSC_ETHEREUM_RPC_URL").unwrap_or_default();
        let bridge_contract =
            std::env::var("WSC_ETHEREUM_USDC_BRIDGE_CONTRACT").unwrap_or_default();
        let token_contract = std::env::var("WSC_ETHEREUM_USDC_TOKEN").unwrap_or_default();
        let deposit_event_topic =
            std::env::var("WSC_ETHEREUM_USDC_DEPOSIT_TOPIC").unwrap_or_default();
        let confirmations = std::env::var("WSC_ETHEREUM_CONFIRMATIONS")
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(12);
        let network =
            std::env::var("WSC_ETHEREUM_NETWORK").unwrap_or_else(|_| "sepolia".to_owned());
        let enabled = !rpc_url.is_empty()
            && !bridge_contract.is_empty()
            && !token_contract.is_empty()
            && !deposit_event_topic.is_empty();
        Self {
            network,
            rpc_url,
            bridge_contract,
            token_contract,
            deposit_event_topic,
            confirmations,
            enabled,
        }
    }

    pub fn usdc_definition(&self) -> AssetDefinition {
        let reference = if self.token_contract.is_empty() {
            format!("{}:token-placeholder", self.network)
        } else {
            format!("{}:{}", self.network, self.token_contract)
        };
        AssetDefinition {
            id: AssetId::wrapped("ethereum", "USDC", reference, 6),
            display_name: "USD Coin".to_owned(),
            wrapped: true,
            enabled: self.enabled,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SolanaBridgeConfig {
    pub network: String,
    pub mode: String,
    pub rpc_url: String,
    pub bridge_program: String,
    pub vault_address: String,
    pub vault_keypair: String,
    pub wsol_mint: String,
    pub spl_usdc_mint: String,
    pub spl_usdc_vault_token_account: String,
    pub spl_usdc_enabled: bool,
    pub commitment: String,
    pub confirmations: u64,
    pub enabled: bool,
}

impl SolanaBridgeConfig {
    pub fn from_env() -> Self {
        let rpc_url = std::env::var("WSC_SOLANA_RPC_URL").unwrap_or_default();
        let mode = std::env::var("WSC_SOLANA_MODE").unwrap_or_else(|_| "disabled".to_owned());
        let bridge_program = std::env::var("WSC_SOLANA_BRIDGE_PROGRAM").unwrap_or_default();
        let vault_address = std::env::var("WSC_SOLANA_VAULT_ADDRESS").unwrap_or_default();
        let vault_keypair = std::env::var("WSC_SOLANA_VAULT_KEYPAIR").unwrap_or_default();
        let wsol_mint = std::env::var("WSC_SOLANA_WSOL_MINT").unwrap_or_default();
        let spl_usdc_mint = std::env::var("WSC_SOLANA_SPL_USDC_MINT").unwrap_or_default();
        let spl_usdc_vault_token_account =
            std::env::var("WSC_SOLANA_SPL_USDC_VAULT_TOKEN_ACCOUNT").unwrap_or_default();
        let network = std::env::var("WSC_SOLANA_NETWORK").unwrap_or_else(|_| "devnet".to_owned());
        let commitment =
            std::env::var("WSC_SOLANA_COMMITMENT").unwrap_or_else(|_| "finalized".to_owned());
        let confirmations = std::env::var("WSC_SOLANA_CONFIRMATIONS")
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(32);
        let custody_ready = mode == "custody" && !vault_address.is_empty();
        let program_ready = mode == "program" && !bridge_program.is_empty();
        let enabled =
            !rpc_url.is_empty() && !wsol_mint.is_empty() && (custody_ready || program_ready);
        let spl_usdc_enabled =
            enabled && !spl_usdc_mint.is_empty() && !spl_usdc_vault_token_account.is_empty();
        Self {
            network,
            mode,
            rpc_url,
            bridge_program,
            vault_address,
            vault_keypair,
            wsol_mint,
            spl_usdc_mint,
            spl_usdc_vault_token_account,
            spl_usdc_enabled,
            commitment,
            confirmations,
            enabled,
        }
    }

    pub fn spl_usdc_definition(&self) -> AssetDefinition {
        let reference = if self.spl_usdc_mint.is_empty() {
            format!("{}:mint-placeholder", self.network)
        } else {
            format!("{}:{}", self.network, self.spl_usdc_mint)
        };
        AssetDefinition {
            id: AssetId::wrapped("solana", "USDC", reference, 6),
            display_name: "USD Coin (Solana)".to_owned(),
            wrapped: true,
            enabled: self.spl_usdc_enabled,
        }
    }

    pub fn wsol_definition(&self) -> AssetDefinition {
        let reference = if self.mode == "custody" {
            if self.vault_address.is_empty() || self.wsol_mint.is_empty() {
                format!("{}:custody-placeholder", self.network)
            } else {
                format!(
                    "{}:vault:{}:{}",
                    self.network, self.vault_address, self.wsol_mint
                )
            }
        } else if self.bridge_program.is_empty() || self.wsol_mint.is_empty() {
            format!("{}:bridge-placeholder", self.network)
        } else {
            format!(
                "{}:{}:{}",
                self.network, self.bridge_program, self.wsol_mint
            )
        };
        AssetDefinition {
            id: AssetId::wrapped("solana", "WSOL", reference, 9),
            display_name: "Wrapped SOL".to_owned(),
            wrapped: true,
            enabled: self.enabled,
        }
    }
}

#[derive(Clone)]
pub struct SolanaRpcClient {
    client: Client,
    endpoint: String,
}

impl SolanaRpcClient {
    pub fn new(endpoint: impl Into<String>) -> Self {
        Self {
            client: Client::new(),
            endpoint: endpoint.into(),
        }
    }

    pub async fn health(&self) -> Result<String, BridgeError> {
        self.call("getHealth", json!([])).await
    }

    pub async fn slot(&self, commitment: &str) -> Result<u64, BridgeError> {
        self.call("getSlot", json!([{ "commitment": commitment }]))
            .await
    }

    async fn call<T: for<'de> Deserialize<'de>>(
        &self,
        method: &str,
        params: Value,
    ) -> Result<T, BridgeError> {
        let response: RpcResponse<T> = self
            .client
            .post(&self.endpoint)
            .json(&json!({ "jsonrpc": "2.0", "id": 1, "method": method, "params": params }))
            .send()
            .await?
            .json()
            .await?;
        if let Some(error) = response.error {
            return Err(BridgeError::Rpc(error.message));
        }
        response
            .result
            .ok_or_else(|| BridgeError::Response("missing result".to_owned()))
    }
}

#[derive(Clone)]
pub struct EthereumRpcClient {
    client: Client,
    endpoint: String,
}

impl EthereumRpcClient {
    pub fn new(endpoint: impl Into<String>) -> Self {
        Self {
            client: Client::new(),
            endpoint: endpoint.into(),
        }
    }

    pub async fn chain_id(&self) -> Result<String, BridgeError> {
        self.call("eth_chainId", json!([])).await
    }

    pub async fn block_number(&self) -> Result<u64, BridgeError> {
        let value: String = self.call("eth_blockNumber", json!([])).await?;
        u64::from_str_radix(value.trim_start_matches("0x"), 16)
            .map_err(|error| BridgeError::Response(format!("invalid block number: {error}")))
    }

    pub async fn get_logs(&self, filter: EthLogFilter) -> Result<Vec<EthLog>, BridgeError> {
        self.call("eth_getLogs", json!([filter])).await
    }

    async fn call<T: for<'de> Deserialize<'de>>(
        &self,
        method: &str,
        params: Value,
    ) -> Result<T, BridgeError> {
        let response: RpcResponse<T> = self
            .client
            .post(&self.endpoint)
            .json(&json!({ "jsonrpc": "2.0", "id": 1, "method": method, "params": params }))
            .send()
            .await?
            .json()
            .await?;
        if let Some(error) = response.error {
            return Err(BridgeError::Rpc(error.message));
        }
        response
            .result
            .ok_or_else(|| BridgeError::Response("missing result".to_owned()))
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct EthLogFilter {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from_block: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub to_block: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub address: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub topics: Option<Vec<String>>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EthLog {
    pub address: String,
    pub topics: Vec<String>,
    pub data: String,
    pub block_number: Option<String>,
    pub transaction_hash: Option<String>,
    pub log_index: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum BridgeOperationStatus {
    Detected,
    AwaitingFinality,
    MintQueued,
    Minted,
    BurnQueued,
    Released,
    Failed,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BridgeOperation {
    pub operation_id: String,
    pub asset_id: String,
    pub source_chain: String,
    pub source_transaction: String,
    pub destination_address: String,
    pub amount: String,
    pub status: BridgeOperationStatus,
}

#[derive(Debug, Deserialize)]
struct RpcResponse<T> {
    result: Option<T>,
    error: Option<RpcError>,
}

#[derive(Debug, Deserialize)]
struct RpcError {
    message: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_config_is_safe_by_default() {
        let config = EthereumBridgeConfig::from_env();
        assert!(!config.enabled);
        assert_eq!(config.confirmations, 12);
    }

    #[test]
    fn wsol_identity_contains_network_and_program() {
        let config = SolanaBridgeConfig {
            network: "devnet".to_owned(),
            mode: "program".to_owned(),
            rpc_url: "https://example.invalid".to_owned(),
            bridge_program: "program".to_owned(),
            vault_address: String::new(),
            vault_keypair: String::new(),
            wsol_mint: "mint".to_owned(),
            spl_usdc_mint: String::new(),
            spl_usdc_vault_token_account: String::new(),
            spl_usdc_enabled: false,
            commitment: "finalized".to_owned(),
            confirmations: 32,
            enabled: true,
        };
        assert_eq!(
            config.wsol_definition().id.canonical_key(),
            "solana:WSOL:devnet:program:mint"
        );
    }

    #[test]
    fn custody_identity_contains_vault_and_mint() {
        let config = SolanaBridgeConfig {
            network: "devnet".to_owned(),
            mode: "custody".to_owned(),
            rpc_url: "https://example.invalid".to_owned(),
            bridge_program: String::new(),
            vault_address: "Vault111111111111111111111111111111111111111".to_owned(),
            vault_keypair: "/root/vault.json".to_owned(),
            wsol_mint: "So11111111111111111111111111111111111111112".to_owned(),
            spl_usdc_mint: String::new(),
            spl_usdc_vault_token_account: String::new(),
            spl_usdc_enabled: false,
            commitment: "finalized".to_owned(),
            confirmations: 32,
            enabled: true,
        };
        assert_eq!(
            config.wsol_definition().id.canonical_key(),
            "solana:WSOL:devnet:vault:Vault111111111111111111111111111111111111111:So11111111111111111111111111111111111111112"
        );
    }

    #[test]
    fn weth_identity_contains_network_and_bridge() {
        let config = EthereumBridgeConfig {
            network: "sepolia".to_owned(),
            rpc_url: "https://example.invalid".to_owned(),
            bridge_contract: "0xabc".to_owned(),
            deposit_event_topic: "0xtopic".to_owned(),
            confirmations: 12,
            enabled: true,
        };
        assert_eq!(
            config.weth_definition().id.canonical_key(),
            "ethereum:WETH:sepolia:0xabc"
        );
    }
}
