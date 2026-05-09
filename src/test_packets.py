import socket
import struct


def send_packet(sock, addr, packet_type, seq_num, payload, wait_reply=True):
    header = struct.pack(">BI", packet_type, seq_num)
    packet = header + payload.encode()
    sock.sendto(packet, addr)
    if wait_reply:
        response = sock.recvfrom(1024)
        print(f"server replied: {response[0].decode()}")
    else:
        print("unreliable packet sent, no ACK expected")

sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
sock.settimeout(2)

addr = ("127.0.0.1", 8080)

# First packet to register the client (PlayerPosition type=0, seq=1)
print("-- Connecting --")
send_packet(sock, addr, 0, 1, "join")

# Send a reliable GameEvent (type=1)
print("\n-- Sending GameEvent (reliable) --")
send_packet(sock, addr, 2, 42, "player_picked_up_key")

# Send an unreliable PlayerPosition (type=0)
print("\n-- Sending PlayerPosition (unreliable) --")
send_packet(sock, addr, 0, 43, "X:100,Y:200", wait_reply=False)