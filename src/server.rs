// server.rs — owns ServerState struct and server-level shared state

use std::collections::HashMap;
use std::net::SocketAddr;
use crate::client::{Client, PendingPacket};

pub struct ServerState {
    pub clients:    HashMap<SocketAddr, Client>,
    pub pending:    HashMap<u32, PendingPacket>,    // Sequence number -> Pending Message
}
impl ServerState {
    pub fn update_position_player(&mut self, addr: SocketAddr, payload: &[u8]) {
        // Payload format: "x:100.0, y:100.0"
        
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

    pub fn snapshot_payload(&self) -> Vec<u8> {
        let s: String = self.clients.values()
            .map(|c| format!("{},{:.1},{:.1}", c.id, c.x, c.y))
            .collect::<Vec<_>>()
            .join("|");
        s.into_bytes()
    }
}
