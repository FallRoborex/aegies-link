import socket
import struct
import uuid
import hmac
import hashlib
import time

SHARED_SECRET = b"aegis-dev-secret"
SERVER = ("127.0.0.1", 8080)

# ── helpers ───────────────────────────────────────────────────────────────────

def _raw_packet(packet_type, seq_num, payload=b""):
    if isinstance(payload, str):
        payload = payload.encode()
    return struct.pack(">BI", packet_type, seq_num) + payload

def send_packet(sock, packet_type, seq_num, payload=b""):
    sock.sendto(_raw_packet(packet_type, seq_num, payload), SERVER)

def send_signed(sock, session_key, packet_type, seq_num, payload=b""):
    pkt = _raw_packet(packet_type, seq_num, payload)
    tag = hmac.new(session_key, pkt, hashlib.sha256).digest()
    sock.sendto(pkt + tag, SERVER)

def connect_and_auth(sock):
    """3-step handshake → returns (session_key, welcome_message)."""
    send_packet(sock, 0, 0)

    data, _ = sock.recvfrom(1024)
    assert data[0] == 5, f"Expected AuthChallenge (5), got {data[0]}"
    nonce = data[6:]

    session_key = hmac.new(SHARED_SECRET, nonce, hashlib.sha256).digest()
    send_packet(sock, 6, 0, session_key)

    data, _ = sock.recvfrom(1024)
    assert data[0] == 2, f"Expected Welcome (2), got {data[0]}"
    seq = struct.unpack(">I", data[1:5])[0]
    msg = data[6:].decode(errors="replace")
    sock.sendto(f"ACK:{seq}".encode(), SERVER)
    return session_key, msg

# ── snapshot helpers ──────────────────────────────────────────────────────────

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
    sock.settimeout(0)
    last_tick = 0
    while True:
        try:
            data, _ = sock.recvfrom(2048)
            r = parse_snapshot(data)
            if r and r[0] > last_tick:
                last_tick = r[0]
        except (BlockingIOError, socket.timeout):
            break
    sock.settimeout(0.5)
    return last_tick

def recv_snapshot_after(sock, after_tick):
    deadline = time.time() + 1.0
    while time.time() < deadline:
        try:
            data, _ = sock.recvfrom(2048)
        except socket.timeout:
            continue
        r = parse_snapshot(data)
        if r and r[0] > after_tick:
            return r
    return None, []

# ── test ──────────────────────────────────────────────────────────────────────

sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
sock.settimeout(2)

print("-- Connecting & authenticating --")
session_key, welcome = connect_and_auth(sock)
print(f"  {welcome}")

time.sleep(0.1)

positions = [(10.0, 0.0), (20.0, 10.0), (30.0, 20.0), (40.0, 30.0), (50.0, 40.0)]
last_tick = 0

for i, (x, y) in enumerate(positions):
    baseline = drain(sock)
    print(f"\n-- Moving to ({x:.1f}, {y:.1f}) --")
    send_signed(sock, session_key, 1, 100 + i, f"x:{x},y:{y}")
    time.sleep(0.035)

    tick, players = recv_snapshot_after(sock, after_tick=baseline)
    if tick:
        last_tick = tick
        print(f"  Snapshot tick={tick}  players={len(players)}")
        for uid, px, py in players:
            print(f"    [{uid}...]  x={px:.1f}  y={py:.1f}")
    else:
        print("  No snapshot received")
    time.sleep(0.05)

print("\nDone.")
sock.close()
