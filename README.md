# Limbo

A lightweight browser for Windows 11. A small Rust host (Tauri 2) wraps the
Microsoft Edge WebView2 engine that already ships with Windows, with its own
quiet UI, an aggressive memory saver, Google everywhere, Chrome extensions,
and a full import from Firefox. Built for a 4 GB laptop.

- **Memory saver:** background tabs slow down, sleep, then unload; a maximum
  number of awake tabs; a live memory readout in the toolbar.
- **Google:** the address bar searches Google, suggests from Google and your
  history, and the new tab page is a search box.
- **Firefox import:** passwords (including primary-password profiles),
  history, bookmarks, cookies, form entries, site icons, open tabs, and a list
  of your add-ons with their Chrome equivalents.
- **Chrome extensions (MV3):** "Add to Limbo" on Chrome Web Store and Edge
  Add-ons pages, toolbar popups, options, updates.
- **Passwords:** Limbo's own vault (Windows DPAPI), autofill, save prompts,
  Windows Hello before revealing.

Status: all features are implemented. The core logic and the UI (against a
mock host) are tested on Linux; the Windows app builds, lints clean and
packages into a 2.7 MB installer in CI, but hasn't been run interactively on
Windows yet: the Phase 0 spikes in [docs/DECISIONS.md](docs/DECISIONS.md) and
the memory budgets in [docs/MEMORY_REPORT.md](docs/MEMORY_REPORT.md) need the
owner's laptop.

## Get the installer

Every push builds a Windows installer in CI: open the latest run under
**Actions → CI**, then download the `limbo-installer` artifact
(`Limbo_0.1.0_x64-setup.exe`). It installs for the current user, no admin
needed.

## Portable (PortableApps.com Format)

CI also builds **Limbo Portable** for the PortableApps.com Platform:

- `limbo-portable-paf` artifact: `LimboPortable_0.1.0_English.paf.exe` (2.8 MB), the standard
  PortableApps.com installer (Platform → Apps → Install a new app).
- `limbo-portable-folder` artifact: the same package as a folder
  (`LimboPortable\`), for copying into the Platform's `PortableApps` folder
  (then Apps → Refresh app icons).

Limbo Portable keeps everything in its `Data` folder and writes nothing to the
registry or `%LOCALAPPDATA%`. It uses the WebView2 runtime built into Windows
11 and can't be the default browser. The launcher is
`packaging/portableapps/launcher`; the package's metadata, icons and help page
are in `packaging/portableapps/LimboPortable`.

## Layout

| Path | What |
|---|---|
| `crates/limbo-core` | Everything that doesn't need Windows: omnibox, suggestions, frecency, tab lifecycle, SQLite storage, password vault format, Firefox import and decryption, CRX verification. Builds and tests on any OS. |
| `src-tauri` | The Windows host: window, tabs, WebView2 integration, memory readout, passwords, extensions, downloads, IPC. |
| `ui` | The browser chrome: Svelte 5 + TypeScript, plain CSS, no UI framework. Runs in a normal browser against a mock host. |
| `packaging/portableapps` | The PortableApps.com Format package: `LimboPortable.exe` launcher (Rust) and the package files. |
| `tools` | `measure-memory.ps1`, the icon generator, Firefox test data, a Windows type-check from Linux. |
| `docs` | [Decisions](docs/DECISIONS.md), [memory report](docs/MEMORY_REPORT.md), [manual test script](docs/MANUAL_TEST.md). |

## Build on Windows

Prerequisites: Windows 10/11 with the WebView2 runtime (preinstalled on
Windows 11), [Rust](https://rustup.rs) (the toolchain in
`rust-toolchain.toml` installs itself), Node.js 22, and Visual Studio Build
Tools with "Desktop development with C++".

```powershell
cd ui
npm ci
npm run tauri -- dev      # run with hot reload
npm run tauri -- build    # installer in target\release\bundle\nsis\
```

Debug builds include DevTools; build with `--features devtools` to keep them
in a release build.

## Develop anywhere

```sh
cargo test                        # core tests (any OS)
cargo clippy --all-targets        # core lints

cd ui
npm ci
npm run dev                       # the UI at http://localhost:5173 with a mock host
npm run check && npm test         # types and unit tests
npm run e2e                       # Playwright flows against the mock
npm run build                     # production bundle, fails over budget

tools/xcheck-windows.sh           # type-check the Windows host from Linux/macOS
tools/fetch-firefox-testdata.sh   # real Firefox profiles for the decryption tests
```

Open `http://localhost:5173/?onboarding=1` to see the first-run flow. In the
browser console, `__limboMock.emit(event, payload)` plays the host's part (for
example a `context-menu` or `perm:request` event).

## Measure memory

```powershell
tools\measure-memory.ps1 -Detail
```

Sums the private working set of `limbo.exe` and Limbo's own
`msedgewebview2.exe` processes, the same number the toolbar shows.
