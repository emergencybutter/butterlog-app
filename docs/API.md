# Butterlog Webhook API Documentation

This document describes the Web API for Butterlog, designed for consumption by both humans and agents.

## Base URL
`https://butterlog.flyvoyager.net/` (or your local deployment URL)

## Authentication

The API supports two authentication methods:

1.  **Session-based (Passport Discord)**: Used by the web frontend. User is authenticated via Discord OAuth2.
2.  **Webhook Token**: Used by automated clients (agents). The `:webhookToken` is part of the URL path. If a token is unknown, a new user account is automatically created for it.

---

## Endpoints

### Flight Management

#### Create a Flight
`POST /users/:webhookToken/flights`

Creates a new flight entry.

*   **Path Parameters:**
    *   `webhookToken`: Your unique authentication token.
*   **Request Body (JSON):**
    *   `departure` (string): ICAO code of the departure airport (e.g., "KLAX").
    *   `statistics` (object): A `FlightSummary` object (see [Data Structures](#data-structures)).
*   **Response:**
    *   `201 Created`: Returns the created `Flight` object.
    *   `400 Bad Request`: Missing required fields.
    *   `401 Unauthorized`: Invalid or missing authentication.

#### Update a Flight
`PUT /users/:webhookToken/flights/:id`

Updates an existing flight (e.g., when it lands or progress is made).

*   **Path Parameters:**
    *   `webhookToken`: Your unique authentication token.
    *   `id` (number): The database ID of the flight.
*   **Request Body (JSON):**
    *   `arrival` (string, optional): ICAO code of the arrival airport.
    *   `statistics` (object): An updated `FlightSummary` object.
*   **Response:**
    *   `200 OK`: Returns the updated `Flight` object.
    *   `404 Not Found`: Flight ID does not exist for this user.

#### Get Flight Details
`GET /users/:webhookToken/flights/:id`

Retrieves a specific flight's data.

*   **Path Parameters:**
    *   `webhookToken`: Your unique authentication token.
    *   `id` (number): The database ID of the flight.
*   **Response:**
    *   `200 OK`: Returns the `Flight` object.
    *   `404 Not Found`: Flight ID does not exist for this user.

---

### Screenshot Management

#### Upload a Screenshot
`POST /users/:webhookToken/flights/:id/screenshots`

Uploads an image for a specific flight. Images are automatically resized to 1600px width and compressed as optimized JPEGs.

*   **Path Parameters:**
    *   `webhookToken`: Your unique authentication token.
    *   `id` (number): The database ID of the flight.
*   **Request Body (Multipart/Form-Data):**
    *   `screenshot` (file): The image file to upload.
*   **Response:**
    *   `201 Created`: Returns `{ "hash": "sha256-hash-of-processed-image" }`.
    *   `400 Bad Request`: No file uploaded.
    *   `404 Not Found`: Flight not found.

#### Delete a Screenshot
`DELETE /users/:webhookToken/flights/:id/screenshots/:hash`

Removes a screenshot from a flight.

*   **Path Parameters:**
    *   `webhookToken`: Your unique authentication token.
    *   `id` (number): The database ID of the flight.
    *   `hash` (string): The SHA-256 hash of the screenshot.
*   **Response:**
    *   `204 No Content`: Successfully deleted.
    *   `404 Not Found`: Flight or screenshot not found.

---

### Discord Configuration (Requires Session Auth)

*   `GET /discord-notification-channels`: List configured Discord channel IDs.
*   `POST /discord-notification-channels`: Add a new channel (`{ "channelId": "..." }`).
*   `DELETE /discord-notification-channels/:channelId`: Remove a channel.

---

## Data Structures

### Flight Object
```json
{
  "id": 123,
  "user_id": 1,
  "departure": "KLAX",
  "arrival": "KSFO",
  "statistics": { ... },
  "screenshots": ["hash1", "hash2"]
}
```

### FlightSummary Object
The `statistics` field contains detailed flight data:
```json
{
  "log_path": "string",
  "airframe_name": "string",
  "departure": { "icao": "KLAX", "name": "Los Angeles Intl" },
  "arrival": { "icao": "KSFO", "name": "San Francisco Intl" },
  "takeoff_time": "ISO8601 Date String or null",
  "landing_time": "ISO8601 Date String or null",
  "start_time": "ISO8601 Date String or null",
  "end_time": "ISO8601 Date String or null",
  "takeoff_snapshot": object | null,
  "landing_snapshot": object | null,
  "current_snapshot": object | null,
  "max_entries": object | null
}
```
