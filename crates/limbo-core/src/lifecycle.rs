//! Tab lifecycle state machine (build plan 6.2). Pure logic with an injected
//! clock (milliseconds); the host turns [`Action`]s into WebView2 calls.
//!
//! ```text
//! ACTIVE ──(switched away)──► HIDDEN        IsVisible=false
//! HIDDEN ──(30 s)───────────► LOW_MEMORY    MemoryUsageTargetLevel=Low
//! LOW_MEMORY ──(5 min)──────► SUSPENDED     TrySuspend (skipped for busy tabs)
//! SUSPENDED ──(30 min)──────► DISCARDED     webview closed, snapshot kept
//! any ──(activated)─────────► ACTIVE
//! ```
//!
//! "Awake" tabs (ACTIVE, HIDDEN, LOW_MEMORY) are capped by `max_awake`: going over
//! the cap suspends the least recently used non-exempt tab immediately.
//!
//! The host never polls: it sleeps until [`Lifecycle::next_deadline`] and then
//! calls [`Lifecycle::tick`], so an idle browser does no work.

use serde::{Deserialize, Serialize};

pub type TabId = u64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TabState {
    Active,
    Hidden,
    LowMemory,
    Suspended,
    Discarded,
}

impl TabState {
    pub fn is_awake(self) -> bool {
        matches!(self, TabState::Active | TabState::Hidden | TabState::LowMemory)
    }

    /// The four-state label shown to the user (state dots in the memory popover).
    pub fn label(self) -> &'static str {
        match self {
            TabState::Active => "active",
            TabState::Hidden | TabState::LowMemory => "awake",
            TabState::Suspended => "sleeping",
            TabState::Discarded => "unloaded",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Preset {
    Balanced,
    Aggressive,
    Off,
    Custom,
}

/// Timer and cap configuration ("Memory saver" in Settings).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Policy {
    pub preset: Preset,
    /// HIDDEN -> LOW_MEMORY. `None` disables the transition.
    pub hidden_to_low_ms: Option<u64>,
    /// LOW_MEMORY -> SUSPENDED.
    pub low_to_suspend_ms: Option<u64>,
    /// SUSPENDED -> DISCARDED.
    pub suspend_to_discard_ms: Option<u64>,
    /// Maximum awake tabs; `None` = no limit.
    pub max_awake: Option<u32>,
    /// Pinned tabs are never discarded unless this is set.
    pub discard_pinned: bool,
}

const SEC: u64 = 1000;
const MIN: u64 = 60 * SEC;

impl Policy {
    pub const MAX_AWAKE_LIMIT: u32 = 20;

    pub fn balanced() -> Policy {
        Policy {
            preset: Preset::Balanced,
            hidden_to_low_ms: Some(30 * SEC),
            low_to_suspend_ms: Some(5 * MIN),
            suspend_to_discard_ms: Some(30 * MIN),
            max_awake: Some(4),
            discard_pinned: false,
        }
    }

    pub fn aggressive() -> Policy {
        Policy {
            preset: Preset::Aggressive,
            hidden_to_low_ms: Some(10 * SEC),
            low_to_suspend_ms: Some(MIN),
            suspend_to_discard_ms: Some(5 * MIN),
            max_awake: Some(2),
            discard_pinned: false,
        }
    }

    pub fn off() -> Policy {
        Policy {
            preset: Preset::Off,
            hidden_to_low_ms: None,
            low_to_suspend_ms: None,
            suspend_to_discard_ms: None,
            max_awake: None,
            discard_pinned: false,
        }
    }

    pub fn from_preset(preset: Preset) -> Policy {
        match preset {
            Preset::Aggressive => Policy::aggressive(),
            Preset::Off => Policy::off(),
            Preset::Balanced | Preset::Custom => Policy::balanced(),
        }
    }

