// server.rs — ServerState and server-level operations

use std::collections::HashMap;
use std::net::SocketAddr;
use std::time::Duration;
use crate::client::{Client, ClientState, PendingPacket};

const PENDING_TIMEOUT: Duration = Duration::from_secs(5);

pub struct ServerState {
    pub clients: HashMap<SocketAddr, Client>,
    pub pending: HashMap<u32, PendingPacket>, // seq → retransmit queue
}

impl ServerState {
    // Remove Pending clients that never completed authentication within the timeout.
    pub fn evict_stale_pending(&mut self) {
        self.clients.retain(|_, c| match &c.state {
            ClientState::Pending { created_at, .. } => {
                created_at.elapsed() < PENDING_TIMEOUT
            }
            ClientState::Authenticated { .. } => true,
        });
    }

    // Update position for an authenticated client.
    // Payload format: "x:100.0,y:200.0"
    pub fn update_position_player(&mut self, addr: SocketAddr, payload: &[u8]) {
        if let Some(client) = self.clients.get_mut(&addr) {
            if let Ok(text) = std::str::from_utf8(payload) {
                let parts: Vec<&str> = text.split(',').collect();
                if parts.len() == 2 {
                    let x_str = parts[0].trim().trim_start_matches("x:");
                    let y_str = parts[1].trim().trim_start_matches("y:");
                    if let (Ok(x), Ok(y)) = (x_str.parse::<f32>(), y_str.parse::<f32>()) {
                        client.x = x;
                        client.y = y;
                    }
                }
            }
        }
    }

    // Snapshot payload for authenticated clients only (text format for debugging).
    pub fn snapshot_payload(&self) -> Vec<u8> {
        let s: String = self.clients.values()
            .filter(|c| matches!(c.state, ClientState::Authenticated { .. }))
            .map(|c| format!("{},{:.1},{:.1}", c.id, c.x, c.y))
            .collect::<Vec<_>>()
            .join("|");
        s.into_bytes()
    }
}
