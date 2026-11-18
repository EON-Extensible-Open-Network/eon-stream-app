// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 EON contributors
//
// Additional permission under GNU GPL version 3 section 7:
// see LICENSE-EXCEPTION.md (EON Module ABI Exception 1.0).

//! What a title looks like, remembered for the cards that cannot ask.
//!
//! Watch history records what happened -- an episode id, a position -- not
//! what a title looks like, so "Continue watching" had no poster to draw and
//! fell back to initials. Every playback now writes down the title's name,
//! poster and backdrop here, keyed by the title (the series, for an episode),
//! in the profile's `eon-art.json`. Entries from before are filled in once,
//! in the background, by asking the addons (`commands::fill_art`).

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

const FILE: &str = "eon-art.json";
/// Kept for this many titles; the oldest go first.
const KEPT: usize = 600;

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct Look {
    pub name: String,
    #[serde(default)]
    pub poster: Option<String>,
    #[serde(default)]
    pub background: Option<String>,
    #[serde(default)]
    pub at: u64,
    /// Genres and year, for the recommendations on the device.
    #[serde(default)]
    pub genres: Vec<String>,
    #[serde(default)]
    pub year: Option<i32>,
}

impl Look {
    /// What a title's metadata says it looks like.
    #[must_use]
    pub fn of(meta: &eon_stream_core::Meta) -> Self {
        Self {
            name: meta.display_name().to_owned(),
            poster: meta.poster.clone(),
            background: meta.background.clone(),
            at: 0,
            genres: meta.genres.clone(),
            year: crate::recommend::year_of(meta.release_info.as_deref()),
        }
    }
}

#[derive(Default, Serialize, Deserialize)]
pub struct Art(HashMap<String, Look>);

impl Art {
    pub fn load(profile: &str) -> Self {
        std::fs::read_to_string(crate::profiles::file(profile, FILE))
            .ok()
            .and_then(|json| serde_json::from_str(&json).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, profile: &str) {
        if let Ok(json) = serde_json::to_string(self) {
            let _ = std::fs::write(crate::profiles::file(profile, FILE), json);
        }
    }

    #[must_use]
    pub fn get(&self, id: &str) -> Option<&Look> {
        self.0.get(id)
    }

    pub fn put(&mut self, id: &str, look: Look) {
        self.0.insert(
            id.to_owned(),
            Look {
                at: eon_stream_core::history::now_unix(),
                ..look
            },
        );
        if self.0.len() > KEPT {
            if let Some(oldest) = self
                .0
                .iter()
                .min_by_key(|(_, look)| look.at)
                .map(|(id, _)| id.clone())
            {
                self.0.remove(&oldest);
            }
        }
    }
}

/// The title a history entry belongs to: its series for an episode.
#[must_use]
pub fn title_of(entry: &eon_stream_core::WatchEntry) -> (String, String) {
    match &entry.series_id {
        Some(series) => ("series".to_owned(), series.clone()),
        None => (entry.content_type.clone(), entry.id.clone()),
    }
}

/// A title's own name out of what history wrote down: "Dexter · S01E02 ·
/// Crocodile" is Dexter.
#[must_use]
pub fn bare_name(name: &str) -> String {
    name.split(" · ").next().unwrap_or(name).trim().to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_loses_the_episode_it_was_written_with() {
        assert_eq!(bare_name("Dexter · S01E02 · Crocodile"), "Dexter");
        assert_eq!(bare_name("Inception"), "Inception");
    }
}
