use std::path::PathBuf;
use std::sync::Arc;

use cdk_common::common::FeeReserve;
use cdk_ldk_node::config::Config;
use clap::Parser;
use tokio::signal;
use tracing_subscriber::EnvFilter;

#[derive(Parser, Debug)]
#[command(author, version, about = "CDK LDK Node - A Lightning Network node implementation", long_about = None)]
struct Args {
    /// Path to working directory where config.toml is located
    #[arg(
        short,
        long,
        value_name = "DIR",
        help = "Specify a custom working directory containing the config.toml file"
    )]
    work_dir: Option<PathBuf>,
}

fn main() -> anyhow::Result<()> {
    // Parse command line arguments
    let args = Args::parse();

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
            "{default_filter},{hyper_filter},{h2_filter},{rustls_filter}"
        ));

        tracing_subscriber::fmt().with_env_filter(env_filter).init();

        // Load configuration
        let config = if let Some(work_dir) = &args.work_dir {
            Config::load_with_path(work_dir)?
        } else {
            Config::load()?
        };

        // Extract configuration values
        let listen_addr = config.payment_processor_listen_host();
        let listen_port = config.payment_processor_listen_port();
        let chain_source = config.chain_source();
        let network = config.bitcoin_network();
        let storage_dir_path = config.storage_dir_path();
        let gossip_source = config.gossip_source();

        let ldk_node_listen_addr = config.ldk_node_listen_addr()?;

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

        let tls_dir = config.payment_processor_tls_dir();

        payment_server.start(tls_dir).await?;

        // Start gRPC management server
        let grpc_addr = config.grpc_socket_addr()?;
        cdk_ldk.start_management_service(grpc_addr)?;

        // Wait for shutdown signal
        signal::ctrl_c().await?;

        // Stop both servers
        tracing::info!("Received shutdown signal, stopping servers");
        payment_server.stop().await?;
        cdk_ldk.stop()?;

        Ok(())
    })
}
