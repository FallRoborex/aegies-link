// packet.rs — owns PacketType enum, Packet struct, and all flag/retransmission constants

// Flags
pub const FLAG_UNRELIABLE: u8 = 0b0000_0000;
pub const FLAG_RELIABLE: u8 = 0b0000_0001;
pub const FLAG_ORDERED: u8 = 0b0000_0011;

// Retransmission
pub const RETRY_INTERVAL_MS: u64 = 100; // Retry every 100 ms
pub const MAX_RETRIES: u32 = 5;         // Give up after five tries

#[derive(Debug, Clone, Copy)]
pub enum PacketType {
    Connection          = 0,    // Handshake - register clients
    PlayerPosition      = 1,    // Unreliable - fire and forget
    GameEvent           = 2,    // reliable - must arrive
    ChatMessages        = 3,    // reliable - must arrive + ordered
    Snapshot            = 4,    // server -> all clients, world states
}

// Packet Header
pub struct Packet {
    pub packet_type:        PacketType,
    pub sequence_number:    u32,
    pub flags:              u8,
    pub payload:            Vec<u8>,
}

impl Packet {

    // Build a packet out of raw bytes coming from the wire
    pub fn from_bytes(data: &[u8]) -> Option<Packet> {
        if data.len() < 5 {
            return None; // too small to be a valid packet
        }

        let packet_type = match data[0] {
            0 => PacketType::Connection,
            1 => PacketType::PlayerPosition,
            2 => PacketType::GameEvent,
            3 => PacketType::ChatMessages,
            4 => PacketType::Snapshot,
            _ => return None
        };

        let flags = match packet_type {
            PacketType::Connection    => FLAG_UNRELIABLE,
            PacketType::PlayerPosition => FLAG_UNRELIABLE,
            PacketType::GameEvent     => FLAG_RELIABLE,
            PacketType::ChatMessages  => FLAG_ORDERED, 
            PacketType::Snapshot      => FLAG_UNRELIABLE,
        };

        // Sequence number is 4 bytes (byte 1-4), big endian
        let sequence_number = u32::from_be_bytes([data[1], data[2], data[3], data[4]]);

        let payload = data[5..].to_vec();

        Some(Packet { packet_type, sequence_number, flags, payload })
    }

    // Turn a packet into raw bytes to send over the wire
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.push(self.packet_type as u8);
        bytes.extend_from_slice(&self.sequence_number.to_be_bytes());
        bytes.push(self.flags);
        bytes.extend_from_slice(&self.payload);
        bytes
    }

    pub fn is_reliable(&self) -> bool {
        self.flags & FLAG_RELIABLE != 0
    }
}
