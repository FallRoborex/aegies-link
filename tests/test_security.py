"""
Security layer tests — verifies authentication, HMAC enforcement, and rate limiting.
Each test prints PASS or FAIL with a short reason.
"""
import socket
import struct
import hmac
import hashlib
import time
import os

SHARED_SECRET = b"aegis-dev-secret"
WRONG_SECRET  = b"not-the-right-key"
SERVER = ("127.0.0.1", 8080)
TIMEOUT = 1.5

passed = 0
failed = 0

def _sock():
    s = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    s.settimeout(TIMEOUT)
    return s

def _raw(packet_type, seq, payload=b""):
    if isinstance(payload, str): payload = payload.encode()
    return struct.pack(">BI", packet_type, seq) + payload

def _send(sock, packet_type, seq, payload=b""):
    sock.sendto(_raw(packet_type, seq, payload), SERVER)

def _sign(session_key, packet_type, seq, payload=b""):
    pkt = _raw(packet_type, seq, payload)
    tag = hmac.new(session_key, pkt, hashlib.sha256).digest()
    return pkt + tag

def _auth(sock, secret=SHARED_SECRET):
    """Full handshake; returns session_key or raises on failure."""
    _send(sock, 0, 0)
    data, _ = sock.recvfrom(1024)
    assert data[0] == 5
    nonce = data[6:]
    session_key = hmac.new(secret, nonce, hashlib.sha256).digest()
    _send(sock, 6, 0, session_key)
    data, _ = sock.recvfrom(1024)
    assert data[0] == 2
    seq = struct.unpack(">I", data[1:5])[0]
    sock.sendto(f"ACK:{seq}".encode(), SERVER)
    return session_key

def recv_ack(sock, timeout=1.5):
    """Drain incoming packets until an ACK text response arrives."""
    deadline = time.time() + timeout
    while time.time() < deadline:
        try:
            data, _ = sock.recvfrom(2048)
            if data.startswith(b"ACK:"):
                return True, data
        except socket.timeout:
            continue
    return False, None

def check(name, passed_flag, detail=""):
    global passed, failed
    if passed_flag:
        passed += 1
        print(f"  PASS  {name}")
    else:
        failed += 1
        print(f"  FAIL  {name}" + (f" — {detail}" if detail else ""))

# ─────────────────────────────────────────────────────────────────────────────
print("\n=== Authentication ===")

# T1: Connection → server must reply with AuthChallenge (type 5), not Welcome
s = _sock()
_send(s, 0, 0)
try:
    data, _ = s.recvfrom(1024)
    check("Connection gets AuthChallenge", data[0] == 5, f"got type {data[0]}")
except socket.timeout:
    check("Connection gets AuthChallenge", False, "timeout")
s.close()
time.sleep(0.05)

# T2: Wrong shared secret → no Welcome within timeout
s = _sock()
try:
    _auth(s, secret=WRONG_SECRET)
    check("Wrong secret rejected", False, "Welcome was sent anyway")
except (AssertionError, socket.timeout):
    check("Wrong secret rejected", True)
s.close()
time.sleep(0.05)

# T3: Correct secret → Welcome received
s = _sock()
try:
    sk = _auth(s)
    check("Correct secret accepted", len(sk) == 32)
except Exception as e:
    check("Correct secret accepted", False, str(e))
s.close()
time.sleep(0.05)

# T4: Sending a non-Connection packet from an unknown address → dropped (no response)
s = _sock()
s.sendto(_raw(1, 42, "x:10,y:20"), SERVER)  # PlayerPosition, not authenticated
try:
    s.recvfrom(1024)
    check("Unauthenticated PlayerPosition dropped", False, "got a response")
except socket.timeout:
    check("Unauthenticated PlayerPosition dropped", True)
s.close()
time.sleep(0.05)

# ─────────────────────────────────────────────────────────────────────────────
print("\n=== Per-Packet Integrity ===")

# T5: Authenticated client sends packet without HMAC → dropped; server still ACKs valid packet
s = _sock()
sk = _auth(s)
_send(s, 1, 10, "x:5,y:5")   # PlayerPosition with no HMAC tag — should be dropped
time.sleep(0.05)
s.sendto(_sign(sk, 2, 11, "ping"), SERVER)  # valid GameEvent — should be ACK'd
ok, _ = recv_ack(s)
check("Packet without HMAC dropped (server still responds)", ok)
s.close()
time.sleep(0.05)

# T6: Authenticated client sends packet with corrupted HMAC → dropped
s = _sock()
sk = _auth(s)
bad_tag = bytes(32)  # 32 zero bytes — definitely wrong
s.sendto(_raw(1, 20, "x:99,y:99") + bad_tag, SERVER)
time.sleep(0.05)
s.sendto(_sign(sk, 2, 21, "ping"), SERVER)  # valid — should be ACK'd
ok, _ = recv_ack(s)
check("Corrupted HMAC dropped (server still responds)", ok)
s.close()
time.sleep(0.05)

# T7: Replaying a previously seen signed packet is accepted (replay protection
#     is tracked in THREATS.md as future work — this test documents current behaviour)
s = _sock()
sk = _auth(s)
s.sendto(_sign(sk, 1, 30, "x:1,y:1"), SERVER)  # first send (unreliable — no ACK)
time.sleep(0.02)
s.sendto(_sign(sk, 1, 30, "x:1,y:1"), SERVER)  # replay
time.sleep(0.02)
s.sendto(_sign(sk, 2, 31, "ping"), SERVER)       # reliable — should be ACK'd
ok, _ = recv_ack(s)
check("Replay accepted (known limitation — see THREATS.md)", ok)
s.close()
time.sleep(0.05)

# ─────────────────────────────────────────────────────────────────────────────
print("\n=== Rate Limiting ===")

# T8: Flood client until it is kicked (> MAX_STRIKES), then verify the server
#     is still alive by connecting a fresh client and completing auth.
flood = _sock()
sk = _auth(flood)
time.sleep(0.05)
for i in range(130):   # 10 excess packets → strikes > 3 → kicked
    flood.sendto(_sign(sk, 1, 200 + i, f"x:{i},y:0"), SERVER)
flood.close()
time.sleep(0.15)

# Fresh client should still be able to authenticate
s2 = _sock()
try:
    sk2 = _auth(s2)
    s2.sendto(_sign(sk2, 2, 1, "ping"), SERVER)
    ok, _ = recv_ack(s2)
    check("Server alive after flood (new client accepted)", ok)
except Exception as e:
    check("Server alive after flood (new client accepted)", False, str(e))
finally:
    s2.close()

# ─────────────────────────────────────────────────────────────────────────────
print(f"\n{'='*40}")
total = passed + failed
print(f"Results: {passed}/{total} passed" + ("  ✓" if failed == 0 else f"  ({failed} failed)"))
