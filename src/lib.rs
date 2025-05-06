use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::anyhow;
use async_trait::async_trait;
use cdk_common::amount::to_unit;
use cdk_common::common::FeeReserve;
use cdk_common::payment::{
    self, Bolt11Settings, Bolt12IncomingPaymentOptions, CreateIncomingPaymentResponse,
    IncomingPaymentOptions, MakePaymentResponse, MintPayment, OutgoingPaymentOptions,
    PaymentIdentifier, PaymentQuoteResponse, WaitPaymentResponse,
};
use cdk_common::util::{hex, unix_time};
use cdk_common::{Amount, CurrencyUnit, MeltOptions, MeltQuoteState};
use futures::{Stream, StreamExt};
use ldk_node::bitcoin::Network;
use ldk_node::bitcoin::hashes::Hash;
use ldk_node::lightning::ln::channelmanager::PaymentId;
use ldk_node::lightning::ln::msgs::SocketAddress;
use ldk_node::lightning_invoice::{Bolt11InvoiceDescription, Description};
use ldk_node::payment::{PaymentDirection, PaymentKind, PaymentStatus, SendingParameters};
use ldk_node::{Builder, Event, Node};
use tokio::runtime::Runtime;
use tokio::sync::Mutex;
use tokio_stream::wrappers::ReceiverStream;
use tokio_util::sync::CancellationToken;
use tracing::instrument;

pub mod proto;

pub struct CdkLdkNode {
    inner: Arc<Node>,
    fee_reserve: FeeReserve,
    wait_invoice_cancel_token: CancellationToken,
    wait_invoice_is_active: Arc<AtomicBool>,
    sender: tokio::sync::mpsc::Sender<WaitPaymentResponse>,
    receiver: Arc<Mutex<Option<tokio::sync::mpsc::Receiver<WaitPaymentResponse>>>>,
    events_cancel_token: CancellationToken,
}

#[derive(Debug, Clone)]
pub struct BitcoinRpcConfig {
    pub host: String,
    pub port: u16,
    pub user: String,
    pub password: String,
}

#[derive(Debug, Clone)]
pub enum ChainSource {
    Esplora(String),
    BitcoinRpc(BitcoinRpcConfig),
}

#[derive(Debug, Clone)]
pub enum GossipSource {
    P2P,
    RapidGossipSync(String),
}

impl CdkLdkNode {
    pub fn new(
        network: Network,
        chain_source: ChainSource,
        gossip_source: GossipSource,
        storage_dir_path: String,
        fee_reserve: FeeReserve,
        listening_address: Vec<SocketAddress>,
    ) -> anyhow::Result<Self> {
        let mut builder = Builder::new();
        builder.set_network(network);
        builder.set_storage_dir_path(storage_dir_path);

        match chain_source {
            ChainSource::Esplora(esplora_url) => {
                builder.set_chain_source_esplora(esplora_url, None);
            }
            ChainSource::BitcoinRpc(BitcoinRpcConfig {
                host,
                port,
                user,
                password,
            }) => {
                builder.set_chain_source_bitcoind_rpc(host, port, user, password);
            }
        }

        match gossip_source {
            GossipSource::P2P => {
                builder.set_gossip_source_p2p();
            }
            GossipSource::RapidGossipSync(rgs_url) => {
                builder.set_gossip_source_rgs(rgs_url);
            }
        }

        builder.set_listening_addresses(listening_address)?;

        builder.set_node_alias("cdk-ldk-node".to_string())?;

        let node = builder.build()?;

        tracing::info!("Creating tokio channel for payment notifications");
        let (sender, receiver) = tokio::sync::mpsc::channel(8);

        let id = node.node_id();

        let adr = node.announcement_addresses();

        tracing::info!("Created node {} with address {:?}", id, adr);
        tracing::info!("Initialized message channels for payment notifications");

        Ok(Self {
            inner: node.into(),
            fee_reserve,
            wait_invoice_cancel_token: CancellationToken::new(),
            wait_invoice_is_active: Arc::new(AtomicBool::new(false)),
            sender,
            receiver: Arc::new(Mutex::new(Some(receiver))),
            events_cancel_token: CancellationToken::new(),
        })
    }

