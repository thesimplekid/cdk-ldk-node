//! Utility functions for interacting with cdk-ldk-node

use std::path::PathBuf;

use anyhow::Result;
use tonic::transport::{Certificate, Channel, ClientTlsConfig, Identity};

/// Creates a channel for connecting to the LDK node, with optional TLS
pub async fn create_channel(address: String, work_dir: PathBuf) -> Result<Channel> {
    if work_dir.join("tls").is_dir() {
        // TLS directory exists, configure TLS
        let server_root_ca_cert = std::fs::read_to_string(work_dir.join("tls/ca.pem"))?;
        let server_root_ca_cert = Certificate::from_pem(server_root_ca_cert);
        let client_cert = std::fs::read_to_string(work_dir.join("tls/client.pem"))?;
        let client_key = std::fs::read_to_string(work_dir.join("tls/client.key"))?;
        let client_identity = Identity::from_pem(client_cert, client_key);
        let tls = ClientTlsConfig::new()
            .ca_certificate(server_root_ca_cert)
            .identity(client_identity);

        let channel = Channel::from_shared(address)?
            .tls_config(tls)?
            .connect()
            .await?;
        Ok(channel)
    } else {
        // No TLS directory, skip TLS configuration
        let channel = Channel::from_shared(address)?.connect().await?;
        Ok(channel)
    }
}

/// Format payment response information for display
pub fn format_payment_response(payment: &crate::proto::PaymentResponse) -> String {
    let mut output = String::new();

    if payment.success {
        output.push_str("Payment succeeded!\n");
        output.push_str(&format!("Payment hash: {}\n", payment.payment_hash));
        output.push_str(&format!("Payment preimage: {}\n", payment.payment_preimage));
        output.push_str(&format!("Fee paid (msats): {}\n", payment.fee_msats));
    } else {
        output.push_str(&format!(
            "Payment failed: {}\n",
            payment
                .failure_reason
                .clone()
                .unwrap_or_else(|| "Unknown reason".to_string())
        ));
    }

    output
}

/// Format node information for display
pub fn format_node_info(info: &crate::proto::GetInfoResponse) -> String {
    let mut output = String::new();

    output.push_str("Node Information:\n");
    output.push_str("----------------\n");
    output.push_str(&format!("Node ID: {}\n", info.node_id));
    output.push_str(&format!("Alias: {}\n", info.alias));
    output.push_str(&format!(
        "Listening Addresses: {}\n",
        info.listening_addresses.join(", ")
    ));
    output.push_str(&format!(
        "Announcement Addresses: {}\n",
        info.announcement_addresses.join(", ")
    ));
    output.push_str(&format!(
        "Connected peer count: {}\n",
        info.num_connected_peers
    ));
    output.push_str(&format!("Peer count: {}\n", info.num_peers));
    output.push_str(&format!(
        "Connected channel count: {}\n",
        info.num_active_channels
    ));
    output.push_str(&format!(
        "Inactive channel count: {}\n",
        info.num_inactive_channels
    ));

    output
}

/// Format balance information for display
pub fn format_balance_info(balance: &crate::proto::ListBalanceResponse) -> String {
    let mut output = String::new();

    output.push_str(&format!(
        "Total onchain balance (sats): {}\n",
        balance.total_onchain_balance_sats
    ));
    output.push_str(&format!(
        "Spendable onchain balance (sats): {}\n",
        balance.spendable_onchain_balance_sats
    ));
    output.push_str(&format!(
        "Total lightning balance (sats): {}\n",
        balance.total_lightning_balance_sats
    ));

    output
}

/// Format channels information for display
pub fn format_channels_info(response: &crate::proto::ListChannelsResponse) -> String {
    let mut output = String::new();

    output.push_str("Lightning Channels:\n");
    output.push_str("-----------------\n");

    if response.channels.is_empty() {
        output.push_str("No channels found.\n");
    } else {
        for (i, channel) in response.channels.iter().enumerate() {
            output.push_str(&format!("Channel #{}:\n", i + 1));
            output.push_str(&format!("  ID: {}\n", channel.channel_id));
            output.push_str(&format!(
                "  Counterparty: {}\n",
                channel.counterparty_node_id
            ));
            output.push_str(&format!("  Balance: {} msats\n", channel.balance_msat));
            output.push_str(&format!(
                "  Outbound Capacity: {} msats\n",
                channel.outbound_capacity_msat
            ));
            output.push_str(&format!(
                "  Inbound Capacity: {} msats\n",
                channel.inbound_capacity_msat
            ));
            output.push_str(&format!("  Usable: {}\n", channel.is_usable));
            output.push_str(&format!("  Public: {}\n", channel.is_public));
            if !channel.short_channel_id.is_empty() {
                output.push_str(&format!(
                    "  Short Channel ID: {}\n",
                    channel.short_channel_id
                ));
            }
            output.push('\n');
        }
    }

    output
}
