# aegis-link

A secure, cloud-native multiplayer game server built from scratch in Rust. This is a portfolio project combining systems programming (CS) with applied network security (Cybersecurity minor) — custom reliable UDP, a full HMAC-SHA256 authentication layer, replay attack protection, and a real-time terminal dashboard.

---

## What it does

aegis-link acts as the authoritative server for a multiplayer game. Clients connect over UDP, complete a challenge-response handshake, and then send signed position/event packets. The server runs a 60 Hz game loop that broadcasts a signed world snapshot to every authenticated client.

Everything on the wire is either authenticated or dropped.

---

## Architecture

```
┌──────────────────────────────────────────────────────┐
│                     aegis-link                       │
│                                                      │
│  ┌────────────┐  ┌─────────────┐  ┌───────────────┐  │
│  │  Receive   │  │  Game Loop  │  │  Retransmit   │  │
│  │   Loop     │  │   60 Hz     │  │    Task       │  │
│  │ (auth +    │  │ (snapshots) │  │ (reliable     │  │
│  │  dispatch) │  │             │  │  delivery)    │  │
│  └────────────┘  └─────────────┘  └───────────────┘  │
│         │               │                │           │
│         └───────────────┴────────────────┘           │
│                    Arc<Mutex<ServerState>>           │
│                                                      │
│  ┌────────────────────────────────────────────────┐  │
│  │             Ratatui Dashboard (TUI)            │  │
│  │   header | player table + stats | event log    │  │
│  └────────────────────────────────────────────────┘  │
└──────────────────────────────────────────────────────┘
```

Three concurrent Tokio tasks share state through a single `Arc<Mutex<ServerState>>`. The TUI dashboard runs on a dedicated `std::thread` to keep terminal rendering separate from the async runtime.

---

## Packet format

All game packets use a compact binary header:

```
[ type(1) | sequence_number(4) | flags(1) | payload(N) ]
```

Reliable packets additionally get retransmitted every 100 ms (up to 5 attempts) until the recipient sends back a plaintext `ACK:<seq>`.

Snapshot packets carry the full world state:

```
[ tick_u32(4) | flags(1) | tick_u64(8) | player_count(1) | [ uuid(16) | x_f32(4) | y_f32(4) ]* ]
```

Every snapshot is signed with the recipient's session key before it goes on the wire.

---

## Security layer

The security design is documented in full in [THREATS.md](THREATS.md). Summary:

### Authentication — challenge-response handshake

```
Client                              Server
  │─── Connection ─────────────────>│
  │<── AuthChallenge (nonce 16B) ───│
  │─── AuthResponse                 │
  │    HMAC-SHA256(secret, nonce) ──>│  ← server derives same value, compares
  │<── Welcome (UUID assigned) ─────│
```

The shared secret never travels on the wire. Each session key is `HMAC-SHA256(shared_secret, nonce)` with a fresh random nonce per connection, so no two sessions share a key.

### Per-packet HMAC integrity

Every post-auth packet sent by the client must carry a 32-byte HMAC-SHA256 tag keyed by its session key. The server calls `verify_and_strip()` before doing anything with the bytes. Packets that fail verification — including forged, tampered, or spoofed packets — are silently dropped.

### Replay attack protection

Sequence numbers live inside the signed bytes, so an attacker cannot bump them. The server enforces a monotonic sequence check: any packet with `seq ≤ last_accepted_seq` is dropped without an ACK. Replayed packets cannot roll back state and cannot reset the sequence window.

### Signed outbound snapshots

Each snapshot is signed individually per client using that client's session key. A MitM attacker cannot inject or tamper with a snapshot without invalidating its HMAC tag.

### Rate limiting + eviction

- Position packets: 120/sec per client
- Event/chat packets: 20/sec per client
- Exceeding either limit accumulates a strike; 3 strikes → kicked
- Unauthenticated (Pending) clients are evicted after 5 seconds

### What is intentionally out of scope

- Server-side position validation (velocity bounds) — deferred; edge case of server-teleport would create false positives
- Per-user credentials (single shared secret model)
- ACK signing (plaintext ACKs can be faked to suppress retransmission — noted, out of scope for this phase)