    pub fn start(&self, runtime: Option<Arc<Runtime>>) -> anyhow::Result<()> {
        match runtime {
            Some(runtime) => self.inner.start_with_runtime(runtime)?,
            None => self.inner.start()?,
        };
        let node_config = self.inner.config();

        tracing::info!("Starting node with network {}", node_config.network);

        tracing::info!("Node status: {:?}", self.inner.status());

        self.handle_events()?;

        Ok(())
    }

    pub fn stop(&self) -> anyhow::Result<()> {
        tracing::info!("Stopping CdkLdkNode");
        // Cancel all tokio tasks
        tracing::info!("Cancelling event handler");
        self.events_cancel_token.cancel();

        // Cancel any wait_invoice streams
        if self.is_wait_invoice_active() {
            tracing::info!("Cancelling wait_invoice stream");
            self.wait_invoice_cancel_token.cancel();
        }

        // Stop the LDK node
        tracing::info!("Stopping LDK node");
        self.inner.stop()?;
        tracing::info!("CdkLdkNode stopped successfully");
        Ok(())
    }

    pub fn handle_events(&self) -> anyhow::Result<()> {
        let node = self.inner.clone();
        let sender = self.sender.clone();
        let cancel_token = self.events_cancel_token.clone();

        tracing::info!("Starting event handler task");

        tokio::spawn(async move {
            tracing::info!("Event handler loop started");
            loop {
                tokio::select! {
                    _ = cancel_token.cancelled() => {
                        tracing::info!("Event handler cancelled");
                        break;
                    }
                    event = node.next_event_async() => {
                        tracing::debug!("Received event: {:?}", event);
                        match event {
                            Event::PaymentReceived {
                                payment_id,
                                payment_hash,
                                amount_msat,
                                custom_records: _
                            } => {
                                tracing::info!("Received payment for hash={} of amount={} msat", payment_hash, amount_msat);

                                if let Some(payment_id) = payment_id {
                                    // Convert to sats for the response
                                    let amount_sat = amount_msat / 1000;
                                    let payment_id_hex = hex::encode(payment_id.0);

                                    tracing::info!("Attempting to send payment notification: id={}, amount={}", payment_id_hex, amount_sat);

                                    let payment_details = node.payment(&payment_id).unwrap();

                                    let (payment_identifier, payment_id) = match payment_details.kind {
                                        PaymentKind::Bolt11 { hash, preimage, secret } => {
                                            (PaymentIdentifier::PaymentHash(hash.0), hash.to_string())


                                        }
                                        PaymentKind::Bolt12Offer { hash, preimage, secret, offer_id, payer_note, quantity } => {
                                            (PaymentIdentifier::OfferId(offer_id.to_string()), hash.unwrap().to_string())

                                        }
                                        _ => todo!()



                                    };


                                    let wait_payment_response = WaitPaymentResponse {
                                        payment_identifier,
                                        payment_amount: amount_sat.into(),
                                        unit: CurrencyUnit::Sat,
                                        payment_id

                                    };



                                    match sender.send(wait_payment_response).await {
                                        Ok(_) => tracing::info!("Successfully sent payment notification to stream"),
                                        Err(err) => tracing::error!(
                                            "Could not send payment received notification on channel: {}",
                                            err
                                        ),
                                    }
                                } else {
                                    tracing::warn!("Received payment without payment_id");
                                }
                            }
                            event => {
                                tracing::debug!("Received other ldk node event: {:?}", event);
                            }
                        }

                        if let Err(err) = node.event_handled() {
                            tracing::error!("Error handling node event: {}", err);
                        } else {
                            tracing::debug!("Successfully handled node event");
                        }
                    }
                }
            }
            tracing::info!("Event handler loop terminated");
        });

        tracing::info!("Event handler task spawned");
        Ok(())
    }
}

