# Manual test script

Run on the owner's laptop (Windows 11, 4 GB RAM) before calling a build done.
Automated tests cover the core logic (`cargo test`) and the UI against a mock
host (`npm run e2e` in `ui/`); this script covers what only the real app on
Windows can show. Mark each line ✅ / ❌ with a note. Takes about 45 minutes.

Run it twice: once in light and once in dark theme. Repeat the sections
marked **(DPI)** at 100%, 125% and 150% display scaling
(Settings → System → Display → Scale).

## 0. Install

- [ ] The installer is ≤ 10 MB and installs without admin rights.
- [ ] No white flash when Limbo first opens; the window appears already drawn.
- [ ] First run shows onboarding: theme → import from Firefox → uBlock Origin
      Lite → default browser.
- [ ] "Open Windows Settings" on the last step opens Default apps with Limbo
      listed. After choosing it, links from other apps open in Limbo.

## 1. Window and title bar **(DPI)**

- [ ] The bar is 44 px tall; minimize / maximize / close are 46 × 44 targets;
      close turns red on hover.
- [ ] Hovering the maximize button for a moment shows Windows 11 snap layouts.
- [ ] Dragging the empty bar moves the window; double-click maximizes.
- [ ] Mica tint behind the bar on Windows 11 (Settings → Appearance →
      Translucent title bar toggles it).
- [ ] Resize the window quickly: the page's edges stay pixel-aligned with the
      bar, no gaps or overlap.
- [ ] F11 full screen hides the bar; F11 again restores it.

## 2. Omnibox and Google

- [ ] Ctrl+L, Alt+D and F6 focus the address bar and select all.
- [ ] Unfocused it shows only the domain; focused, the full URL. On a Google
      results page it shows the search words.
- [ ] `weather` searches Google. `example.com` opens https://example.com.
      `localhost:3000` and `192.168.1.1` open as addresses. `?example.com`
      searches.
- [ ] `github` + Ctrl+Enter opens https://www.github.com.
- [ ] Alt+Enter opens the result in a new tab.
- [ ] Typing shows history/bookmark suggestions at once and Google's shortly
      after; the caret never jumps while they update.
- [ ] Inline completion: typing `git` completes to a visited `github.com`;
      Backspace removes the completion.
- [ ] Shift+Delete on a history suggestion removes it.
- [ ] Settings → General → Google suggestions off: only local suggestions.
- [ ] The new tab page's search box moves focus to the address bar.

## 3. Tabs **(DPI)**

- [ ] Ctrl+T, Ctrl+W, Ctrl+Shift+T (reopens with its history), Ctrl+Tab,
      Ctrl+Shift+Tab, Ctrl+1…8, Ctrl+9 all work with a web page focused and
      with the address bar focused.
- [ ] New tabs slide in; closing slides neighbours over. Closing several tabs
      by clicking × repeatedly: the next × lands under the pointer.
- [ ] Drag to reorder is smooth; right-click → Pin, Mute, Duplicate, Sleep,
      Unload, Close others / to the right.
- [ ] Middle-click closes a tab; middle-click a link opens it in the background.
- [ ] Tab titles fade out instead of ending in "…"; favicons are crisp.
- [ ] With 12+ tabs, background tabs shrink to favicons, the active tab keeps
      its title, and the overview button appears. Ctrl+Shift+A shows every tab
      as a picture; clicking one switches to it.
- [ ] Hovering a tab shows its title, site and memory.
- [ ] A tab playing audio shows a speaker; clicking it mutes.
- [ ] Ctrl+Shift+N opens a private tab: violet chrome, not in history, not
      restored after restart.
- [ ] Mouse back/forward buttons navigate.

## 4. Memory saver

- [ ] The pill next to the menu shows total memory and updates smoothly.
- [ ] Ctrl+Shift+M opens the memory popover: total, system share bar,
      sparkline, one row per tab/extension plus "Browser engine".
- [ ] Hover a background tab's row → Sleep: its dot turns amber. Unload: grey,
      memory "—". Clicking that tab reloads it, showing its last picture
      while it wakes.
- [ ] Balanced: a background tab goes to sleep after 5 minutes, unloads after
      30 (check the dots).
- [ ] Maximum awake tabs 2: opening a third tab puts the least recently used
      one to sleep at once. A tab playing music is never picked.
- [ ] "Sleep all background tabs" works.
- [ ] Settings → Memory saver shows the same live panel.
- [ ] Open ~20 heavy tabs until Windows is low on memory: background tabs
      unload by themselves, the active one never does; the pill turns amber /
      red.
- [ ] Restart Limbo: only the active tab loads; the rest come back unloaded
      with their titles and icons.

## 5. Overlays (the snapshot technique) **(DPI)**

Over a busy page (e.g. a playing YouTube video, a scrolled Wikipedia article):

- [ ] Menu (⋯), page right-click menu, command palette (Ctrl+K), memory
      popover, downloads, site info (lock icon), bookmark star, clear data
      dialog: each opens over the page with no flicker, no black frame, and no
      visible jump between the live page and its picture.
- [ ] Esc always closes the topmost layer; the live page comes back exactly
      where it was.