    /// Changing the stepper switches the preset label to "Custom" (unless it
    /// happens to match the preset's own default).
    pub fn with_max_awake(mut self, max_awake: Option<u32>) -> Policy {
        let max_awake = max_awake.map(|n| n.clamp(1, Self::MAX_AWAKE_LIMIT));
        if self.max_awake != max_awake {
            self.max_awake = max_awake;
            if self.preset != Preset::Custom && Policy::from_preset(self.preset).max_awake != max_awake {
                self.preset = Preset::Custom;
            }
        }
        self
    }
}

impl Default for Policy {
    fn default() -> Self {
        Policy::balanced()
    }
}

/// Signals that make a tab "busy". Busy tabs are skipped by automatic suspension.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TabFlags {
    pub pinned: bool,
    pub audible: bool,
    /// Camera, microphone or screen capture in use.
    pub capturing: bool,
    pub downloading: bool,
    /// The page has unsaved form input (a `beforeunload` handler / dirty form).
    pub dirty_form: bool,
}

impl TabFlags {
    fn busy(&self) -> bool {
        self.audible || self.capturing || self.downloading || self.dirty_form
    }

    /// Never picked by the max-awake cap.
    fn cap_exempt(&self) -> bool {
        self.pinned || self.busy()
    }
}

/// What the host must do to a webview.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// `IsVisible = true` (and focus).
    Show(TabId),
    /// `IsVisible = false`: throttles timers and rendering.
    Hide(TabId),
    /// `MemoryUsageTargetLevel = Low`.
    MemoryLow(TabId),
    /// `MemoryUsageTargetLevel = Normal`.
    MemoryNormal(TabId),
    /// `TrySuspendAsync`. Report failures with [`Lifecycle::suspend_failed`].
    Suspend(TabId),
    /// `Resume` (before showing).
    Resume(TabId),
    /// Close the webview; keep url/title/favicon/snapshot.
    Discard(TabId),
    /// Create the webview again and navigate, showing the snapshot until first paint.
    Recreate(TabId),
}

#[derive(Debug, Clone)]
struct Tab {
    id: TabId,
    state: TabState,
    /// When the tab entered `state`.
    since: u64,
    /// Last time the tab was active (LRU order).
    last_active: u64,
    flags: TabFlags,
}

#[derive(Debug, Clone, Default)]
pub struct Lifecycle {
    tabs: Vec<Tab>,
    policy: Policy,
    active: Option<TabId>,
    /// Set when exempt tabs alone exceed the cap; the UI shows a one-time hint.
    exempt_overflow: bool,
}

/// Snapshot of one tab for the UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TabStatus {
    pub id: TabId,
    pub state: TabState,
    pub flags: TabFlags,
}

impl Lifecycle {
    pub fn new(policy: Policy) -> Lifecycle {
        Lifecycle { tabs: Vec::new(), policy, active: None, exempt_overflow: false }
    }

    pub fn policy(&self) -> Policy {
        self.policy
    }

    pub fn active(&self) -> Option<TabId> {
        self.active
    }

    pub fn state(&self, id: TabId) -> Option<TabState> {
        self.tab(id).map(|t| t.state)
    }

    pub fn statuses(&self) -> Vec<TabStatus> {
        self.tabs.iter().map(|t| TabStatus { id: t.id, state: t.state, flags: t.flags }).collect()
    }

    pub fn awake_count(&self) -> usize {
        self.tabs.iter().filter(|t| t.state.is_awake()).count()
    }

    pub fn exempt_overflow(&self) -> bool {
        self.exempt_overflow
    }

    fn tab(&self, id: TabId) -> Option<&Tab> {
        self.tabs.iter().find(|t| t.id == id)
    }

    fn tab_mut(&mut self, id: TabId) -> Option<&mut Tab> {
        self.tabs.iter_mut().find(|t| t.id == id)
    }

    fn set_state(&mut self, id: TabId, state: TabState, now: u64) {
        if let Some(t) = self.tab_mut(id) {
            t.state = state;
            t.since = now;
        }
    }

