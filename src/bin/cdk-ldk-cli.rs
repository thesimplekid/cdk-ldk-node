use std::path::PathBuf;

use anyhow::Result;
use cdk_ldk_node::proto::client::CdkLdkClient;
use cdk_ldk_node::utils;
use clap::{Parser, Subcommand};

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
    /// List channels
    ListChannels,
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

    // Use the new method from the client to create a client with the work_dir
    let mut client = CdkLdkClient::create_with_work_dir(cli.address.to_string(), work_dir).await?;

    match cli.command {
        Commands::GetInfo => {
            let info = client.get_info().await?;
            print!("{}", utils::format_node_info(&info));
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
            print!("{}", utils::format_balance_info(&balance));
        }
        Commands::ListChannels => {
            let response = client.list_channels().await?;
            print!("{}", utils::format_channels_info(&response));
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
            print!("{}", utils::format_payment_response(&payment));
        }
        Commands::PayBolt12 {
            offer,
            amount_msats,
        } => {
            let payment = client.pay_bolt12_offer(offer, amount_msats).await?;
            print!("{}", utils::format_payment_response(&payment));
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
