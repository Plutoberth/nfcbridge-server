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
    use tokio_tungstenite::{tungstenite::{handshake::server::{ErrorResponse, Request, Response}, protocol::frame::{coding::CloseCode, CloseFrame}}, WebSocketStream};
    use url::Url;


struct Room {
    host: WebSocketStream<TcpStream>,
    password: String,
}

type RoomHashmap = HashMap<String, Room>;

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

    let mut state = CallbackState::default();
    let cb = create_connection_hdr_callback(&mut state);
    let ws_stream = tokio_tungstenite::accept_hdr_async(stream,  cb).await;
    
        let mut ws_stream = match ws_stream {
            Ok(ws) => ws,
            Err(e) => {
                println!("Handshake rejected or failed: {}", e);
                return; // stop handling this connection, keep server running
            }
        };

    let room_name = state.room_name.expect("Room name should be set here");
    if state.is_host {
        if rooms.lock().await.contains_key(&room_name) {
            println!("Room already exists: {}", room_name);
            // Close stream with indicative error to the host
            ws_stream.close(Some(CloseFrame {
                code: CloseCode::Error,
                reason: "Room already exists".into(),
            })).await.expect("Failed to close connection");
            return;
        }

        rooms.lock().await.insert(room_name.clone(), Room { host: ws_stream, password: state.password.unwrap() });
        println!("Room created, waiting for a client to join...");
        return;
    }


    let Some(host) = rooms.lock().await.remove(&room_name) else {
        println!("No such room: {}", room_name);
        // Close stream with indicative error to the client
        ws_stream.close(Some(CloseFrame {
            code: CloseCode::Error,
            reason: "No such room".into(),
        })).await.expect("Failed to close connection");
        return;
    };

    if host.password != state.password.unwrap() {
        println!("Invalid password for room: {}", room_name);
        // Close stream with indicative error to the client
        ws_stream.close(Some(CloseFrame {
            code: CloseCode::Error,
            reason: "Invalid password".into(),
        })).await.expect("Failed to close connection");
        // Reinsert the host back into the rooms list
        rooms.lock().await.insert(room_name.clone(), host);
        return;
    }

    bend_pipe(host.host, ws_stream).await;
    println!("Room {} closed", room_name);
}

#[derive(Default)]
struct CallbackState {
    is_host: bool,
    room_name: Option<String>,
    password: Option<String>,
}

fn create_connection_hdr_callback(state: &mut CallbackState) -> impl FnMut(&Request, Response) -> Result<Response, ErrorResponse> {
    move |req: &Request, response: Response| {
        // Hack to let url parse work
        let full_url = format!("http://ws.com{}", req.uri());
        let url_parsed = Url::parse(&full_url);
        let Ok(url_parsed) = url_parsed else {
            let response = Response::builder()
                .status(400)
                .body(Some("Invalid URL".into())).unwrap();
            println!("Invalid URL in request: {}", req.uri());
            return Err(response);
        };
        
        // Match "host" or "join" path with match statement
        match url_parsed.path() {
            "/host" => state.is_host = true,
            "/join" => state.is_host = false,
            _ => {
                let response = Response::builder()
                    .status(400)
                    .body(Some("Invalid path - can be either /host or /join".into())).unwrap();
                return Err(response);
            }
        }

        state.room_name = url_parsed.query_pairs().find(|(k, _)| k == "room").map(|(_, v)| v.to_string());

        if state.room_name.is_none() {
            let response = Response::builder()
                .status(400)
                .body(Some("Missing room name".into())).unwrap();
            return Err(response);
        }

        state.password = url_parsed.query_pairs().find(|(k, _)| k == "password").map(|(_, v)| v.to_string());

        if state.password.is_none() {
            let response = Response::builder()
                .status(400)
                .body(Some("Missing password".into())).unwrap();
            return Err(response);
        }

        // Ensure that room name and password are alphanumeric or hyphens/underscores
        fn validate_val(val: &str) -> bool {
            val.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_') && val.len() > 2
        }

        if !validate_val(state.password.as_ref().unwrap()) || !validate_val(state.room_name.as_ref().unwrap()) {
            let response = Response::builder()
                .status(400)
                .body(Some("Invalid room name or password - only alphanumeric, hyphens and underscores allowed, and must be longer than 2 characters".into())).unwrap();
            return Err(response);
        }

        return Ok(response);
    }
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