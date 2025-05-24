use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use anyhow::{anyhow, Result};
use config::{Config as ConfigBuilder, File as ConfigFile};
use ldk_node::bitcoin::Network;
use ldk_node::lightning::ln::msgs::SocketAddress;
use serde::Deserialize;

use crate::{BitcoinRpcConfig, ChainSource, GossipSource};

// Environment variables
pub const ENV_LN_BACKEND: &str = "CDK_PAYMENT_PROCESSOR_LN_BACKEND";
pub const ENV_LISTEN_HOST: &str = "CDK_PAYMENT_PROCESSOR_LISTEN_HOST";
pub const ENV_LISTEN_PORT: &str = "CDK_PAYMENT_PROCESSOR_LISTEN_PORT";
pub const ENV_PAYMENT_PROCESSOR_TLS_DIR: &str = "CDK_PAYMENT_PROCESSOR_TLS_DIR";
pub const ENV_GRPC_HOST: &str = "CDK_GRPC_HOST";
pub const ENV_GRPC_PORT: &str = "CDK_GRPC_PORT";

// Chain source configuration
pub const ENV_CHAIN_SOURCE: &str = "CDK_CHAIN_SOURCE";
pub const ENV_ESPLORA_URL: &str = "CDK_ESPLORA_URL";
pub const ENV_BITCOIN_RPC_HOST: &str = "CDK_BITCOIN_RPC_HOST";
pub const ENV_BITCOIN_RPC_PORT: &str = "CDK_BITCOIN_RPC_PORT";
pub const ENV_BITCOIN_RPC_USER: &str = "CDK_BITCOIN_RPC_USER";
pub const ENV_BITCOIN_RPC_PASS: &str = "CDK_BITCOIN_RPC_PASS";

// Network configuration
pub const ENV_BITCOIN_NETWORK: &str = "CDK_BITCOIN_NETWORK";

// Storage configuration
pub const ENV_STORAGE_DIR_PATH: &str = "CDK_STORAGE_DIR_PATH";

// LDK Node configuration
pub const ENV_LDK_NODE_HOST: &str = "CDK_LDK_NODE_HOST";
pub const ENV_LDK_NODE_PORT: &str = "CDK_LDK_NODE_PORT";

// Gossip source configuration
pub const ENV_GOSSIP_SOURCE_TYPE: &str = "CDK_GOSSIP_SOURCE_TYPE";
pub const ENV_RGS_URL: &str = "CDK_RGS_URL";

// TOML configuration file
const CONFIG_FILENAME: &str = "config.toml";

// Get the default config directory path
fn get_default_config_dir() -> PathBuf {
    let mut home_dir = home::home_dir().unwrap_or_else(|| PathBuf::from("."));
    home_dir.push(".cdk-ldk-node");
    home_dir
}

/// Configuration for the CDK LDK Node
#[derive(Debug, Clone, Deserialize, Default)]
pub struct Config {
    /// Payment processor configuration
    #[serde(default)]
    pub payment_processor: PaymentProcessorConfig,

    /// Chain source configuration
    #[serde(default)]
    pub chain_source: ChainSourceConfig,

    /// Network configuration
    #[serde(default)]
    pub network: NetworkConfig,

    /// GRPC API configuration
    #[serde(default)]
    pub grpc: GrpcConfig,

    /// Storage configuration
    #[serde(default)]
    pub storage: StorageConfig,

    /// LDK Node configuration
    #[serde(default)]
    pub ldk_node: LdkNodeConfig,

    /// Gossip source configuration
    #[serde(default)]
    pub gossip_source: GossipSourceConfig,
}

/// Payment processor configuration
#[derive(Debug, Clone, Deserialize, Default)]
pub struct PaymentProcessorConfig {
    /// Host to listen on
    pub listen_host: Option<String>,

    /// Port to listen on
    pub listen_port: Option<u16>,

    /// TLS directory for certificates
    pub tls_dir: Option<String>,
}

/// Chain source configuration
#[derive(Debug, Clone, Deserialize, Default)]
pub struct ChainSourceConfig {
    /// Type of chain source (esplora or bitcoinrpc)
    pub source_type: Option<String>,

    /// Esplora URL
    pub esplora_url: Option<String>,

    /// Bitcoin RPC configuration
    #[serde(default)]
    pub bitcoinrpc: BitcoinRpcConfigInternal,
}

/// Bitcoin RPC Configuration
#[derive(Debug, Clone, Deserialize, Default)]
pub struct BitcoinRpcConfigInternal {
    /// RPC host
    pub host: Option<String>,

    /// RPC port
    pub port: Option<u16>,

    /// RPC username
    pub user: Option<String>,

    /// RPC password
    pub password: Option<String>,
}

/// Network configuration
#[derive(Debug, Clone, Deserialize, Default)]
pub struct NetworkConfig {
    /// Bitcoin network (mainnet, testnet, signet, regtest)
    pub bitcoin_network: Option<String>,
}

