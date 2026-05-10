// main.rs — startup, game-loop, retry task, and the authenticated receive loop

mod auth;
mod client;
mod packet;
mod rate_limiter;
mod server;

use std::collections::HashMap;
use std::net::SocketAddr;
use std::time::{Duration, Instant};
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::net::UdpSocket;
use uuid::Uuid;

use auth::HMAC_LEN;
use client::{Client, ClientState, PendingPacket};
use packet::{Packet, PacketType, FLAG_RELIABLE, FLAG_UNRELIABLE, RETRY_INTERVAL_MS, MAX_RETRIES};
use rate_limiter::{RateLimiter, POSITION_RATE_LIMIT, MAX_STRIKES};
use server::ServerState;

// Shared server secret — set AEGIS_SECRET in the environment for production.
fn shared_secret() -> Vec<u8> {
    std::env::var("AEGIS_SECRET")
        .unwrap_or_else(|_| "aegis-dev-secret".to_string())
        .into_bytes()
}

// Local enum used to copy auth state out of the HashMap before borrowing mutably.
enum AuthStatus {
    Unknown,
    Pending([u8; 16]),       // nonce
    Authenticated([u8; 32]), // session_key
}

#[tokio::main]
async fn main() {
    let socket = Arc::new(UdpSocket::bind("0.0.0.0:8080").await.unwrap());
    println!("Aegis-link server listening on port 8080");

    let state = Arc::new(Mutex::new(ServerState {
        clients: HashMap::new(),
        pending: HashMap::new(),
    }));

    // --- Retransmit task ---
    let retry_socket = Arc::clone(&socket);
    let retry_state  = Arc::clone(&state);
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_millis(50)).await;
            let mut state    = retry_state.lock().await;
            let mut to_remove = Vec::new();

            for (seq, pending) in state.pending.iter_mut() {
                if pending.sent_at.elapsed().as_millis() as u64 >= RETRY_INTERVAL_MS {
                    if pending.retries >= MAX_RETRIES {
                        println!("Packet #{seq} to {} gave up", pending.addr);
                        to_remove.push(*seq);
                    } else {
                        pending.retries  += 1;
                        pending.sent_at   = Instant::now();
                        let _ = retry_socket.send_to(&pending.data, pending.addr).await;
                    }
                }
            }
            for seq in to_remove { state.pending.remove(&seq); }

            // Evict Pending clients that never completed auth
            state.evict_stale_pending();
        }
    });

    // --- 60 fps game-loop: broadcast world snapshot to authenticated clients ---
    let game_socket = Arc::clone(&socket);
    let game_state  = Arc::clone(&state);
    tokio::spawn(async move {
        let mut tick: u64 = 0;
        loop {
            tokio::time::sleep(Duration::from_millis(16)).await;
            tick += 1;

            let state = game_state.lock().await;

            // Build snapshot body: tick(8) + count(1) + per-player[uuid(16)+x(4)+y(4)]
            let mut body = Vec::new();
            body.extend_from_slice(&tick.to_be_bytes());

            let auth_clients: Vec<_> = state.clients.values()
                .filter(|c| matches!(c.state, ClientState::Authenticated { .. }))
                .collect();

            body.push(auth_clients.len() as u8);
            for c in &auth_clients {
                body.extend_from_slice(c.id.as_bytes());
                body.extend_from_slice(&c.x.to_be_bytes());
                body.extend_from_slice(&c.y.to_be_bytes());
            }

            // Wrap in a Packet header and send to every authenticated client
            let mut pkt = Vec::new();
            pkt.push(PacketType::Snapshot as u8);
            pkt.extend_from_slice(&(tick as u32).to_be_bytes());
            pkt.push(FLAG_UNRELIABLE);
            pkt.extend_from_slice(&body);

            for c in &auth_clients {
                let _ = game_socket.send_to(&pkt, c.addr).await;
            }

            if tick % 60 == 0 {
                println!("Tick {tick} | Players online {}", auth_clients.len());
            }
        }
    });

    // --- Main receive loop ---
    let secret = shared_secret();
    let mut buf = [0u8; 1024];

    loop {
        let (len, addr) = socket.recv_from(&mut buf).await.unwrap();
        let raw = &buf[..len];

        // Plain-text ACK messages ("ACK:<seq>") bypass packet parsing
        if let Ok(text) = std::str::from_utf8(raw) {
            if text.starts_with("ACK:") {
                if let Ok(seq) = text[4..].trim().parse::<u32>() {
                    let mut state = state.lock().await;
                    if state.pending.remove(&seq).is_some() {
                        println!("ACK #{seq} from {addr}");
                    }
                }
                continue;
            }
        }

        // Snapshot the client's auth status before taking a mutable borrow
        let auth_status = {
            let state = state.lock().await;
            match state.clients.get(&addr) {
                None => AuthStatus::Unknown,
                Some(c) => match &c.state {
                    ClientState::Pending { nonce, .. }       => AuthStatus::Pending(*nonce),
                    ClientState::Authenticated { session_key } => AuthStatus::Authenticated(*session_key),
                },
            }
        };

        match auth_status {
            // ── Unknown client: only accept Connection ──────────────────────
            AuthStatus::Unknown => {
                let packet = match Packet::from_bytes(raw) {
                    Some(p) if matches!(p.packet_type, PacketType::Connection) => p,
                    _ => {
                        println!("Dropping non-Connection packet from unknown {addr}");
                        continue;
                    }
                };
                let _ = packet; // Connection payload ignored for now

                let nonce = auth::generate_nonce();
                let id    = Uuid::new_v4();

                let challenge = Packet {
                    packet_type:     PacketType::AuthChallenge,
                    sequence_number: 0,
                    flags:           FLAG_UNRELIABLE,
                    payload:         nonce.to_vec(),
                };
                socket.send_to(&challenge.to_bytes(), addr).await.unwrap();

                let mut state = state.lock().await;
                state.clients.insert(addr, Client {
                    id,
                    addr,
                    last_seq: 0,
                    x:        0.0,
                    y:        0.0,
                    state:    ClientState::Pending { nonce, created_at: Instant::now() },
                    rate:     RateLimiter::new(POSITION_RATE_LIMIT),
                });
                println!("AuthChallenge → {addr}");
            }

            // ── Pending client: verify AuthResponse ─────────────────────────
            AuthStatus::Pending(nonce) => {
                let packet = match Packet::from_bytes(raw) {
                    Some(p) if matches!(p.packet_type, PacketType::AuthResponse) => p,
                    // Allow the client to retry by re-sending Connection
                    Some(p) if matches!(p.packet_type, PacketType::Connection) => {
                        let challenge = Packet {
                            packet_type:     PacketType::AuthChallenge,
                            sequence_number: 0,
                            flags:           FLAG_UNRELIABLE,
                            payload:         nonce.to_vec(),
                        };
                        socket.send_to(&challenge.to_bytes(), addr).await.unwrap();
                        continue;
                    }
                    _ => {
                        println!("Expected AuthResponse from {addr}, dropping");
                        continue;
                    }
                };

                let expected_key = auth::derive_session_key(&secret, &nonce);

                if packet.payload.len() != HMAC_LEN || packet.payload.as_slice() != expected_key {
                    println!("Auth failed for {addr} — wrong response, evicting");
                    state.lock().await.clients.remove(&addr);
                    continue;
                }

                let session_key = expected_key;
                let mut state   = state.lock().await;

                if let Some(client) = state.clients.get_mut(&addr) {
                    client.state = ClientState::Authenticated { session_key };
                }

                let id = state.clients[&addr].id;
                println!("Auth OK  {addr}  id={id}");

                let welcome = Packet {
                    packet_type:     PacketType::GameEvent,
                    sequence_number: 1,
                    flags:           FLAG_RELIABLE,
                    payload:         format!("Welcome! Your ID is {id}").into_bytes(),
                };
                let welcome_bytes = welcome.to_bytes();
                socket.send_to(&welcome_bytes, addr).await.unwrap();
                state.pending.insert(1, PendingPacket {
                    data:    welcome_bytes,
                    addr,
                    sent_at: Instant::now(),
                    retries: 0,
                    seq:     1,
                });
            }

            // ── Authenticated client: verify HMAC → rate-limit → dispatch ───
            AuthStatus::Authenticated(session_key) => {
                // 1. Verify and strip HMAC tag
                let packet_bytes = match auth::verify_and_strip(&session_key, raw) {
                    Some(b) => b,
                    None => {
                        println!("HMAC fail from {addr}, dropping");
                        continue;
                    }
                };

                // 2. Rate-limit check
                {
                    let mut state = state.lock().await;
                    if let Some(client) = state.clients.get_mut(&addr) {
                        if !client.rate.allow() {
                            println!("Rate limit exceeded for {addr} (strikes: {})", client.rate.strikes);
                            if client.rate.strikes >= MAX_STRIKES {
                                state.clients.remove(&addr);
                                println!("Kicked {addr} for flooding");
                            }
                            continue;
                        }
                    }
                }

                // 3. Parse
                let packet = match Packet::from_bytes(&packet_bytes) {
                    Some(p) => p,
                    None => {
                        println!("Malformed packet from {addr}, dropping");
                        continue;
                    }
                };

                let mut state = state.lock().await;

                // 4. ACK reliable packets
                if packet.is_reliable() {
                    let ack = format!("ACK: {}", packet.sequence_number);
                    socket.send_to(ack.as_bytes(), addr).await.unwrap();
                }

                // 5. Update last_seq
                if let Some(client) = state.clients.get_mut(&addr) {
                    client.last_seq = packet.sequence_number;
                }

                // 6. Dispatch
                match packet.packet_type {
                    PacketType::PlayerPosition => {
                        state.update_position_player(addr, &packet.payload);
                    }
                    PacketType::GameEvent | PacketType::ChatMessages => {
                        let msg = String::from_utf8_lossy(&packet.payload);
                        println!("[{addr}] event: {msg}");
                    }
                    _ => {}
                }
            }
        }
    }
}
