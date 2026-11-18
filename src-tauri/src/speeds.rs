// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 EON contributors
//
// Additional permission under GNU GPL version 3 section 7:
// see LICENSE-EXCEPTION.md (EON Module ABI Exception 1.0).

//! How fast each torrent really was, for sorting sources by speed.
//!
//! A torrent's speed cannot be known without joining its swarm. What an
//! addon reports -- seeders -- is a tracker's count from whenever the addon
//! last asked, and the DHT's own peer lists are polluted enough to count
//! peers for a hash nobody has (measured: fourteen for a made-up one, one
//! for a healthy swarm, in the same eight seconds). So this keeps what was
//! *measured*: every torrent that plays has its download speed recorded
//! while it fills the prebuffer and while it plays unthrottled, as a running
//! average, in `eon-speeds.json` beside the executable. A source played
//! before is sorted by its real speed; one never played, by an estimate from
//! its seeders, and the interface says which is which.
//!
//! Only info hashes and numbers are kept -- no titles -- and at most the
//! thousand most recent.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::state::beside_exe;

const FILE: &str = "eon-speeds.json";
const KEPT: usize = 1000;

/// One torrent's measured speed.
#[derive(Clone, Copy, Serialize, Deserialize)]
pub struct Speed {
    /// Bytes per second, averaged.
    pub bps: u64,
    /// How many measurements the average holds, capped so a swarm that
    /// changed is noticed.
    pub samples: u32,
    /// When it was last measured, Unix seconds.
    pub at: u64,
}

/// Every measured speed, by lowercase info hash.
#[derive(Default, Serialize, Deserialize)]
pub struct Speeds(HashMap<String, Speed>);

impl Speeds {
    pub fn load() -> Self {
        std::fs::read_to_string(beside_exe(FILE))
            .ok()
            .and_then(|json| serde_json::from_str(&json).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) {
        if let Ok(json) = serde_json::to_string(self) {
            let _ = std::fs::write(beside_exe(FILE), json);
        }
    }

    /// A speed for a hash, if one was ever measured.
    #[must_use]
    pub fn get(&self, info_hash: &str) -> Option<u64> {
        self.0
            .get(&info_hash.to_ascii_lowercase())
            .map(|speed| speed.bps)
    }

    /// Fold a measurement in. Readings below 1 KB/s are a swarm not yet
    /// connected rather than a speed, and are dropped.
    pub fn record(&mut self, info_hash: &str, bps: u64) {
        if bps < 1024 {
            return;
        }
        let now = eon_stream_core::history::now_unix();
        let entry = self
            .0
            .entry(info_hash.to_ascii_lowercase())
            .or_insert(Speed {
                bps,
                samples: 0,
                at: now,
            });
        let n = u64::from(entry.samples.min(19));
        entry.bps = (entry.bps * n + bps) / (n + 1);
        entry.samples = entry.samples.saturating_add(1);
        entry.at = now;
        if self.0.len() > KEPT {
            let mut by_age: Vec<(String, u64)> = self
                .0
                .iter()
                .map(|(hash, speed)| (hash.clone(), speed.at))
                .collect();
            by_age.sort_by_key(|(_, at)| *at);
            for (hash, _) in by_age.into_iter().take(self.0.len() - KEPT) {
                self.0.remove(&hash);
            }
        }
    }
}

/// A speed to sort by: measured when there is one, otherwise estimated from
/// the seeders an addon reported. The estimate is deliberately modest -- a
/// seeder rarely gives more than a few dozen kilobytes a second to a
/// stranger -- so a measured source is not buried under a big, stale count.
#[must_use]
pub fn estimate(seeders: Option<u32>) -> u64 {
    u64::from(seeders.unwrap_or_default()).saturating_mul(24 * 1024)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_average_moves_towards_new_readings() {
        let mut speeds = Speeds::default();
        speeds.record("ABC", 1_000_000);
        assert_eq!(speeds.get("abc"), Some(1_000_000));
        speeds.record("abc", 3_000_000);
        assert_eq!(speeds.get("abc"), Some(2_000_000));
        // Noise, not a speed.
        speeds.record("abc", 10);
        assert_eq!(speeds.get("abc"), Some(2_000_000));
    }

    #[test]
    fn a_seeder_is_worth_a_modest_amount() {
        assert_eq!(estimate(None), 0);
        assert_eq!(estimate(Some(10)), 10 * 24 * 1024);
    }
}