/// GRPC API configuration
#[derive(Debug, Clone, Deserialize, Default)]
pub struct GrpcConfig {
    /// GRPC host
    pub host: Option<String>,

    /// GRPC port
    pub port: Option<String>,
}

/// Storage configuration
#[derive(Debug, Clone, Deserialize, Default)]
pub struct StorageConfig {
    /// Directory path for storage
    pub dir_path: Option<String>,
}

/// LDK Node configuration
#[derive(Debug, Clone, Deserialize, Default)]
pub struct LdkNodeConfig {
    /// Host to listen on
    pub host: Option<String>,

    /// Port to listen on
    pub port: Option<u16>,
}

/// Gossip source configuration
#[derive(Debug, Clone, Deserialize, Default)]
pub struct GossipSourceConfig {
    /// Rapid Gossip Sync URL (used when source_type = "rgs")
    pub rgs_url: Option<String>,
}

impl Config {
    /// Load configuration from config.toml and environment variables
    /// Environment variables take precedence over config file values
    pub fn load() -> Result<Self> {
        let mut config_builder = ConfigBuilder::builder();

        // Try to load from config file
        let home_config_path = get_default_config_dir().join(CONFIG_FILENAME);
        let current_dir_config_path = Path::new(CONFIG_FILENAME);

        // Add the config file as a source if it exists
        if home_config_path.exists() {
            tracing::info!("Loading config from {}", home_config_path.display());
            config_builder = config_builder.add_source(ConfigFile::from(home_config_path));
        } else if current_dir_config_path.exists() {
            tracing::info!("Loading config from {}", current_dir_config_path.display());
            config_builder = config_builder.add_source(ConfigFile::from(current_dir_config_path));
        } else {
            tracing::info!("No config file found, using default configuration");

            // Attempt to create the default config file
            // This is a convenience, but not required to continue
            if let Err(create_err) = Self::create_default_config_file() {
                tracing::warn!("Failed to create default config file: {}", create_err);
            }
        }

        // Add environment variables as a source
        config_builder = config_builder.add_source(
            config::Environment::with_prefix("CDK_LDK_NODE")
                .separator("_")
                .try_parsing(true),
        );

        // Build the config from all sources
        let config = config_builder.build()?;

        // Try to deserialize the config into our Config struct
        let config = config.try_deserialize::<Config>()?;

        Ok(config)
    }

    /// Load configuration from a specific path and environment variables
    /// Environment variables take precedence over config file values
    pub fn load_with_path<P: AsRef<Path>>(work_dir: P) -> Result<Self> {
        let mut config_builder = ConfigBuilder::builder();

        // Try to load from config file in the specified directory
        let config_path = work_dir.as_ref().join(CONFIG_FILENAME);

        if config_path.exists() {
            tracing::info!("Loading config from {}", config_path.display());
            config_builder = config_builder.add_source(ConfigFile::from(config_path));
        } else {
            tracing::warn!(
                "No config file found at {}, using default configuration",
                config_path.display()
            );
        }

        // Add environment variables as a source
        config_builder = config_builder.add_source(
            config::Environment::with_prefix("CDK_LDK_NODE")
                .separator("_")
                .try_parsing(true),
        );

        // Build the config from all sources
        let config = config_builder.build()?;

        // Try to deserialize the config into our Config struct
        let config = config.try_deserialize::<Config>()?;

        Ok(config)
    }

    /// Create the default configuration file in the home directory
    /// This will create the .cdk-ldk-node directory if it doesn't exist
    fn create_default_config_file() -> Result<()> {
        let config_dir = get_default_config_dir();
        if !config_dir.exists() {
            tracing::info!("Creating config directory at {}", config_dir.display());
            std::fs::create_dir_all(&config_dir)?;
        }

        let config_path = config_dir.join(CONFIG_FILENAME);

        // Skip if the file already exists
        if config_path.exists() {
            return Ok(());
        }

        tracing::info!("Creating default config file at {}", config_path.display());

        let default_config = r#"# CDK-LDK-Node Configuration

[payment_processor]
# Host to listen on
listen_host = "127.0.0.1"

# Port to listen on
listen_port = 8089

[chain_source]
# Type of chain source (esplora or bitcoinrpc)
source_type = "esplora"

# Esplora URL (used when source_type = "esplora")
esplora_url = "https://mutinynet.com/api"

# Bitcoin RPC configuration (used when source_type = "bitcoinrpc")
[chain_source.bitcoinrpc]
host = "127.0.0.1"
port = 18443
user = "testuser"
password = "testpass"

[network]
# Bitcoin network (mainnet, testnet, signet, regtest)
bitcoin_network = "regtest"

[grpc]
# GRPC API configuration
host = "127.0.0.1"
port = "50051"

[ldk_node]
# LDK Node configuration
host = "127.0.0.1"
port = 8090

[gossip_source]
# Type of gossip source (p2p or rgs)
# - p2p: Use peer-to-peer gossip (default)
# - rgs: Use Rapid Gossip Sync from a URL
source_type = "p2p"

# Rapid Gossip Sync URL (used when source_type = "rgs")
# Uncomment and set this only if using source_type = "rgs"
# rgs_url = "https://rapidsync.example.com"

# Example for using Rapid Gossip Sync:
# [gossip_source]
# source_type = "rgs"
# rgs_url = "https://mutinynet.com/api/graphql"
"#;

        std::fs::write(config_path, default_config)?;

        Ok(())
    }

