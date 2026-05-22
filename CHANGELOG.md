# Changelog

All notable changes to this project will be documented in this file.

## [Unreleased]

### Added
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

