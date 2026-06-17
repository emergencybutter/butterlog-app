# Multiplayer Connectivity for Peers Behind the Same NAT

> Status: **implemented**. LAN host candidates are published and used for
> same-NAT peer selection across the app, service, and traffic simulator.

## Problem

Two players behind the **same NAT** (same home router / public IP) cannot exchange
multiplayer traffic. Each sees the other locally in the peer list, but no telemetry
ever arrives, so the aircraft never appears.

## How the transport works today

The transport is pure STUN-based peer-to-peer hole punching (`butterlog-app/src-tauri/src/multiplayer.rs`):

1. Each client discovers its **public** address via STUN and stores it as
   `public_address`.
2. It publishes *only* that public address to the service: `multiplayer_ping_core`
   writes `udp_address` into `multiplayer_peers` (`butterlog-service/src/handlers.rs`).
3. The service returns every other peer's **public** address
   (`SELECT udp_address ... WHERE user_id <> $1`).
4. Clients send telemetry directly to those public addresses, and the receiver
   **only accepts packets from known peers** — the source address must be in the
   peer list (`multiplayer.rs`, `is_known_peer` check).

## Root cause: no NAT hairpinning

When two peers are behind the same NAT, STUN reports the **same public IP** for both
(the router's WAN IP), differing only in port. So peer A is told to send to
`WAN_IP:portB`. That datagram leaves A, reaches the router's *own external* address,
and to be delivered to B the router must **hairpin** it (a.k.a. NAT loopback) — route
a packet from an internal host, addressed to the router's external IP, back to another
internal host.

**Many consumer routers do not support hairpinning.** The packet is dropped, so
same-NAT peers never reach each other even though they are one hop apart on the LAN.

## Goals / non-goals

- **Goal:** two peers on the same LAN exchange telemetry directly, with no reliance on
  router hairpinning.
- **Goal:** no regression for peers on different NATs (today's public-address path).
- **Non-goal (v1):** full ICE connectivity checks / candidate racing. Kept as a
  possible follow-up (see Alternatives).
- **Non-goal:** server relay (TURN-style). Reserved as a last-resort fallback only.

## Design: LAN host candidates (ICE-lite)

Gather and exchange a **local/LAN candidate** alongside the public one. When two peers
share a public IP, talk directly over the LAN instead of bouncing off the router.

### 1. Client gathers a local candidate

In `multiplayer.rs`, after binding the send socket:

- Derive the LAN IP with the standard trick: a throwaway `UdpSocket`,
  `connect("8.8.8.8:80")` (no packet is sent), then read `local_addr().ip()`.
- Combine it with the multiplayer socket's actual local port:
  `local_address = LAN_IP:local_port`.

This mirrors how `public_address` is derived, so both candidates share the same port.

### 2. Wire format: publish both candidates

Ping request (`MultiplayerPingRequest` in `handlers.rs`, ping body in `multiplayer.rs`):

```jsonc
{ "udp_address": "<public>", "local_udp_address": "<lan>" }
```

Response: `local_udp_address` is added to the existing `peer_details` array (which also
carries `username`), so peers come back as flat records:

```jsonc
{
  "peers": ["1.2.3.4:50001", "..."],          // bare addresses, kept for old clients
  "peer_details": [
    { "udp_address": "1.2.3.4:50001",
      "local_udp_address": "192.168.1.20:50001",
      "username": "Alice" }
  ]
}
```

Both the legacy path-token handler and the bearer handler call `multiplayer_ping_core`,
so they get this for free. The fields are additive: older clients that send only
`udp_address` keep working (their `local_udp_address` is null), and a client talking to
an older service falls back to the bare `peers` list.

### 3. Service / schema

- New migration: add `local_udp_address TEXT` (nullable) to `multiplayer_peers`.
- `update_and_get_peers` stores `local_udp_address` and returns both columns for the
  other peers.

### 4. Candidate selection (client)

For each peer, compare the peer's **public IP** to our own (`public_address`, IP only,
ignore port):

- **Same public IP** → target the peer's `local` address (same LAN; no hairpinning).
- **Different public IP** → target the peer's `public` address (unchanged behaviour).

Use the chosen candidate to build **both**:

- the **send-target** list, and
- the **known-sender allowlist** that gates `is_known_peer`.

This matters because on the same LAN the receiver sees the sender's *real LAN address*
as the packet source, so that LAN address must be in the allowlist or the packet is
dropped as an unknown sender.

### 5. Tracking stays correct

Tracked aircraft are keyed by source address (`multiplayer.rs` `tracked_aircrafts`).
Because selection picks exactly **one** candidate per peer, each peer maps to a single
source path — no duplicate aircraft.

## Alternatives considered

- **Full ICE with candidate racing.** Send a small probe to *all* candidates and lock
  onto the first that replies. More resilient (handles asymmetric reachability), but
  requires keying tracked aircraft by a **stable peer id** (e.g. the service `user_id`)
  instead of source address, so a peer can migrate paths without spawning a second
  aircraft. Deferred; the same-public-IP heuristic covers the reported case at far less
  complexity.
- **Server relay (TURN-style).** Forward telemetry through the service when no candidate
  connects. Guarantees delivery but consumes server bandwidth for every packet; keep
  only as a final fallback if direct paths fail.

## Compatibility & rollout

- The `local_udp_address` request field and the `peer_details[].local_udp_address`
  response field are additive.
- Mixed-version peers degrade gracefully: if a peer didn't publish a local candidate,
  fall back to its public address (today's behaviour); against an older service the
  client falls back to the bare `peers` list.
- The schema column is nullable; existing rows need no backfill.

## Implementation

- **Service:** migration `20260616000000_multiplayer_peer_local_address.sql`;
  `MultiplayerPingRequest`/`PeerDetail` and `update_and_get_peers` in `handlers.rs`. The
  upsert uses `COALESCE` so a caller without a local address (e.g. flight create/update)
  doesn't wipe the stored one.
- **App:** `multiplayer.rs` — `discover_local_address`, `local_address` on the manager,
  `update_peers_from_candidates` (same-public-IP selection), and the ping body now
  publishes `local_udp_address`.
- **Simulator:** `traffic_simulator.rs` publishes a `local_udp_address` per mock client.

## Testing

The simulator and app run on one machine/LAN, so the case is reproducible locally: the
simulator now publishes a LAN host candidate, and because it shares the app's public IP,
the app selects the **local** candidate and renders the aircraft (whereas the public-only
path would silently drop on a non-hairpinning router).
