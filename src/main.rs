
use tokio::net::UdpSocket;

#[tokio::main]
async fn main() {
    let socket = UdpSocket::bind("0.0.0.0:7878").await.unwrap();
    println!("Aegis-link server listening on port 7878"); 

    let mut buf = [0u8; 1024];

    loop {
        let (len, addr) = socket.recv_from(&mut buf).await.unwrap();
        let msg = String::from_utf8_lossy(&buf[..len]);
        println!("Reveived from {}: {}", addr, msg);

        socket.send_to(b"ACK", addr).await.unwrap();
    }

}
