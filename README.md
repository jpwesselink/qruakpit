# Qruakpit

A little duck in a plane flies across your screen, above every app, towing a banner reminding you of your next meeting. Rust + Tauri fork of [Ooble Studio's Quakpit](https://github.com/Ooble-Studio/QuakPit).

## Differences from upstream

- Rust + Tauri v2 instead of Electron (smaller binary, native menu bar app).
- React + Vite frontend instead of vanilla TS.
- **Reads from macOS Calendar.app via EventKit** instead of a Microsoft Graph OAuth flow, so company Outlook accounts work without any Azure AD app registration. Whatever account you have set up in Calendar.app (Outlook/Exchange, Google, iCloud, Local) shows up automatically.
- macOS only.

This fork stays strictly within Quakpit's open-source free feature set:
one banner theme, one character, one plane colour, one sound pack, one
calendar source at a time. Upstream's paid Pro tier (extra themes, characters,
colours, sound packs, flight speeds, custom flier image, multi-calendar) is
**not** duplicated here. If you want those, buy a Pro license from
[Ooble Studio](https://ooble.studio).

## Calendars supported

- **macOS Calendar.app (EventKit)** — the primary path. Pick this for company Outlook / Exchange / Office 365 accounts where you can't register an OAuth app: add the account once in System Settings -> Internet Accounts and Qruakpit will read it. Also handles Google, iCloud and other accounts you have in Calendar.app.
- **iCal subscription URLs** — paste any `.ics` URL. Works for Outlook "Publish a calendar" links, public Google calendars, etc.
- **Google Calendar** (OAuth, optional) — for personal Google calendars you don't want in Calendar.app.
- **iCloud (CalDAV)** (optional) — for iCloud calendars you don't want in Calendar.app.

## Status

Working: scaffold, vendored assets, NSPanel overlay window, scheduler, tray + global shortcut + auto-launch + updater wiring, iCal provider, EventKit provider, React overlay animation + audio, React settings UI. `cargo check` and `pnpm build` both green.

Stubbed (returns empty results, no auth flow yet): Google OAuth, iCloud CalDAV. These are layered on top of working EventKit access, which on a managed-Mac setup is usually sufficient on its own.

## Develop

Requires Node 20+, pnpm 9+, Rust 1.77+, Xcode command-line tools.

```bash
pnpm install
pnpm tauri:dev
```

On first calendar use macOS will prompt for Calendar access. If you accidentally deny, re-enable via System Settings -> Privacy & Security -> Calendars -> Qruakpit.

## License

MIT. See `LICENSE`. Upstream Quakpit license preserved as `LICENSE-quakpit`. Attribution to Ooble Studio in `NOTICE`.
