use std::collections::HashMap;
use std::net::SocketAddr;
use tokio::net::UdpSocket;
use uuid::Uuid;

// Flags
const FLAG_UNRELIABLE: u8 = 0b0000_0000;
const FLAG_RELIABLE: u8 = 0b0000_0001;
const FLAG_ORDERED: u8 = 0b0000_0011;

#[derive(Debug, Clone, Copy)]
enum PacketType {
    PlayerPosition      = 0,    // Unreliable - fire and forget
    GameEvent           = 1,    // reliable - must arrive
    ChatMessages        = 2,    // reliable - must arrive + ordered 
}


// Packet Header
struct Packet {
    packet_type:        PacketType,
    sequence_number:    u32,
    flags:              u8,
    payload: Vec<u8> 
}

impl Packet {

    // Build a packet out of raw bytes coming from the wired
    fn from_bytes(data: &[u8]) -> Option<Packet> {
        if data.len() < 6 {
            return None; // too small to be a valid packet
        }

        let packet_type = match data[0] {
            0 => PacketType::PlayerPosition,
            1 => PacketType::GameEvent,
            2 => PacketType::ChatMessages,
            _ => return None
        };

        let flags = match packet_type {
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


struct Client {
    id:         Uuid,
    addr:       SocketAddr, 
    last_seq:   u32,
}

#[tokio::main]
async fn main() {
    let socket = UdpSocket::bind("0.0.0.0:8080").await.unwrap();
    println!("Aegis-link server listening on port 8080"); 

    // Generate a Unique UID
    let mut clients: HashMap<SocketAddr, Client> = HashMap::new();
    let mut buf = [0u8; 1024];

    loop {
        let (len, addr) = socket.recv_from(&mut buf).await.unwrap();

        // Try to parse the packet
        let packet = match Packet::from_bytes(&buf[..len]) {
        Some(p) => p,
        None => {
            println!("Received malformed packet from {:?}, dropping it", addr);
            continue;
            }
        };

        // Register the new client
        if !clients.contains_key(&addr) {
            let id = Uuid::new_v4();
            println!("New Client Connected! Assigned ID: {}", id);
            clients.insert(addr, Client { id, addr, last_seq: 0 });
            socket
                .send_to(format!("Welcome! Your ID is {}", id).as_bytes(), addr).await.unwrap();
            continue;
        }

        let client = clients.get_mut(&&addr).unwrap();

        if packet.is_reliable() {
            let ack = format!("ACK: {}", packet.sequence_number); 
            socket.send_to(ack.as_bytes(), addr).await.unwrap();
            println!("Client {} sent reliable packet #{} - ACK sent", client.id, packet.sequence_number);
        } else {
            println!("Client {} sent unreliable packet #{} - No need for ACK", client.id, packet.sequence_number);
        }

        client.last_seq = packet.sequence_number;
        let msg = String::from_utf8_lossy(&buf[..len]);
        println!("Payload: {:?} {}", packet.sequence_number, msg);
    }

}
