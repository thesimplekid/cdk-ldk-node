# WIP: Do not use with real funds.

## To run node: 
```
cargo r --bin cdk-ldk-node
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
