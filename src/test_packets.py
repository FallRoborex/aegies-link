import socket
import struct
import time


sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
sock.settimeout(5)
addr = ("127.0.0.1", 8080)

# Send connect packet
header = struct.pack(">BI", 0, 0)
sock.sendto(header, addr)
print("Connected - NOT sending any ACK, watching for retries...")

# Just listen and print everything that arrives
for i in range(20):
    try:
        data, _ = sock.recvfrom(1024)
        if len(data) >= 6:
            seq = struct.unpack(">I", data[1:5])[0]
            msg = data[6:].decode()
            print(f"Received packet seq #{seq}: {msg}")
        else:
            print(f"Received: {data.decode()}")
    except socket.timeout:
        print("Nothing arrived")
        break


def send_packet(sock, addr, packet_type, seq_num, payload=b"", expect_response=True):
    if isinstance(payload, str):
        payload = payload.encode()
    header = struct.pack(">BI", packet_type, seq_num)
    packet = header + payload
    sock.sendto(packet, addr)

    if expect_response:
        try:
            response, _ = sock.recvfrom(1024)
            # Check if it's a plain ACK text
            try:
                text = response.decode()
                if text.startswith("ACK:"):
                    seq = text[4:].strip()
                    print(f"Server ACK'd packet #{seq}")
                    return
            except:
                pass

            # It's a structured packet - read sequence from header
            if len(response) >= 6:
                resp_seq = struct.unpack(">I", response[1:5])[0]
                msg = response[6:].decode()
                print(f"Server replied (seq #{resp_seq}): {msg}")
                sock.sendto(f"ACK:{resp_seq}".encode(), addr)
                print(f"Sent ACK:{resp_seq}")
        except socket.timeout:
            print("No response (timed out)")
    else:
        print("No response expected (unreliable)")

sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
sock.settimeout(2)
addr = ("127.0.0.1", 8080)

# Connect
print("-- Connecting --")
send_packet(sock, addr, 0, 0)

time.sleep(0.5)


# Send a reliable GameEvent but DON'T ACK it
print("\n-- Sending GameEvent (reliable) - NOT sending ACK --")
header = struct.pack(">BI", 2, 99)
packet = header + b"important_event"
sock.sendto(packet, addr)

# Just wait and watch the server retry
time.sleep(3)
print("Done waiting - check server output")



# Reliable GameEvent
print("\n-- Sending GameEvent (reliable) --")
send_packet(sock, addr, 2, 42, "player_picked_up_key", expect_response=True)

# Send ACK back
sock.sendto(b"ACK:42", addr)
print("Sent ACK:42")

time.sleep(0.5)

# Unreliable PlayerPosition
print("\n-- Sending PlayerPosition (unreliable) --")
send_packet(sock, addr, 1, 43, "x:100,y:200", expect_response=False)