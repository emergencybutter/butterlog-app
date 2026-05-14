# Butterlog App User Manual

Butterlog is a flight logging and analysis tool for flight simulators (Microsoft Flight Simulator and X-Plane). It automatically tracks your flights, records landing performance, and manages your flight screenshots.

---

## Getting Started

### Installation

Before installing 0.3.x, uninstall butterlog 0.2.x. Future versions won't require such manual step.

1. Download the latest version of Butterlog.
2. Run the installer or executable.
3. On first launch, Windows might show a "SmartScreen" warning. Click "More info" and "Run anyway".

### Connecting to your Simulator

Butterlog should detect and connect to your simulator automatically. Butterlog supports **Microsoft Flight Simulator 2020 and 2024 (SimConnect)** and **X-Plane 12 (REST API)**.

### Import G1000 garmin logs

You can import G1000 logs either from real worl flights or previous versions of butterlog. Click on import and select all the files you want to import.

---

## Flight Monitoring

Once connected, Butterlog stays in the background and watches for flight activity.

### Observing flights in the app

- **Automatic Logging**: A new flight log starts automatically when you begin a flight.
- **Live Metrics**: The main window shows real-time data including Altitude, Ground Speed, Vertical Speed, and OAT.
- **Landing Analysis**: Butterlog specifically focuses on your touchdown performance, recording your vertical speed and G-force at the moment of landing.

### Observing flights in flysto.net or cloudahoy

Butterlog supports exporting files to a standard garmin format that is recognized by real world flight analyzers. Flysto.net received reasonable testing, less so cloudahoy, but it should work too. Once on the flightdetail page, click on export. The app should produce a csv file and open a window pointing to it. You can upload that file to flysto.net as you would normally.

---

## Settings & Configuration

Access the **Settings** menu to customize your experience:

- **General**:
  - **Start Minimized**: Launches Butterlog to the system tray.
  - **Run on System Startup**: Automatically starts Butterlog when you log into Windows.
- **Screenshots**:
  - **Screenshot Directory**: Point this to where your simulator saves screenshots.
  - **Enable Regex Matching**: Filter files to ensure only simulator screenshots are captured.
  - **Auto-upload Screenshots**: Automatically sends new screenshots to your configured webhook service.

---

## Screenshot Management

Butterlog links your screenshots to specific flights based on when they were taken.

### Automatic Capture

If configured, Butterlog watches your screenshot folder. When you take a screenshot in-sim:

1. Butterlog detects the new file.
2. It records the current aircraft, location (lat/lon), and flight ID.
3. The screenshot appears in the **Flight Details** view.

---

## Webhook Integration

Connect Butterlog to your personal dashboard or community server using Webhooks. Currently only voyager aviation is supported (https://flyvoyager.net/). Contact @emergencybutter on discord if you want to integrate your discord server or other backend.

1. Enable **Webhook Service** in Settings.
2. go to https://butterlog.flyvoyager.net/api then follow instructions. In particular you will need to copy paste a unique URL back into the settings page.

---

## Support

Come to the (voyager aviation discord)[https://flyvoyager.net/discord] and ask for help in #butterlog.
