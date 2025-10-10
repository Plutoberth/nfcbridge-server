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

use futures_util::{future, SinkExt, StreamExt, TryStreamExt};
use tokio::net::{TcpListener, TcpStream};
    use tokio_tungstenite::{tungstenite::{handshake::server::{ErrorResponse, Request, Response}}, WebSocketStream};
    use url::Url;


struct Room {
    host: WebSocketStream<TcpStream>,
    password: String,
}

type RoomHashmap = HashMap<String, Room>;

#[tokio::main]
async fn main() -> Result<(), Error> {
    let rooms: RoomHashmap = HashMap::new();
    let room_shared = std::sync::Arc::new(std::sync::Mutex::new(rooms));

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

async fn accept_connection(stream: TcpStream, rooms: Arc<std::sync::Mutex<RoomHashmap>>) {
    let addr = stream.peer_addr().expect("connected streams should have a peer address");
    println!("Peer address: {}", addr);

    let mut state = CallbackState { client: None };
    let cb = create_connection_hdr_callback(&mut state, &rooms);
    let ws_stream = tokio_tungstenite::accept_hdr_async(stream,  cb).await;
    
    let ws_stream = match ws_stream {
        Ok(ws) => ws,
        Err(e) => {
            println!("Handshake rejected or failed: {}", e);
            return; // stop handling this connection, keep server running
        }
    };

    match state.client {
        Some(Client::Host(host)) => {
            println!("Room hosted, waiting for a client to join...");
            rooms.lock().unwrap().insert(host.room_name, Room { host: ws_stream, password: host.password });
            return; // stop handling this connection, keep server running
        },
        Some(Client::Client(mut client)) => {
            println!("Creating room {}", client.room_name);
            // Flush host
            client.room.host.flush().await.ok();
            bend_pipe(client.room.host, ws_stream).await;
            println!("Room {} closed", client.room_name);
            return; // stop handling this connection, keep server running
        },
        None => {
            println!("No client state after handshake - this should not happen");
            return; // stop handling this connection, keep server running
        }
    }

}

struct RoomHost {
    room_name: String,
    password: String,
}

struct RoomClient {
    room_name: String,
    room: Room,
}

enum Client {
    Host(RoomHost),
    Client(RoomClient),
}

struct CallbackState {
    client: Option<Client>,
}

fn create_connection_hdr_callback(state: &mut CallbackState, rooms: &Arc<std::sync::Mutex<RoomHashmap>>) -> impl FnMut(&Request, Response) -> Result<Response, ErrorResponse> {
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
        
        let is_host = match url_parsed.path() {
            "/host" => true,
            "/join" => false,
            _ => {
                let response = Response::builder()
                    .status(400)
                    .body(Some("Invalid path - can be either /host or /join".into())).unwrap();
                return Err(response);
            }
        };

        let room_name = url_parsed.query_pairs().find(|(k, _)| k == "room").map(|(_, v)| v.to_string());
        let Some(room_name) = room_name else {
            let response = Response::builder()
                .status(400)
                .body(Some("Missing room name".into())).unwrap();
            return Err(response);
        };

        let password = url_parsed.query_pairs().find(|(k, _)| k == "password").map(|(_, v)| v.to_string());
        let Some(password) = password else {
            let response = Response::builder()
                .status(400)
                .body(Some("Missing password".into())).unwrap();
            return Err(response);
        };

        // Ensure that room name and password are alphanumeric or hyphens/underscores
        fn validate_val(val: &str) -> bool {
            val.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_') && val.len() > 0
        }

        if !validate_val(&password) || !validate_val(&room_name) {
            let response = Response::builder()
                .status(400)
                .body(Some("Invalid room name or password - only alphanumeric, hyphens and underscores allowed".into())).unwrap();
            return Err(response);
        }


        if is_host {
            let rooms = rooms.lock().unwrap();
            if rooms.contains_key(&room_name) {
                println!("Room already exists: {}", room_name);
                // Close stream with indicative error to the host
                
                let response = Response::builder()
                    .status(400)
                    .body(Some("Room already exists".into())).unwrap();
                return Err(response);
            }
            state.client = Some(Client::Host(RoomHost { room_name: room_name.clone(), password: password.clone() }));
            println!("Room created, waiting for a client to join...");
        } else {
            let Some(host) = rooms.lock().unwrap().remove(&room_name) else {
                let response = Response::builder()
                    .status(400)
                    .body(Some("No such room".into())).unwrap();
                return Err(response);
            };

            if host.password != password {
                let response = Response::builder()
                    .status(400)
                    .body(Some("Invalid password".into())).unwrap();
                // Reinsert the host back into the rooms list
                rooms.lock().unwrap().insert(room_name.clone(), host);
                return Err(response);
            }
            state.client = Some(Client::Client(RoomClient { room_name: room_name.clone(), room: host }));
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