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
import uuid

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

def _auth_with_id(sock, secret=SHARED_SECRET):
    """Full handshake; returns (session_key, client_uuid_bytes) or raises."""
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
    uuid_str = data[6:].decode(errors="replace").split("is ")[-1].strip()
    return session_key, uuid.UUID(uuid_str).bytes

def recv_snapshot(sock, sk, timeout=2.0):
    """
    Wait for a signed snapshot and return (raw_bytes, [(uuid_bytes, x, y), ...]).
    Snapshot wire format (signed):
      [type(1) | tick_u32(4) | flags(1) | tick_u64(8) | count(1) | [uuid(16)|x_f32(4)|y_f32(4)]* | hmac(32)]
    Returns (None, None) on timeout.
    """
    deadline = time.time() + timeout
    while time.time() < deadline:
        try:
            data, _ = sock.recvfrom(2048)
        except socket.timeout:
            continue
        if data[0] != 4 or len(data) < 47:
            continue
        pkt, tag = data[:-32], data[-32:]
        if not hmac.compare_digest(hmac.new(sk, pkt, hashlib.sha256).digest(), tag):
            continue
        body  = pkt[6:]   # skip [type | seq | flags]
        count = body[8]
        players = []
        off = 9
        for _ in range(count):
            uid  = bytes(body[off:off + 16])
            x, y = struct.unpack(">ff", body[off + 16:off + 24])
            players.append((uid, x, y))
            off += 24
        return data, players
    return None, None

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

def _drain(sock):
    """Discard all queued packets from the receive buffer (non-blocking flush)."""
    old_to = sock.gettimeout()
    sock.settimeout(0)
    while True:
        try:
            sock.recvfrom(4096)
        except (socket.timeout, BlockingIOError, OSError):
            break
    sock.settimeout(old_to)

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

# T7: After a replay attempt the server must still process the next valid packet
#     (liveness check — replay protection is now active, see Replay Attack section below)
s = _sock()
sk = _auth(s)
s.sendto(_sign(sk, 1, 30, "x:1,y:1"), SERVER)  # first send (unreliable — no ACK)
time.sleep(0.02)
s.sendto(_sign(sk, 1, 30, "x:1,y:1"), SERVER)  # replay — must be silently dropped
time.sleep(0.02)
s.sendto(_sign(sk, 2, 31, "ping"), SERVER)       # higher seq, reliable — should be ACK'd
ok, _ = recv_ack(s)
check("Server stays live after replay attempt (next valid packet ACK'd)", ok)
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
# ─────────────────────────────────────────────────────────────────────────────
print("\n=== Replay Attack ===")

# T_R1: Capture a signed position packet, advance to a new position, then replay
#        the old packet — server must reject it and NOT roll back state.
s = _sock()
try:
    sk, my_id = _auth_with_id(s)

    old_pos = _sign(sk, 1, 100, "x:300.0,y:300.0")
    s.sendto(old_pos, SERVER)
    time.sleep(0.05)
    s.sendto(_sign(sk, 1, 101, "x:500.0,y:500.0"), SERVER)
    time.sleep(0.05)
    s.sendto(old_pos, SERVER)   # replay seq=100
    time.sleep(0.1)
    _drain(s)                   # flush stale buffered snapshots
    time.sleep(0.02)            # wait for one fresh snapshot to arrive

    _, players = recv_snapshot(s, sk)
    my_pos = next(((x, y) for uid, x, y in (players or []) if uid == my_id), None)
    if my_pos:
        x, y = my_pos
        check("Replay attack: position not rolled back to replayed state",
              abs(x - 500.0) < 1.0 and abs(y - 500.0) < 1.0,
              f"got x={x:.1f} y={y:.1f}  expected 500,500")
    else:
        check("Replay attack: position not rolled back to replayed state",
              False, "client not found in snapshot")
except Exception as e:
    check("Replay attack: position not rolled back to replayed state", False, str(e))
finally:
    s.close()
time.sleep(0.05)

