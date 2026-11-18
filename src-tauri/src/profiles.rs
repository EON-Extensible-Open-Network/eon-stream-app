// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 EON contributors
//
// Additional permission under GNU GPL version 3 section 7:
// see LICENSE-EXCEPTION.md (EON Module ABI Exception 1.0).

//! Profiles: several people on one computer, each with their own continue
//! watching, library and scores.
//!
//! A profile is local and is not an account (madde 7): no password, nothing
//! sent anywhere, the same as one person keeping a notebook. What a profile
//! owns is what is personal -- history, library, scores. What a computer
//! owns stays shared -- addons, settings, the torrent cache, downloads.
//!
//! The first profile, `main`, keeps its files where they always were,
//! beside the executable, so a library from before profiles is simply the
//! main profile's. Others live in `eon-profiles/<id>/`. The list, which one
//! is in use, and whether the first-run setup was done are in
//! `eon-profiles.json`.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::state::beside_exe;

const FILE: &str = "eon-profiles.json";
const FOLDER: &str = "eon-profiles";
pub const MAIN: &str = "main";
const MAX_PROFILES: usize = 8;

#[derive(Clone, Serialize, Deserialize)]
pub struct Profile {
    pub id: String,
    pub name: String,
    /// Which of the avatar colours, 0 to 5.
    pub color: u8,
    /// A child's profile: only titles a catalogue files under family and
    /// animation, in rows, the feed and search alike.
    #[serde(default)]
    pub kids: bool,
    /// Whether opening it asks for a PIN. The PIN itself is in
    /// `eon-pins.json`, as a hash, and never reaches the interface.
    #[serde(default)]
    pub locked: bool,
}

const PINS: &str = "eon-pins.json";

/// A profile's PIN, as stored. A household lock, like a streaming service's
/// profile PIN: it keeps a child out of a parent's profile, not a person
/// with the computer's files out of anything.
fn pin_hash(profile: &str, pin: &str) -> String {
    eon_stream_core::signature::content_hash(format!("eon-profile-pin:{profile}:{pin}").as_bytes())
}

fn pins() -> std::collections::HashMap<String, String> {
    std::fs::read_to_string(beside_exe(PINS))
        .ok()
        .and_then(|json| serde_json::from_str(&json).ok())
        .unwrap_or_default()
}

/// Whether a PIN opens a profile. A profile without one opens with anything.
#[must_use]
pub fn pin_opens(profile: &str, pin: Option<&str>) -> bool {
    match pins().get(profile) {
        None => true,
        Some(stored) => pin.is_some_and(|pin| pin_hash(profile, pin) == *stored),
    }
}

/// Set a profile's PIN, or clear it with `None`.
///
/// # Errors
///
/// As a message key: a PIN that is not four digits.
pub fn set_pin(profile: &str, pin: Option<&str>) -> Result<(), &'static str> {
    let mut all = pins();
    match pin {
        Some(pin) if pin.len() == 4 && pin.chars().all(|c| c.is_ascii_digit()) => {
            all.insert(profile.to_owned(), pin_hash(profile, pin));
        }
        Some(_) => return Err("ui.profiles.pinform"),
        None => {
            all.remove(profile);
        }
    }
    if let Ok(json) = serde_json::to_string_pretty(&all) {
        let _ = std::fs::write(beside_exe(PINS), json);
    }
    Ok(())
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Profiles {
    pub profiles: Vec<Profile>,
    pub current: String,
    /// Whether the first-run setup was finished or skipped.
    #[serde(default)]
    pub onboarded: bool,
    /// How the window opens: `fullscreen`, `maximized` (the default:
    /// windowed full screen), `small`, `medium` or `large`. The computer's,
    /// not a profile's.
    #[serde(default = "default_window")]
    pub window: String,
}

fn default_window() -> String {
    "maximized".to_owned()
}

/// The window modes there are.
pub const WINDOW_MODES: &[&str] = &["fullscreen", "maximized", "large", "medium", "small"];

impl Default for Profiles {
    fn default() -> Self {
        Self {
            profiles: vec![Profile {
                id: MAIN.to_owned(),
                name: String::new(),
                color: 0,
                kids: false,
                locked: false,
            }],
            current: MAIN.to_owned(),
            onboarded: false,
            window: default_window(),
        }
    }
}

