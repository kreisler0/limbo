//! Memory readout model (build plan 6.3): turns per-process samples into the
//! rows shown in the memory popover. Collection itself lives in the host.
//!
//! Metric: **private working set** (what Task Manager's "Memory" column shows),
//! the same metric `tools/measure-memory.ps1` uses, so the readout and the
//! budget gates agree.

use serde::Serialize;

use crate::lifecycle::{TabId, TabState, TabStatus};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ProcessKind {
    /// limbo.exe itself.
    Host,
    Browser,
    Gpu,
    Renderer,
    Utility,
    Other,
}

/// One process of the app, attributed by the host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessSample {
    pub pid: u32,
    pub kind: ProcessKind,
    pub bytes: u64,
    /// Tabs with at least one frame in this (renderer) process.
    pub tabs: Vec<TabId>,
    /// Extensions with a frame (popup/options/background page) in this process.
    pub extensions: Vec<String>,
    /// The renderer of Limbo's own UI webview.
    pub is_ui: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum RowKind {
    Tab,
    Extension,
    Engine,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryRow {
    pub kind: RowKind,
    /// Tab id (as string), extension id, or `"engine"`.
    pub id: String,
    pub bytes: u64,
    /// The process is shared with other tabs/extensions; bytes are an even split.
    pub shared: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<&'static str>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MemorySample {
    pub total_bytes: u64,
    pub system_avail_bytes: u64,
    pub system_total_bytes: u64,
    pub awake: usize,
    pub max_awake: Option<u32>,
    pub rows: Vec<MemoryRow>,
}

/// Attributes process memory to tabs, extensions and the engine.
///
/// * A renderer serving several tabs is split evenly; those rows are `shared`.
/// * A tab may be spread over several processes (site isolation); its shares add up.
/// * Sleeping tabs report whatever their renderer still holds; unloaded tabs report 0.
/// * Browser, GPU, utility, the host and the UI renderer form one "engine" row, along
///   with renderers that have no tab or extension (spare renderers, service workers).
pub fn attribute(processes: &[ProcessSample], tabs: &[TabStatus]) -> (u64, Vec<MemoryRow>) {
    let mut total = 0u64;
    let mut engine = 0u64;
    let mut tab_bytes: Vec<(TabId, u64, bool)> = tabs.iter().map(|t| (t.id, 0, false)).collect();
    let mut ext_bytes: Vec<(String, u64, bool)> = Vec::new();

    for p in processes {
        total += p.bytes;
        let known_tabs: Vec<TabId> =
            p.tabs.iter().copied().filter(|id| tab_bytes.iter().any(|(t, _, _)| t == id)).collect();
        let owners = known_tabs.len() + p.extensions.len();
        if p.kind != ProcessKind::Renderer || p.is_ui || owners == 0 {
            engine += p.bytes;
            continue;
        }
        let share = p.bytes / owners as u64;
        let remainder = p.bytes - share * owners as u64;
        let shared = owners > 1;
        for (i, id) in known_tabs.iter().enumerate() {
            if let Some(entry) = tab_bytes.iter_mut().find(|(t, _, _)| t == id) {
                entry.1 += share + if i == 0 { remainder } else { 0 };
                entry.2 |= shared;
            }
        }
        for ext in &p.extensions {
            match ext_bytes.iter_mut().find(|(e, _, _)| e == ext) {
                Some(entry) => {
                    entry.1 += share;
                    entry.2 |= shared;
                }
                None => ext_bytes.push((ext.clone(), share, shared)),
            }
        }
    }

    let mut rows: Vec<MemoryRow> = tab_bytes
        .into_iter()
        .map(|(id, bytes, shared)| {
            let state = tabs.iter().find(|t| t.id == id).map(|t| t.state);
            let bytes = if state == Some(TabState::Discarded) { 0 } else { bytes };
            MemoryRow { kind: RowKind::Tab, id: id.to_string(), bytes, shared, state: state.map(TabState::label) }
        })
        .collect();
    rows.extend(ext_bytes.into_iter().map(|(id, bytes, shared)| MemoryRow {
        kind: RowKind::Extension,
        id,
        bytes,
        shared,
        state: None,
    }));
    rows.push(MemoryRow { kind: RowKind::Engine, id: "engine".into(), bytes: engine, shared: false, state: None });
    rows.sort_by(|a, b| b.bytes.cmp(&a.bytes).then_with(|| a.id.cmp(&b.id)));
    (total, rows)
}

/// "184 MB" (MiB, rounded), the format used by the pill and the popover.
pub fn format_mb(bytes: u64) -> String {
    let mb = (bytes as f64 / (1024.0 * 1024.0)).round() as u64;
    format!("{mb} MB")
}

/// System memory pressure with hysteresis (build plan 6.2): pressure starts
/// below 15% available (or on the OS low-memory notification) and ends above 20%.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PressureMonitor {
    active: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum PressureLevel {
    Normal,
    /// Below 20% available: the pill turns amber.
    Elevated,
    /// Below 10%: the pill turns red.
    Critical,
}

impl PressureMonitor {
    pub const START_PERCENT: u64 = 15;
    pub const END_PERCENT: u64 = 20;

    /// Returns true while tabs should be discarded.
    pub fn update(&mut self, avail: u64, total: u64, os_low_memory: bool) -> bool {
        let pct = if total == 0 { 100 } else { avail * 100 / total };
        if pct < Self::START_PERCENT || os_low_memory {
            self.active = true;
        } else if pct > Self::END_PERCENT {
            self.active = false;
        }
        self.active
    }

    pub fn level(avail: u64, total: u64) -> PressureLevel {
        let pct = if total == 0 { 100 } else { avail * 100 / total };
        if pct < 10 {
            PressureLevel::Critical
        } else if pct < 20 {
            PressureLevel::Elevated
        } else {
            PressureLevel::Normal
        }
    }
}

/// How often the host samples process memory (6.3: sampling must be near free).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SamplingMode {
    /// Popover open: every 2 s.
    Popover,
    /// Only the toolbar pill visible: every 10 s.
    Pill,
    /// Window minimized or pill hidden: never.
    Off,
}