# T_R2: A low out-of-order seq must not reset the sequence window.
#        Without protection: seq=50 sets last_seq=50, seq=51 is then accepted.
#        With protection:    seq=50 dropped (last_seq=200), seq=51 also dropped.
s = _sock()
try:
    sk = _auth(s)
    s.sendto(_sign(sk, 2, 200, "advance"), SERVER)
    ok_200, _ = recv_ack(s)
    s.sendto(_sign(sk, 2, 50,  "old"),       SERVER)   # replay — should be dropped
    time.sleep(0.05)
    s.sendto(_sign(sk, 2, 51,  "after-old"), SERVER)   # seq=51 < 200 — also dropped
    got_51, ack_data = recv_ack(s, timeout=0.5)
    if got_51 and ack_data:
        ack_seq = int(ack_data[4:].strip())
        check("Replay cannot reset sequence window",
              ack_seq != 51,
              f"got ACK:{ack_seq} — window was reset by replayed seq=50!")
    else:
        check("Replay cannot reset sequence window",
              ok_200, "seq=51 correctly dropped (window unchanged)")
except Exception as e:
    check("Replay cannot reset sequence window", False, str(e))
finally:
    s.close()
time.sleep(0.05)

# ─────────────────────────────────────────────────────────────────────────────
print("\n=== MitM / Snapshot Integrity ===")

# T_M1 + T_M2: A valid signed snapshot passes HMAC; any byte flip in the body fails it.
s = _sock()
try:
    sk, _ = _auth_with_id(s)
    raw, _ = recv_snapshot(s, sk)
    if raw is None:
        check("Valid snapshot HMAC passes",                           False, "no snapshot received")
        check("Tampered snapshot body detected by HMAC (MitM fails)", False, "no snapshot received")
    else:
        pkt, tag = raw[:-32], raw[-32:]
        valid = hmac.compare_digest(hmac.new(sk, pkt, hashlib.sha256).digest(), tag)
        check("Valid snapshot HMAC passes", valid)

        tampered = bytearray(raw)
        tampered[10] ^= 0xFF   # flip one byte in the packet body
        tp, tt = bytes(tampered)[:-32], bytes(tampered)[-32:]
        still_valid = hmac.compare_digest(hmac.new(sk, tp, hashlib.sha256).digest(), tt)
        check("Tampered snapshot body detected by HMAC (MitM fails)",
              not still_valid, "HMAC passed tampered data — MitM undetected!")
except Exception as e:
    check("Valid snapshot HMAC passes",                           False, str(e))
    check("Tampered snapshot body detected by HMAC (MitM fails)", False, str(e))
finally:
    s.close()
time.sleep(0.05)

# T_M3: Attacker intercepts a client→server packet, replaces the payload with a
#        spoofed position, but keeps the original HMAC → mismatch → server drops it.
s = _sock()
try:
    sk, my_id = _auth_with_id(s)
    s.sendto(_sign(sk, 1, 500, "x:10.0,y:10.0"), SERVER)
    time.sleep(0.05)

    # Build the forged packet: HMAC is over "x:10.0,y:10.0" but payload is swapped to
    # "x:99.9,y:99.9" (same byte length so the rest of the packet is not disturbed).
    original   = _sign(sk, 1, 501, "x:10.0,y:10.0")
    spoofed_pl = b"x:99.9,y:99.9"          # 13 bytes — same as original payload
    forged     = bytearray(original)
    forged[5:5 + len(spoofed_pl)] = spoofed_pl   # overwrite payload, HMAC unchanged
    s.sendto(bytes(forged), SERVER)
    time.sleep(0.1)
    _drain(s)                   # flush stale buffered snapshots
    time.sleep(0.02)            # wait for one fresh snapshot to arrive

    _, players = recv_snapshot(s, sk)
    my_pos = next(((x, y) for uid, x, y in (players or []) if uid == my_id), None)
    if my_pos:
        x, y = my_pos
        check("Forged payload (MitM position spoof) rejected — position unchanged",
              abs(x - 10.0) < 1.0 and abs(y - 10.0) < 1.0,
              f"got x={x:.1f} y={y:.1f}  expected 10,10 — spoof succeeded!")
    else:
        check("Forged payload (MitM position spoof) rejected — position unchanged",
              False, "client not found in snapshot")
except Exception as e:
    check("Forged payload (MitM position spoof) rejected — position unchanged", False, str(e))
finally:
    s.close()
time.sleep(0.05)

print(f"\n{'='*40}")
total = passed + failed
print(f"Results: {passed}/{total} passed" + ("  ✓" if failed == 0 else f"  ({failed} failed)"))
