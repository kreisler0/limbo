//! Drives the tab lifecycle timers. The thread sleeps until the next deadline
//! (or until woken), so an idle browser does no periodic work.

use std::time::Duration;

use crate::state::SharedBrowser;
use crate::tabs;

pub fn start(b: &SharedBrowser) {
    let b = b.clone();
    std::thread::Builder::new()
        .name("limbo-lifecycle".into())
        .spawn(move || {
            loop {
                let (actions, deadline) = {
                    let now = b.now_ms();
                    let mut inner = b.inner.lock();
                    let actions = inner.lifecycle.tick(now);
                    (actions, inner.lifecycle.next_deadline())
                };
                if !actions.is_empty() {
                    tabs::apply_actions(&b, actions);
                    tabs::emit_all_states(&b);
                }
                let timeout = deadline.map(|d| Duration::from_millis(d.saturating_sub(b.now_ms()).max(50)));
                b.lifecycle_signal.wait(timeout);
            }
        })
        .expect("spawn lifecycle thread");
}