    /// Get payment processor listen host
    pub fn payment_processor_listen_host(&self) -> String {
        self.payment_processor
            .listen_host
            .clone()
            .unwrap_or_else(|| "127.0.0.1".to_string())
    }

    /// Get payment processor listen port
    pub fn payment_processor_listen_port(&self) -> u16 {
        self.payment_processor.listen_port.unwrap_or(8089)
    }

    /// Get payment processor TLS directory
    pub fn payment_processor_tls_dir(&self) -> Option<PathBuf> {
        self.payment_processor.tls_dir.clone().map(PathBuf::from)
    }

    /// Get chain source
    pub fn chain_source(&self) -> ChainSource {
        let source_type = self
            .chain_source
            .source_type
            .clone()
            .unwrap_or_else(|| "esplora".to_string());

        if source_type.to_lowercase() == "bitcoinrpc" {
            let host = self
                .chain_source
                .bitcoinrpc
                .host
                .clone()
                .unwrap_or_else(|| "127.0.0.1".to_string());
            let port = self.chain_source.bitcoinrpc.port.unwrap_or(18443);
            let user = self
                .chain_source
                .bitcoinrpc
                .user
                .clone()
                .unwrap_or_else(|| "testuser".to_string());
            let password = self
                .chain_source
                .bitcoinrpc
                .password
                .clone()
                .unwrap_or_else(|| "testpass".to_string());

            ChainSource::BitcoinRpc(BitcoinRpcConfig {
                host,
                port,
                user,
                password,
            })
        } else {
            let esplora_url = self
                .chain_source
                .esplora_url
                .clone()
                .unwrap_or_else(|| "https://mutinynet.com/api".to_string());

            ChainSource::Esplora(esplora_url)
        }
    }

    /// Get Bitcoin network
    pub fn bitcoin_network(&self) -> Network {
        match self
            .network
            .bitcoin_network
            .clone()
            .unwrap_or_else(|| "regtest".to_string())
            .to_lowercase()
            .as_str()
        {
            "mainnet" | "bitcoin" => Network::Bitcoin,
            "testnet" => Network::Testnet,
            "signet" => Network::Signet,
            _ => Network::Regtest,
        }
    }

    /// Get storage directory path
    pub fn storage_dir_path(&self) -> String {
        if let Some(dir_path) = &self.storage.dir_path {
            return dir_path.clone();
        }

        let mut home_dir = home::home_dir().unwrap_or_else(|| PathBuf::from("."));
        home_dir.push(".cdk-ldk-node");
        home_dir.push("ldk-node");
        home_dir.to_string_lossy().to_string()
    }

    /// Get LDK node listen socket address
    pub fn ldk_node_listen_addr(&self) -> Result<SocketAddress> {
        let host = self
            .ldk_node
            .host
            .clone()
            .unwrap_or_else(|| "127.0.0.1".to_string());
        let port = self.ldk_node.port.unwrap_or(8090);

        SocketAddress::from_str(&format!("{host}:{port}"))
            .map_err(|_| anyhow!("Invalid socket address"))
    }

    /// Get gossip source (RapidGossipSync if URL is provided, otherwise P2P)
    pub fn gossip_source(&self) -> GossipSource {
        if let Some(rgs_url) = self.gossip_source.rgs_url.clone() {
            GossipSource::RapidGossipSync(rgs_url)
        } else {
            GossipSource::P2P
        }
    }

    /// Get GRPC host
    pub fn grpc_host(&self) -> String {
        self.grpc
            .host
            .clone()
            .unwrap_or_else(|| "127.0.0.1".to_string())
    }

    /// Get GRPC port
    pub fn grpc_port(&self) -> String {
        self.grpc
            .port
            .clone()
            .unwrap_or_else(|| "50051".to_string())
    }

    /// Get GRPC socket address
    pub fn grpc_socket_addr(&self) -> Result<SocketAddr> {
        format!(
            "{host}:{port}",
            host = self.grpc_host(),
            port = self.grpc_port()
        )
        .parse::<SocketAddr>()
        .map_err(|e| anyhow!("Failed to parse GRPC socket address: {}", e))
    }
}