    /// Registers a tab.
    ///
    /// * `discarded`: restored from a session without a webview.
    /// * otherwise the webview exists and starts hidden (background tab); call
    ///   [`Lifecycle::activate`] to make it the active one.
    pub fn add(&mut self, id: TabId, flags: TabFlags, discarded: bool, now: u64) -> Vec<Action> {
        if self.tab(id).is_some() {
            return Vec::new();
        }
        let state = if discarded { TabState::Discarded } else { TabState::Hidden };
        self.tabs.push(Tab { id, state, since: now, last_active: now, flags });
        self.enforce_max_awake(now)
    }

    /// Forgets a closed tab. The host picks the next active tab itself.
    pub fn remove(&mut self, id: TabId) {
        self.tabs.retain(|t| t.id != id);
        if self.active == Some(id) {
            self.active = None;
        }
        self.refresh_overflow_hint();
    }

    /// Makes `id` the active tab. Actions for the target come first so the
    /// host can show the new tab before hiding the old one (no blank frame).
    pub fn activate(&mut self, id: TabId, now: u64) -> Vec<Action> {
        let Some(target) = self.tab(id).cloned() else {
            return Vec::new();
        };
        let mut actions = Vec::new();
        match target.state {
            TabState::Active => return actions,
            TabState::Hidden => actions.push(Action::Show(id)),
            TabState::LowMemory => {
                actions.push(Action::MemoryNormal(id));
                actions.push(Action::Show(id));
            }
            TabState::Suspended => {
                actions.push(Action::Resume(id));
                actions.push(Action::MemoryNormal(id));
                actions.push(Action::Show(id));
            }
            TabState::Discarded => actions.push(Action::Recreate(id)),
        }
        if let Some(prev) = self.active.filter(|&p| p != id)
            && self.state(prev) == Some(TabState::Active)
        {
            actions.push(Action::Hide(prev));
            self.set_state(prev, TabState::Hidden, now);
            if let Some(t) = self.tab_mut(prev) {
                t.last_active = now;
            }
        }
        self.set_state(id, TabState::Active, now);
        if let Some(t) = self.tab_mut(id) {
            t.last_active = now;
        }
        self.active = Some(id);
        actions.extend(self.enforce_max_awake(now));
        actions
    }

    /// Updates busy/pinned signals. Call [`Lifecycle::tick`] afterwards: a tab
    /// that stopped playing audio may already be past its deadline.
    pub fn set_flags(&mut self, id: TabId, flags: TabFlags, now: u64) -> Vec<Action> {
        if let Some(t) = self.tab_mut(id) {
            t.flags = flags;
        }
        let mut actions = self.enforce_max_awake(now);
        actions.extend(self.tick(now));
        actions
    }

    pub fn flags(&self, id: TabId) -> Option<TabFlags> {
        self.tab(id).map(|t| t.flags)
    }

    /// Applies a new policy immediately (the cap is enforced right away).
    pub fn set_policy(&mut self, policy: Policy, now: u64) -> Vec<Action> {
        self.policy = policy;
        self.exempt_overflow = false;
        let mut actions = self.enforce_max_awake(now);
        actions.extend(self.tick(now));
        actions
    }

    /// TrySuspend failed (e.g. the page started audio): stay LOW_MEMORY and
    /// retry after another full interval.
    pub fn suspend_failed(&mut self, id: TabId, now: u64) {
        if self.state(id) == Some(TabState::Suspended) {
            self.set_state(id, TabState::LowMemory, now);
        }
    }

