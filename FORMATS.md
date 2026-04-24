# Internal log file format

A flight log file is a sqlite3 database. It contains a table `metrics` with a row per log entry, and a column for each data we collect.
Another table in the same file `summary` contains additional data that we compute or determine once. For instance:

* Departure aiport
* Arrival airport
* Aircraft type
* Aircraft full name

These logs are internal, they are stored in the `flightlogs` subdirectory of the app directory.


# Export Log file format (G1000)

For each flight, a csv file is produce. The CSV file has a specific format described below.

First two lines of CSV comments contain some values. ${airframeName} below needs to be substituted with the name airframe doing the flight.
```
#airframe_info, log_version="1.00", airframe_name="${airframeName}", unit_software_part_number="006-BXXX9-DE", unit_software_version="15.24", system_software_part_number="006-BXXXX-37", system_id="25XXXX67", mode=NORMAL, simulator_id="ButterLogV2",`
```
The second line describes the types of each column.
```
#yyy-mm-dd, hh:mm:ss,   hh:mm,  ident,      degrees,      degrees, ft Baro,  inch,  ft msl, deg C,     kt,     kt,     fpm,    deg,    deg,      G,      G,   deg,   deg, volts, volts,  amps,   gals,   gals,      gph,   deg F,     psi,     Hg,    rpm,       %,   deg F,   deg F,   deg F,   deg F,   deg F,   deg F,   deg F,   deg F,   deg F,   deg F,   deg F,   deg F,   deg F,   deg F,  ft wgs,  kt, enum,    deg,    MHz,    MHz,     MHz,     MHz,    fsd,    fsd,     kt,   deg,     nm,    deg,    deg,   bool,  enum,   enum,   deg,   deg,   fpm,   enum,   mt,    mt,     mt,    mt,     mt\n`
```
The third line, not a comment is the CSV header, with the name of each column.
```
Lcl Date, Lcl Time, UTCOfst, AtvWpt,     Latitude,    Longitude,    AltB, BaroA,  AltMSL,   OAT,    IAS, GndSpd,    VSpd,  Pitch,   Roll,  LatAc, NormAc,   HDG,   TRK, volt1, volt2,  amp1,  FQtyL,  FQtyR, E1 FFlow, E1 OilT, E1 OilP, E1 MAP, E1 RPM, E1 %Pwr, E1 CHT1, E1 CHT2, E1 CHT3, E1 CHT4, E1 CHT5, E1 CHT6, E1 EGT1, E1 EGT2, E1 EGT3, E1 EGT4, E1 EGT5, E1 EGT6, E1 TIT1, E1 TIT2,  AltGPS, TAS, HSIS,    CRS,   NAV1,   NAV2,    COM1,    COM2,   HCDI,   VCDI,WndSpd,WndDr, WptDst, WptBrg, MagVar, AfcsOn, RollM, PitchM, RollC, PichC, VSpdG, GPSfix,  HAL,   VAL, HPLwas, HPLfd, VPLwas
```
What follows is normal CSV data.

Lcl Date and Lcl Time stand for local date and local time. UTCOfst is the utc offset. The format is described in javascript like so:
    line['Lcl Date'] = `${now.getFullYear()}-${twodigits(now.getMonth() + 1)}-${twodigits(now.getDate())}`
    line['Lcl Time'] = `${twodigits(now.getHours())}:${twodigits(now.getMinutes())}:${twodigits(now.getSeconds())}`
    line['UTCOfst'] = formatTZOffset(-now.getTimezoneOffset())


AtvWpt stands for active waypoint.
AltB is the indicated barometric altitude.
BaroA is the KOHLSMAN SETTING.
AltMSL is the altitude MSL as determined by GPS.
OAT is the outside air temperature.
IAS the indicated air speed.
GndSpd the ground speed as computed by GPS.
VSpd the vertical speed.
LatAc, NormAc are lateral and normal acceleration.
HDG is the true heading.
TRK is the true track.
FQtyL,  FQtyR, E1 FFlow are fuel quanties and flow. E1 stands for Engine 1.
HSIS is usually 'GPS', it can be set to other things but I don't really know what.
AfcsOn says whether the autopilot is on or not.
WndSpd is the  AMBIENT WIND VELOCITY
WndDr is the AMBIENT WIND DIRECTION
MagVar magnetic variation
RollM ususally set to 'NONE'
PitchM usually set to 'NONE'
GPSfix usually set to '3DDiff'


These logs are meant to be published, they are exported to the `butterlog` directory of the window `Document` directory. However this path can be overriden in settings.