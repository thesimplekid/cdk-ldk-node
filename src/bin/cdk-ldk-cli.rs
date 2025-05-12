use std::path::PathBuf;

use anyhow::Result;
use cdk_ldk_node::proto::client::CdkLdkClient;
use clap::{Parser, Subcommand};
use tonic::transport::{Certificate, Channel, ClientTlsConfig, Identity};

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Cli {
    #[arg(short, long, default_value = "http://127.0.0.1:50051")]
    address: String,

    #[arg(short, long, default_value = "~/.cdk-ldk-cli")]
    work_dir: String,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Get node info
    GetInfo,
    /// Get a new bitcoin address
    GetNewAddress,
    /// Open a new channel
    OpenChannel {
        #[arg(short, long)]
        node_id: String,
        #[arg(long)]
        address: String,
        #[arg(short, long)]
        port: u32,
        #[arg(long)]
        amount_msats: u64,
        #[arg(long)]
        push_msats: Option<u64>,
    },
    /// Close a channel
    CloseChannel {
        #[arg(short, long)]
        channel_id: String,
        #[arg(short, long)]
        node_pubkey: String,
    },
    /// List balances
    ListBalance,
    /// Send bitcoin on-chain
    SendOnchain {
        #[arg(short, long)]
        amount_sat: u64,
        #[arg(short, long)]
        address: String,
    },
    /// Pay a bolt11 invoice
    PayBolt11 {
        #[arg(short, long)]
        invoice: String,
        #[arg(short, long)]
        amount_msats: Option<u64>,
    },
    /// Pay a bolt12 offer
    PayBolt12 {
        #[arg(short, long)]
        offer: String,
        #[arg(short, long)]
        amount_msats: u64,
    },
    /// Create a BOLT11 invoice
    CreateBolt11Invoice {
        #[arg(short, long)]
        amount_msats: u64,
        #[arg(short, long)]
        description: String,
        #[arg(short, long)]
        expiry_seconds: Option<u32>,
    },
    /// Create a BOLT12 offer
    CreateBolt12Offer {
        #[arg(short, long)]
        amount_msats: Option<u64>,
        #[arg(short, long)]
        description: String,
        #[arg(short, long)]
        expiry_seconds: Option<u32>,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let work_dir: PathBuf = cli.work_dir.parse()?;

    let channel = if cli.work_dir.parse::<PathBuf>()?.join("tls").is_dir() {
        // TLS directory exists, configure TLS
        let server_root_ca_cert = std::fs::read_to_string(work_dir.join("tls/ca.pem"))?;
        let server_root_ca_cert = Certificate::from_pem(server_root_ca_cert);
        let client_cert = std::fs::read_to_string(work_dir.join("tls/client.pem"))?;
        let client_key = std::fs::read_to_string(work_dir.join("tls/client.key"))?;
        let client_identity = Identity::from_pem(client_cert, client_key);
        let tls = ClientTlsConfig::new()
            .ca_certificate(server_root_ca_cert)
            .identity(client_identity);

        Channel::from_shared(cli.address.to_string())?
            .tls_config(tls)?
            .connect()
            .await?
    } else {
        // No TLS directory, skip TLS configuration
        Channel::from_shared(cli.address.to_string())?
            .connect()
            .await?
    };

    let mut client = CdkLdkClient::new(channel);

    match cli.command {
        Commands::GetInfo => {
            let info = client.get_info().await?;
            println!("Node Information:");
            println!("----------------");
            println!("Node ID: {}", info.node_id);
            println!("Alias: {}", info.alias);
            println!(
                "Listening Addresses: {}",
                info.listening_addresses.join(", ")
            );
            println!(
                "Announcement Addresses: {}",
                info.announcement_addresses.join(", ")
            );
            println!("Connected peer count: {}", info.num_connected_peers);
            println!("Peer count: {}", info.num_peers);
            println!("Connected channel count: {}", info.num_active_channels);
            println!("Inactive channel count: {}", info.num_inactive_channels);
        }
        Commands::GetNewAddress => {
            let address = client.get_new_address().await?;
            println!("New address: {address}");
        }
        Commands::OpenChannel {
            node_id,
            address,
            port,
            amount_msats,
            push_msats,
        } => {
            let channel_id = client
                .open_channel(node_id, address, port, amount_msats, push_msats)
                .await?;
            println!("Opened channel with ID: {channel_id}");
        }
        Commands::CloseChannel {
            channel_id,
            node_pubkey,
        } => {
            client.close_channel(channel_id, node_pubkey).await?;
            println!("Channel closed successfully");
        }
        Commands::ListBalance => {
            let balance = client.list_balance().await?;
            println!(
                "Total onchain balance (sats): {}",
                balance.total_onchain_balance_sats
            );
            println!(
                "Spendable onchain balance (sats): {}",
                balance.spendable_onchain_balance_sats
            );
            println!(
                "Total lightning balance (sats): {}",
                balance.total_lightning_balance_sats
            );
        }
        Commands::SendOnchain {
            amount_sat,
            address,
        } => {
            let txid = client.send_onchain(amount_sat, address).await?;
            println!("Transaction sent with txid: {txid}");
        }
        Commands::PayBolt11 {
            invoice,
            amount_msats,
        } => {
            let payment = client.pay_bolt11_invoice(invoice, amount_msats).await?;
            if payment.success {
                println!("Payment succeeded!");
                println!("Payment hash: {}", payment.payment_hash);
                println!("Payment preimage: {}", payment.payment_preimage);
                println!("Fee paid (msats): {}", payment.fee_msats);
            } else {
                println!(
                    "Payment failed: {}",
                    payment
                        .failure_reason
                        .unwrap_or_else(|| "Unknown reason".to_string())
                );
            }
        }
        Commands::PayBolt12 {
            offer,
            amount_msats,
        } => {
            let payment = client.pay_bolt12_offer(offer, amount_msats).await?;
            if payment.success {
                println!("Payment succeeded!");
                println!("Payment hash: {}", payment.payment_hash);
                println!("Payment preimage: {}", payment.payment_preimage);
                println!("Fee paid (msats): {}", payment.fee_msats);
            } else {
                println!(
                    "Payment failed: {}",
                    payment
                        .failure_reason
                        .unwrap_or_else(|| "Unknown reason".to_string())
                );
            }
        }
        Commands::CreateBolt11Invoice {
            amount_msats,
            description,
            expiry_seconds,
        } => {
            let invoice = client
                .create_bolt11_invoice(amount_msats, description, expiry_seconds)
                .await?;
            println!("Invoice created successfully!");
            println!("Payment hash: {}", invoice.payment_hash);
            println!("Invoice: {}", invoice.invoice);

            // Format expiry time as human-readable date
            println!("Expires: {}", invoice.expiry_time);
        }
        Commands::CreateBolt12Offer {
            amount_msats,
            description,
            expiry_seconds,
        } => {
            let offer = client
                .create_bolt12_offer(amount_msats, description, expiry_seconds)
                .await?;
            println!("Offer created successfully!");
            println!("Offer ID: {}", offer.offer_id);
            println!("Offer: {}", offer.offer);

            // Format expiry time as human-readable date
            println!("Expires: {}", offer.expiry_time);
        }
    }

    Ok(())
}