- [ ] Find in page (Ctrl+F) shows a bar below the page; highlights update as
      you type; Enter / Shift+Enter step; Esc closes.

## 6. Keyboard only

- [ ] Tab moves through the bar; the focus ring is visible for keyboard focus
      and never after a mouse click.
- [ ] Menus: arrows, Home/End, type-ahead letters, Enter, Esc.
- [ ] Command palette: finds tabs, bookmarks, history, commands; Enter on
      nothing matching searches Google.
- [ ] The cursor is an arrow everywhere on the chrome; only text fields show
      the I-beam; chrome text can't be selected.

## 7. History, bookmarks, downloads

- [ ] Ctrl+H history page: grouped by day, search, delete one or several.
- [ ] Ctrl+D bookmarks the page (star turns blue); the popover edits the name
      and folder. Ctrl+Shift+B shows the bookmarks bar.
- [ ] Deleting a bookmark shows "Undo" in the address bar; undo restores it.
- [ ] Download a large file: progress ring on the downloads button; pause,
      resume, cancel; "Show in folder". Ctrl+J opens the list.
- [ ] Zoom (Ctrl+= / Ctrl+− / Ctrl+0) is remembered per site and shows the
      level in the address bar.

## 8. Import from Firefox

With Firefox installed and used (Firefox open and closed, both):

- [ ] Menu → Import from Firefox finds the profile.
- [ ] Import everything: counts shown match Firefox (passwords in
      `about:logins`, bookmarks in the Library).
- [ ] With a Firefox primary password: Limbo asks for it once.
- [ ] Imported cookies keep you signed in to sites (Google may ask once).
- [ ] Firefox's open tabs appear as unloaded tabs.
- [ ] The add-on list offers "Add" for Chrome versions (uBlock Origin → uBlock
      Origin Lite, Dark Reader…) and marks Firefox-only ones.
- [ ] The Firefox profile folder is unchanged (compare modified dates before
      and after).

## 9. Passwords

- [ ] Signing in on a new site offers to save; "Never for this site" works.
- [ ] Autofill on: google.com, github.com, amazon.com, reddit.com,
      login.microsoftonline.com, discord.com, netflix.com, a single-page app
      login, a bank-like demo. Record which work. (Iframe logins are a known
      gap.)
- [ ] With two saved logins, the key icon in the address bar offers a choice.
- [ ] Settings → Passwords: reveal and copy ask for Windows Hello / PIN;
      the revealed password hides again after 30 s.
- [ ] Export asks for Hello, writes a CSV; importing that CSV merges without
      duplicates.

## 10. Extensions

- [ ] On a Chrome Web Store page, "Add to Limbo" appears in the address bar;
      the review dialog lists permissions; installing adds the icon.
- [ ] uBlock Origin Lite blocks ads on a test page (e.g. d3ward.github.io/toolz/adblock).
- [ ] Dark Reader and Bitwarden: popup opens under the icon, works, closes on
      Esc or clicking away. Record anything that doesn't work.
- [ ] Right-click an extension icon: Pin/Unpin, Options, Remove.
- [ ] Settings → Extensions: enable/disable, remove, developer mode → Load
      unpacked.

## 11. Google sign-in (stop-and-report if blocked)

- [ ] Sign in at accounts.google.com, including 2-step verification. No "This
      browser or app may not be secure" page.
- [ ] "Sign in with Google" on a third-party site opens a small popup window,
      completes, and closes itself; the site is signed in.

## 12. Permissions, context menu, printing, crashes

- [ ] Camera/mic request (e.g. meet.google.com): a prompt under the address
      bar; "Remember" keeps the choice; Settings → Privacy lists and resets it.
- [ ] Right-click a link, an image, selected text, a text field: the menu
      has the right items and they work (copy, paste, save image, open in new
      tab, Search Google for "…").
- [ ] Ctrl+P prints.
- [ ] Crash a tab (address bar: `edge://kill` is blocked, so use Task Manager
      → end the tab's renderer process): "This page crashed · Reload".
- [ ] End the WebView2 browser process: Limbo restarts with the tabs restored.

## 13. Security

- [ ] Open a page that runs
      `console.log(window.__TAURI__, window.__TAURI_INTERNALS__)` in DevTools
      (Settings → Extensions → Developer mode, then F12): both `undefined`,
      and `window.chrome.webview.postMessage({t: 'fillRequest'})` from a
      subframe does nothing.
- [ ] A link to `file:///C:/Windows/win.ini` on a web page doesn't open;
      typing that address in the omnibox does.
- [ ] `%LOCALAPPDATA%\Limbo\browser.db` contains no plaintext passwords
      (search it for a known password with a hex editor).
- [ ] `%LOCALAPPDATA%\Limbo\Logs\limbo.log` contains no URLs or passwords.

## 14. Performance

- [ ] `docs/MEMORY_REPORT.md` procedure: every budget filled in.
- [ ] Open DevTools on the UI webview (developer mode), Performance tab, CPU
      4× slowdown: opening the menu, palette and tab overview stays at 60 fps.
- [ ] Windows Settings → Accessibility → Visual effects → Animation effects
      off: movement is replaced by quick fades.
