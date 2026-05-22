export interface FlightMetrics {
  Latitude: number;
  Longitude: number;
  AltB: number;
  BaroA: number;
  AltMSL: number;
  OAT: number;
  IAS: number;
  GndSpd: number;
  VSpd: number;
  Pitch: number;
  Roll: number;
  LatAc: number;
  NormAc: number;
  HDG: number;
  TRK: number;
  volt1: number;
  volt2: number;
  amp1: number;
  FQtyL: number;
  FQtyR: number;
  "E1 FFlow": number;
  "E1 OilT": number;
  "E1 OilP": number;
  "E1 MAP": number;
  "E1 RPM": number;
  "E1 %Pwr": number;
  "E1 CHT1": number;
  "E1 CHT2": number;
  "E1 CHT3": number;
  "E1 CHT4": number;
  "E1 CHT5": number;
  "E1 CHT6": number;
  "E1 EGT1": number;
  "E1 EGT2": number;
  "E1 EGT3": number;
  "E1 EGT4": number;
  "E1 EGT5": number;
  "E1 EGT6": number;
  "E1 TIT1": number;
  "E1 TIT2": number;
  AltGPS: number;
  TAS: number;
  HSIS: number;
  CRS: number;
  NAV1: number;
  NAV2: number;
  COM1: number;
  COM2: number;
  HCDI: number;
  VCDI: number;
  WndSpd: number;
  WndDr: number;
  WptDst: number;
  WptBrg: number;
  MagVar: number;
  AfcsOn: number;
  RollM: number;
  PitchM: number;
  RollC: number;
  PichC: number;
  VSpdG: number;
  GPSfix: number;
  HAL: number;
  VAL: number;
  HPLwas: number;
  HPLfd: number;
  VPLwas: number;
  sim_on_ground: number;
  altitude_agl: number;
  gforce: number;
}

export interface FlightEvent {
  timestamp: string;
  eventType: 'takeoff' | 'landing' | 'top_of_climb' | 'top_of_descent' | 'autopilot_on' | 'autopilot_off';
  latitude: number;
  longitude: number;
  touchdownFpm?: number;
  landingG?: number;
  offsetPercent?: number;
  thresholdDistFt?: number;
  vsVariance?: number;
  iasVariance?: number;
}

export interface FlightLogRow {
  timestamp: string;
  metrics: FlightMetrics;
}

export interface FlightSummary {
    filename: string;
    startIcao: string;
    startAirportName: string;
    endIcao: string;
    endAirportName: string;
    startTime: string;
    endTime: string;
    durationMinutes: number;
    fileSizeBytes: number;
    aircraftTitle: string;
    atcModel: string;
    atcId: string;
    maxAltitude: number;
    maxGroundSpeed: number;
    fuelConsumed: number;
    events: FlightEvent[];
    screenshotCount: number;
}
export interface Screenshot {
  id: number;
  path: string;
  timestamp: string;
  latitude: number;
  longitude: number;
  remoteHash?: string;
}

export interface Runway {
  airport_ident: string;
  length_ft: number | null;
  width_ft: number | null;
  le_ident: string | null;
  le_latitude_deg: number | null;
  le_longitude_deg: number | null;
  he_ident: string | null;
  he_latitude_deg: number | null;
  he_longitude_deg: number | null;
}
