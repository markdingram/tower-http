# client

This example has a client for the key value store servers.

## Running the example

Start the server:

```
cd ../axum-key-value-store && \
    RUST_LOG=axum_key_value_store=trace,tower_http=trace \
    cargo run --bin axum-key-value-store
```

Run the client:

```
RUST_LOG=client=trace,tower_http=trace \
    cargo run --bin client
```