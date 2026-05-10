# Threat Model — aegis-link

## Assets

| Asset | Why it matters |
|---|---|
| Player positions and game events | Core game state — forging these lets a client cheat |
| Client identity (UUID) | Spoofing another client's UUID lets a player impersonate them |
| Server availability | Flooding crashes or degrades the game for all players |
| Shared secret | Compromise lets an attacker forge any packet for any client |

---

## Threats and Mitigations

### 1. Spoofed / forged packets
**Threat:** An attacker crafts a UDP packet with a victim's source IP and sends fake position or event data.

**Mitigation (implemented):** Every post-auth packet carries a 32-byte HMAC-SHA256 tag keyed with the client's session key. The server silently drops any packet whose tag does not verify. Because each session key is derived from `HMAC(shared_secret, nonce)` with a unique random nonce per connection, an attacker who does not know the shared secret cannot forge a valid tag.

---

### 2. Replay attacks
**Threat:** An attacker records a valid signed packet and re-sends it later to repeat an action (e.g., re-triggering a game event).

**Mitigation (partial):** Sequence numbers are included inside the HMAC-signed bytes, so the replayed packet is structurally valid. The server currently does **not** enforce a monotonic sequence check — replays are accepted.

**Future work:** Track the last accepted sequence number per client and reject packets with `seq ≤ last_seq`. For unreliable channels a sliding window (e.g., 64-packet bitmap) prevents out-of-order drops while still rejecting true replays.

---

### 3. Unauthorised access
**Threat:** A client attempts to participate without knowing the shared secret.

**Mitigation (implemented):** The server issues a random 16-byte nonce on every Connection packet. The client must respond with `HMAC-SHA256(shared_secret, nonce)` within 5 seconds. Clients that do not respond, or respond with the wrong tag, are evicted from the pending table. Clients that never completed auth cannot send game packets (they are dropped at the `Unknown` branch).

---

### 4. Flood / connection exhaustion (DDoS)
**Threat:** An attacker sends a high volume of packets to degrade server performance or kick a legitimate client.

**Mitigation (implemented):**
- **Per-client rate limiter:** 120 PlayerPosition packets/sec and 20 GameEvent/ChatMessage packets/sec. Clients that exceed the limit accumulate strikes; after 3 strikes they are disconnected.
- **Pending client eviction:** Unauthenticated (Pending) clients are removed after 5 seconds, limiting the cost of connection-flood attacks.

**Limitations:** The rate limiter operates per source address. An attacker with many IPs (botnet) can still saturate the server. Mitigations at the network edge (e.g., firewall, cloud WAF) are outside scope.

---

### 5. Server-to-client packet injection (MitM)
**Threat:** A network attacker injects or replays server snapshot packets to a client, causing the client to render false game state.

**Mitigation (not yet implemented):** Server outbound packets are currently unsigned. A full mitigation requires signing snapshots with the session key or using DTLS. Noted as future work.

---

### 6. Position / input cheating
**Threat:** An authenticated client sends physically impossible position updates (teleporting, speed hacking).

**Mitigation (not yet implemented):** The server trusts client-reported positions. Future work: server-side velocity and bounds checks in `update_position_player`.

---

### 7. Shared-secret compromise
**Threat:** The server's `AEGIS_SECRET` environment variable leaks, allowing an attacker to authenticate as any player.

**Mitigation (partial):** The secret is read from the environment at startup and never logged. Each client's session key is derived uniquely (`HMAC(secret, nonce)`), so leaking one session key does not expose others. Full mitigation requires rotating the secret and invalidating all active sessions, which is not yet implemented.

---

## Known Limitations

| Limitation | Tracked in |
|---|---|
| No replay protection (monotonic seq check) | Section 2 above |
| Server outbound packets unsigned | Section 5 above |
| No server-side position validation | Section 6 above |
| Single shared secret (no per-user credentials) | Section 7 above |
| Welcome packet always uses seq=1 (pending-table collision with multiple simultaneous connects) | Code comment in main.rs |
| ACK messages are plaintext and unsigend (attacker can fake ACKs to suppress retransmission) | Out of scope for this phase |