impl Profiles {
    pub fn load() -> Self {
        let loaded: Option<Self> = std::fs::read_to_string(beside_exe(FILE))
            .ok()
            .and_then(|json| serde_json::from_str(&json).ok());
        match loaded {
            Some(mut profiles) => {
                if !profiles.profiles.iter().any(|p| p.id == profiles.current) {
                    profiles.current = MAIN.to_owned();
                }
                profiles
            }
            // No list yet: either a first run, or a copy from before
            // profiles. In the second case there is a library already, and
            // the setup would be asking about things already set.
            None => Self {
                onboarded: beside_exe("eon-addons.json").is_file(),
                ..Self::default()
            },
        }
    }

    pub fn save(&self) {
        if let Ok(json) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(beside_exe(FILE), json);
        }
    }

    /// Add a profile. Answers it.
    ///
    /// # Errors
    ///
    /// As a message key: an empty name, or already as many as there may be.
    pub fn add(&mut self, name: &str, color: u8) -> Result<Profile, &'static str> {
        let name = name.trim();
        if name.is_empty() || name.chars().count() > 24 {
            return Err("ui.profiles.badname");
        }
        if self.profiles.len() >= MAX_PROFILES {
            return Err("ui.profiles.toomany");
        }
        let base: String = name
            .chars()
            .filter(char::is_ascii_alphanumeric)
            .map(|c| c.to_ascii_lowercase())
            .take(12)
            .collect();
        let base = if base.is_empty() {
            "p".to_owned()
        } else {
            base
        };
        let mut id = base.clone();
        let mut n = 2;
        while id == MAIN || self.profiles.iter().any(|p| p.id == id) {
            id = format!("{base}{n}");
            n += 1;
        }
        let profile = Profile {
            id,
            name: name.to_owned(),
            color: color % 6,
            kids: false,
            locked: false,
        };
        self.profiles.push(profile.clone());
        self.save();
        Ok(profile)
    }

    /// Remove a profile and everything it kept.
    ///
    /// # Errors
    ///
    /// As a message key: the main profile, or the one in use.
    pub fn remove(&mut self, id: &str) -> Result<(), &'static str> {
        if id == MAIN || id == self.current {
            return Err("ui.profiles.cannotremove");
        }
        self.profiles.retain(|p| p.id != id);
        self.save();
        // Its own folder, named by its id under ours.
        let folder = beside_exe(FOLDER).join(id);
        if folder.is_dir() {
            let _ = std::fs::remove_dir_all(folder);
        }
        Ok(())
    }

    pub fn rename(&mut self, id: &str, name: &str, color: u8) -> Result<(), &'static str> {
        let name = name.trim();
        if name.chars().count() > 24 {
            return Err("ui.profiles.badname");
        }
        let profile = self
            .profiles
            .iter_mut()
            .find(|p| p.id == id)
            .ok_or("ui.profiles.unknown")?;
        profile.name = name.to_owned();
        profile.color = color % 6;
        self.save();
        Ok(())
    }
}

impl Profiles {
    /// Whether the profile in use is a child's.
    #[must_use]
    pub fn kids(&self) -> bool {
        self.profiles.iter().any(|p| p.id == self.current && p.kids)
    }
}

/// The genres a child's profile is shown.
pub const KIDS_GENRES: &[&str] = &["animation", "family", "kids", "children"];

/// Whether a title's genres make it a child's.
#[must_use]
pub fn for_kids(genres: &[String]) -> bool {
    genres
        .iter()
        .any(|g| KIDS_GENRES.contains(&g.to_lowercase().as_str()))
}

/// Where a profile keeps one of its files.
#[must_use]
pub fn file(profile: &str, name: &str) -> PathBuf {
    if profile == MAIN {
        return beside_exe(name);
    }
    let folder = beside_exe(FOLDER).join(profile);
    let _ = std::fs::create_dir_all(&folder);
    folder.join(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_profile_gets_an_id_of_its_own() {
        let mut profiles = Profiles {
            profiles: vec![Profile {
                id: MAIN.into(),
                name: "Sadık".into(),
                color: 0,
                kids: false,
                locked: false,
            }],
            current: MAIN.into(),
            onboarded: true,
            window: default_window(),
        };
        // Saving writes beside the test binary; harmless.
        let a = profiles.add("Ayşe", 1).expect("added");
        let b = profiles.add("Ayşe", 2).expect("added again");
        assert_eq!(a.id, "aye");
        assert_eq!(b.id, "aye2");
        assert!(profiles.add("  ", 0).is_err());
        assert!(profiles.remove(MAIN).is_err());
        assert!(profiles.remove(&a.id).is_ok());
        assert_eq!(profiles.profiles.len(), 2);
    }
}