    /// Advances timers. Safe to call at any time; returns nothing when no
    /// deadline has passed.
    pub fn tick(&mut self, now: u64) -> Vec<Action> {
        let mut actions = Vec::new();
        let policy = self.policy;
        for t in self.tabs.iter_mut() {
            if Some(t.id) == self.active {
                continue;
            }
            let elapsed = now.saturating_sub(t.since);
            match t.state {
                TabState::Hidden => {
                    if policy.hidden_to_low_ms.is_some_and(|d| elapsed >= d) {
                        actions.push(Action::MemoryLow(t.id));
                        t.state = TabState::LowMemory;
                        t.since = now;
                    }
                }
                TabState::LowMemory => {
                    if policy.low_to_suspend_ms.is_some_and(|d| elapsed >= d) && !t.flags.busy() {
                        actions.push(Action::Suspend(t.id));
                        t.state = TabState::Suspended;
                        t.since = now;
                    }
                }
                TabState::Suspended => {
                    let exempt = t.flags.busy() || (t.flags.pinned && !policy.discard_pinned);
                    if policy.suspend_to_discard_ms.is_some_and(|d| elapsed >= d) && !exempt {
                        actions.push(Action::Discard(t.id));
                        t.state = TabState::Discarded;
                        t.since = now;
                    }
                }
                TabState::Active | TabState::Discarded => {}
            }
        }
        actions
    }

    /// The next time [`Lifecycle::tick`] would do something, or `None` if nothing is pending.
    pub fn next_deadline(&self) -> Option<u64> {
        let p = self.policy;
        self.tabs
            .iter()
            .filter(|t| Some(t.id) != self.active)
            .filter_map(|t| {
                let delay = match t.state {
                    TabState::Hidden => p.hidden_to_low_ms,
                    TabState::LowMemory if !t.flags.busy() => p.low_to_suspend_ms,
                    TabState::Suspended if !t.flags.busy() && (!t.flags.pinned || p.discard_pinned) => {
                        p.suspend_to_discard_ms
                    }
                    _ => None,
                }?;
                Some(t.since.saturating_add(delay))
            })
            .min()
    }

    /// Manual "Sleep" from the memory popover.
    pub fn sleep(&mut self, id: TabId, now: u64) -> Vec<Action> {
        if Some(id) == self.active {
            return Vec::new();
        }
        let Some(state) = self.state(id) else { return Vec::new() };
        let mut actions = Vec::new();
        match state {
            TabState::Hidden => {
                actions.push(Action::MemoryLow(id));
                actions.push(Action::Suspend(id));
            }
            TabState::LowMemory => actions.push(Action::Suspend(id)),
            _ => return actions,
        }
        self.set_state(id, TabState::Suspended, now);
        self.refresh_overflow_hint();
        actions
    }

    /// "Sleep all background tabs".
    pub fn sleep_all_background(&mut self, now: u64) -> Vec<Action> {
        let ids: Vec<TabId> = self
            .tabs
            .iter()
            .filter(|t| Some(t.id) != self.active && matches!(t.state, TabState::Hidden | TabState::LowMemory))
            .map(|t| t.id)
            .collect();
        ids.into_iter().flat_map(|id| self.sleep(id, now)).collect()
    }

    /// Manual "Unload" from the memory popover.
    pub fn unload(&mut self, id: TabId, now: u64) -> Vec<Action> {
        if Some(id) == self.active {
            return Vec::new();
        }
        match self.state(id) {
            Some(TabState::Discarded) | None => Vec::new(),
            Some(_) => {
                self.set_state(id, TabState::Discarded, now);
                self.refresh_overflow_hint();
                vec![Action::Discard(id)]
            }
        }
    }

    /// Under system memory pressure: the least recently used SUSPENDED or
    /// LOW_MEMORY tab that may be discarded. Never the active tab.
    pub fn pressure_victim(&self) -> Option<TabId> {
        let p = self.policy;
        self.tabs
            .iter()
            .filter(|t| Some(t.id) != self.active)
            .filter(|t| matches!(t.state, TabState::Suspended | TabState::LowMemory))
            .filter(|t| !t.flags.busy() && (!t.flags.pinned || p.discard_pinned))
            .min_by_key(|t| t.last_active)
            .map(|t| t.id)
    }

