# Limbo

A lightweight, custom browser for Windows: a small Rust host (Tauri 2) around
the Microsoft Edge WebView2 engine that ships with Windows, with its own
minimal UI, an aggressive memory saver, Google everywhere, Chrome extensions,
and a full import from Firefox.

Work in progress. See `docs/` for decisions and status.

## Layout

| Path | What |
|---|---|
| `crates/limbo-core` | Platform-independent logic (omnibox, storage, memory policy, Firefox import, extension packaging). Tested on any OS. |
| `src-tauri` | The Windows host: window, tabs, WebView2 integration. |
| `ui` | The browser chrome (Svelte 5 + TypeScript, no CSS framework). |
| `tools` | Memory measurement, icon generation, test-data helpers. |
| `docs` | Decisions, memory report, manual test script. |

## Develop

```sh
cargo test                       # core tests (any OS)
tools/fetch-firefox-testdata.sh  # decrypt real Firefox test profiles
```
