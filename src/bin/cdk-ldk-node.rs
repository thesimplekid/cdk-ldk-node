use std::env;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;

use cdk_common::common::FeeReserve;
use cdk_ldk_node::proto::cdk_ldk_management_server::CdkLdkManagementServer;
use cdk_ldk_node::proto::server::CdkLdkServer;
use cdk_ldk_node::{BitcoinRpcConfig, ChainSource, GossipSource};
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

fn main() -> anyhow::Result<()> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;

    let runtime = Arc::new(runtime);

    let runtime_clone = runtime.clone();

    runtime.block_on(async {
        let default_filter = "debug";

        let sqlx_filter = "sqlx=warn";
        let hyper_filter = "hyper=warn";
        let h2_filter = "h2=warn";
        let rustls_filter = "rustls=warn";

        let env_filter = EnvFilter::new(format!(
            "{},{},{},{},{}",
            default_filter, sqlx_filter, hyper_filter, h2_filter, rustls_filter
        ));

        tracing_subscriber::fmt().with_env_filter(env_filter).init();

        let listen_addr: String = env::var(ENV_LISTEN_HOST).unwrap_or("127.0.0.1".to_string());
        let listen_port: u16 = env::var(ENV_LISTEN_PORT)
            .unwrap_or("8089".to_string())
            .parse()?;

        let gossip_source =
            GossipSource::RapidGossipSync("https://rgs.mutinynet.com/snapshot/0".to_string());
        let chain_source = ChainSource::Esplora("https://mutinynet.com/api".to_string());
        //
        // let chain_source = ChainSource::BitcoinRpc(BitcoinRpcConfig {
        //     host: "0.0.0.0".to_string(),
        //     port: 18443,
        //     user: "testuser".to_string(),
        //     password: "testpass".to_string(),
        // });

        let ldk_node_listen_addr = SocketAddress::from_str(&format!("127.0.0.1:8090")).unwrap();

        let cdk_ldk = cdk_ldk_node::CdkLdkNode::new(
            chain_source,
            gossip_source,
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
