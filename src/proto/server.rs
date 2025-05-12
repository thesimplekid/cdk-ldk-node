use std::str::FromStr;
use std::sync::Arc;

use ldk_node::bitcoin::secp256k1::PublicKey;
use ldk_node::bitcoin::Address;
use ldk_node::lightning::ln::msgs::SocketAddress;
use ldk_node::payment::{PaymentKind, PaymentStatus};
use ldk_node::UserChannelId;
use tonic::{Request, Response, Status};

use super::cdk_ldk_management_server::CdkLdkManagement;
use super::*;
use crate::CdkLdkNode;

pub struct CdkLdkServer {
    node: Arc<CdkLdkNode>,
}

impl CdkLdkServer {
    pub fn new(node: Arc<CdkLdkNode>) -> Self {
        Self { node }
    }
}

#[tonic::async_trait]
impl CdkLdkManagement for CdkLdkServer {
    async fn get_info(
        &self,
        _request: Request<GetInfoRequest>,
    ) -> Result<Response<GetInfoResponse>, Status> {
        let node = self.node.inner.as_ref();

        let node_id = node.node_id();
        let alias = node
            .node_alias()
            .map(|a| a.to_string())
            .unwrap_or("".to_string());

        let config = self.node.inner.config();

        let announcement_addresses = config
            .announcement_addresses
            .as_ref()
            .unwrap_or(&vec![])
            .iter()
            .map(|a| a.to_string())
            .collect();

        let listening_addresses = config
            .announcement_addresses
            .unwrap_or_default()
            .iter()
            .map(|a| a.to_string())
            .collect();

        let (num_peers, num_connected_peers) =
            node.list_peers()
                .iter()
                .fold((0, 0), |(mut peers, mut connected), p| {
                    if p.is_connected {
                        connected += 1;
                    }
                    peers += 1;

                    (peers, connected)
                });

        let (num_active_channels, num_inactive_channels) =
            node.list_channels()
                .iter()
                .fold((0, 0), |(mut active, mut inactive), c| {
                    if c.is_usable {
                        active += 1;
                    } else {
                        inactive += 1;
                    }
                    (active, inactive)
                });

        Ok(Response::new(GetInfoResponse {
            node_id: node_id.to_string(),
            alias,
            announcement_addresses,
            listening_addresses,
            num_peers,
            num_connected_peers,
            num_active_channels,
            num_inactive_channels,
        }))
    }

    async fn get_new_address(
        &self,
        _request: Request<GetNewAddressRequest>,
    ) -> Result<Response<GetNewAddressResponse>, Status> {
        let address = self
            .node
            .inner
            .onchain_payment()
            .new_address()
            .map_err(|e| Status::internal(e.to_string()))?;

        Ok(Response::new(GetNewAddressResponse {
            address: address.to_string(),
        }))
    }

    async fn open_channel(
        &self,
        request: Request<OpenChannelRequest>,
    ) -> Result<Response<OpenChannelResponse>, Status> {
        let req = request.into_inner();

        let socket_addr = SocketAddress::from_str(&format!("{}:{}", req.address, req.port))
            .map_err(|e| Status::internal(e.to_string()))?;

        let pubkey =
            PublicKey::from_str(&req.node_id).map_err(|e| Status::internal(e.to_string()))?;

        self.node
            .inner
            .connect(pubkey, socket_addr.clone(), true)
            .map_err(|e| Status::internal(e.to_string()))?;

        let channel = self
            .node
            .inner
            .open_announced_channel(
                pubkey,
                socket_addr,
                req.amount_msats,
                req.push_to_counter_party_msats,
                None,
            )
            .map_err(|e| Status::internal(e.to_string()))?;

        Ok(Response::new(OpenChannelResponse {
            channel_id: channel.0.to_string(),
        }))
    }

    async fn close_channel(
        &self,
        request: Request<CloseChannelRequest>,
    ) -> Result<Response<CloseChannelResponse>, Status> {
        let req = request.into_inner();

        let node_pubkey = req
            .node_pubkey
            .parse()
            .map_err(|e| Status::invalid_argument(format!("Invalid node pubkey: {e}")))?;

        let channel_id: u128 = req
            .channel_id
            .parse()
            .map_err(|e| Status::invalid_argument(format!("Invalid channel id: {e}")))?;

        let channel_id = UserChannelId(channel_id);

        self.node
            .inner
            .close_channel(&channel_id, node_pubkey)
            .map_err(|e| Status::internal(e.to_string()))?;

        Ok(Response::new(CloseChannelResponse {}))
    }

