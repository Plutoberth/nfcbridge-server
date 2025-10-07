//! A simple echo server.
//!
//! You can test this out by running:
//!
//!     cargo run --example echo-server 127.0.0.1:12345
//!
//! And then in another window run:
//!
//!     cargo run --example client ws://127.0.0.1:12345/
//!
//! Type a message into the client window, press enter to send it and
//! see it echoed back.

use std::{env, io::Error};

use futures_util::{future, StreamExt, TryStreamExt};
use log::info;
use tokio::net::{TcpListener, TcpStream};
    use tokio_tungstenite::tungstenite::handshake::server::{Request, Response};


#[tokio::main]
async fn main() -> Result<(), Error> {
    let _ = env_logger::try_init();
    let addr = env::args().nth(1).unwrap_or_else(|| "127.0.0.1:8080".to_string());

    // Create the event loop and TCP listener we'll accept connections on.
    let try_socket = TcpListener::bind(&addr).await;
    let listener = try_socket.expect("Failed to bind");
    info!("Listening on: {}", addr);

    while let Ok((stream, _)) = listener.accept().await {
        tokio::spawn(accept_connection(stream));
    }

    Ok(())
}

async fn accept_connection(stream: TcpStream) {
    let addr = stream.peer_addr().expect("connected streams should have a peer address");
    info!("Peer address: {}", addr);


    let mut query_string: Option<String> = None;

    let ws_stream = tokio_tungstenite::accept_hdr_async(stream,  |req: &Request, response: Response| {
        // accept a path like /room/<name>
        let path = req.uri().path();
        if path.starts_with("/room/") {
            query_string = path.strip_prefix("/room/").map(|s| s.to_string());
            return Ok(response);
        } else {
            panic!("Path must start with /room/");
            // // invalid path
            // let response = Response::builder()
            //     .status(400)
            //     .body(Some("Invalid path".into()))
            //     .unwrap();
            // return Ok(response);
        }
    })
    .await
    .expect("Error during the websocket handshake occurred");
    
    //let room_name = room_holder.lock().ok().and_then(|g| g.clone());
    
    if let Some(r) = query_string {
        println!("New WebSocket connection: {} (room={})", addr, r);
    } else {
        println!("New WebSocket connection: {}", addr);
    }

    let (write, read) = ws_stream.split();
    // We should not forward messages other than text or binary.
    read.try_filter(|msg| future::ready(msg.is_text() || msg.is_binary()))
        .forward(write)
        .await
        .expect("Failed to forward messages")
}