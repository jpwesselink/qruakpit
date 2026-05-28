import { useCallback, useEffect, useState } from "react";
import { api } from "@shared/invoke";
import type {
  EventKitCalendarDto,
  IcalFeed,
  Prefs,
  ProviderStatus,
} from "@shared/types";
import { THEMES } from "@shared/themes";
import { FONTS } from "@shared/fonts";
import { HEADS, PLANE_COLORS } from "@shared/fliers";

export default function SettingsApp() {
  const [prefs, setPrefs] = useState<Prefs | null>(null);
  const [providers, setProviders] = useState<ProviderStatus[]>([]);
  const [feeds, setFeeds] = useState<IcalFeed[]>([]);
  const [eventkitCals, setEventkitCals] = useState<EventKitCalendarDto[]>([]);
  const [eventkitError, setEventkitError] = useState<string | null>(null);
  const [newFeedUrl, setNewFeedUrl] = useState("");

  const reloadAll = useCallback(async () => {
    const [p, s, f] = await Promise.all([
      api.prefsGet(),
      api.calStatus(),
      api.icalList(),
    ]);
    setPrefs(p);
    setProviders(s);
    setFeeds(f);
  }, []);

  useEffect(() => {
    void reloadAll();
  }, [reloadAll]);

  // Auto-load EventKit calendars if access was already granted in a previous run.
  useEffect(() => {
    api
      .eventkitListCalendars()
      .then(setEventkitCals)
      .catch(() => {
        /* not yet granted; user can click "Grant access" */
      });
  }, []);

  const update = useCallback(
    async (patch: Partial<Prefs>) => {
      const next = await api.prefsSet(patch);
      setPrefs(next);
    },
    []
  );

  const connectEventKit = useCallback(async () => {
    setEventkitError(null);
    try {
      const next = await api.calConnect("eventkit");
      setProviders(next);
      const list = await api.eventkitListCalendars();
      setEventkitCals(list);
    } catch (e) {
      setEventkitError(String(e));
    }
  }, []);

  const toggleEventKitCal = useCallback(
    async (id: string, on: boolean) => {
      const next = eventkitCals.map((c) =>
        c.id === id ? { ...c, enabled: on } : c
      );
      setEventkitCals(next);
      const ids = next.filter((c) => c.enabled).map((c) => c.id);
      const statuses = await api.eventkitSetEnabled(ids);
      setProviders(statuses);
    },
    [eventkitCals]
  );

  const addFeed = useCallback(async () => {
    if (!newFeedUrl.trim()) return;
    try {
      const next = await api.icalAdd(newFeedUrl.trim());
      setFeeds(next);
      setNewFeedUrl("");
    } catch (e) {
      alert(`Failed to add feed: ${e}`);
    }
  }, [newFeedUrl]);

  const removeFeed = useCallback(async (id: string) => {
    const next = await api.icalRemove(id);
    setFeeds(next);
  }, []);

  if (!prefs) return <div className="app"><header className="top"><h1>Loading…</h1></header></div>;

  const eventkit = providers.find((p) => p.id === "eventkit");

  return (
    <div className="app">
      <header className="top">
        <h1>Qruakpit</h1>
        <p className="sub">Plane meeting reminder</p>
      </header>

      <main>
        <section className="card">
          <h2>Calendars</h2>

          <div className="row">
            <div className="label">
              <span className="name">{eventkit?.name ?? "macOS Calendar"}</span>
              <span className="detail">
                {eventkit?.detail ?? "Reads from Calendar.app (Outlook, Exchange, Google, iCloud, ...)"}
              </span>
            </div>
            <button onClick={connectEventKit}>
              {eventkit?.connected ? "Refresh" : "Grant access"}
            </button>
          </div>
          {eventkitError && (
            <div className="error">
              {eventkitError}{" "}
              {eventkitError.toLowerCase().includes("denied") && (
                <button
                  className="secondary"
                  onClick={() =>
                    void api.openExternal(
                      "x-apple.systempreferences:com.apple.preference.security?Privacy_Calendars"
                    )
                  }
                  style={{ marginLeft: 8 }}
                >
                  Open Privacy settings
                </button>
              )}
            </div>
          )}

          {eventkitCals.length > 0 && (
            <div className="eventkit-list">
              {eventkitCals.map((c) => (
                <label key={c.id}>
                  <input
                    type="checkbox"
                    checked={c.enabled}
                    onChange={(e) => toggleEventKitCal(c.id, e.target.checked)}
                  />
                  <span>{c.title}</span>
                  <span className="src">{c.source}</span>
                </label>
              ))}
            </div>
          )}
        </section>

        <section className="card">
          <h2>iCal subscription URLs</h2>
          {feeds.length === 0 && (
            <div className="row">
              <span className="detail">
                Paste any .ics URL (works for Outlook "Publish a calendar" links, public Google calendars, etc.)
              </span>
            </div>
          )}
          {feeds.map((f) => (
            <div key={f.id} className="row">
              <div className="label">
                <span className="name">{f.name}</span>
                <span className="detail">{f.url}</span>
              </div>
              <button className="danger" onClick={() => removeFeed(f.id)}>
                Remove
              </button>
            </div>
          ))}
          <div className="ical-add">
            <input
              type="text"
              placeholder="https://example.com/calendar.ics"
              value={newFeedUrl}
              onChange={(e) => setNewFeedUrl(e.target.value)}
            />
            <button onClick={addFeed}>Add</button>
          </div>
        </section>

        <section className="card">
          <h2>Behavior</h2>
          <div className="row">
            <div className="label">
              <span className="name">Lead time (minutes)</span>
              <span className="detail">How early to fly the plane before each meeting.</span>
            </div>
            <input
              type="number"
              min={1}
              max={60}
              value={prefs.leadMinutes}
              onChange={(e) => update({ leadMinutes: Number(e.target.value) })}
            />
          </div>
          <div className="row">
            <div className="label">
              <span className="name">Banner message</span>
              <span className="detail">{"{title} and {minutes} are filled in."}</span>
            </div>
            <input
              type="text"
              value={prefs.messageTemplate}
              onChange={(e) => update({ messageTemplate: e.target.value })}
              style={{ minWidth: 300 }}
            />
          </div>
          <div className="row">
            <div className="label">
              <span className="name">Fly at start time too</span>
              <span className="detail">Second fly-by when a meeting starts.</span>
            </div>
            <input
              type="checkbox"
              checked={prefs.flyAtStart}
              onChange={(e) => update({ flyAtStart: e.target.checked })}
            />
          </div>
          <div className="row">
            <div className="label">
              <span className="name">Sound</span>
              <span className="detail">Engine drone + quack mid-flight.</span>
            </div>
            <input
              type="checkbox"
              checked={prefs.soundEnabled}
              onChange={(e) => update({ soundEnabled: e.target.checked })}
            />
          </div>
          <div className="row">
            <div className="label">
              <span className="name">Target display</span>
              <span className="detail">Where the plane flies.</span>
            </div>
            <select
              value={prefs.targetDisplay}
              onChange={(e) => update({ targetDisplay: e.target.value as Prefs["targetDisplay"] })}
            >
              <option value="cursor">Display with cursor</option>
              <option value="primary">Primary display</option>
            </select>
          </div>
          <div className="row">
            <div className="label">
              <span className="name">Launch at login</span>
              <span className="detail">Start Qruakpit when you log in.</span>
            </div>
            <input
              type="checkbox"
              checked={prefs.launchAtLogin}
              onChange={(e) => update({ launchAtLogin: e.target.checked })}
            />
          </div>
        </section>

        <section className="card">
          <h2>Look</h2>
          {THEMES.length > 1 && (
            <div className="row">
              <div className="label">
                <span className="name">Banner theme</span>
              </div>
              <select
                value={prefs.theme}
                onChange={(e) => update({ theme: e.target.value })}
              >
                {THEMES.map((t) => (
                  <option key={t.id} value={t.id}>
                    {t.name}
                  </option>
                ))}
              </select>
            </div>
          )}
          <div className="row">
            <div className="label">
              <span className="name">Font</span>
            </div>
            <select
              value={prefs.font}
              onChange={(e) => update({ font: e.target.value })}
            >
              {FONTS.map((f) => (
                <option key={f.id} value={f.id}>
                  {f.name}
                </option>
              ))}
            </select>
          </div>
          {HEADS.length > 1 && (
            <div className="row">
              <div className="label">
                <span className="name">Character</span>
              </div>
              <select
                value={prefs.flierHead}
                onChange={(e) => update({ flierHead: e.target.value })}
              >
                {HEADS.map((h) => (
                  <option key={h.id} value={h.id}>
                    {h.name}
                  </option>
                ))}
              </select>
            </div>
          )}
          {PLANE_COLORS.length > 1 && (
            <div className="row">
              <div className="label">
                <span className="name">Plane colour</span>
              </div>
              <select
                value={prefs.flierColor}
                onChange={(e) => update({ flierColor: e.target.value })}
              >
                {PLANE_COLORS.map((c) => (
                  <option key={c.id} value={c.id}>
                    {c.name}
                  </option>
                ))}
              </select>
            </div>
          )}
        </section>
      </main>

      <div className="test-flight">
        <button onClick={() => void api.flightTest()}>Send test flight</button>
      </div>
    </div>
  );
}
