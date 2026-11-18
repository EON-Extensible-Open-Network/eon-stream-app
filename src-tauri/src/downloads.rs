// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 EON contributors
//
// Additional permission under GNU GPL version 3 section 7:
// see LICENSE-EXCEPTION.md (EON Module ABI Exception 1.0).

//! Downloads: a title fetched whole, to watch later without a connection.
//!
//! The cache (`keeper`) is the opposite: whatever streams is temporary, and
//! is deleted to stay under its limit. A download is the viewer's, so it is
//! kept out of all of that -- not counted against the cache limit, never
//! given back, never cleared with the buffer, and left on disk when the
//! program closes. Only the one file asked for is fetched, so an episode
//! does not bring its whole season pack with it.
//!
//! The list is `eon-downloads.json` beside the executable. Downloads not
//! finished when the program closed carry on when it opens again. A finished
//! one plays from its file, with nothing on the network.

use std::path::PathBuf;

use eon_stream_engine::TorrentRequest;
use serde::{Deserialize, Serialize};

use crate::state::beside_exe;

const FILE: &str = "eon-downloads.json";

/// What to give the engine to fetch it again.
#[derive(Clone, Serialize, Deserialize)]
pub struct Request {
    #[serde(default)]
    pub magnet: Option<String>,
    pub hex: String,
    #[serde(default)]
    pub trackers: Vec<String>,
}

impl Request {
    #[must_use]
    pub fn from(request: &TorrentRequest) -> Option<Self> {
        match request {
            TorrentRequest::Magnet(magnet) => Some(Self {
                magnet: Some(magnet.clone()),
                hex: request.info_hash().ok()?,
                trackers: Vec::new(),
            }),
            TorrentRequest::InfoHash { hex, trackers } => Some(Self {
                magnet: None,
                hex: hex.to_ascii_lowercase(),
                trackers: trackers.clone(),
            }),
            #[allow(unreachable_patterns)]
            _ => None,
        }
    }

    #[must_use]
    pub fn to_request(&self) -> TorrentRequest {
        match &self.magnet {
            Some(magnet) => TorrentRequest::Magnet(magnet.clone()),
            None => TorrentRequest::InfoHash {
                hex: self.hex.clone(),
                trackers: self.trackers.clone(),
            },
        }
    }
}

/// One download.
#[derive(Clone, Serialize, Deserialize)]
pub struct Download {
    pub request: Request,
    /// Which file of the torrent, once known.
    #[serde(default)]
    pub file: Option<usize>,
    /// The file the addon named, if it named one.
    #[serde(default)]
    pub wanted_file: Option<usize>,
    /// What it is, for the list and for "continue watching".
    pub content_type: String,
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub series_id: Option<String>,
    #[serde(default)]
    pub poster: Option<String>,
    /// The file on disk once it is whole.
    #[serde(default)]
    pub path: Option<PathBuf>,
    #[serde(default)]
    pub length: Option<u64>,
    pub added: u64,
}

#[derive(Default, Serialize, Deserialize)]
pub struct Downloads {
    items: Vec<Download>,
}

impl Downloads {
    pub fn load() -> Self {
        std::fs::read_to_string(beside_exe(FILE))
            .ok()
            .and_then(|json| serde_json::from_str(&json).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) {
        if let Ok(json) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(beside_exe(FILE), json);
        }
    }

    #[must_use]
    pub fn all(&self) -> &[Download] {
        &self.items
    }

    pub fn all_mut(&mut self) -> &mut Vec<Download> {
        &mut self.items
    }

    /// Info hashes of every download: what the cache must never touch.
    #[must_use]
    pub fn hashes(&self) -> Vec<String> {
        self.items.iter().map(|d| d.request.hex.clone()).collect()
    }

    #[must_use]
    pub fn get(&self, hex: &str) -> Option<&Download> {
        self.items
            .iter()
            .find(|d| d.request.hex.eq_ignore_ascii_case(hex))
    }

    pub fn add(&mut self, download: Download) {
        self.items
            .retain(|d| !d.request.hex.eq_ignore_ascii_case(&download.request.hex));
        self.items.push(download);
    }

    pub fn remove(&mut self, hex: &str) -> Option<Download> {
        let at = self
            .items
            .iter()
            .position(|d| d.request.hex.eq_ignore_ascii_case(hex))?;
        Some(self.items.remove(at))
    }
}

/// One download, as the list shows it.
#[derive(Serialize)]
pub struct DownloadView {
    pub hex: String,
    pub name: String,
    pub poster: Option<String>,
    pub content_type: String,
    pub id: String,
    /// 0 to 100.
    pub percent: u8,
    pub done: bool,
    pub down_bps: u64,
    pub length: Option<u64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_request_survives_being_written_down() {
        let request = TorrentRequest::InfoHash {
            hex: "DD8255ECDC7CA55FB0BBF81323D87062DB1F6D1C".to_owned(),
            trackers: vec!["udp://tracker.example:1337".to_owned()],
        };
        let kept = Request::from(&request).expect("an info hash");
        assert_eq!(kept.hex, "dd8255ecdc7ca55fb0bbf81323d87062db1f6d1c");
        let json = serde_json::to_string(&kept).expect("json");
        let back: Request = serde_json::from_str(&json).expect("back");
        assert!(matches!(back.to_request(), TorrentRequest::InfoHash { .. }));
    }
}
