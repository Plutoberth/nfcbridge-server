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

use std::{collections::HashMap, env, io::Error, sync::Arc};

use futures_util::{future, StreamExt, TryStreamExt};
use tokio::net::{TcpListener, TcpStream};
    use tokio_tungstenite::{tungstenite::{handshake::server::{Request, Response}, protocol::frame::{coding::CloseCode, CloseFrame}}, WebSocketStream};


type RoomHashmap = HashMap<String, WebSocketStream<TcpStream>>;

#[tokio::main]
async fn main() -> Result<(), Error> {
    let rooms: RoomHashmap = HashMap::new();
    let room_shared = std::sync::Arc::new(tokio::sync::Mutex::new(rooms));

    let addr = env::args().nth(1).unwrap_or_else(|| "127.0.0.1:8080".to_string());

    // Create the event loop and TCP listener we'll accept connections on.
    let try_socket = TcpListener::bind(&addr).await;
    let listener = try_socket.expect("Failed to bind");
    println!("Listening on: {}", addr);

    while let Ok((stream, _)) = listener.accept().await {
        tokio::spawn(accept_connection(stream, room_shared.clone()));
    }

    Ok(())
}

async fn accept_connection(stream: TcpStream, rooms: Arc<tokio::sync::Mutex<RoomHashmap>>) {
    let addr = stream.peer_addr().expect("connected streams should have a peer address");
    println!("Peer address: {}", addr);


    let mut is_host: bool = false;    
    let mut room_name: Option<String> = None;

    let ws_stream = tokio_tungstenite::accept_hdr_async(stream,  |req: &Request, response: Response| {
        // accept a path like /room/<name>
        let path = req.uri().path();
        if !(path.starts_with("/host/") || path.starts_with("/join/") ) {
            // invalid path - reject the handshake with a 400 response
            let response = Response::builder()
                .status(400)
                .body(Some("Invalid path - can be either /host/<name> or /join/<name>".into())).unwrap();
            return Err(response);
        } 

        if path.starts_with("/host/") {
            is_host = true;
        } else {
            is_host = false;
        }
        room_name = path.strip_prefix(if is_host { "/host/" } else { "/join/" }).map(|s| s.to_string());

        // Ensure that room name is alphanumeric or hyphens/underscores
        if let Some(ref name) = room_name {
            if !name.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_') {
                let response = Response::builder()
                    .status(400)
                    .body(Some("Invalid room name - only alphanumeric, hyphens and underscores allowed".into())).unwrap();
                return Err(response);
            }
        } else {
            let response = Response::builder()
                .status(400)
                .body(Some("Room name missing".into())).unwrap();
            return Err(response);
        }

        return Ok(response);
    })
    .await;

    let room_name = room_name.expect("Room name should be set here");

    let mut ws_stream = match ws_stream {
        Ok(ws) => ws,
        Err(e) => {
            println!("Handshake rejected or failed: {}", e);
            return; // stop handling this connection, keep server running
        }
    };
    
    if is_host {
        if rooms.lock().await.contains_key(&room_name) {
            println!("Room already exists: {}", room_name);
            // Close stream with indicative error to the host
            ws_stream.close(Some(CloseFrame {
                code: CloseCode::Error,
                reason: "Room already exists".into(),
            })).await.expect("Failed to close connection");
            return;
        }
        
        rooms.lock().await.insert(room_name.clone(), ws_stream);
        println!("Room created, waiting for a client to join...");  
        return;
    }

    // Client flow
    let Some(host) = rooms.lock().await.remove(&room_name) else {
        println!("No such room: {}", room_name);
        // Close stream with indicative error to the client
        ws_stream.close(Some(CloseFrame {
            code: CloseCode::Error,
            reason: "No such room".into(),
        })).await.expect("Failed to close connection");
        return;
    };

    bend_pipe(host, ws_stream).await;
    println!("Room {} closed", room_name);
}

async fn bend_pipe(side1: WebSocketStream<TcpStream>, side2: WebSocketStream<TcpStream>) {
    let (side1_write, side1_read) = side1.split();
    let (side2_write, side2_read) = side2.split();

    let side1_to_side2 = side1_read.try_filter(|msg| future::ready(msg.is_text() || msg.is_binary()))
        .forward(side2_write);
    let side2_to_side1 = side2_read.try_filter(|msg| future::ready(msg.is_text() || msg.is_binary()))
        .forward(side1_write);

    future::select(side1_to_side2, side2_to_side1).await;
}