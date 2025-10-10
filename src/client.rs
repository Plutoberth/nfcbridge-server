//! A simple example of hooking up stdin/stdout to a WebSocket stream.
//!
//! This example will connect to a server specified in the argument list and
//! then forward all data read on stdin to the server, printing out all data
//! received on stdout.
//!
//! Note that this is not currently optimized for performance, especially around
//! buffer management. Rather it's intended to show an example of working with a
//! client.
//!
//! You can use this example together with the `server` example.

use std::{env, error::Error, time::Instant};

use futures_util::{future, pin_mut, StreamExt};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::time::{sleep, Duration};
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message};
use std::sync::Arc;
use tokio::sync::Mutex as AsyncMutex;
use std::collections::HashMap;
use std::time::SystemTime;

#[tokio::main]
async fn main() {
    let url =
        env::args().nth(1).unwrap_or_else(|| panic!("this program requires at least one argument"));

    // outgoing channel - everything that should be sent to the websocket goes here
    let (out_tx, out_rx) = futures_channel::mpsc::unbounded();
    tokio::spawn(read_stdin(out_tx.clone()));

    // outstanding pings: payload string -> Instant when sent
    let outstanding = Arc::new(AsyncMutex::new(HashMap::<String, Instant>::new()));

    // spawn a periodic ping sender (every 5 seconds)
    {
        let out_tx = out_tx.clone();
        let outstanding = outstanding.clone();
        tokio::spawn(async move {
            loop {
                sleep(Duration::from_secs(5)).await;
                // create payload as current millis since unix epoch in decimal ASCII
                let now = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap();
                let payload_str = format!("{}", now.as_millis());
                let payload_for_send = payload_str.clone();
                let _ = out_tx.unbounded_send(Message::Ping(payload_for_send.clone().into()));
                let mut guard = outstanding.lock().await;
                guard.insert(payload_str.clone(), Instant::now());
            }
        });
    }

    // Print content of Http response message
    let res = connect_async(&url).await;

    let Ok((ws_stream, _)) = res else {
        eprintln!("Error during connection to {}: {}", url, res.unwrap_err());
        return;
    };

    println!("WebSocket handshake has been successfully completed");

    let (write, read) = ws_stream.split();

    // Forward outgoing messages to the websocket writer
    let outgoing_to_ws = out_rx.map(Ok).forward(write);

    // Handle incoming messages separately
    let outstanding_for_read = outstanding.clone();
    let out_tx_for_read = out_tx.clone();
    let ws_to_stdout = read.for_each(move |message| {
        let outstanding = outstanding_for_read.clone();
        let out_tx = out_tx_for_read.clone();
        async move {
            let message = match message {
                Ok(m) => m,
                Err(e) => {
                    eprintln!("Failed to read message: {}", e);
                    return;
                }
            };

            match message {
                Message::Text(txt) => {
                    // Chat text - print to stdout
                    tokio::io::stdout().write_all(txt.as_bytes()).await.unwrap();
                }
                Message::Binary(data) => {
                    // Chat binary - print raw data
                    tokio::io::stdout().write_all(&data).await.unwrap();
                }
                Message::Ping(payload) => {
                    // reply with Pong carrying same payload; do not print payload
                    let _ = out_tx.unbounded_send(Message::Pong(payload));
                }
                Message::Pong(payload) => {
                    // If this matches an outstanding ping payload, compute RTT
                    if let Ok(s) = String::from_utf8(payload.to_vec()) {
                        let mut guard = outstanding.lock().await;
                        if let Some(sent) = guard.remove(&s) {
                            guard.clear();
                            let rtt = Instant::now().duration_since(sent);
                            println!("Ping RTT: {} ms", rtt.as_millis());
                        }
                    }
                }
                Message::Close(_) => {
                    // ignore; the forwarder will terminate
                }
                _ => {}
            }
        }
    });

    pin_mut!(outgoing_to_ws, ws_to_stdout);
    future::select(outgoing_to_ws, ws_to_stdout).await;
}

// Our helper method which will read data from stdin and send it along the
// sender provided.
async fn read_stdin(tx: futures_channel::mpsc::UnboundedSender<Message>) {
    let mut stdin = tokio::io::stdin();
    loop {
        let mut buf = vec![0; 1024];
        let n = match stdin.read(&mut buf).await {
            Err(_) | Ok(0) => break,
            Ok(n) => n,
        };
        buf.truncate(n);
        tx.unbounded_send(Message::binary(buf)).unwrap();
    }
}