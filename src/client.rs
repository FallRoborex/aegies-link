// client.rs — owns Client struct and PendingPacket struct

use std::net::SocketAddr;
use std::time::Instant;
use uuid::Uuid;

pub struct PendingPacket {
    pub data:       Vec<u8>,        // Raw bytes to send
    pub addr:       SocketAddr,     // who to send it
    pub sent_at:    Instant,        // when was the last tried
    pub retries:    u32,            // how many tries we've retried
    pub seq:        u32,            // sequence number for matching ACKs
}

pub struct Client {
    pub id:         Uuid,
    pub addr:       SocketAddr,
    pub last_seq:   u32,
}