/// Mint payment trait
#[async_trait]
impl MintPayment for CdkLdkNode {
    type Err = payment::Error;

    /// Base Settings
    async fn get_settings(&self) -> Result<serde_json::Value, Self::Err> {
        let settings = Bolt11Settings {
            mpp: false,
            unit: CurrencyUnit::Sat,
            invoice_description: true,
            amountless: false,
        };
        Ok(serde_json::to_value(settings)?)
    }

    /// Create a new invoice
    #[instrument(skip(self))]
    async fn create_incoming_payment_request(
        &self,
        unit: &CurrencyUnit,
        options: IncomingPaymentOptions,
    ) -> Result<CreateIncomingPaymentResponse, Self::Err> {
        match options {
            IncomingPaymentOptions::Bolt11(bolt11_options) => {
                let amount_msat = to_unit(bolt11_options.amount, unit, &CurrencyUnit::Msat)?;
                let description = bolt11_options.description.unwrap_or_default();
                let time = bolt11_options
                    .unix_expiry
                    .map(|t| t - unix_time())
                    .unwrap_or(36000);

                let description =
                    Bolt11InvoiceDescription::Direct(Description::new(description).unwrap());

                let payment = self
                    .inner
                    .bolt11_payment()
                    .receive(amount_msat.into(), &description, time as u32)
                    .unwrap();

                let payment_hash = payment.payment_hash().to_string();
                let payment_identifier = PaymentIdentifier::PaymentHash(
                    hex::decode(&payment_hash)?
                        .try_into()
                        .map_err(|_| anyhow!("Invalid payment hash length"))?,
                );

                Ok(CreateIncomingPaymentResponse {
                    request_lookup_id: payment_identifier,
                    request: payment.to_string(),
                    expiry: Some(unix_time() + time),
                })
            }
            IncomingPaymentOptions::Bolt12(bolt12_options) => {
                let Bolt12IncomingPaymentOptions {
                    description,
                    amount,

                    unix_expiry,
                    single_use: _,
                } = *bolt12_options;

                let time = unix_expiry.map(|t| t - unix_time()).unwrap_or(36000);

                let offer = match amount {
                    Some(amount) => {
                        let amount_msat = to_unit(amount, unit, &CurrencyUnit::Msat)?;

                        self.inner
                            .bolt12_payment()
                            .receive(
                                amount_msat.into(),
                                &description.unwrap_or("".to_string()),
                                Some(time as u32),
                                None,
                            )
                            .unwrap()
                    }
                    None => self
                        .inner
                        .bolt12_payment()
                        .receive_variable_amount(
                            &description.unwrap_or("".to_string()),
                            Some(time as u32),
                        )
                        .unwrap(),
                };
                let payment_identifier = PaymentIdentifier::OfferId(offer.id().to_string());

                Ok(CreateIncomingPaymentResponse {
                    request_lookup_id: payment_identifier,
                    request: offer.to_string(),
                    expiry: Some(unix_time() + time),
                })
            }
        }
    }

