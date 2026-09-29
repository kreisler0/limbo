//! Frecency: a simplified port of Firefox's algorithm (frequency x recency).
//!
//! Each of the most recent visits (up to [`SAMPLE_SIZE`]) earns points from its
//! age bucket, scaled by a bonus for how the visit happened (typed visits count
//! double). The page score is the average points per sampled visit, scaled by
//! the total visit count.

use serde::{Deserialize, Serialize};

pub const US_PER_DAY: i64 = 86_400_000_000;
/// Firefox samples the 10 most recent visits.
pub const SAMPLE_SIZE: usize = 10;

/// Visit transition types. The numeric values match Firefox's
/// `moz_historyvisits.visit_type` so imported rows map 1:1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(i64)]
pub enum Transition {
    Link = 1,
    Typed = 2,
    Bookmark = 3,
    Embed = 4,
    RedirectPermanent = 5,
    RedirectTemporary = 6,
    Download = 7,
    FramedLink = 8,
    Reload = 9,
}

impl Transition {
    pub fn from_i64(v: i64) -> Transition {
        match v {
            2 => Transition::Typed,
            3 => Transition::Bookmark,
            4 => Transition::Embed,
            5 => Transition::RedirectPermanent,
            6 => Transition::RedirectTemporary,
            7 => Transition::Download,
            8 => Transition::FramedLink,
            9 => Transition::Reload,
            _ => Transition::Link,
        }
    }

    /// Bonus in percent.
    fn bonus(self) -> i64 {
        match self {
            Transition::Link | Transition::FramedLink => 100,
            Transition::Typed => 200,
            Transition::Bookmark => 140,
            Transition::RedirectPermanent => 50,
            Transition::RedirectTemporary => 25,
            Transition::Embed | Transition::Download | Transition::Reload => 0,
        }
    }
}

/// Recency weight for a visit `age_us` old: buckets at 4/14/31/90 days.
pub fn recency_weight(age_us: i64) -> i64 {
    let days = age_us.max(0) / US_PER_DAY;
    match days {
        0..=4 => 100,
        5..=14 => 70,
        15..=31 => 50,
        32..=90 => 30,
        _ => 10,
    }
}

/// Computes frecency.
///
/// * `recent_visits`: `(visit_time_us, transition)`, most recent first; only the
///   first [`SAMPLE_SIZE`] are used.
/// * `visit_count`: total visits for the page (may exceed the sample).
/// * `bookmarked`: pages that are bookmarked but never visited still rank.
pub fn compute(recent_visits: &[(i64, Transition)], visit_count: i64, bookmarked: bool, now_us: i64) -> i64 {
    let sample = &recent_visits[..recent_visits.len().min(SAMPLE_SIZE)];
    if sample.is_empty() {
        // Firefox gives unvisited bookmarks a small positive score so they're suggested.
        return if bookmarked { 140 } else { 0 };
    }
    let mut points = 0i64;
    for &(time, transition) in sample {
        let mut bonus = transition.bonus();
        if bookmarked && transition == Transition::Link {
            bonus += 40;
        }
        points += recency_weight(now_us - time) * bonus / 100;
    }
    if points == 0 {
        return 0;
    }
    let count = visit_count.max(sample.len() as i64);
    // ceil(count * points / sampled)
    let n = sample.len() as i64;
    (count * points + n - 1) / n
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_800_000_000_000_000;

    fn days_ago(d: i64) -> i64 {
        NOW - d * US_PER_DAY
    }

    #[test]
    fn buckets() {
        assert_eq!(recency_weight(0), 100);
        assert_eq!(recency_weight(4 * US_PER_DAY), 100);
        assert_eq!(recency_weight(5 * US_PER_DAY), 70);
        assert_eq!(recency_weight(14 * US_PER_DAY), 70);
        assert_eq!(recency_weight(31 * US_PER_DAY), 50);
        assert_eq!(recency_weight(90 * US_PER_DAY), 30);
        assert_eq!(recency_weight(91 * US_PER_DAY), 10);
        assert_eq!(recency_weight(-5), 100, "clock skew counts as now");
    }

    #[test]
    fn typed_counts_double() {
        let link = compute(&[(days_ago(1), Transition::Link)], 1, false, NOW);
        let typed = compute(&[(days_ago(1), Transition::Typed)], 1, false, NOW);
        assert_eq!(link, 100);
        assert_eq!(typed, 200);
    }

    #[test]
    fn recent_beats_old_and_frequent_beats_rare() {
        let recent = compute(&[(days_ago(1), Transition::Link)], 1, false, NOW);
        let old = compute(&[(days_ago(200), Transition::Link)], 1, false, NOW);
        assert!(recent > old);
        let visits: Vec<_> = (0..10).map(|i| (days_ago(i), Transition::Link)).collect();
        let frequent = compute(&visits, 50, false, NOW);
        assert!(frequent > recent * 10);
    }

    #[test]
    fn sample_is_capped_but_count_scales() {
        let visits: Vec<_> = (0..30).map(|_| (days_ago(1), Transition::Link)).collect();
        assert_eq!(compute(&visits, 30, false, NOW), 30 * 100);
    }

    #[test]
    fn unvisited_bookmark_and_zero_bonus() {
        assert_eq!(compute(&[], 0, true, NOW), 140);
        assert_eq!(compute(&[], 0, false, NOW), 0);
        assert_eq!(compute(&[(days_ago(1), Transition::Embed)], 1, false, NOW), 0);
    }

    #[test]
    fn transition_roundtrip() {
        for v in 1..=9 {
            assert_eq!(Transition::from_i64(v) as i64, v);
        }
        assert_eq!(Transition::from_i64(42), Transition::Link);
    }
}
