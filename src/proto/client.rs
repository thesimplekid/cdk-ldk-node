use std::path::PathBuf;

use anyhow::Result;
use tonic::transport::Channel;

use super::cdk_ldk_management_client::CdkLdkManagementClient;
use super::*;

pub struct CdkLdkClient {
    client: CdkLdkManagementClient<Channel>,
}

impl CdkLdkClient {
    pub fn new(channel: Channel) -> Self {
        Self {
            client: CdkLdkManagementClient::new(channel),
        }
    }

    pub async fn connect(addr: String) -> Result<Self> {
        let client = CdkLdkManagementClient::connect(addr).await?;
        Ok(Self { client })
    }

    /// Create a client with TLS configuration based on the work_dir
    pub async fn create_with_work_dir(address: String, work_dir: PathBuf) -> Result<Self> {
        let channel = crate::utils::create_channel(address, work_dir).await?;
        Ok(Self::new(channel))
    }

    pub async fn get_info(&mut self) -> Result<GetInfoResponse> {
        let request = GetInfoRequest {};
        let response = self.client.get_info(request).await?;
        Ok(response.into_inner())
    }

    pub async fn get_new_address(&mut self) -> Result<String> {
        let request = GetNewAddressRequest {};
        let response = self.client.get_new_address(request).await?;
        Ok(response.into_inner().address)
    }

    pub async fn open_channel(
        &mut self,
        node_id: String,
        address: String,
        port: u32,
        amount_msats: u64,
        push_to_counter_party_msats: Option<u64>,
    ) -> Result<String> {
        let request = OpenChannelRequest {
            node_id,
            address,
            port,
            amount_msats,
            push_to_counter_party_msats,
        };
        let response = self.client.open_channel(request).await?;
        Ok(response.into_inner().channel_id)
    }

    pub async fn close_channel(&mut self, channel_id: String, node_pubkey: String) -> Result<()> {
        let request = CloseChannelRequest {
            channel_id,
            node_pubkey,
        };
        self.client.close_channel(request).await?;
        Ok(())
    }

    pub async fn list_balance(&mut self) -> Result<ListBalanceResponse> {
        let request = ListBalanceRequest {};
        let response = self.client.list_balance(request).await?;
        Ok(response.into_inner())
    }

    pub async fn list_channels(&mut self) -> Result<ListChannelsResponse> {
        let request = ListChannelsRequest {};
        let response = self.client.list_channels(request).await?;
        Ok(response.into_inner())
    }

    pub async fn send_onchain(&mut self, amount_sat: u64, address: String) -> Result<String> {
        let request = SendOnchainRequest {
            amount_sat,
            address,
        };
        let response = self.client.send_onchain(request).await?;
        Ok(response.into_inner().txid)
    }

    pub async fn pay_bolt11_invoice(
        &mut self,
        invoice: String,
        amount_msats: Option<u64>,
    ) -> Result<PaymentResponse> {
        let request = PayBolt11InvoiceRequest {
            invoice,
            amount_msats,
        };
        let response = self.client.pay_bolt11_invoice(request).await?;
        Ok(response.into_inner())
    }

    pub async fn pay_bolt12_offer(
        &mut self,
        offer: String,
        amount_msats: u64,
    ) -> Result<PaymentResponse> {
        let request = PayBolt12OfferRequest {
            offer,
            amount_msats,
        };
        let response = self.client.pay_bolt12_offer(request).await?;
        Ok(response.into_inner())
    }

    pub async fn create_bolt11_invoice(
        &mut self,
        amount_msats: u64,
        description: String,
        expiry_seconds: Option<u32>,
    ) -> Result<CreateInvoiceResponse> {
        let request = CreateBolt11InvoiceRequest {
            amount_msats,
            description,
            expiry_seconds,
        };
        let response = self.client.create_bolt11_invoice(request).await?;
        Ok(response.into_inner())
    }

    pub async fn create_bolt12_offer(
        &mut self,
        amount_msats: Option<u64>,
        description: String,
        expiry_seconds: Option<u32>,
    ) -> Result<CreateOfferResponse> {
        let request = CreateBolt12OfferRequest {
            amount_msats,
            description,
            expiry_seconds,
        };
        let response = self.client.create_bolt12_offer(request).await?;
        Ok(response.into_inner())
    }
}