    /// Get payment quote
    /// Used to get fee and amount required for a payment request
    #[instrument(skip_all)]
    async fn get_payment_quote(
        &self,
        unit: &CurrencyUnit,
        options: OutgoingPaymentOptions,
    ) -> Result<PaymentQuoteResponse, Self::Err> {
        match options {
            OutgoingPaymentOptions::Bolt11(bolt11_options) => {
                let bolt11 = bolt11_options.bolt11;

                let amount_msat = match bolt11_options.melt_options {
                    Some(melt_options) => melt_options.amount_msat(),
                    None => bolt11
                        .amount_milli_satoshis()
                        .ok_or(anyhow!("Unknown invoice amount"))?
                        .into(),
                };

                let amount = to_unit(amount_msat, &CurrencyUnit::Msat, unit)?;

                let relative_fee_reserve =
                    (self.fee_reserve.percent_fee_reserve * u64::from(amount) as f32) as u64;

                let absolute_fee_reserve: u64 = self.fee_reserve.min_fee_reserve.into();

                let fee = match relative_fee_reserve > absolute_fee_reserve {
                    true => relative_fee_reserve,
                    false => absolute_fee_reserve,
                };

                let payment_hash = bolt11.payment_hash().to_string();
                let payment_hash_bytes = hex::decode(&payment_hash)?
                    .try_into()
                    .map_err(|_| anyhow!("Invalid payment hash length"))?;

                Ok(PaymentQuoteResponse {
                    request_lookup_id: PaymentIdentifier::PaymentHash(payment_hash_bytes),
                    amount,
                    fee: fee.into(),
                    state: MeltQuoteState::Unpaid,
                    options: None,
                })
            }
            OutgoingPaymentOptions::Bolt12(bolt12_options) => {
                let offer = bolt12_options.offer;

                let amount_msat = match bolt12_options.melt_options {
                    Some(melt_options) => melt_options.amount_msat(),
                    None => {
                        let amount = offer.amount().ok_or(payment::Error::AmountMismatch)?;

                        match amount {
                            ldk_node::lightning::offers::offer::Amount::Bitcoin {
                                amount_msats,
                            } => amount_msats.into(),
                            _ => return Err(payment::Error::AmountMismatch),
                        }
                    }
                };
                let amount = to_unit(amount_msat, &CurrencyUnit::Msat, unit)?;

                let relative_fee_reserve =
                    (self.fee_reserve.percent_fee_reserve * u64::from(amount) as f32) as u64;

                let absolute_fee_reserve: u64 = self.fee_reserve.min_fee_reserve.into();

                let fee = match relative_fee_reserve > absolute_fee_reserve {
                    true => relative_fee_reserve,
                    false => absolute_fee_reserve,
                };

                Ok(PaymentQuoteResponse {
                    request_lookup_id: PaymentIdentifier::OfferId(offer.id().to_string()),
                    amount,
                    fee: fee.into(),
                    state: MeltQuoteState::Unpaid,
                    options: None,
                })
            }
        }
    }

