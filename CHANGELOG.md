# Changelog

All notable changes to this project will be documented in this file.

## [Unreleased]

### Fixed
- MSFS flights could log the wrong departure: it was taken at SimStart, while the position was still where the sim had been before it loaded the flight (a flight from PHOG was logged as departing KBFI until the final summary corrected it; a flight that never finished kept it). The departure is now the airport nearest the aircraft when it first moves faster than 3 kt on the ground.
- X-Plane telemetry recorded almost every frame while the aircraft stood still: X-Plane's direction of travel (`hpath`) jitters through every heading when the aircraft isn't moving, so each frame looked like a change. It's now held below 1 m/s groundspeed (about 2 kt), which halves the ticks of a recent short flight and cuts those while stopped by about 70%.
- X-Plane telemetry always fell back to the Web API: plugin 0.3.0 never answered the app's subscription (fixed in plugin 0.3.1). The app now also waits up to 2 minutes for that answer, since it comes from X-Plane's flight loop, which doesn't run while X-Plane is loading; a plugin that doesn't say hello within 3 seconds still falls back at once.
- After a flight ended, live tracking kept uploading to it every 20 seconds ("already ended") until the next flight: the sim monitor's periodic sync read the ended flight's id back from its log and made it current again. Syncs for an ended flight's log are now dropped.
- X-Plane aircraft with fuel-injected piston engines (such as the C172 SP) were reported as turboprops; X-Plane's turboprops were reported as jets.

### Changed
- Shared flight tracks are thinned on a new policy: straight and level keeps one sample per 5 minutes (was 1 per minute), and everything else — climbing, descending, turning — keeps one sample per second (was 1 per 10 seconds). Climbs, descents, turns and pattern work are now recorded at full logger fidelity while cruise costs a fifth of what it did.
- "Straight and level" is now judged from the aircraft's instantaneous vertical speed and bank angle rather than only from how far it has drifted since the last kept sample. The drift check remains as a backstop, so a slow wander — or a sim reporting an unreliable vertical speed — still forces a sample.
- Multiplayer telemetry now carries the sender's deduced ICAO type, operating airline and raw livery. Receivers use the deduced ICAO (instead of the unreliable raw ATC model) and the airline to pick a closer local model — including the matching livery title on MSFS, where liveries are separate titles.
- The operating airline is carried on the multiplayer wire as a portable ICAO code (e.g. `UAL`) rather than a name: the X-Plane plugin can key its livery directly off it, and MSFS receivers map the code back to a name against their own table to match installed livery titles.
- The app→X-Plane plugin protocol now ships an `on_ground` flag alongside each aircraft's position (peer-to-peer telemetry already carried it inside the metrics block). The X-Plane plugin uses it to clamp on-ground aircraft to the local terrain (via a terrain probe) instead of trusting the transmitted MSL altitude, so multiplayer traffic no longer floats above or sinks into the runway.

### Added
- The app builds and runs on macOS and Linux. MSFS support (SimConnect) is behind a new `msfs` Cargo feature, on by default but only compiled in on Windows; elsewhere the app runs with X-Plane only. A GitHub Actions workflow builds, tests and packages the app for macOS (universal .dmg) and Linux (.deb, .rpm, AppImage), bundling the X-Plane plugin from its latest green master build and signing when the secrets listed in `.github/workflows/build.yml` are set. On Windows, `--no-default-features` builds without the MSFS SDK. The sibling `simplesimconnect` repos must still be checked out next to the app, because Cargo reads them to resolve the lockfile, but they aren't built off Windows.
- The app now ships the X-Plane plugin and keeps it up to date: at startup it updates the plugin in every X-Plane 12 install that has it (found through `x-plane_install_12.txt`), and Settings → X-Plane Plugin shows each install's status with an Install or Update button. An update made while X-Plane is running takes effect on its next start. The hand-built plugin zips are no longer needed.
- X-Plane telemetry from the plugin (0.3.0 and later): the app subscribes over the plugin's localhost link (TCP port 49021) and records every sim frame, stamped with the sim clock, as `capture: "xp_plugin"`, instead of the Web API's ~8 updates a second. The plugin also reports aircraft and airport loads. Without the plugin, or if its link drops, recording goes on through the Web API in a new session.
- The backend log is also written to a file, one per day under `logs` in the app data directory (`logs-dev` with `--dev`), kept for 7 days and capped at 50 MB a day, so a run can be read after the fact. "Open Log File" in the Backend Logs tab shows it.
- Flights are now visible online in full detail *while you fly*. The app streams the track to the service every 20 seconds, and `/content/flights/:id` shows a live map, charts, events and screenshots that update as you go, handing off to the permanent share once you land. Controlled by a new "Show flights live on the web while flying" setting, on by default.
- Optional remote control of your own simulator from your live flight page: pause and resume, plus beta autopilot controls (heading bug, altitude, vertical speed, HDG and NAV modes). Off by default behind "Allow remote commands", with the beta controls behind a second opt-in. Only you can command your own flight.
- The Multiplayer Debugging tab now shows each tracked peer's deduced ICAO type, airline (ICAO code + name), livery, and the local model chosen to represent them.
- Testcase for Phenom 300E Voyager Aviation livery/airline detection mapping to VYA.
- Groundwork for sim-native telemetry: a bounded, crash-safe spool (`telemetry_spool` in the app data directory) and an uploader that sends it to the service's session endpoints, deleting each chunk once the service has decoded it.
- MSFS sim-native telemetry sampler, in shadow mode beside the flight log: every SimVar the app logs, read on every sim frame as changed values only (SimConnect `CHANGED | TAGGED`), sampled at per-phase rates (up to 30 Hz near the ground, 5 Hz manoeuvring, 1 Hz in cruise) and recorded into the spool, one session per aircraft loaded. The sim's system events (sim start and stop, pause, crash and crash reset, flight and aircraft loaded with their file paths, position changed by hand) are recorded alongside, at the moment they happen. Off unless a `telemetry_beta` file exists in the app data directory and the app is signed in.
- X-Plane sim-native telemetry, in the same shadow mode and behind the same switch: the datarefs the app logs, plus true heading, sim clock, pause, replay, sim rate and crash, recorded from the Web API subscription (about 8 updates a second) through the same per-phase sampler. Pause, replay, crash and position jumps are recorded as events.

