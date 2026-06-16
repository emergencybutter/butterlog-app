# Changelog

All notable changes to this project will be documented in this file.

## [Unreleased]

### Changed
- Multiplayer telemetry now carries the sender's deduced ICAO type, operating airline and raw livery. Receivers use the deduced ICAO (instead of the unreliable raw ATC model) and the airline to pick a closer local model — including the matching livery title on MSFS, where liveries are separate titles.
- The operating airline is carried on the multiplayer wire as a portable ICAO code (e.g. `UAL`) rather than a name: the X-Plane plugin can key its livery directly off it, and MSFS receivers map the code back to a name against their own table to match installed livery titles.
- The app→X-Plane plugin protocol now ships an `on_ground` flag alongside each aircraft's position (peer-to-peer telemetry already carried it inside the metrics block). The X-Plane plugin uses it to clamp on-ground aircraft to the local terrain (via a terrain probe) instead of trusting the transmitted MSL altitude, so multiplayer traffic no longer floats above or sinks into the runway.

### Added
- The Multiplayer Debugging tab now shows each tracked peer's deduced ICAO type, airline (ICAO code + name), livery, and the local model chosen to represent them.

## [0.3.14] - 2026-06-15

### Changed
- Deduce the ICAO type and operating airline more accurately: ignore third-party add-on developer/studio names (PMDG, Fenix, iFly, FSLTL, Asobo, Laminar, Black Square, etc.) in the title/livery, and require at least one distinctive word before inferring an operator.
- Removed the sim's raw ATC model from the UI (the aircraft title and ATC ID / tail number are still shown).
- When enumerating AI aircraft and liveries, only unresolved entries are logged, reducing log noise.
- API authentication moved from token-in-URL to the `Authorization: Bearer` header; the service token and service URL are now stored as separate `apiToken`/`serviceUrl` config fields (existing configs migrate automatically on startup).
- Multiplayer UDP listener now drops telemetry packets from senders that are not in the peer list provided by the service, preventing unsolicited traffic injection.
- Status polling is paused while the window is hidden in the tray and slowed to 1s when no flight is being logged (200ms while logging).
- Backend and UI log buffers are capped at 2000 lines so long tray sessions no longer grow memory unbounded.
- Flight history scanning caches parsed summaries by file modification time instead of re-parsing every flight database on each refresh.
- Blocking SQLite/file/image work in webhook sync, screenshot upload, and `get_remote_id` now runs on blocking threads instead of the async runtime.
- Discord login stores the webhook URL using the active service URL (respects `--service-url`).

### Fixed
- ICAO type / airline resolution is no longer disabled for an entire session when monitoring starts before the reference data finishes loading; the word indexes now build lazily once their data is available.
- Replaced several `unwrap()` panics in CSV import/export and flight summary parsing with proper error messages.
- `get_metrics`/`get_current_flight_id` no longer fall back to a disconnected monitor when no simulator is connected.

### Added
- Show the deduced ICAO type and operating airline (resolved from the title/livery) in the app (Flight Details and history), on the service web UI (flight detail, history cards, share page), and in Discord notifications.
- Recognize many more aircraft titles when deducing the ICAO type: developer names fused to the model (e.g. `FenixA321`), joined variant shorthands (`ATR72`, `A320neo`, `A380X`, `737-MAX8`), and added CubCrafters XCub (CC19), Progressive Aerodyne SeaRey (SREY) and Cessna 408 SkyCourier (C408) to the type database.
- After a flight is closed, a new flight log now starts automatically the next time the aircraft moves.
- Approach stability score: variance of G-force, roll and indicated airspeed over the minute before touchdown (excluding the final 5 seconds of flare/touchdown), combined into a 0-100 score and displayed next to the landing score in the Landing Scorecard.
- Support for fetching ATC Model and ATC ID (tail number) for both MSFS (SimConnect) and X-Plane (Web REST API) connections.
- Persist `atc_model` and `atc_id` in the SQLite database `summary` table for flight logs, ensuring backwards-compatibility.
- Include `atc_model` and `atc_id` fields in the JSON webhook payload sent to third-party endpoints.
- Display the aircraft title with its ATC ID (tail number, formatted as `Title (ID)`) in the Flight History list, the expanded logs list, and the Flight Details view.
- Query available local aircraft and helicopters using `SimConnect_EnumerateSimObjectsAndLiveries` upon connection.
- Cache available models locally and map remote multiplayer aircraft to the closest matching local model (substring, keyword, helicopter-specific, and default fallback logic).
- Settings option "Enable VATSIM Traffic" to toggle VATSIM network traffic synchronization.
- Periodically fetch live VATSIM network data from `https://data.vatsim.net/v3/vatsim-data.json` every 15 seconds.
- Filter VATSIM aircraft within a 20.0 NM radius of the user's aircraft.
- Spawn and update nearby VATSIM pilots in Microsoft Flight Simulator using the multiplayer livery/fallback mapping system.
- Implement remote aircraft timeout and cleanup, removing VATSIM traffic from the simulator via `ai_remove_object` if they have not been updated for over 45 seconds.
- Add setting option "Inject traffic from other butterlog users" to allow direct P2P aircraft traffic synchronization.
- Implement STUN (Session Traversal Utilities for NAT) protocol to discover local client's public UDP address.
- Publish public UDP address to webhook coordination server.
- Periodically publish flight position data directly to other butterlog clients' discovered UDP addresses every 250ms.
- Listen for UDP position packets from other users, keep track of aircraft within 20 NM that have reported in the last 60 seconds (1 minute), and inject them as traffic in the simulator.