    /// Pay request
    #[instrument(skip(self))]
    async fn make_payment(
        &self,
        unit: &CurrencyUnit,
        options: OutgoingPaymentOptions,
    ) -> Result<MakePaymentResponse, Self::Err> {
        match options {
            OutgoingPaymentOptions::Bolt11(bolt11_options) => {
                let bolt11 = bolt11_options.bolt11;

                let send_params = bolt11_options.max_fee_amount.map(|f| {
                    let amount_msat = to_unit(f, unit, &CurrencyUnit::Msat).unwrap();

                    SendingParameters {
                        max_total_routing_fee_msat: Some(Some(amount_msat.into())),
                        max_channel_saturation_power_of_half: None,
                        max_total_cltv_expiry_delta: None,
                        max_path_count: None,
                    }
                });

                let payment_id = match bolt11_options.melt_options {
                    Some(MeltOptions::Amountless { amountless }) => self
                        .inner
                        .bolt11_payment()
                        .send_using_amount(&bolt11, amountless.amount_msat.into(), send_params)
                        .unwrap(),
                    None => self
                        .inner
                        .bolt11_payment()
                        .send(&bolt11, send_params)
                        .unwrap(),
                    _ => return Err(payment::Error::UnsupportedPaymentOption),
                };

                // Check payment status for up to 10 seconds
                let start = std::time::Instant::now();
                let timeout = std::time::Duration::from_secs(10);

                let (status, payment_details) = loop {
                    let details = self
                        .inner
                        .payment(&payment_id)
                        .ok_or(anyhow!("Payment not found"))?;

                    match details.status {
                        PaymentStatus::Succeeded => break (MeltQuoteState::Paid, details),
                        PaymentStatus::Failed => break (MeltQuoteState::Failed, details),
                        PaymentStatus::Pending => {
                            if start.elapsed() > timeout {
                                break (MeltQuoteState::Pending, details);
                            }
                            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                            continue;
                        }
                    }
                };

                let payment_proof = match payment_details.kind {
                    PaymentKind::Bolt11 {
                        hash: _,
                        preimage,
                        secret: _,
                    } => preimage.map(|p| p.to_string()),
                    _ => return Err(anyhow!("Unexpected payment kind").into()),
                };

                let total_spent = payment_details
                    .amount_msat
                    .ok_or(anyhow!("Could not get amount spent"))?;

                let total_spent = to_unit(total_spent, &CurrencyUnit::Msat, unit)?;

                Ok(MakePaymentResponse {
                    payment_lookup_id: PaymentIdentifier::PaymentHash(
                        bolt11.payment_hash().to_byte_array(),
                    ),
                    payment_proof,
                    status,
                    total_spent,
                    unit: unit.clone(),
                })
            }
            OutgoingPaymentOptions::Bolt12(bolt12_options) => {
                let offer = bolt12_options.offer;

                let payment_id = match bolt12_options.melt_options {
                    Some(MeltOptions::Amountless { amountless }) => self
                        .inner
                        .bolt12_payment()
                        .send_using_amount(&offer, amountless.amount_msat.into(), None, None)
                        .unwrap(),
                    None => self
                        .inner
                        .bolt12_payment()
                        .send(&offer, None, None)
                        .unwrap(),
                    _ => return Err(payment::Error::UnsupportedPaymentOption),
                };

                // Check payment status for up to 10 seconds
                let start = std::time::Instant::now();
                let timeout = std::time::Duration::from_secs(10);

                let (status, payment_details) = loop {
                    let details = self
                        .inner
                        .payment(&payment_id)
                        .ok_or(anyhow!("Payment not found"))?;

                    match details.status {
                        PaymentStatus::Succeeded => break (MeltQuoteState::Paid, details),
                        PaymentStatus::Failed => break (MeltQuoteState::Failed, details),
                        PaymentStatus::Pending => {
                            if start.elapsed() > timeout {
                                break (MeltQuoteState::Pending, details);
                            }
                            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                            continue;
                        }
                    }
                };

                let payment_proof = match payment_details.kind {
                    PaymentKind::Bolt11 {
                        hash: _,
                        preimage,
                        secret: _,
                    } => preimage.map(|p| p.to_string()),
                    _ => return Err(anyhow!("Unexpected payment kind").into()),
                };

                let total_spent = payment_details
                    .amount_msat
                    .ok_or(anyhow!("Could not get amount spent"))?;

                let total_spent = to_unit(total_spent, &CurrencyUnit::Msat, unit)?;

                Ok(MakePaymentResponse {
                    payment_lookup_id: PaymentIdentifier::OfferId(offer.id().to_string()),
                    payment_proof,
                    status,
                    total_spent,
                    unit: unit.clone(),
                })
            }
        }
    }

    /// Listen for invoices to be paid to the mint
    /// Returns a stream of request_lookup_id once invoices are paid
    #[instrument(skip(self))]
    async fn wait_any_incoming_payment(
        &self,
    ) -> Result<Pin<Box<dyn Stream<Item = WaitPaymentResponse> + Send>>, Self::Err> {
        tracing::info!("Starting stream for invoices - wait_any_incoming_payment called");

        // Set active flag to indicate stream is active
        self.wait_invoice_is_active.store(true, Ordering::SeqCst);
        tracing::debug!("wait_invoice_is_active set to true");

        let receiver = self.receiver.lock().await.take().ok_or_else(|| {
            tracing::error!("Failed to get receiver from mutex");
            anyhow!("Could not get receiver")
        })?;

        tracing::info!("Receiver obtained successfully, creating response stream");

        // Transform the String stream into a WaitPaymentResponse stream
        let response_stream = ReceiverStream::new(receiver);

        // Create a combined stream that also handles cancellation
        let cancel_token = self.wait_invoice_cancel_token.clone();
        let is_active = self.wait_invoice_is_active.clone();

        let stream = Box::pin(response_stream);

        // Set up a task to clean up when the stream is dropped
        tokio::spawn(async move {
            cancel_token.cancelled().await;
            tracing::info!("wait_invoice stream cancelled");
            is_active.store(false, Ordering::SeqCst);
        });

        tracing::info!("wait_any_incoming_payment returning stream");
        Ok(stream)
    }

