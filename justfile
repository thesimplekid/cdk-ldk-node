# justfile for cdk-ldk-node

# Default recipe (runs when you just type 'just')
default:
    @just --list

# Start the node with the test script
start-node:
    @echo "Starting CDK-LDK node..."
    ./misc/start_node.sh

# Start the node with specific network (mainnet, testnet, signet, regtest)
start-node-network network="regtest":
    @echo "Starting CDK-LDK node on network: {{network}}..."
    CDK_BITCOIN_NETWORK={{network}} ./misc/start_node.sh

# Start node with Bitcoin RPC connection
start-node-bitcoinrpc host="127.0.0.1" port="18443" user="testuser" pass="testpass":
    @echo "Starting CDK-LDK node with Bitcoin RPC connection..."
    CDK_CHAIN_SOURCE=bitcoinrpc \
    CDK_BITCOIN_RPC_HOST={{host}} \
    CDK_BITCOIN_RPC_PORT={{port}} \
    CDK_BITCOIN_RPC_USER={{user}} \
    CDK_BITCOIN_RPC_PASS={{pass}} \
    ./misc/start_node.sh

# Build the project
build:
    cargo build

# Run tests
test:
    cargo test

# Clean build artifacts
clean:
    cargo clean