use std::env;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;

use anyhow::anyhow;
use cdk_common::common::FeeReserve;
use cdk_ldk_node::proto::cdk_ldk_management_server::CdkLdkManagementServer;
use cdk_ldk_node::proto::server::CdkLdkServer;
use cdk_ldk_node::{BitcoinRpcConfig, ChainSource, GossipSource};
use ldk_node::bitcoin::Network;
use ldk_node::lightning::ln::msgs::SocketAddress;
use tokio::signal;
use tonic::transport::Server;
use tracing_subscriber::EnvFilter;

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

fn main() -> anyhow::Result<()> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;

    let runtime = Arc::new(runtime);

    let runtime_clone = runtime.clone();

    runtime.block_on(async {
        let default_filter = "debug";

        let hyper_filter = "hyper=warn";
        let h2_filter = "h2=warn";
        let rustls_filter = "rustls=warn";

        let env_filter = EnvFilter::new(format!(
            "{},{},{},{}",
            default_filter, hyper_filter, h2_filter, rustls_filter
        ));

        tracing_subscriber::fmt().with_env_filter(env_filter).init();

        let listen_addr: String = env::var(ENV_LISTEN_HOST).unwrap_or("127.0.0.1".to_string());
        let listen_port: u16 = env::var(ENV_LISTEN_PORT)
            .unwrap_or("8089".to_string())
            .parse()?;

        // Configure chain source based on environment variables
        let chain_source_type = env::var(ENV_CHAIN_SOURCE).unwrap_or("esplora".to_string());

        let chain_source = if chain_source_type.to_lowercase() == "bitcoinrpc" {
            // Configure Bitcoin RPC
            let rpc_host = env::var(ENV_BITCOIN_RPC_HOST).unwrap_or("127.0.0.1".to_string());
            let rpc_port: u16 = env::var(ENV_BITCOIN_RPC_PORT)
                .unwrap_or("18443".to_string())
                .parse()?;
            let rpc_user = env::var(ENV_BITCOIN_RPC_USER).unwrap_or("testuser".to_string());
            let rpc_pass = env::var(ENV_BITCOIN_RPC_PASS).unwrap_or("testpass".to_string());

            ChainSource::BitcoinRpc(BitcoinRpcConfig {
                host: rpc_host,
                port: rpc_port,
                user: rpc_user,
                password: rpc_pass,
            })
        } else {
            // Default to Esplora
            let esplora_url =
                env::var(ENV_ESPLORA_URL).unwrap_or("https://mutinynet.com/api".to_string());

            ChainSource::Esplora(esplora_url)
        };

        // Configure Bitcoin network based on environment variable
        let network = match env::var(ENV_BITCOIN_NETWORK)
            .unwrap_or("regtest".to_string())
            .to_lowercase()
            .as_str()
        {
            "mainnet" | "bitcoin" => Network::Bitcoin,
            "testnet" => Network::Testnet,
            "signet" => Network::Signet,
            _ => Network::Regtest, // Default to Regtest
        };

        // Configure storage directory path
        let storage_dir_path = env::var(ENV_STORAGE_DIR_PATH).unwrap_or_else(|_| {
            let mut home_dir = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
            home_dir.push(".cdk-ldk-node");
            home_dir.to_string_lossy().to_string()
        });

        let gossip_source = GossipSource::P2P;

        let ldk_node_listen_addr = SocketAddress::from_str("127.0.0.1:8090")
            .map_err(|_| anyhow!("Invalid socket address"))?;

        let cdk_ldk = cdk_ldk_node::CdkLdkNode::new(
            network,
            chain_source,
            gossip_source,
            storage_dir_path,
            FeeReserve {
                min_fee_reserve: 2.into(),
                percent_fee_reserve: 0.02,
            },
            vec![ldk_node_listen_addr],
        )?;

        cdk_ldk.start(Some(runtime_clone))?;

        let cdk_ldk = Arc::new(cdk_ldk);

        // Start payment processor server
        let mut payment_server = cdk_payment_processor::PaymentProcessorServer::new(
            cdk_ldk.clone(),
            &listen_addr,
            listen_port,
        )?;

        let tls_dir: Option<PathBuf> = env::var(ENV_PAYMENT_PROCESSOR_TLS_DIR)
            .ok()
            .map(PathBuf::from);

        payment_server.start(tls_dir).await?;

        // Start gRPC management server
        let grpc_host = env::var(ENV_GRPC_HOST).unwrap_or("127.0.0.1".to_string());
        let grpc_port = env::var(ENV_GRPC_PORT).unwrap_or("50051".to_string());

        let grpc_addr = format!("{}:{}", grpc_host, grpc_port).parse::<SocketAddr>()?;
        let management_service = CdkLdkServer::new(cdk_ldk);

        let grpc_server = Server::builder()
            .add_service(CdkLdkManagementServer::new(management_service))
            .serve(grpc_addr);

        tokio::spawn(grpc_server);

        // Wait for shutdown signal
        signal::ctrl_c().await?;

        // Stop both servers
        payment_server.stop().await?;

        Ok(())
    })
}