    async fn list_balance(
        &self,
        _request: Request<ListBalanceRequest>,
    ) -> Result<Response<ListBalanceResponse>, Status> {
        let node_balance = self.node.inner.list_balances();

        Ok(Response::new(ListBalanceResponse {
            total_onchain_balance_sats: node_balance.total_onchain_balance_sats,
            spendable_onchain_balance_sats: node_balance.spendable_onchain_balance_sats,
            total_lightning_balance_sats: node_balance.total_lightning_balance_sats,
        }))
    }

    async fn send_onchain(
        &self,
        request: Request<SendOnchainRequest>,
    ) -> Result<Response<SendOnchainResponse>, Status> {
        let req = request.into_inner();

        let address =
            Address::from_str(&req.address).map_err(|e| Status::invalid_argument(e.to_string()))?;

        let txid = self
            .node
            .inner
            .onchain_payment()
            .send_to_address(address.assume_checked_ref(), req.amount_sat, None)
            .map_err(|e| Status::internal(e.to_string()))?;

        Ok(Response::new(SendOnchainResponse {
            txid: txid.to_string(),
        }))
    }

    async fn pay_bolt11_invoice(
        &self,
        request: Request<PayBolt11InvoiceRequest>,
    ) -> Result<Response<PaymentResponse>, Status> {
        let req = request.into_inner();

        // Parse the BOLT11 invoice
        let bolt11 = ldk_node::lightning_invoice::Bolt11Invoice::from_str(&req.invoice)
            .map_err(|e| Status::invalid_argument(format!("Invalid BOLT11 invoice: {e}")))?;

        // Determine sending parameters
        let send_params = None; // Use default parameters

        // Send the payment
        let payment_id = if let Some(amount_msats) = req.amount_msats {
            // Send with a specific amount (amountless invoice or override amount)
            self.node
                .inner
                .bolt11_payment()
                .send_using_amount(&bolt11, amount_msats, send_params)
                .map_err(|e| Status::internal(format!("Failed to pay invoice: {e}")))?
        } else {
            // Send with the amount specified in the invoice
            self.node
                .inner
                .bolt11_payment()
                .send(&bolt11, send_params)
                .map_err(|e| Status::internal(format!("Failed to pay invoice: {e}")))?
        };

        // Check payment status for up to 10 seconds
        let start = std::time::Instant::now();
        let timeout = std::time::Duration::from_secs(10);

        let payment_details = loop {
            let details = self
                .node
                .inner
                .payment(&payment_id)
                .ok_or_else(|| Status::internal("Payment not found"))?;

            match details.status {
                PaymentStatus::Succeeded => break details,
                PaymentStatus::Failed => {
                    return Ok(Response::new(PaymentResponse {
                        payment_hash: bolt11.payment_hash().to_string(),
                        payment_preimage: String::new(),
                        fee_msats: 0,
                        success: false,
                        failure_reason: Some("Payment failed".to_string()),
                    }));
                }
                PaymentStatus::Pending => {
                    if start.elapsed() > timeout {
                        // Return pending status after timeout
                        return Ok(Response::new(PaymentResponse {
                            payment_hash: bolt11.payment_hash().to_string(),
                            payment_preimage: String::new(),
                            fee_msats: 0,
                            success: false,
                            failure_reason: Some("Payment is still pending".to_string()),
                        }));
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                    continue;
                }
            }
        };

        // Extract payment details
        let (preimage, fee_msats) = match payment_details.kind {
            PaymentKind::Bolt11 {
                hash: _,
                preimage,
                secret: _,
            } => (
                preimage.map(|p| p.to_string()).unwrap_or_default(),
                payment_details.fee_paid_msat.unwrap_or(0),
            ),
            _ => (String::new(), 0),
        };

        Ok(Response::new(PaymentResponse {
            payment_hash: bolt11.payment_hash().to_string(),
            payment_preimage: preimage,
            fee_msats,
            success: true,
            failure_reason: None,
        }))
    }

    async fn pay_bolt12_offer(
        &self,
        request: Request<PayBolt12OfferRequest>,
    ) -> Result<Response<PaymentResponse>, Status> {
        let req = request.into_inner();

        // Parse the BOLT12 offer
        let offer = ldk_node::lightning::offers::offer::Offer::from_str(&req.offer)
            .map_err(|e| Status::invalid_argument(format!("Invalid BOLT12 offer: {e:?}")))?;

        // Send the payment with the specified amount
        let payment_id = self
            .node
            .inner
            .bolt12_payment()
            .send_using_amount(&offer, req.amount_msats, None, None)
            .map_err(|e| Status::internal(format!("Failed to pay offer: {e}")))?;

        // Check payment status for up to 10 seconds
        let start = std::time::Instant::now();
        let timeout = std::time::Duration::from_secs(10);

        let payment_details = loop {
            let details = self
                .node
                .inner
                .payment(&payment_id)
                .ok_or_else(|| Status::internal("Payment not found"))?;

            match details.status {
                PaymentStatus::Succeeded => break details,
                PaymentStatus::Failed => {
                    return Ok(Response::new(PaymentResponse {
                        payment_hash: String::new(), // Will be filled with actual hash if available
                        payment_preimage: String::new(),
                        fee_msats: 0,
                        success: false,
                        failure_reason: Some("Payment failed".to_string()),
                    }));
                }
                PaymentStatus::Pending => {
                    if start.elapsed() > timeout {
                        // Return pending status after timeout
                        return Ok(Response::new(PaymentResponse {
                            payment_hash: String::new(),
                            payment_preimage: String::new(),
                            fee_msats: 0,
                            success: false,
                            failure_reason: Some("Payment is still pending".to_string()),
                        }));
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                    continue;
                }
            }
        };

        // Extract payment details
        let (payment_hash, preimage, fee_msats) = match payment_details.kind {
            PaymentKind::Bolt12Offer {
                hash,
                preimage,
                secret: _,
                offer_id: _,
                payer_note: _,
                quantity: _,
            } => (
                hash.map(|h| h.to_string()).unwrap_or_default(),
                preimage.map(|p| p.to_string()).unwrap_or_default(),
                payment_details.fee_paid_msat.unwrap_or(0),
            ),
            _ => (String::new(), String::new(), 0),
        };

        Ok(Response::new(PaymentResponse {
            payment_hash,
            payment_preimage: preimage,
            fee_msats,
            success: true,
            failure_reason: None,
        }))
    }

    async fn create_bolt11_invoice(
        &self,
        request: Request<CreateBolt11InvoiceRequest>,
    ) -> Result<Response<CreateInvoiceResponse>, Status> {
        let req = request.into_inner();

        // Set up the description
        let description = ldk_node::lightning_invoice::Bolt11InvoiceDescription::Direct(
            ldk_node::lightning_invoice::Description::new(req.description)
                .map_err(|_| Status::invalid_argument("Invalid description"))?,
        );

        // Get expiry time (default to 1 hour if not specified)
        let expiry_seconds = req.expiry_seconds.unwrap_or(3600);

        // Create the invoice
        let invoice = self
            .node
            .inner
            .bolt11_payment()
            .receive(req.amount_msats, &description, expiry_seconds)
            .map_err(|e| Status::internal(format!("Failed to create invoice: {e}")))?;

        // Get current time for expiry calculation
        let current_time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        Ok(Response::new(CreateInvoiceResponse {
            payment_hash: invoice.payment_hash().to_string(),
            invoice: invoice.to_string(),
            expiry_time: current_time + expiry_seconds as u64,
        }))
    }

    async fn create_bolt12_offer(
        &self,
        request: Request<CreateBolt12OfferRequest>,
    ) -> Result<Response<CreateOfferResponse>, Status> {
        let req = request.into_inner();

        // Get expiry time (default to 1 hour if not specified)
        let expiry_seconds = req.expiry_seconds.unwrap_or(3600);

        // Create the offer based on whether an amount was specified
        let offer = if let Some(amount_msats) = req.amount_msats {
            self.node
                .inner
                .bolt12_payment()
                .receive(amount_msats, &req.description, Some(expiry_seconds), None)
                .map_err(|e| Status::internal(format!("Failed to create offer: {e}")))?
        } else {
            // Create a variable amount offer
            self.node
                .inner
                .bolt12_payment()
                .receive_variable_amount(&req.description, Some(expiry_seconds))
                .map_err(|e| {
                    Status::internal(format!("Failed to create variable amount offer: {e}"))
                })?
        };

        // Get current time for expiry calculation
        let current_time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        Ok(Response::new(CreateOfferResponse {
            offer_id: offer.id().to_string(),
            offer: offer.to_string(),
            expiry_time: current_time + expiry_seconds as u64,
        }))
    }
}
