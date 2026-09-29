# Decisions

Why Limbo is built the way it is, where it deviates from the build plan, and
what still has to be verified on the owner's Windows 11 laptop (4 GB RAM).
Newest entries at the bottom of each section. `[VERIFY]` marks anything that
could not be confirmed without Windows and WebView2.

## Owner answers (plan section 15)

| Question | Answer | Where it shows up |
|---|---|---|
| Laptop | Windows 11, 4 GB RAM | Memory saver defaults (Balanced, 4 awake tabs), budgets in `MEMORY_REPORT.md` |
| Name and icon | **Limbo**; liquid-glass water blob refracting a white grid, dark grey background | `tools/icon/render_icon.py` → `src-tauri/icons/*`, `ui/src/assets/limbo-mark.png` |
| Tabs on top or sidebar-first | **Tabs on top** | `settings.tab_layout = Top`; the sidebar is an optional extra (Ctrl+B) |
| Keep SmartScreen on | **No** (turn it off) | `--disable-features=msSmartScreenProtection`, `smartscreen_enabled = false`, toggle in Settings → Privacy |
| Import open Firefox tabs on first run | **Yes** | `IMPORT_FIREFOX_TABS_BY_DEFAULT = true`; "Open tabs" is checked in the import wizard |

## Architecture

**Rust core crate split out of the host.** `crates/limbo-core` holds everything
that doesn't need Windows: omnibox classification, frecency, suggestions,
the lifecycle state machine, SQLite storage, Firefox import and decryption,
CRX parsing and signature checks. It builds and tests on any OS (CI runs it on
Linux), so most logic is covered without a Windows machine. `src-tauri` is the
thin Windows host around it.

**Tauri 2 `unstable` multi-webview, one shared WebView2 environment.** The UI
webview is created first; its `ICoreWebView2Environment` is captured with
`with_webview` and passed to every tab (`WebviewBuilder::with_environment`), with
the same user data folder, browser arguments and extension flag, so all tabs
share one browser, GPU and network process. `[VERIFY spike 1]` one
`msedgewebview2.exe` browser process in Task Manager.

