# Changelog

All notable changes to this project will be documented in this file.

## [Unreleased]

### Changed
- API authentication moved from token-in-URL to the `Authorization: Bearer` header; the service token and service URL are now stored as separate `apiToken`/`serviceUrl` config fields (existing configs migrate automatically on startup).
- Multiplayer UDP listener now drops telemetry packets from senders that are not in the peer list provided by the service, preventing unsolicited traffic injection.
- Status polling is paused while the window is hidden in the tray and slowed to 1s when no flight is being logged (200ms while logging).
- Backend and UI log buffers are capped at 2000 lines so long tray sessions no longer grow memory unbounded.
- Flight history scanning caches parsed summaries by file modification time instead of re-parsing every flight database on each refresh.
- Blocking SQLite/file/image work in webhook sync, screenshot upload, and `get_remote_id` now runs on blocking threads instead of the async runtime.
- Discord login stores the webhook URL using the active service URL (respects `--service-url`).

### Fixed
- Replaced several `unwrap()` panics in CSV import/export and flight summary parsing with proper error messages.
- `get_metrics`/`get_current_flight_id` no longer fall back to a disconnected monitor when no simulator is connected.

### Added
- Approach stability score: variance of G-force, roll and indicated airspeed over the minute before touchdown (excluding the final 5 seconds of flare/touchdown), combined into a 0-100 score and displayed next to the landing score in the Landing Scorecard.
- Support for fetching ATC Model and ATC ID (tail number) for both MSFS (SimConnect) and X-Plane (Web REST API) connections.
- Persist `atc_model` and `atc_id` in the SQLite database `summary` table for flight logs, ensuring backwards-compatibility.
- Include `atc_model` and `atc_id` fields in the JSON webhook payload sent to third-party endpoints.
- Update UI to display the aircraft title along with its ATC Model and ATC ID (formatted as `Title [Model] (ID)`) in the Flight History list, the expanded logs list, and the Flight Details view.
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