    /// Discards the pressure victim, if any.
    pub fn relieve_pressure(&mut self, now: u64) -> Option<Action> {
        let id = self.pressure_victim()?;
        self.set_state(id, TabState::Discarded, now);
        Some(Action::Discard(id))
    }

    fn enforce_max_awake(&mut self, now: u64) -> Vec<Action> {
        let mut actions = Vec::new();
        let Some(limit) = self.policy.max_awake else {
            self.exempt_overflow = false;
            return actions;
        };
        let limit = limit.max(1) as usize;
        loop {
            if self.awake_count() <= limit {
                break;
            }
            let victim = self
                .tabs
                .iter()
                .filter(|t| t.state.is_awake() && Some(t.id) != self.active && !t.flags.cap_exempt())
                .min_by_key(|t| t.last_active)
                .map(|t| (t.id, t.state));
            let Some((id, state)) = victim else {
                self.exempt_overflow = true;
                break;
            };
            if state == TabState::Hidden {
                actions.push(Action::MemoryLow(id));
            }
            actions.push(Action::Suspend(id));
            self.set_state(id, TabState::Suspended, now);
        }
        self.refresh_overflow_hint();
        actions
    }

    fn refresh_overflow_hint(&mut self) {
        let Some(limit) = self.policy.max_awake else {
            self.exempt_overflow = false;
            return;
        };
        let exempt_awake = self
            .tabs
            .iter()
            .filter(|t| t.state.is_awake() && (Some(t.id) == self.active || t.flags.cap_exempt()))
            .count();
        self.exempt_overflow = exempt_awake > limit.max(1) as usize;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use Action::*;

    fn lc(policy: Policy) -> Lifecycle {
        Lifecycle::new(policy)
    }

    fn no_cap(mut p: Policy) -> Policy {
        p.max_awake = None;
        p
    }

    #[test]
    fn full_timeline_balanced() {
        let mut l = lc(no_cap(Policy::balanced()));
        l.add(1, TabFlags::default(), false, 0);
        l.add(2, TabFlags::default(), false, 0);
        assert_eq!(l.activate(1, 0), vec![Show(1)]);
        assert_eq!(l.activate(2, 1_000), vec![Show(2), Hide(1)]);
        assert_eq!(l.state(1), Some(TabState::Hidden));

        assert_eq!(l.next_deadline(), Some(31_000));
        assert!(l.tick(30_999).is_empty());
        assert_eq!(l.tick(31_000), vec![MemoryLow(1)]);
        assert_eq!(l.next_deadline(), Some(31_000 + 5 * MIN));
        assert_eq!(l.tick(31_000 + 5 * MIN), vec![Suspend(1)]);
        assert_eq!(l.state(1), Some(TabState::Suspended));
        let t = 31_000 + 5 * MIN + 30 * MIN;
        assert_eq!(l.next_deadline(), Some(t));
        assert_eq!(l.tick(t), vec![Discard(1)]);
        assert_eq!(l.state(1), Some(TabState::Discarded));
        assert_eq!(l.next_deadline(), None, "nothing left to do: host can sleep");

        // Reactivating a discarded tab recreates it.
        assert_eq!(l.activate(1, t + 1), vec![Recreate(1), Hide(2)]);
        assert_eq!(l.state(1), Some(TabState::Active));
    }

    #[test]
    fn activation_from_each_state() {
        let mut l = lc(no_cap(Policy::balanced()));
        for id in 1..=4 {
            l.add(id, TabFlags::default(), false, 0);
        }
        l.activate(4, 0);
        l.tick(30 * SEC); // 1,2,3 -> low memory
        l.sleep(2, 30 * SEC);
        l.unload(3, 30 * SEC);
        assert_eq!(l.activate(1, 40 * SEC), vec![MemoryNormal(1), Show(1), Hide(4)]);
        assert_eq!(l.activate(2, 41 * SEC), vec![Resume(2), MemoryNormal(2), Show(2), Hide(1)]);
        assert_eq!(l.activate(3, 42 * SEC), vec![Recreate(3), Hide(2)]);
        assert_eq!(l.activate(3, 43 * SEC), vec![], "already active");
    }

    #[test]
    fn busy_tabs_are_not_suspended_and_pinned_not_discarded() {
        let mut l = lc(no_cap(Policy::aggressive()));
        let audible = TabFlags { audible: true, ..Default::default() };
        let pinned = TabFlags { pinned: true, ..Default::default() };
        l.add(1, audible, false, 0);
        l.add(2, pinned, false, 0);
        l.add(3, TabFlags::default(), false, 0);
        l.activate(3, 0);
        let mut now = 0;
        for _ in 0..100 {
            now += 10 * SEC;
            l.tick(now);
        }
        assert_eq!(l.state(1), Some(TabState::LowMemory), "audible: never suspended");
        assert_eq!(l.state(2), Some(TabState::Suspended), "pinned: suspended but kept");
        // When the audio stops, the overdue suspension happens right away.
        let acts = l.set_flags(1, TabFlags::default(), now);
        assert_eq!(acts, vec![Suspend(1)]);
    }

    #[test]
    fn max_awake_suspends_lru_immediately() {
        let mut l = lc(Policy::balanced().with_max_awake(Some(2)));
        l.add(1, TabFlags::default(), false, 0);
        l.activate(1, 0);
        l.add(2, TabFlags::default(), false, 10);
        l.activate(2, 10);
        assert_eq!(l.awake_count(), 2);
        l.add(3, TabFlags::default(), false, 20);
        // Tab 3 is new and hidden; tab 1 is least recently used.
        assert_eq!(l.state(1), Some(TabState::Suspended));
        assert_eq!(l.awake_count(), 2);
        let acts = l.activate(3, 30);
        // Activating 3 hides 2; still 2 awake (3 active, 2 hidden) -> no extra suspension.
        assert_eq!(acts, vec![Show(3), Hide(2)]);
        // Re-activating 1 pushes over the limit: 2 (hidden, LRU) is suspended.
        let acts = l.activate(1, 40);
        assert_eq!(acts, vec![Resume(1), MemoryNormal(1), Show(1), Hide(3), MemoryLow(2), Suspend(2)]);
        assert_eq!(l.awake_count(), 2);
    }

    #[test]
    fn max_awake_never_picks_exempt_tabs_and_raises_hint() {
        let mut l = lc(Policy::balanced().with_max_awake(Some(1)));
        let busy = TabFlags { downloading: true, ..Default::default() };
        l.add(1, busy, false, 0);
        l.add(2, TabFlags { pinned: true, ..Default::default() }, false, 1);
        l.add(3, TabFlags::default(), false, 2);
        l.activate(3, 3);
        assert_eq!(l.state(1), Some(TabState::Hidden));
        assert_eq!(l.state(2), Some(TabState::Hidden));
        assert_eq!(l.awake_count(), 3);
        assert!(l.exempt_overflow());
        // Download finishes: tab 1 becomes eligible and is suspended right away.
        let acts = l.set_flags(1, TabFlags::default(), 4);
        assert!(acts.contains(&Suspend(1)));
        assert!(l.exempt_overflow(), "pinned + active still exceed a cap of 1");
    }

    #[test]
    fn changing_cap_applies_instantly_and_marks_custom() {
        let mut l = lc(Policy::balanced());
        for id in 1..=5 {
            l.add(id, TabFlags::default(), false, id);
        }
        l.activate(5, 10);
        assert_eq!(l.awake_count(), 4);
        let p = l.policy().with_max_awake(Some(2));
        assert_eq!(p.preset, Preset::Custom);
        let acts = l.set_policy(p, 20);
        assert_eq!(acts.iter().filter(|a| matches!(a, Suspend(_))).count(), 2);
        assert_eq!(l.awake_count(), 2);
        // Back to the preset default keeps the preset name.
        assert_eq!(Policy::balanced().with_max_awake(Some(4)).preset, Preset::Balanced);
        assert_eq!(Policy::balanced().with_max_awake(Some(99)).max_awake, Some(20));
        assert_eq!(Policy::balanced().with_max_awake(None).max_awake, None);
    }

    #[test]
    fn off_preset_does_nothing() {
        let mut l = lc(Policy::off());
        for id in 1..=30 {
            l.add(id, TabFlags::default(), false, 0);
        }
        l.activate(1, 0);
        assert_eq!(l.next_deadline(), None);
        assert!(l.tick(u64::MAX / 2).is_empty());
        assert_eq!(l.awake_count(), 30);
    }

    #[test]
    fn suspend_failure_retries_later() {
        let mut l = lc(no_cap(Policy::balanced()));
        l.add(1, TabFlags::default(), false, 0);
        l.add(2, TabFlags::default(), false, 0);
        l.activate(2, 0);
        l.tick(30 * SEC);
        assert_eq!(l.tick(30 * SEC + 5 * MIN), vec![Suspend(1)]);
        l.suspend_failed(1, 30 * SEC + 5 * MIN);
        assert_eq!(l.state(1), Some(TabState::LowMemory));
        assert_eq!(l.next_deadline(), Some(30 * SEC + 10 * MIN));
    }

    #[test]
    fn pressure_discards_lru_suspended_or_low_memory_only() {
        let mut l = lc(no_cap(Policy::balanced()));
        for id in 1..=4 {
            l.add(id, TabFlags::default(), false, 0);
        }
        l.activate(1, 0);
        l.activate(2, 10);
        l.activate(3, 20);
        l.activate(4, 30);
        // 1 (last active 10), 2 (20), 3 (30) are hidden. Nothing low yet.
        assert_eq!(l.pressure_victim(), None, "hidden tabs aren't victims");
        l.tick(30 * SEC + 30);
        assert_eq!(l.relieve_pressure(31 * SEC), Some(Discard(1)));
        assert_eq!(l.relieve_pressure(31 * SEC), Some(Discard(2)));
        assert_eq!(l.relieve_pressure(31 * SEC), Some(Discard(3)));
        assert_eq!(l.relieve_pressure(31 * SEC), None, "never the active tab");
    }

    #[test]
    fn restored_session_tabs_start_discarded() {
        let mut l = lc(Policy::balanced());
        l.add(1, TabFlags::default(), false, 0);
        for id in 2..=10 {
            l.add(id, TabFlags::default(), true, 0);
        }
        l.activate(1, 0);
        assert_eq!(l.awake_count(), 1);
        assert_eq!(l.state(7), Some(TabState::Discarded));
        assert_eq!(l.next_deadline(), None);
    }

    #[test]
    fn manual_sleep_and_unload() {
        let mut l = lc(no_cap(Policy::balanced()));
        l.add(1, TabFlags::default(), false, 0);
        l.add(2, TabFlags::default(), false, 0);
        l.add(3, TabFlags::default(), false, 0);
        l.activate(3, 0);
        assert_eq!(l.sleep(3, 1), vec![], "can't sleep the active tab");
        assert_eq!(l.unload(3, 1), vec![], "can't unload the active tab");
        assert_eq!(l.sleep_all_background(1), vec![MemoryLow(1), Suspend(1), MemoryLow(2), Suspend(2)]);
        assert_eq!(l.unload(1, 2), vec![Discard(1)]);
        assert_eq!(l.unload(1, 3), vec![]);
    }

    #[test]
    fn remove_clears_active() {
        let mut l = lc(Policy::balanced());
        l.add(1, TabFlags::default(), false, 0);
        l.activate(1, 0);
        l.remove(1);
        assert_eq!(l.active(), None);
        assert_eq!(l.state(1), None);
    }
}