**Threading rules** (they prevent deadlocks we'd otherwise only find on Windows):
- WebView2 COM objects live on the UI thread. `with_webview` runs synchronously
  when already on it, so the `inner` state lock is never held across a call
  into Tauri or WebView2 (copy, drop the guard, then act).
- Webviews are created from async/worker threads, never inside a WebView2
  event handler (WebView2 re-entrancy).
- COM objects kept in `Send` contexts are wrapped in `ThreadBound<T>`.
- All SQLite work runs on one DB worker thread; the UI thread never waits on disk.

**Lifecycle driver without polling.** The state machine (`limbo-core::lifecycle`)
takes an injected millisecond clock and reports `next_deadline()`; the host
thread sleeps until then or until a signal. Idle CPU stays at ~0%.

**HTTP through `ureq` + native-tls (schannel)** instead of `reqwest`/tokio's
full stack: fewer dependencies, a smaller binary, and the Windows certificate
store. Used for CRX downloads, extension update checks and Google suggestions,
always from worker threads.

**Explicit Tauri ACL.** `build.rs` declares every command
(`AppManifest::commands`) and asserts that `capabilities/ui.json` grants them
only to the webview labelled `ui`. Tab webviews get no capability, so
`window.__TAURI_INTERNALS__` can't call anything from a web page.
`[VERIFY]` with a test page (see `MANUAL_TEST.md`, Security).

## Memory

**Metric: private working set** (Task Manager's "Memory" column), for both the
in-app readout and `tools/measure-memory.ps1`. The plan names "private working
set" for the budgets (6.1) and "private bytes" for the readout (6.3); using one
metric for both is what makes the "within 5%" gate meaningful. Private working
set is what actually occupies RAM on a 4 GB machine; private bytes also counts
memory Windows has paged out. The host reads every process in one
`NtQuerySystemInformation(SystemProcessInformation)` call (no handle per
process). `[VERIFY]` the structure offsets used (`WorkingSetPrivateSize` at 8,
`UniqueProcessId` at 0x50) on the owner's Windows build; they are stable since
Windows 7.

**Attribution.** `GetProcessExtendedInfos` maps renderer processes to frames;
frames are matched to tabs by the main frame id (`ICoreWebView2_20::FrameId`).
A renderer shared by several same-site tabs is split evenly and marked
"shared". Extension service workers can't be attributed to an extension ID
through this API, so extension memory appears only when WebView2 reports an
extension renderer; otherwise it's part of "Browser engine".

**Pressure monitor.** `CreateMemoryResourceNotification` plus
`GlobalMemoryStatusEx` with hysteresis: start discarding below 15% available,
stop above 20%, never the active tab.

**Sampling** runs only while needed: every 2 s while a memory panel is open
(popover or Settings), every 10 s for the pill alone, never while minimized or
when the pill is hidden.

## Browser behaviour

**Private tabs, not a private window.** One window keeps memory down and
matches the tabs-on-top layout. Private tabs use WebView2 InPrivate
(`incognito`), share one InPrivate session, show a violet-tinted chrome, and
are never saved to history or the session.

**Snapshot technique for everything that covers a page** (menus, popovers,
palette, dialogs): capture a JPEG, show it exactly where the page is, hide the
webview, animate the overlay; reverse on close. Menus run their action before
they close, so an action that opens another layer never flashes the live page
in between.

**Toasts are chips inside the omnibox, not bottom-centre.** A bottom toast
would sit over the live page, which HTML can't do without a snapshot swap for
every toast (and a frozen page for 4 s). The omnibox is always visible, so
toasts (zoom level, "Bookmark deleted · Undo", import results) appear
there, one at a time, for 4 s, with an undo action where it makes sense.

**Find bar is a strip below the page, not an overlay.** Find highlights must
update live while typing, so the page can't be replaced by a snapshot. The bar
takes 40 px from the bottom of the page area (the page doesn't jump, unlike a
strip at the top).

**New tab search box hands off to the omnibox.** The plan's auto-focused
Google field on the new tab page would be a second place to type with its own
suggestions. Clicking or focusing it focuses the omnibox instead (Ctrl+T also
focuses the omnibox), so suggestions, inline completion and shortcuts behave
the same everywhere.

**Sidebar docks beside the page; no 6 px hover hot zone.** A tab webview
covers the page area, so the UI never sees the mouse at the left edge unless
6 px of page are given up permanently. The sidebar opens with Ctrl+B or the
command palette and shrinks the page area (no snapshot needed, the page stays
live). It can be pinned open.

**Settings is one scrolling page** with left section links that follow the
scroll position. The memory saver section embeds the same live panel as the
memory popover.

**Tab strip layout.** Tabs, omnibox and buttons share one 44 px bar (plan 7.4).
The tabs get what's left after the omnibox (26 vw, 240–540 px). When tabs get
narrow, the active tab keeps at least 150 px for its title and background tabs
shrink to favicons (40 → 28 px); above 8 tabs the tab overview button appears.

**Context menu coordinates** from `ContextMenuRequested` are treated as
physical pixels relative to the page and divided by the device pixel ratio.
`[VERIFY]` at 125% and 150% scaling.

**Browser-process crash → restart.** If the WebView2 browser process exits,
every webview (the UI too) is gone. Limbo saves the session and restarts
itself; tabs come back unloaded, exactly like a normal start. A single crashed
renderer shows "This page crashed · Reload" for that tab only.

**Popups.** `window.open` with a size (OAuth sign-in) opens a real small
window sharing the opener's environment so `window.opener` works; everything
else opens as a tab next to the opener. There's no "popup blocked" indicator
in v1; the engine's own popup blocker still stops popups opened without a
user gesture. `[VERIFY]`

**Navigation filter.** Tab webviews can't navigate to the UI's origin, and
`file://` only when the user typed it into the omnibox.

## Security choices

- **SmartScreen off** (owner's choice, saves an extra process and network
  lookups). It can be turned back on in Settings → Privacy & security; applies
  after a restart.
- **Password vault:** DPAPI (`CryptProtectData`, current user, entropy
  "Limbo password vault v1"); plaintext only in `zeroize` buffers, never
  logged or written to disk. Reveal, copy and export ask for Windows Hello
  (`UserConsentVerifier`), falling back to the Windows password prompt
  (CredUI + `LogonUserW`) when Hello isn't set up.
- **Autofill messages:** only the top-level document can post; the sender's
  origin (`Source`) must equal the tab's current origin; unknown messages are
  dropped and each tab is rate-limited. A login is filled automatically only
  when exactly one was saved for this exact origin. Logins saved for another
  subdomain of the same site are offered in a picker and ask before filling.
  Logins in cross-origin iframes aren't filled (known issue, plan phase 6).
- **CRX3:** the SHA-256 RSA signature is verified against the key whose hash
  is the extension ID; tampered or mismatched packages are rejected. The public
  key is written into `manifest.json` so WebView2 derives the store ID (keeps
  extension data across updates). `cargo audit` ignores RUSTSEC-2023-0071
  (Marvin attack on `rsa`): it concerns private-key decryption timing, and
  Limbo only verifies signatures with public keys.
- **No telemetry.** Local log only (`Logs\limbo.log`, 5 × 1 MB); no URLs at
  info level.
- **Firefox profile is read-only.** Databases are copied (with their WAL) to a
  temp folder and read from the copy, so a running Firefox is never touched
  and never locked. (`immutable=1` would ignore the WAL and miss recent data.)
- Real-profile import tests only run on the owner's machine with consent; the
  repository's fixtures are synthetic profiles generated by the tests.

## Firefox import details

- `key4.db`: PBES2 (PBKDF2-HMAC-SHA256 + AES-256-CBC, 14-byte IV prefixed
  `04 0E`) and legacy `pbeWithSha1AndTripleDES-CBC`. Several `nssPrivate` rows
  can share `CKA_ID F8…01`; the right one is picked by key length (24 bytes
  3DES, 32 bytes AES). Verified against real Firefox 59, 144, non-ASCII,
  no-password and corrupted profiles (firefox_decrypt test data, fetched by
  `tools/fetch-firefox-testdata.sh`, not vendored).
- A primary password is asked for once, used, and dropped.
- Add-ons: each Firefox add-on is mapped to its Chrome Web Store equivalent
  from a built-in table; unknown ones get a store search button.
  `[VERIFY]` the table's Chrome IDs (`addon_map.rs`); they were compiled
  without store access.

## UI

- Svelte 5 runes, no CSS framework, no component library. Budget: ≤ 150 KB JS
  and ≤ 40 KB CSS gzip, enforced by `npm run build`. Current: 48 KB JS, 9 KB CSS.
- Spring motion solved once at startup into CSS `linear()` easings
  (snappy 400/32, smooth 260/30, gentle 170/26). Only transform and opacity
  animate; reduced motion switches to 120 ms fades.
- Muted text uses 72% opacity instead of the plan's 65% so it meets WCAG AA
  (4.5:1) on both backgrounds; links and primary buttons use a slightly
  darker accent for the same reason.
- A mock backend (`ui/src/lib/mock.ts`, never bundled into the app) plays the
  host's part, so every flow runs in a normal browser for development and
  Playwright tests.

## Phase 0 spikes (to run on the owner's laptop)

These need Windows with the WebView2 runtime. Record results here.

| # | Spike | How | Result |
|---|---|---|---|
| 1 | One browser process for UI + tabs | Open 3 tabs; `tools\measure-memory.ps1 -Detail` shows one `browser` row | pending |
| 2 | `MemoryUsageTargetLevel`, `TrySuspend`, `IsVisible`, `AcceleratorKeyPressed`, `CapturePreview` | Memory popover: Sleep/Unload a tab; Ctrl+T inside a page; open the command palette over a page | pending |
| 3 | Extensions from CRX: uBlock Origin Lite, Dark Reader, Bitwarden | Open each store page → "Add to Limbo"; open each popup | pending |
| 4 | Google sign-in incl. 2FA and "Sign in with Google" OAuth popup | Sign in at accounts.google.com, then on a third-party site | pending (stop-and-report if blocked) |
| 5 | Baseline memory for every 6.1 scenario | `MEMORY_REPORT.md` procedure | pending |
| 6 | Decrypt logins from a real profile | Import from Firefox; compare the count with `about:logins` | pending (verified on test profiles, see above) |
