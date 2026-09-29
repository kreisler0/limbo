# Memory report

Budgets from the build plan (6.1) and the numbers measured against them.
**Nothing below has been measured on Windows yet:** the numbers need the
owner's laptop (Windows 11, 4 GB RAM). Fill in the "Measured" column with the
procedure below and keep the raw CSV next to this file.

## Metric

Sum of the **private working set** (Task Manager's "Memory" column) of
`limbo.exe` plus every `msedgewebview2.exe` whose command line contains
Limbo's profile folder (`%LOCALAPPDATA%\Limbo\Profile`). Other apps that use
WebView2 (Widgets, Teams, Outlook) are excluded. The in-app memory pill uses
the same metric, so the two should agree within 5%.

```powershell
tools\measure-memory.ps1 -Detail                     # one reading, per process
tools\measure-memory.ps1 -Samples 15 -Interval 2 `
  -Csv docs\memory.csv -Label "10 tabs, balanced"    # 30 s of readings
```

Report the **median** of 15 samples taken after the scenario has settled.

## Budgets

| Scenario | Budget | Measured | Status |
|---|---|---|---|
| Cold start, new tab page, no extensions | ≤ 160 MB | – | not measured |
| One tab on google.com search results | ≤ 220 MB | – | not measured |
| 10 tabs of typical sites, 1 active, 9 idle > 5 min | ≤ 450 MB | – | not measured |
| Same + uBlock Origin Lite + password autofill active | ≤ 480 MB | – | not measured |
| Host `limbo.exe` alone | ≤ 25 MB | – | not measured |
| UI webview renderer alone | ≤ 45 MB | – | not measured |
| In-app readout vs this script | within 5% | – | not measured |
| Readout overhead | < 1 MB, < 0.5% CPU | – | not measured |
| Idle CPU after 10 s without input | ~0% | – | not measured |
| Cold start to interactive omnibox | ≤ 800 ms | – | not measured |
| Installer | ≤ 10 MB | 2.74 MB (CI, NSIS) | ✅ pass |

## Procedure

Before each run: restart Windows, wait 2 minutes, close other apps, plug in
the charger (power plans change timer behaviour). Use Settings → Memory saver
→ Balanced unless a row says otherwise.

1. **Cold start.** Settings → General → turn off "Continue where you left
   off". Close Limbo, wait 10 s, start it. Wait 20 s. Measure.
2. **One Google tab.** From (1), type `weather` in the address bar, press
   Enter. Wait 20 s. Measure.
3. **10 tabs.** Open these, one per tab: google.com search results,
   gmail.com (signed in), youtube.com (not playing), github.com,
   en.wikipedia.org/wiki/Windows_11, news.ycombinator.com, reddit.com,
   docs.rs, amazon.com, calendar.google.com. Activate the first tab and leave
   the others alone for 6 minutes (Balanced puts them to sleep after 5).
   Measure.
4. **10 tabs + uBlock Origin Lite + autofill.** Install uBlock Origin Lite
   from the Chrome Web Store (address bar → "Add to Limbo"). Save one GitHub
   login and let autofill fill it once. Repeat (3). Measure.
5. **Host alone / UI renderer alone.** The `-Detail` table from (1): the
   `host` row, and the `renderer` row whose memory doesn't change as you open
   and close tabs.
6. **Readout vs script.** In (3), open the memory popover (Ctrl+Shift+M) and
   run the script at the same moment, three times. Largest difference, in %.
7. **Readout overhead.** In (3), measure with the pill hidden (Settings →
   Memory saver → "Show memory in the toolbar" off) and with the popover open.
   Task Manager → Details → `limbo.exe` CPU column over 60 s.
8. **Idle CPU.** Any scenario: stop touching the laptop for 10 s, then watch
   Task Manager's CPU column for `limbo.exe` and `msedgewebview2.exe` (Limbo's
   only) for 60 s.
9. **Cold start time.** Screen-record at 60 fps: from double-clicking the
   Limbo shortcut to the first frame where typing in the address bar shows
   suggestions. Median of 5.

## Measured elsewhere

Numbers that don't need the laptop, from the development container (Linux)
and CI (GitHub's `windows-latest`):

| What | Result | Budget |
|---|---|---|
| UI bundle, gzip | 48 KB JS, 9 KB CSS | ≤ 150 KB JS, ≤ 40 KB CSS |
| Installer (`Limbo_0.1.0_x64-setup.exe`, CI on `windows-latest`) | 2.74 MB | ≤ 10 MB |
| Local omnibox suggestions, 100k history rows (`history::tests::suggestions_are_fast_with_100k_rows`, release build) | ≤ 17 ms max | ≤ 30 ms (re-measure on the laptop) |

## Findings

_Record what was over budget, what you profiled
(`edge://process-internals` isn't reachable from WebView2; use DevTools →
Memory on the UI webview with `--features devtools`), and what changed._