---

## Kubernetes — known limitation: UDP session affinity

Kubernetes Services support `sessionAffinity: ClientIP` for TCP, which pins a client to the same Pod across requests. **This does not work for UDP.** The kube-proxy iptables rules that implement session affinity only track TCP connections; UDP packets have no connection state, so kube-proxy cannot maintain the mapping.

Consequence: in a multi-replica Deployment, a given client's UDP packets may be load-balanced to different Pods across packets. Since `ServerState` is in-process memory, different Pods have no shared view of authenticated clients — a player authenticated on Pod A will be treated as a stranger by Pod B.

**Workarounds (not implemented here):**

- **Single replica** — simplest; no cross-Pod state problem. The current deployment uses this.
- **External shared state** — move `ServerState` into Redis or a similar store. Pods become stateless. Adds latency and operational complexity.
- **Consistent hashing at the load balancer** — some cloud load balancers (AWS NLB, GCP UDP LB) can hash on `(src_ip, src_port)` to pin a UDP client to a fixed backend. Requires a cloud-managed LB, not stock K8s.
- **StatefulSet + client-side routing** — each Pod is a named, stable game room; clients are told which Pod to target at connection time. This is the architecture real game backends use (e.g. Agones).

---

## Module layout

```
src/
├── main.rs         — startup, 60Hz game loop, retransmit task, receive loop
├── auth.rs         — generate_nonce, derive_session_key, sign, verify_and_strip
├── client.rs       — Client struct, ClientState (Pending / Authenticated), PendingPacket
├── server.rs       — ServerState, evict_stale_pending, update_position_player
├── packet.rs       — PacketType enum, Packet struct, FLAG_RELIABLE / FLAG_UNRELIABLE
├── rate_limiter.rs — sliding 1-second window, strike counter
└── dashboard.rs    — Ratatui TUI (3 panels: header, stats + player table, event log)

tests/
├── test_packets.py    — wire format, sequence numbers, flags
├── test_movement.py   — position update acceptance and rejection
├── test_multiplayer.py — multiple simultaneous clients, snapshot consistency
└── test_security.py   — auth handshake, HMAC enforcement, rate limiting,
                          replay attacks (T_R1/T_R2), MitM / snapshot integrity (T_M1/T_M2/T_M3)
```

---

## Running it

**Requirements:** Rust (stable), Python 3 for the test suite.

```bash
# Set the shared secret (defaults to "aegis-dev-secret" if unset)
export AEGIS_SECRET="your-secret-here"

# Build and run
cargo run --release

# In another terminal — run the security test suite
python3 tests/test_security.py
```

The server binds to `0.0.0.0:8080/udp` and opens the Ratatui dashboard in the terminal.

---

## Tech stack

| Layer | Choice | Why |
|---|---|---|
| Language | Rust | Memory safety without a GC; zero-cost async |
| Async runtime | Tokio | Multi-task concurrency on a single thread pool |
| Crypto | HMAC-SHA256 (`hmac` + `sha2` crates) | Standard MAC; constant-time comparison via `verify_slice` |
| RNG | `rand::thread_rng` | CSPRNG for nonce generation |
| TUI | Ratatui + Crossterm | Terminal dashboard with live stats |
| IDs | UUID v4 | Per-client identity, 128 bits of randomness |
| Tests | Python (stdlib only) | Raw UDP sockets, `struct.pack`, `hmac` — no framework needed |

---

## Roadmap

| Phase | Status | Description |
|---|---|---|
| 1 | Done | Custom reliable UDP layer |
| 2 | Done | Game world, 60 Hz loop, multiplayer state |
| 3 | Done | Security layer (auth, HMAC, replay protection, rate limiting) |
| 4 | Next | Docker + Kubernetes |
| 5 | Planned | Error handling polish, lock-across-await audit, Godot client |

---

## About

Built as a portfolio project to apply both a Computer Science major (systems programming, async concurrency, custom networking protocols) and a Cybersecurity minor (threat modeling, cryptographic authentication, attack simulation via automated tests) to something real. Every security decision is documented in [THREATS.md](THREATS.md) with the attack vector, the mitigation, and honest notes on what remains open.
