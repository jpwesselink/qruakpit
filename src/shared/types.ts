export type TargetDisplay = "cursor" | "primary";

export type Prefs = {
  leadMinutes: number;
  messageTemplate: string;
  soundEnabled: boolean;
  staySignedIn: boolean;
  launchAtLogin: boolean;
  targetDisplay: TargetDisplay;
  theme: string;
  flier: string;
  font: string;
  flyAtStart: boolean;
  soundPack: string;
  flierHead: string;
  flierColor: string;
  eventkitEnabledCalendars: string[];
};

export type Flight = {
  message: string;
  durationMs: number;
  sound?: boolean;
  soundPack?: string;
  theme?: string;
  head?: string;
  color?: string;
  font?: string;
};

export type ProviderId = "google" | "icloud" | "eventkit" | "ical";

export type EventKitCalendarDto = {
  id: string;
  title: string;
  source: string;
  enabled: boolean;
};

export type ProviderStatus = {
  id: ProviderId;
  name: string;
  connected: boolean;
  detail?: string;
  configured?: boolean;
};

export type UpcomingEvent = {
  id: string;
  title: string;
  start: number; // unix ms
  end: number; // unix ms
};

export type IcalFeed = {
  id: string;
  name: string;
  url: string;
};
