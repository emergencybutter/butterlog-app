# Scoping the Multiplayer Peer List to Nearby Players

> Status: **implemented**. The ping endpoint stores each peer's position and
> returns only nearby peers; a caller without a position fix gets none.

## Problem

The `/multiplayer/ping` endpoint hands every online user **every other online
user's** address. Each client then sends UDP telemetry to all of them every
~200–250 ms, regardless of distance, and only filters on receive. With *N*
online users that's an all-to-all mesh: ~*N²* packet flows and *N* hole-punch
targets per client. It doesn't scale, wastes bandwidth on peers the receiver
will discard, and leaks every user's public IP + username to everyone online.

## Prior behavior (before this change)

`update_and_get_peers` (`butterlog-service/src/handlers.rs`):

1. Upserts the caller's presence row in `multiplayer_peers`
   (`user_id, udp_address, local_udp_address, last_seen`).
2. Prunes rows with `last_seen` older than 120 s.
3. Returns **all** other rows:

```sql
SELECT mp.udp_address, mp.local_udp_address, COALESCE(NULLIF(u.global_name,''), u.username)
FROM multiplayer_peers mp
JOIN users u ON u.id = mp.user_id
WHERE mp.user_id <> $1
```

The only filters are "not me" and "active". There is **no position stored and
no distance filter** — the table doesn't know where anyone is.

Proximity is enforced **client-side** in the receiver (`multiplayer.rs`):
the (now sole) inject mode processes/keeps an aircraft only within `100 nm`.

## Goal / non-goals

- **Goal:** the service returns only peers near the caller, so a client sends to
  (and hole-punches) just those — cutting the *N²* fan-out.
- **Decision:** when the caller has no position fix, return **no peers** — it
  can't be near anyone, and inject mode can't render a peer without its own
  coordinates, so returning everyone would only re-introduce the fan-out.
- **Non-goal:** changing the transport. This scopes *who* you talk to; it's still
  all-to-all within a region. Hundreds of co-located users would
  need a relay/SFU — out of scope.

## Design

Store each peer's position on ping and bounding-box filter the returned set
against the caller's position. Filtering the list is the only lever needed:
the client sends to exactly the addresses the service returns, so a smaller
list shrinks both send fan-out and hole-punching.

### 1. Schema — position on the presence row

```sql
ALTER TABLE multiplayer_peers ADD COLUMN IF NOT EXISTS latitude  DOUBLE PRECISION;
ALTER TABLE multiplayer_peers ADD COLUMN IF NOT EXISTS longitude DOUBLE PRECISION;
-- optional, only once the table is large:
CREATE INDEX IF NOT EXISTS idx_multiplayer_peers_pos ON multiplayer_peers (latitude, longitude);
```

### 2. Request — caller sends its position

`MultiplayerPingRequest` gains optional coords (additive):

```rust
pub struct MultiplayerPingRequest {
    pub udp_address: Option<String>,
    #[serde(default)] pub local_udp_address: Option<String>,
    #[serde(default)] pub latitude:  Option<f64>,
    #[serde(default)] pub longitude: Option<f64>,
}
```

### 3. Server — persist coords, bounding-box filter

Persist `latitude/longitude` in the upsert (same `COALESCE`-preserve pattern as
`local_udp_address`, so a position-less caller doesn't wipe a stored one). If the
caller sent **no** position, early-return an empty list. Otherwise compute the
box and filter:

```rust
let (lat, lon) = match (latitude, longitude) {
    (Some(lat), Some(lon)) => (lat, lon),
    _ => return Ok(Some(Vec::new())), // no fix → no peers
};
const RADIUS_NM: f64 = 120.0; // > the client's 100 nm gate, for movement between pings
let lat_delta = RADIUS_NM / 60.0;
let lon_delta = RADIUS_NM / (60.0 * lat.to_radians().cos().abs().max(0.01)); // guard poles
```

```sql
SELECT mp.udp_address, mp.local_udp_address, COALESCE(NULLIF(u.global_name,''), u.username)
FROM multiplayer_peers mp
JOIN users u ON u.id = mp.user_id
WHERE mp.user_id <> $1
  AND ( mp.latitude IS NULL                        -- peer has no known position → keep (client still gates)
     OR ( mp.latitude  BETWEEN $2 AND $3
      AND mp.longitude BETWEEN $4 AND $5 ) )
```

`$2/$3` are the caller's lat range, `$4/$5` the lon range. A square box slightly
over-includes corners — the client's existing 100 nm check trims them. For an exact
circle, run a haversine pass in Rust over the (small) returned set.

### 4. Client — populate coords (omit when no fix)

In the app ping (`multiplayer.rs`), the flight-sync path (`webhook_manager.rs`,
which also returns peers), and the traffic simulator:

```rust
// Send our position so the service scopes to nearby peers. With no fix we omit
// it and the service returns no peers.
let (lat, lon) = match monitor.get_connected_monitor().map(|m| m.get_metrics()) {
    Some(m) if m.latitude != 0.0 || m.longitude != 0.0 => (Some(m.latitude), Some(m.longitude)),
    _ => (None, None),
};
// body: { udp_address, local_udp_address, latitude: lat, longitude: lon }
```

## Edge cases & rollout

- **Rollout / compat:** the request fields and columns are additive, and the
  `OR mp.latitude IS NULL` clause keeps *peers* with unknown position visible.
  But the caller side is a **breaking change**: an existing released client
  doesn't send a position, so against the new service it gets an **empty** peer
  list until it updates to a version that sends coordinates. Ship the
  position-sending client **before or with** the service deploy.
- **No fix yet:** a caller without a position fix sends no coords and gets **no
  peers** (it can't be near anyone, and inject mode needs coordinates to render).
- **Boundary margin:** filter at ~120 nm server-side vs the 100 nm client gate so
  aircraft don't pop in/out at the edge as people move between 200 ms pings under
  the 120 s presence window.
- **Antimeridian / poles:** the box breaks across ±180° longitude and near the
  poles (`cos→0`, clamped). Rare for normal flight ranges; special-case a wrapped
  longitude range if it ever matters.
- **Privacy:** as a side effect, you only learn the addresses/usernames of users
  near you rather than everyone online.

## Implementation

- **Service:** migration `20260617000000_multiplayer_peer_position.sql` (lat/lon
  columns); `MultiplayerPingRequest`, `CreateFlightRequest`, `UpdateFlightRequest`,
  and `update_and_get_peers` in `handlers.rs` (persist + bounding-box filter,
  empty when the caller has no position).
- **App:** `multiplayer.rs` ping sends the connected monitor's position;
  `webhook_manager.rs` flight create/update also sends it (it returns peers too).
- **Simulator:** `traffic_simulator.rs` publishes each mock client's position
  (`ping_position`) so the service scopes peers and same-machine testing works.

## Alternatives considered

- **Keep client-side-only filtering (today).** Simplest, but the *N²* fan-out and
  global address disclosure remain.
- **Geohash / spatial index buckets.** Overkill at current scale; a bounding box
  over a small, 120 s-pruned table is plenty. Revisit if the active set grows.
- **Relay / SFU transport.** Needed for large co-located crowds, but a much bigger
  change than scoping the peer list.
