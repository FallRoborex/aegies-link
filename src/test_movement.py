import socket
import struct
import uuid
import time

def send_packet(sock, addr, packet_type, seq_num, payload=b""):
    if isinstance(payload, str):
        payload = payload.encode()
    sock.sendto(struct.pack(">BI", packet_type, seq_num) + payload, addr)

def parse_snapshot(data):
    if len(data) < 15 or data[0] != 4:
        return None
    tick    = struct.unpack(">Q", data[6:14])[0]
    count   = data[14]
    players = []
    offset  = 15
    for _ in range(count):
        if offset + 24 > len(data):
            break
        uid = uuid.UUID(bytes=data[offset:offset+16])
        x   = struct.unpack(">f", data[offset+16:offset+20])[0]
        y   = struct.unpack(">f", data[offset+20:offset+24])[0]
        players.append((str(uid)[:8], x, y))
        offset += 24
    return tick, players

def drain(sock):
    """Consume all buffered packets and return the highest snapshot tick seen."""
    sock.settimeout(0)
    last_tick = 0
    while True:
        try:
            data, _ = sock.recvfrom(2048)
            result = parse_snapshot(data)
            if result and result[0] > last_tick:
                last_tick = result[0]
        except (BlockingIOError, socket.timeout):
            break
    sock.settimeout(0.5)
    return last_tick

def recv_snapshot_after(sock, after_tick):
    """Read snapshots until we get one with tick > after_tick."""
    deadline = time.time() + 1.0
    while time.time() < deadline:
        try:
            data, _ = sock.recvfrom(2048)
        except socket.timeout:
            continue
        result = parse_snapshot(data)
        if result and result[0] > after_tick:
            return result
    return None, []

sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
sock.settimeout(0.5)
addr = ("127.0.0.1", 8080)

print("-- Connecting --")
send_packet(sock, addr, 0, 0)

try:
    data, _ = sock.recvfrom(1024)
    if len(data) >= 6 and data[0] == 2:
        seq = struct.unpack(">I", data[1:5])[0]
        msg = data[6:].decode(errors="replace")
        print(f"Welcome: {msg}")
        sock.sendto(f"ACK:{seq}".encode(), addr)
except socket.timeout:
    print("No welcome received — is the server running?")
    exit(1)

time.sleep(0.1)

positions = [
    (10.0,  0.0),
    (20.0, 10.0),
    (30.0, 20.0),
    (40.0, 30.0),
    (50.0, 40.0),
]

for i, (x, y) in enumerate(positions):
    seq = 100 + i

    # Drain stale snapshots and record the current tick ceiling
    baseline = drain(sock)

    print(f"\n-- Moving to ({x:.1f}, {y:.1f})  seq={seq}  baseline_tick={baseline} --")
    send_packet(sock, addr, 1, seq, f"x:{x},y:{y}")

    # Wait 2 game-loop ticks (32 ms) so the server processes + emits a fresh snapshot
    time.sleep(0.035)

    tick, players = recv_snapshot_after(sock, after_tick=baseline)
    if tick is not None:
        print(f"  Snapshot tick={tick}  players={len(players)}")
        for uid, px, py in players:
            print(f"    [{uid}...]  x={px:.1f}  y={py:.1f}")
    else:
        print("  No snapshot received")

print("\nDone.")
sock.close()
