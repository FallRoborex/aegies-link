// client.rs — Client struct, ClientState lifecycle, and PendingPacket

use std::net::SocketAddr;
use std::time::Instant;
use uuid::Uuid;
use crate::rate_limiter::RateLimiter;

pub struct PendingPacket {
    pub data:    Vec<u8>,
    pub addr:    SocketAddr,
    pub sent_at: Instant,
    pub retries: u32,
    pub seq:     u32,
}

// Tracks where a client is in the authentication lifecycle.
pub enum ClientState {
    // Connection received; challenge sent; waiting for AuthResponse.
    Pending {
        nonce:      [u8; 16],
        created_at: Instant,
    },
    // AuthResponse verified; client is fully registered.
    Authenticated {
        session_key: [u8; 32],
    },
}

pub struct Client {
    pub id:       Uuid,
    pub addr:     SocketAddr,
    pub last_seq: u32,
    pub x:        f32,
    pub y:        f32,
    pub state:    ClientState,
    pub rate:     RateLimiter,
}
