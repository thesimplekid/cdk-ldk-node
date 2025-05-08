#!/usr/bin/env bash


# Exit script if any command fails
set -e

# Directory where data will be stored
NODE_DATA_DIR="$HOME/.cdk-ldk-node"

# Create data directory if it doesn't exist
mkdir -p "$NODE_DATA_DIR"

# Set environment variables for configuration
export CDK_BITCOIN_NETWORK="regtest"
export CDK_STORAGE_DIR_PATH="$NODE_DATA_DIR"
export CDK_LISTEN_HOST="0.0.0.0"  # Listen on all interfaces
export CDK_LISTEN_PORT="8089"      # Default port
export CDK_GRPC_HOST="0.0.0.0"     # GRPC listen on all interfaces
export CDK_GRPC_PORT="50051"       # Default GRPC port

# Set chain source (esplora is default, but you can change to bitcoinrpc if needed)
# export CDK_CHAIN_SOURCE="esplora"
# export CDK_ESPLORA_URL="https://mutinynet.com/api"

# If using bitcoinrpc, uncomment and configure these:
export CDK_CHAIN_SOURCE="bitcoinrpc"
export CDK_BITCOIN_RPC_HOST="127.0.0.1"
export CDK_BITCOIN_RPC_PORT="18443"
export CDK_BITCOIN_RPC_USER="testuser"
export CDK_BITCOIN_RPC_PASS="testpass"

echo "Starting cdk-ldk-node..."
echo "Data directory: $NODE_DATA_DIR"
echo "Network: $CDK_BITCOIN_NETWORK"
echo "Chain source: $CDK_CHAIN_SOURCE"

# Run the node
cargo run --bin cdk-ldk-node

# Note: Configuration is done through environment variables
