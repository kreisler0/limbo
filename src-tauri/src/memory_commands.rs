//! Memory saver actions from the UI (memory popover, Settings).

use limbo_core::lifecycle::Policy;

use crate::state::{SharedBrowser, TabId};
use crate::tabs;

pub fn apply_policy(b: &SharedBrowser, policy: Policy) {
    let now = b.now_ms();
    let actions = b.inner.lock().lifecycle.set_policy(policy, now);
    tabs::apply_actions(b, actions);
    tabs::emit_all_states(b);
    b.lifecycle_signal.notify();
}

pub fn sleep(b: &SharedBrowser, id: TabId) {
    let now = b.now_ms();
    let actions = b.inner.lock().lifecycle.sleep(id, now);
    tabs::apply_actions(b, actions);
    tabs::emit_all_states(b);
    b.lifecycle_signal.notify();
}

pub fn unload(b: &SharedBrowser, id: TabId) {
    let now = b.now_ms();
    let actions = b.inner.lock().lifecycle.unload(id, now);
    tabs::apply_actions(b, actions);
    tabs::emit_all_states(b);
    b.lifecycle_signal.notify();
}

pub fn sleep_all(b: &SharedBrowser) {
    let now = b.now_ms();
    let actions = b.inner.lock().lifecycle.sleep_all_background(now);
    tabs::apply_actions(b, actions);
    tabs::emit_all_states(b);
    b.lifecycle_signal.notify();
}