impl SamplingMode {
    pub fn interval_ms(self) -> Option<u64> {
        match self {
            SamplingMode::Popover => Some(2_000),
            SamplingMode::Pill => Some(10_000),
            SamplingMode::Off => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lifecycle::TabFlags;

    const MB: u64 = 1024 * 1024;

    fn status(id: TabId, state: TabState) -> TabStatus {
        TabStatus { id, state, flags: TabFlags::default() }
    }

    fn proc(pid: u32, kind: ProcessKind, mb: u64, tabs: &[TabId]) -> ProcessSample {
        ProcessSample { pid, kind, bytes: mb * MB, tabs: tabs.to_vec(), extensions: vec![], is_ui: false }
    }

    #[test]
    fn attribution() {
        let tabs = vec![
            status(1, TabState::Active),
            status(2, TabState::Hidden),
            status(3, TabState::Hidden),
            status(4, TabState::Discarded),
        ];
        let mut ui = proc(10, ProcessKind::Renderer, 30, &[]);
        ui.is_ui = true;
        let mut ext = proc(11, ProcessKind::Renderer, 12, &[]);
        ext.extensions = vec!["ublock".into()];
        let procs = vec![
            proc(1, ProcessKind::Host, 8, &[]),
            proc(2, ProcessKind::Browser, 40, &[]),
            proc(3, ProcessKind::Gpu, 30, &[]),
            proc(4, ProcessKind::Utility, 12, &[]),
            ui,
            ext,
            proc(5, ProcessKind::Renderer, 60, &[1]),
            proc(6, ProcessKind::Renderer, 50, &[2, 3]),
            proc(7, ProcessKind::Renderer, 10, &[1, 99]), // 99 isn't a known tab
            proc(8, ProcessKind::Renderer, 5, &[]),       // spare renderer
        ];
        let (total, rows) = attribute(&procs, &tabs);
        assert_eq!(total, 257 * MB);
        let row = |id: &str| rows.iter().find(|r| r.id == id).unwrap().clone();
        assert_eq!(row("1").bytes, 70 * MB);
        assert!(!row("1").shared);
        assert_eq!(row("2").bytes, 25 * MB);
        assert!(row("2").shared);
        assert_eq!(row("3").bytes, 25 * MB);
        assert_eq!(row("4").bytes, 0);
        assert_eq!(row("4").state, Some("unloaded"));
        assert_eq!(row("ublock").bytes, 12 * MB);
        assert_eq!(row("ublock").kind, RowKind::Extension);
        assert_eq!(row("engine").bytes, (8 + 40 + 30 + 12 + 30 + 5) * MB);
        // Sorted by size.
        assert!(rows.windows(2).all(|w| w[0].bytes >= w[1].bytes));
        let sum: u64 = rows.iter().map(|r| r.bytes).sum();
        assert_eq!(sum, total, "every byte is attributed exactly once");
    }

    #[test]
    fn remainder_bytes_are_not_lost() {
        let tabs = vec![status(1, TabState::Active), status(2, TabState::Hidden), status(3, TabState::Hidden)];
        let procs = vec![ProcessSample {
            pid: 1,
            kind: ProcessKind::Renderer,
            bytes: 100,
            tabs: vec![1, 2, 3],
            extensions: vec![],
            is_ui: false,
        }];
        let (total, rows) = attribute(&procs, &tabs);
        assert_eq!(rows.iter().map(|r| r.bytes).sum::<u64>(), total);
    }

    #[test]
    fn mb_format() {
        assert_eq!(format_mb(184 * MB + MB / 3), "184 MB");
        assert_eq!(format_mb(0), "0 MB");
    }

    #[test]
    fn pressure_hysteresis() {
        let mut m = PressureMonitor::default();
        assert!(!m.update(30, 100, false));
        assert!(m.update(14, 100, false));
        assert!(m.update(18, 100, false), "stays on until above 20%");
        assert!(!m.update(21, 100, false));
        assert!(m.update(50, 100, true), "OS notification forces it");
        assert_eq!(PressureMonitor::level(25, 100), PressureLevel::Normal);
        assert_eq!(PressureMonitor::level(19, 100), PressureLevel::Elevated);
        assert_eq!(PressureMonitor::level(9, 100), PressureLevel::Critical);
    }

    #[test]
    fn sampling_intervals() {
        assert_eq!(SamplingMode::Popover.interval_ms(), Some(2000));
        assert_eq!(SamplingMode::Pill.interval_ms(), Some(10000));
        assert_eq!(SamplingMode::Off.interval_ms(), None);
    }
}
