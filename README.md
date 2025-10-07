# nfcbridge-server

A very simple bent pipe websocket server written in Rust - can be used to setup connections between two clients.
This server does not have any security whatsoever.

To use:
```py
cargo run --bin echo-server 127.0.0.1:12345
cargo run --bin client ws://127.0.0.1:12345/host/my-room-name
cargo run --bin client ws://127.0.0.1:12345/join/my-room-name
```