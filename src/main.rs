use std::collections::HashMap;
use std::net::SocketAddr;
use tokio::net::UdpSocket;
use uuid::Uuid;

struct Client {
    id: Uuid,
    addr: SocketAddr
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
        let msg = String::from_utf8_lossy(&buf[..len]);

        if !clients.contains_key(&addr) {
            let id = Uuid::new_v4();
            clients.insert(addr, Client { id, addr });
            println!("New Client Connected! Assigned ID: {}", id);
            socket.send_to(format!("Welcome! Your ID is {}", id).as_bytes(), addr).await.unwrap();
        } else {
            let client = clients.get(&addr).unwrap();
            println!("Message from {}: {}", client.id, msg);
            socket.send_to(b"ACK", addr).await.unwrap();
        }
    }

}
