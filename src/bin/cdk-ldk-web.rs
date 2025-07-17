use std::sync::Arc;

use anyhow::Result;
use cdk_ldk_node::{config, CdkLdkNode};
use clap::Parser;
use tokio::time::Duration;
use tracing_subscriber::EnvFilter;

#[derive(Parser)]
#[command(author, version, about = "CDK LDK Node with Web Interface", long_about = None)]
struct Args {
    #[arg(
        short,
        long,
        default_value = "~/.cdk-ldk-node",
        help = "Specify a custom working directory"
    )]
    work_dir: String,

    #[arg(
        long,
        default_value = "0.0.0.0:3030",
        help = "Web interface address"
    )]
    web_addr: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    // Configure tracing
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("cdk_ldk_node=info".parse()?))
        .init();

    // Parse work directory
    let work_dir = cdk_ldk_node::utils::expand_path(&args.work_dir)?;

    // Parse web address
    let web_addr = args.web_addr.parse()?;

    tracing::info!("Starting CDK LDK Node with web interface");
    tracing::info!("Work directory: {:?}", work_dir);
    tracing::info!("Web interface will be available at: http://{}", web_addr);

    // Load configuration
    let config = config::Config::load_with_path(&work_dir)?;
    tracing::info!("Configuration loaded from work directory: {:?}", work_dir);

    // Create and start the node
    let node = Arc::new(CdkLdkNode::new(
        config.bitcoin_network(),
        config.chain_source(),
        config.gossip_source(),
        config.storage_dir_path(),
        cdk_common::common::FeeReserve {
            min_fee_reserve: 2.into(),
            percent_fee_reserve: 0.02,
        },
        vec![config.ldk_node_listen_addr()?],
    )?);

    tracing::info!("Node created successfully");

    // Start the node
    node.start(None)?;
    tracing::info!("Node started");

    // Start the web server
    node.start_web_server(web_addr)?;
    tracing::info!("Web server started on {}", web_addr);
    tracing::info!("Visit http://{} to access the management interface", web_addr);

    // Handle shutdown gracefully
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {
            tracing::info!("Received Ctrl-C, shutting down gracefully...");
        }
        _ = tokio::time::sleep(Duration::from_millis(u64::MAX)) => {
            // This will never complete, but keeps the process alive
        }
    }

    tracing::info!("Stopping node...");
    node.stop()?;
    tracing::info!("Node stopped successfully");

    Ok(())
}
