# WIP: Do not use with real funds.

## To run node: 
```
cargo r --bin cdk-ldk-node
```

## To run node CLI:
```
cargo r --bin cdk-ldk-cli
```

## Configuration

There are two ways to configure the node:

1. Using a `config.toml` file in the home directory at `~/.cdk-ldk-node/config.toml`
2. Using environment variables

Environment variables will take precedence over values in the config file if both are set.

### Config File

By default, the application looks for configuration in these locations, in order:
1. `~/.cdk-ldk-node/config.toml` (preferred)
2. `./config.toml` (current directory, as a fallback)

If no configuration file exists, the application will automatically create one with default values at `~/.cdk-ldk-node/config.toml`.

### Environment Variables

You can also configure the node using environment variables. These will override any values set in the config file.

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

# Payment processor settings
CDK_PAYMENT_PROCESSOR_LISTEN_HOST=127.0.0.1
CDK_PAYMENT_PROCESSOR_LISTEN_PORT=8089

# GRPC API settings
CDK_GRPC_HOST=127.0.0.1
CDK_GRPC_PORT=50051

# LDK Node settings
CDK_LDK_NODE_HOST=127.0.0.1
CDK_LDK_NODE_PORT=8090
```

## Integration with CDK-MINT

To run with cdk-mintd, add the following to your cdk-mintd config file:

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