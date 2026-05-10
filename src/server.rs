// server.rs — owns ServerState struct and server-level shared state

use std::collections::HashMap;
use std::net::SocketAddr;
use crate::client::{Client, PendingPacket};

pub struct ServerState {
    pub clients:    HashMap<SocketAddr, Client>,
    pub pending:    HashMap<u32, PendingPacket>,    // Sequence number -> Pending Message
}
