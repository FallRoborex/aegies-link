use std::collections::HashMap;
use std::hash::Hash;
use std::net::SocketAddr;
use std::os::linux::raw::stat;
use std::time::{Duration, Instant};
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::net::UdpSocket;
use uuid::Uuid;

// Flags
const FLAG_UNRELIABLE: u8 = 0b0000_0000;
const FLAG_RELIABLE: u8 = 0b0000_0001;
const FLAG_ORDERED: u8 = 0b0000_0011;

// Retransmission
const RETRY_INTERVAL_MS: u64 = 100; // Retry every 100 ms
const MAX_RETRIES: u32 = 5;         // Give up after five tries

#[derive(Debug, Clone, Copy)]
enum PacketType {
    Connection          = 0,    // Handshake - register clients
    PlayerPosition      = 1,    // Unreliable - fire and forget
    GameEvent           = 2,    // reliable - must arrive
    ChatMessages        = 3,    // reliable - must arrive + ordered 
}


// Packet Header
struct Packet {
    packet_type:        PacketType,
    sequence_number:    u32,
    flags:              u8,
    payload:            Vec<u8> 
}

impl Packet {

    // Build a packet out of raw bytes coming from the wired
    fn from_bytes(data: &[u8]) -> Option<Packet> {
        if data.len() < 5 {
            return None; // too small to be a valid packet
        }

        let packet_type = match data[0] {
            0 => PacketType::Connection,
            1 => PacketType::PlayerPosition,
            2 => PacketType::GameEvent,
            3 => PacketType::ChatMessages,
            _ => return None
        };

        let flags = match packet_type {
            PacketType::Connection => FLAG_UNRELIABLE,
            PacketType::PlayerPosition => FLAG_UNRELIABLE,
            PacketType::GameEvent => FLAG_RELIABLE,
            PacketType::ChatMessages => FLAG_ORDERED
        };

        // Sequence number is 4 bytes (byte 1-4), bid endian
        let sequence_number = u32::from_be_bytes([data[1], data[2], data[3], data[4]]);

        let payload = data[5..].to_vec();

        Some(Packet { packet_type, sequence_number, flags, payload })

    }

    // Turn a packet into raw bytes to send over the wire
    fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::new(); 
        bytes.push(self.packet_type as u8);
        bytes.extend_from_slice(&self.sequence_number.to_be_bytes());
        bytes.push(self.flags);
        bytes.extend_from_slice(&self.payload);
        bytes
    }

    fn is_reliable(&self) -> bool {
        self.flags & FLAG_RELIABLE != 0
    }
}

struct PendingPacket {
    data:       Vec<u8>,        // Raw bytes to send
    addr:       SocketAddr,     // who to send it
    sent_at:    Instant,        // when was the last tried
    retries:    u32,            // how many tries we've retried
    seq:        u32             // sequence number for matching ACKs
}


struct Client {
    id:         Uuid,
    addr:       SocketAddr, 
    last_seq:   u32,
}

struct ServerState {
    clients:    HashMap<SocketAddr, Client>,
    pending:    HashMap<u32, PendingPacket>,    // Sequence number -> Pending Message
}

#[tokio::main]
async fn main() {
    let socket = Arc::new(UdpSocket::bind("0.0.0.0:8080").await.unwrap());
    println!("Aegis-link server listening on port 8080"); 


    let state = Arc::new(Mutex::new(ServerState{
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
