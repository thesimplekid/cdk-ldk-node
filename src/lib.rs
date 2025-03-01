use std::pin::Pin;
use std::str::FromStr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::anyhow;
use async_trait::async_trait;
use cdk_common::amount::to_unit;
use cdk_common::common::FeeReserve;
use cdk_common::payment::{
    self, Bolt11Settings, CreateIncomingPaymentResponse, MakePaymentResponse, MintPayment,
    PaymentQuoteResponse,
};
use cdk_common::util::{hex, unix_time};
use cdk_common::{
    Amount, Bolt11Invoice, CurrencyUnit, MeltOptions, MeltQuoteState, MintQuoteState, mint,
};
use futures::Stream;
use ldk_node::bitcoin::Network;
use ldk_node::lightning::ln::channelmanager::PaymentId;
use ldk_node::lightning::ln::msgs::SocketAddress;
use ldk_node::payment::{PaymentDirection, PaymentKind, PaymentStatus, SendingParameters};
use ldk_node::{Builder, Event, Node};
use tokio::runtime::Runtime;
use tokio::sync::Mutex;
use tokio_stream::wrappers::ReceiverStream;
use tokio_util::sync::CancellationToken;

pub mod proto;

pub struct CdkLdkNode {
    inner: Arc<Node>,
    fee_reserve: FeeReserve,
    wait_invoice_cancel_token: CancellationToken,
    wait_invoice_is_active: Arc<AtomicBool>,
    sender: tokio::sync::mpsc::Sender<String>,
    receiver: Arc<Mutex<Option<tokio::sync::mpsc::Receiver<String>>>>,
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
        chain_source: ChainSource,
        gossip_source: GossipSource,
        fee_reserve: FeeReserve,
        listening_address: Vec<SocketAddress>,
    ) -> anyhow::Result<Self> {
        let builder = Builder::new();
        builder.set_network(Network::Regtest);

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

        builder.set_node_alias("Cdk-mint-node".to_string())?;

        let node = builder.build()?;

        let (sender, receiver) = tokio::sync::mpsc::channel(8);

        Ok(Self {
            inner: node,
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

        self.handle_events()?;

        Ok(())
    }

    pub fn stop(&self) -> anyhow::Result<()> {
        self.events_cancel_token.cancel();
        self.inner.stop()?;
        Ok(())
    }

    pub fn handle_events(&self) -> anyhow::Result<()> {
        let node = self.inner.clone();
        let sender = self.sender.clone();
        let cancel_token = self.events_cancel_token.clone();

        tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = cancel_token.cancelled() => {
                        tracing::info!("Event handler cancelled");
                        break;
                    }
                    event = node.next_event_async() => {
                        match event {
                            Event::PaymentReceived {
                                payment_id,
                                payment_hash,
                                amount_msat: _,
                            } => {
                                tracing::info!("Received payment for {}", payment_hash);
                                if let Some(payment_id) = payment_id {
                                    if let Err(err) = sender.send(hex::encode(payment_id.0)).await {
                                        tracing::error!(
                                            "Could not send payment received on channel: {}",
                                            err
                                        );
                                    }
                                }
                            }
                            event => {
                                tracing::info!("Received ldk node event: {:?}", event);
                            }
                        }
                        node.event_handled();
                    }
                }
            }
        });

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
        };
        Ok(serde_json::to_value(settings)?)
    }

    /// Create a new invoice
    async fn create_incoming_payment_request(
        &self,
        amount: Amount,
        unit: &CurrencyUnit,
        description: String,
        unix_expiry: Option<u64>,
    ) -> Result<CreateIncomingPaymentResponse, Self::Err> {
        let amount_msat = to_unit(amount, unit, &CurrencyUnit::Msat)?;

        let time = unix_expiry.map(|t| t - unix_time()).unwrap_or(36000);

        let payment = self
            .inner
            .bolt11_payment()
            .receive(amount_msat.into(), &description, time as u32)
            .unwrap();

        Ok(CreateIncomingPaymentResponse {
            request_lookup_id: payment.payment_hash().to_string(),
            request: payment.to_string(),
            expiry: Some(unix_time() + time),
        })
    }

    /// Get payment quote
    /// Used to get fee and amount required for a payment request
    async fn get_payment_quote(
        &self,
        request: &str,
        unit: &CurrencyUnit,
        options: Option<MeltOptions>,
    ) -> Result<PaymentQuoteResponse, Self::Err> {
        let bolt11 = Bolt11Invoice::from_str(request)?;

        let amount_msat = match options {
            Some(amount) => amount.amount_msat(),
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

        Ok(PaymentQuoteResponse {
            request_lookup_id: bolt11.payment_hash().to_string(),
            amount,
            fee: fee.into(),
            state: MeltQuoteState::Unpaid,
        })
    }

    /// Pay request
    async fn make_payment(
        &self,
        melt_quote: mint::MeltQuote,
        partial_amount: Option<Amount>,
        max_fee_amount: Option<Amount>,
    ) -> Result<MakePaymentResponse, Self::Err> {
        if partial_amount.is_some() {
            return Err(anyhow!("LDK Node does not support mpp payments").into());
        }

        let bolt11 = Bolt11Invoice::from_str(&melt_quote.request)?;

        //TODO: Do we need to check this or will attempting to pay error with ldk
        // let pay_state = self
        //     .check_outgoing_payment(&bolt11.payment_hash().to_string())
        //     .await?;

        // match pay_state.status {
        //     MeltQuoteState::Unpaid | MeltQuoteState::Unknown | MeltQuoteState::Failed => (),
        //     MeltQuoteState::Paid => {
        //         tracing::debug!("Melt attempted on invoice already paid");
        //         return Err(Self::Err::InvoiceAlreadyPaid);
        //     }
        //     MeltQuoteState::Pending => {
        //         tracing::debug!("Melt attempted on invoice already pending");
        //         return Err(Self::Err::InvoicePaymentPending);
        //     }
        // }

        let send_params = max_fee_amount.map(|f| {
            let amount_msat = to_unit(f, &melt_quote.unit, &CurrencyUnit::Msat).unwrap();

            SendingParameters {
                max_total_routing_fee_msat: Some(
                    ldk_node::payment::MaxTotalRoutingFeeLimit::Some {
                        amount_msat: amount_msat.into(),
                    },
                ),
                max_channel_saturation_power_of_half: None,
                max_total_cltv_expiry_delta: None,
                max_path_count: None,
            }
        });

        let payment_id = self
            .inner
            .bolt11_payment()
            .send(&bolt11, send_params)
            .unwrap();

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

        let total_spent = to_unit(total_spent, &CurrencyUnit::Msat, &melt_quote.unit)?;

        Ok(MakePaymentResponse {
            payment_lookup_id: hex::encode(payment_id.0),
            payment_proof,
            status,
            total_spent,
            unit: melt_quote.unit,
        })
    }

    /// Listen for invoices to be paid to the mint
    /// Returns a stream of request_lookup_id once invoices are paid
    async fn wait_any_incoming_payment(
        &self,
    ) -> Result<Pin<Box<dyn Stream<Item = String> + Send>>, Self::Err> {
        tracing::info!("Starting stream for fake invoices");
        let receiver = self
            .receiver
            .lock()
            .await
            .take()
            .ok_or(anyhow!("Could not get receiver"))?;
        let receiver_stream = ReceiverStream::new(receiver);
        Ok(Box::pin(receiver_stream))
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
        request_lookup_id: &str,
    ) -> Result<MintQuoteState, Self::Err> {
        let payment_id = PaymentId(hex::decode(request_lookup_id).unwrap().try_into().unwrap());

        let payment_details = self
            .inner
            .payment(&payment_id)
            .ok_or(anyhow!("Payment not found"))?;

        if payment_details.direction == PaymentDirection::Outbound {
            return Err(anyhow!("Invalid payment direction").into());
        }

        let status = match payment_details.status {
            PaymentStatus::Pending => MintQuoteState::Pending,
            PaymentStatus::Succeeded => MintQuoteState::Paid,
            PaymentStatus::Failed => MintQuoteState::Unpaid,
        };

        Ok(status)
    }

    /// Check the status of an outgoing payment
    async fn check_outgoing_payment(
        &self,
        request_lookup_id: &str,
    ) -> Result<MakePaymentResponse, Self::Err> {
        let payment_id = PaymentId(hex::decode(request_lookup_id).unwrap().try_into().unwrap());

        let payment_details = self
            .inner
            .payment(&payment_id)
            .ok_or(anyhow!("Payment not found"))?;

        if payment_details.direction == PaymentDirection::Outbound {
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
            payment_lookup_id: request_lookup_id.to_string(),
            payment_proof,
            status,
            total_spent: total_spent.into(),
            unit: CurrencyUnit::Msat,
        })
    }
}

impl Drop for CdkLdkNode {
    fn drop(&mut self) {
        self.wait_invoice_cancel_token.cancel();
        let _ = self.stop();
    }
}