### Fixed
- X-Plane flights now log pitch, roll and heading the way MSFS flights do. Pitch and roll were stored with X-Plane's signs, the opposite of MSFS's, so exports showed them inverted and X-Plane peers appeared in other users' MSFS with inverted pitch and bank. Heading was magnetic instead of true, so it was off by the local magnetic variation (about 15° in the US northwest): on the web, in exports, for peers' traffic, and when matching the landing to a runway. The X-Plane plugin still gets traffic attitude in X-Plane's own signs.
- X-Plane radio frequencies are logged in MHz (122.800) instead of a millionth of the right value (0.1228).
- X-Plane oil temperature, manifold pressure and percent power are logged again. They were read from dataref names X-Plane 12 doesn't have (`oil_temp_deg_C`, `MP_in_hg`, `ENGHP`), so they were always zero. CHT and EGT now come from the datarefs that are always in °C; the ones read before are in °F on some aircraft.
- X-Plane helicopters and gliders are recognised as such. The class was read from `acf_class`, which X-Plane 12 doesn't have, so every aircraft was an airplane.
- The aircraft title now outweighs the livery when deducing the ICAO type. FSReborn liveries end in a studio code such as "(S108)", which is the Stinson 108's type code, so the Phenom 300E was logged as an S108. Livery words now count for a quarter of title words; they still decide when the title names no type. "Phenom 300E" also resolves to E55P instead of tying with the Phenom 100.
- No operating airline is deduced for pistons or light (WTC Light) turboprops such as the Caravan, King Air, PC-12 and TBM. A livery word that happens to match an airline name is noise on a privately flown aircraft. Regional turboprops (ATR, Dash 8, Saab 340, Beech 1900) still get one.
- Lancair Legacy titles such as "LEGACY G3X GTN750 TURBO" now resolve to LEG2 instead of the Embraer E135. "Legacy" alone means the Lancair; Embraer Legacy titles still resolve from their model number (600, 450, 500).
- Flights no longer start at 0,0 when MSFS is still loading. While it loads, MSFS parks the aircraft at "null island" with coordinates that are only *nearly* zero, which slipped past the exact-zero checks; those samples were logged and the departure came out as "Airborne" instead of the airport you spawned at. Any position within ~1 km of 0,0 is now treated as no position yet.
- No more empty "Airborne → Airborne" flights after loading a new flight in MSFS. A loading sample (0,90 at an indicated -9.4 million ft) got past the null-island check and started a log. That log also inherited the previous flight's takeoff time, so it went to the service immediately. Samples indicating below -5,000 ft are now ignored. Starting a log on movement now clears the previous flight's takeoff and landing state, as SimStart already did.
- A touch-and-go, go-around or bounce no longer becomes the flight's takeoff. Leaving the ground again after a touchdown replaced the takeoff time and position with that moment at the arrival field, so the distance reported to the service was measured from there: a KTYS-KTHA Baron with a touch-and-go at KTHA came out at 0.4 nm and 12 minutes airborne instead of about 110 nm and 57 minutes. The first takeoff of a flight is now kept, on MSFS and X-Plane alike, and on a resumed flight too.
- Flight logs with fewer than 60 samples (about a minute of movement) are now deleted when the flight closes. The existing "shorter than 2 minutes" check measures up to the moment of closing, so one stray sample followed by a long idle stretch used to survive it.
- "Start automatically on login" now actually starts the app on Windows. Older builds left stale startup entries (`net.flyingbutter.butterlog`, `net.flyvoyager.butterlog`) pointing at an exe that no longer exists, and Windows stopped processing startup apps when it reached them. The app now removes them on launch.
- Waiting for MSFS no longer leaks a socket per attempt. Availability is probed by a short-lived child process (`butterlog --probe-simconnect`) that opens a SimConnect client, reports the result as its exit code and exits, so whatever the SDK holds onto after a failed open goes back to the OS with that process. The app only connects in-process once the probe says the sim is answering.

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


