import socket
import struct
import uuid
import time
import threading

def send_packet(sock, addr, packet_type, seq_num, payload=b""):
    if isinstance(payload, str):
        payload = payload.encode()
    sock.sendto(struct.pack(">BI", packet_type, seq_num) + payload, addr)

def parse_snapshot(data):
    if len(data) < 15 or data[0] != 4:
        return None
    tick  = struct.unpack(">Q", data[6:14])[0]
    count = data[14]
    players = {}
    offset  = 15
    for _ in range(count):
        if offset + 24 > len(data):
            break
        uid = str(uuid.UUID(bytes=data[offset:offset+16]))[:8]
        x   = struct.unpack(">f", data[offset+16:offset+20])[0]
        y   = struct.unpack(">f", data[offset+20:offset+24])[0]
        players[uid] = (x, y)
        offset += 24
    return tick, players

def drain(sock):
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
    deadline = time.time() + 1.0
    while time.time() < deadline:
        try:
            data, _ = sock.recvfrom(2048)
        except socket.timeout:
            continue
        result = parse_snapshot(data)
        if result and result[0] > after_tick:
            return result
    return None, {}

print_lock = threading.Lock()

def log(player_id, msg):
    with print_lock:
        print(f"[P{player_id}] {msg}")

def run_player(player_id, positions, start_seq, barrier):
    sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    sock.settimeout(0.5)
    addr = ("127.0.0.1", 8080)

    # Connect
    send_packet(sock, addr, 0, 0)
    try:
        data, _ = sock.recvfrom(1024)
        if len(data) >= 6 and data[0] == 2:
            seq = struct.unpack(">I", data[1:5])[0]
            msg = data[6:].decode(errors="replace")
            log(player_id, f"Connected: {msg}")
            sock.sendto(f"ACK:{seq}".encode(), addr)
    except socket.timeout:
        log(player_id, "No welcome — aborting")
        return

    time.sleep(0.1)

    # Wait until both players are connected before moving
    barrier.wait()
    log(player_id, "Starting movement")

    for i, (x, y) in enumerate(positions):
        seq = start_seq + i

        baseline = drain(sock)
        send_packet(sock, addr, 1, seq, f"x:{x},y:{y}")
        time.sleep(0.035)

        tick, players = recv_snapshot_after(sock, after_tick=baseline)
        if tick is not None:
            player_list = "  ".join(f"[{uid}...] ({px:.0f},{py:.0f})" for uid, (px, py) in players.items())
            log(player_id, f"tick={tick} ({len(players)} players): {player_list}")
        else:
            log(player_id, f"No snapshot for move {i+1}")

        time.sleep(0.05)

    sock.close()

# Two players, different paths and sequence ranges
p1_path = [(10, 0), (20, 10), (30, 20), (40, 30), (50, 40)]
p2_path = [(0, 10), (10, 20), (20, 30), (30, 40), (40, 50)]

barrier = threading.Barrier(2)

t1 = threading.Thread(target=run_player, args=(1, p1_path, 100, barrier))
t2 = threading.Thread(target=run_player, args=(2, p2_path, 200, barrier))

t1.start()
t2.start()
t1.join()
t2.join()

print("\nDone.")
