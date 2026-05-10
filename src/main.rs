// main.rs — owns startup, the main receive loop, and wires all modules together

mod packet;
mod client;
mod server;

use std::collections::HashMap;
use std::net::SocketAddr;
use std::time::{Duration, Instant};
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::net::UdpSocket;
use uuid::Uuid;

use packet::{Packet, PacketType, FLAG_RELIABLE, RETRY_INTERVAL_MS, MAX_RETRIES};
use client::{Client, PendingPacket};
use server::ServerState;

#[tokio::main]
async fn main() {
    let socket = Arc::new(UdpSocket::bind("0.0.0.0:8080").await.unwrap());
    println!("Aegis-link server listening on port 8080");

    let state = Arc::new(Mutex::new(ServerState {
        clients: HashMap::new(),
        pending: HashMap::new()
    }));

    let retry_socket = Arc::clone(&socket);
    let retry_state = Arc::clone(&state);

    tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_millis(50)).await;

            let mut state = retry_state.lock().await;
            let mut to_remove = Vec::new();

            for (seq, pending) in state.pending.iter_mut() {
                let elapsed = pending.sent_at.elapsed().as_millis() as u64;

                if elapsed >= RETRY_INTERVAL_MS {
                    if pending.retries >= MAX_RETRIES {
                        println!("Packet #{} to {} gave up after {} retries",
                            seq, pending.addr, MAX_RETRIES);
                        to_remove.push(*seq);
                    } else {
                        pending.retries += 1;
                        pending.sent_at = Instant::now();
                        println!("Retrying packet #{} to {} (attempt {})",
                            seq, pending.addr, pending.retries);
                        let _ = retry_socket.send_to(&pending.data, pending.addr).await;
                    }
                }
            }

            for seq in to_remove {
                state.pending.remove(&seq);
            }
        }
    });

    // Generate a Unique UID
    let mut clients: HashMap<SocketAddr, Client> = HashMap::new();
    let mut buf = [0u8; 1024];

    loop {
        let (len, addr) = socket.recv_from(&mut buf).await.unwrap();

        // Checks if this ACK message (text like "ACK:42")
        let raw = &buf[..len];
        if let Ok(text) = std::str::from_utf8(raw) {
            if text.starts_with("ACK:") {
                if let Ok(seq) = text[4..].trim().parse::<u32>() {
                    let mut state = state.lock().await;
                    if state.pending.remove(&seq).is_some() {
                        println!("ACK received for packet = #{} - removed from the waiting room", seq);
                    }
                }
                continue;
            }
        }

        // Try to parse the packet
        let packet = match Packet::from_bytes(&buf[..len]) {
            Some(p) => p,
            None => {
                println!("Received malformed packet from {:?}, dropping it", addr);
                continue;
            }
        };

        let mut state = state.lock().await;

        // Handles connect packet before anything else
        if matches!(packet.packet_type, PacketType::Connection) {
            // Register the new client
            if !state.clients.contains_key(&addr) {
                let id = Uuid::new_v4();
                println!("New Client Connected! Assigned ID: {}", id);
                state.clients.insert(addr, Client { id, addr, last_seq: 0 });

                // Send welcome as a reliable packet
                let welcome = Packet {
                    packet_type:        PacketType::GameEvent,
                    sequence_number:    1,
                    flags:              FLAG_RELIABLE,
                    payload:            format!("Welcome! Your ID is {}", id).into_bytes(),
                };

                let welcome_bytes = welcome.to_bytes();
                socket.send_to(&welcome_bytes, addr).await.unwrap();

                // Put it in the waiting room
                state.pending.insert(1, PendingPacket {
                    data:       welcome_bytes,
                    addr,
                    sent_at:    Instant::now(),
                    retries:    0,
                    seq:        1
                });
                continue;
            }
        }

        if packet.is_reliable() {
            let ack = format!("ACK: {}", packet.sequence_number);
            socket.send_to(ack.as_bytes(), addr).await.unwrap();
            println!("Reliable packet #{} received - ACK sent", packet.sequence_number);
        } else {
            println!("Unreliable packet #{} received - no ACK needed", packet.sequence_number);
        }

        if let Some(client) = state.clients.get_mut(&addr) {
            client.last_seq = packet.sequence_number;
        }

        let msg = String::from_utf8_lossy(&packet.payload);
        println!("Payload: {:?} {}", packet.sequence_number, msg);
    }
}