    /// Is wait invoice active
    fn is_wait_invoice_active(&self) -> bool {
        self.wait_invoice_is_active.load(Ordering::SeqCst)
    }

    /// Cancel wait invoice
    fn cancel_wait_invoice(&self) {
        self.wait_invoice_cancel_token.cancel()
    }

    /// Check the status of an incoming payment
    async fn check_incoming_payment_status(
        &self,
        payment_identifier: &PaymentIdentifier,
    ) -> Result<Vec<WaitPaymentResponse>, Self::Err> {
        let payment_id_str = match payment_identifier {
            PaymentIdentifier::PaymentHash(hash) => hex::encode(hash),
            PaymentIdentifier::CustomId(id) => id.clone(),
            _ => return Err(anyhow!("Unsupported payment identifier type").into()),
        };

        let payment_id = PaymentId(
            hex::decode(&payment_id_str)?
                .try_into()
                .map_err(|_| anyhow!("Invalid payment ID length"))?,
        );

        let payment_details = self
            .inner
            .payment(&payment_id)
            .ok_or(anyhow!("Payment not found"))?;

        if payment_details.direction == PaymentDirection::Outbound {
            return Err(anyhow!("Invalid payment direction").into());
        }

        let amount = payment_details
            .amount_msat
            .ok_or(anyhow!("Could not get payment amount"))?;

        let response = WaitPaymentResponse {
            payment_identifier: payment_identifier.clone(),
            payment_amount: amount.into(),
            unit: CurrencyUnit::Msat,
            payment_id: payment_id_str,
        };

        Ok(vec![response])
    }

    /// Check the status of an outgoing payment
    async fn check_outgoing_payment(
        &self,
        request_lookup_id: &PaymentIdentifier,
    ) -> Result<MakePaymentResponse, Self::Err> {
        let payment_id_str = match request_lookup_id {
            PaymentIdentifier::PaymentHash(hash) => hex::encode(hash),
            PaymentIdentifier::CustomId(id) => id.clone(),
            _ => return Err(anyhow!("Unsupported payment identifier type").into()),
        };

        let payment_id = PaymentId(
            hex::decode(&payment_id_str)?
                .try_into()
                .map_err(|_| anyhow!("Invalid payment ID length"))?,
        );

        let payment_details = self
            .inner
            .payment(&payment_id)
            .ok_or(anyhow!("Payment not found"))?;

        // This check seems reversed in the original code, so I'm fixing it here
        if payment_details.direction != PaymentDirection::Outbound {
            return Err(anyhow!("Invalid payment direction").into());
        }

        let status = match payment_details.status {
            PaymentStatus::Pending => MeltQuoteState::Pending,
            PaymentStatus::Succeeded => MeltQuoteState::Paid,
            PaymentStatus::Failed => MeltQuoteState::Failed,
        };

        let payment_proof = match payment_details.kind {
            PaymentKind::Bolt11 {
                hash: _,
                preimage,
                secret: _,
            } => preimage.map(|p| p.to_string()),
            _ => return Err(anyhow!("Unexpected payment kind").into()),
        };

        let total_spent = payment_details
            .amount_msat
            .ok_or(anyhow!("Could not get amount spent"))?;

        Ok(MakePaymentResponse {
            payment_lookup_id: request_lookup_id.clone(),
            payment_proof,
            status,
            total_spent: total_spent.into(),
            unit: CurrencyUnit::Msat,
        })
    }
}

impl Drop for CdkLdkNode {
    fn drop(&mut self) {
        tracing::info!("Drop called on CdkLdkNode");
        self.wait_invoice_cancel_token.cancel();
        tracing::debug!("Cancelled wait_invoice token in drop");
        if let Err(e) = self.stop() {
            tracing::error!("Error stopping node during drop: {}", e);
        } else {
            tracing::info!("Successfully stopped node during drop");
        }
    }
}
