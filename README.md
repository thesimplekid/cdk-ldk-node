# WIP: Do not use with real funds.

## To run node: 
```
cargo r --bin cdk-ldk-node
```

## Configuration Environment Variables

You can configure the chain source by setting the following environment variables:

```bash
# Set to 'esplora' or 'bitcoinrpc'
CDK_CHAIN_SOURCE=esplora

# Esplora configuration
CDK_ESPLORA_URL=https://mutinynet.com/api

# Bitcoin RPC configuration
CDK_BITCOIN_RPC_HOST=127.0.0.1
CDK_BITCOIN_RPC_PORT=18443
CDK_BITCOIN_RPC_USER=testuser
CDK_BITCOIN_RPC_PASS=testpass

# Bitcoin network - can be 'mainnet', 'testnet', 'signet', or 'regtest' (default is 'regtest')
CDK_BITCOIN_NETWORK=regtest

# Storage directory path for Lightning Network state (defaults to ~/.cdk-ldk-node)
CDK_STORAGE_DIR_PATH=/path/to/data
```

Other configuration options:

```bash
CDK_PAYMENT_PROCESSOR_LISTEN_HOST=127.0.0.1
CDK_PAYMENT_PROCESSOR_LISTEN_PORT=8089
CDK_GRPC_HOST=127.0.0.1
CDK_GRPC_PORT=50051
```

## To run node cli:
```
cargo r --bin cdk-ldk-cli
```



To run with cdk-mind add the below to cdk-mintd config file:

```toml
[ln]

# Required ln backend `cln`, `ldk`, `greenlight`
ln_backend = "grpcprocessor"

# [lnbits]
# admin_api_key = "da1d25a677424da5a9510fbb49a6b48c"
# invoice_api_key = "28accdb7c60c4ef19952c64ca22238e2"
# lnbits_api = "https://demo.lnbits.com"
# fee_percent=0.04
# reserve_fee_min=4

[grpc_processor]
supported_units=["sat"]
addr="http://127.0.0.1"
port="8089"

```
