import { invoke } from "@tauri-apps/api/core";
import type { Prefs, ProviderStatus, UpcomingEvent, IcalFeed, EventKitCalendarDto } from "./types";

export const api = {
  prefsGet: () => invoke<Prefs>("prefs_get"),
  prefsSet: (patch: Partial<Prefs>) => invoke<Prefs>("prefs_set", { patch }),

  calStatus: () => invoke<ProviderStatus[]>("cal_status"),
  calConnect: (provider: string, params?: { username?: string; password?: string }) =>
    invoke<ProviderStatus[]>("cal_connect", { provider, params: params ?? {} }),
  calDisconnect: (provider: string) =>
    invoke<ProviderStatus[]>("cal_disconnect", { provider }),
  calConfigure: (provider: string, params: { clientId?: string; clientSecret?: string }) =>
    invoke<ProviderStatus[]>("cal_configure", { provider, params }),

  icalList: () => invoke<IcalFeed[]>("ical_list"),
  icalAdd: (url: string, name?: string) => invoke<IcalFeed[]>("ical_add", { url, name }),
  icalRemove: (id: string) => invoke<IcalFeed[]>("ical_remove", { id }),

  eventsUpcoming: () => invoke<UpcomingEvent[]>("events_upcoming"),

  flightTest: () => invoke<boolean>("flight_test"),

  openExternal: (url: string) => invoke<void>("open_external", { url }),

  eventkitListCalendars: () => invoke<EventKitCalendarDto[]>("eventkit_list_calendars"),
  eventkitSetEnabled: (ids: string[]) =>
    invoke<ProviderStatus[]>("eventkit_set_enabled", { ids }),
};
