// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 EON contributors
//
// Additional permission under GNU GPL version 3 section 7:
// see LICENSE-EXCEPTION.md (EON Module ABI Exception 1.0).

//! EON Account in EON Stream (plan madde 46): one account per profile, and the
//! profile's watch progress and scores kept in the account's vault, sealed on
//! this computer before they leave it.
//!
//! **Nothing here contacts anything until the viewer enters a server address.**
//! The project runs no server yet (madde 30), so the address starts empty.
//! A guest's progress and scores stay where they always were, in the
//! profile's files beside the executable, and a signed-in viewer's stay there
//! too: the vault is a copy that follows them, not the only place they live.
//!
//! ## Merging
//!
//! Each document is `{"v": 1, "entries": {key: item}, "removed": {key: when}}`.
//! Two copies merge entry by entry: the newer item wins, and a removal wins
//! over anything older than it. A removal is noticed by comparing this
//! profile's entries with the keys it had after the last sync, so no removal
//! site in the program has to remember to report one.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};

use eon_account_client::Account;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

use crate::commands::Shared;

/// Where a profile's account lives: its own folder beside its other files.
pub fn dir(profile: &str) -> PathBuf {
    crate::profiles::file(profile, "eon-account")
}

/// What the profile has told the account so far, beside the account folder.
const STATE: &str = "eon-account-sync.json";

/// Removals are remembered this long, then forgotten: any device that has not
/// synced in half a year is told about the removal by its absence instead.
const REMOVED_KEEP_SECS: u64 = 180 * 86_400;

#[derive(Default, Serialize, Deserialize)]
pub struct SyncState {
    /// The welcome has asked this profile to sign in once.
    #[serde(default)]
    pub prompted: bool,
    /// When the last sync finished.
    #[serde(default)]
    pub last_sync: Option<u64>,
    /// The keys each document had after the last sync.
    #[serde(default)]
    pub known: BTreeMap<String, BTreeSet<String>>,
}

impl SyncState {
    pub fn load(profile: &str) -> Self {
        std::fs::read_to_string(crate::profiles::file(profile, STATE))
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, profile: &str) {
        if let Ok(text) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(crate::profiles::file(profile, STATE), text);
        }
    }
}

/// What the interface may know: never a token, a key or a password.
#[derive(Serialize)]
pub struct AccountView {
    pub server: Option<String>,
    pub username: Option<String>,
    pub prompted: bool,
    pub last_sync: Option<u64>,
    pub syncing: bool,
}

static SYNCING: AtomicBool = AtomicBool::new(false);

pub fn view(profile: &str) -> AccountView {
    let account = Account::open(&dir(profile));
    let state = SyncState::load(profile);
    AccountView {
        server: account.server(),
        username: account.username(),
        prompted: state.prompted,
        last_sync: state.last_sync,
        syncing: SYNCING.load(Ordering::Relaxed),
    }
}

/// How this computer is named in the account's session list: the program and
/// the operating system, nothing that identifies the machine.
pub fn device_name() -> String {
    let os = match std::env::consts::OS {
        "windows" => "Windows",
        "macos" => "macOS",
        "linux" => "Linux",
        other => other,
    };
    format!("EON Stream on {os}")
}

/// The key an item is stored under in a document.
fn key_of(item: &Value) -> Option<String> {
    Some(format!(
        "{}:{}",
        item.get("content_type")?.as_str()?,
        item.get("id")?.as_str()?
    ))
}

/// A document from this profile's own items, with the removals noticed since
/// the last sync.
pub fn local_document(
    items: &Map<String, Value>,
    known: Option<&BTreeSet<String>>,
    now: u64,
) -> Value {
    let removed: Map<String, Value> = known
        .into_iter()
        .flatten()
        .filter(|k| !items.contains_key(*k))
        .map(|k| (k.clone(), json!(now)))
        .collect();
    json!({"v": 1, "entries": items, "removed": removed})
}

fn stamp(item: &Value, field: &str) -> u64 {
    item.get(field).and_then(Value::as_u64).unwrap_or(0)
}

/// Two copies of a document made one. `field` names each item's timestamp.
pub fn merge(a: &Value, b: &Value, field: &str, now: u64) -> Value {
    let mut removed: Map<String, Value> = Map::new();
    for doc in [a, b] {
        if let Some(map) = doc.get("removed").and_then(Value::as_object) {
            for (k, when) in map {
                let when = when.as_u64().unwrap_or(0);
                let keep = removed
                    .get(k)
                    .and_then(Value::as_u64)
                    .unwrap_or(0)
                    .max(when);
                if keep + REMOVED_KEEP_SECS > now {
                    removed.insert(k.clone(), json!(keep));
                }
            }
        }
    }
    let mut entries: Map<String, Value> = Map::new();
    for doc in [a, b] {
        if let Some(map) = doc.get("entries").and_then(Value::as_object) {
            for (k, item) in map {
                let newer = entries
                    .get(k)
                    .is_none_or(|have| stamp(item, field) > stamp(have, field));
                if newer {
                    entries.insert(k.clone(), item.clone());
                }
            }
        }
    }
    entries.retain(|k, item| {
        removed
            .get(k)
            .and_then(Value::as_u64)
            .is_none_or(|when| stamp(item, field) > when)
    });
    // A removal that something newer replaced has done its work.
    removed.retain(|k, _| !entries.contains_key(k));
    json!({"v": 1, "entries": entries, "removed": removed})
}

/// The documents a profile keeps in its vault: (vault name, timestamp field).
const DOCS: [(&str, &str); 2] = [("history", "updated_at"), ("ratings", "at")];

/// The profile's items of one document, keyed.
fn items_of(session: &crate::state::Session, name: &str) -> Map<String, Value> {
    let list: Vec<Value> = match name {
        "history" => serde_json::to_value(&session.history)
            .ok()
            .and_then(|v| v.get("entries").and_then(Value::as_object).cloned())
            .map(|m| m.into_iter().map(|(_, v)| v).collect())
            .unwrap_or_default(),
        _ => serde_json::to_value(&session.ratings)
            .ok()
            .and_then(|v| v.get("items").and_then(Value::as_array).cloned())
            .unwrap_or_default(),
    };
    list.into_iter()
        .filter_map(|item| key_of(&item).map(|k| (k, item)))
        .collect()
}

/// Puts merged items back into the profile. Returns false if they did not read.
fn apply(session: &mut crate::state::Session, name: &str, entries: &Map<String, Value>) -> bool {
    match name {
        "history" => {
            let doc = json!({ "entries": entries });
            match serde_json::from_value(doc) {
                Ok(history) => {
                    session.history = history;
                    session.save_history();
                    true
                }
                Err(_) => false,
            }
        }
        _ => {
            let doc = json!({ "items": entries.values().collect::<Vec<_>>() });
            match serde_json::from_value(doc) {
                Ok(ratings) => {
                    session.ratings = ratings;
                    session.ratings.save(&session.profiles.current);
                    true
                }
                Err(_) => false,
            }
        }
    }
}

/// Brings this profile and its vault together. Does nothing when no one is
/// signed in. Holds the session lock only to copy out and to fold back.
pub fn sync(shared: &Shared) -> Result<(), eon_account_client::Error> {
    if SYNCING.swap(true, Ordering::AcqRel) {
        return Ok(());
    }
    let result = sync_inner(shared);
    SYNCING.store(false, Ordering::Release);
    result
}

fn sync_inner(shared: &Shared) -> Result<(), eon_account_client::Error> {
    let lost = || eon_account_client::Error::Io("the host lost its own state".into());
    let now = eon_stream_core::history::now_unix();
    let (profile, locals) = {
        let session = shared.lock().map_err(|_| lost())?;
        let profile = session.profiles.current.clone();
        let state = SyncState::load(&profile);
        let locals: Vec<(&str, &str, Value)> = DOCS
            .iter()
            .map(|(name, field)| {
                let items = items_of(&session, name);
                (
                    *name,
                    *field,
                    local_document(&items, state.known.get(*name), now),
                )
            })
            .collect();
        (profile, locals)
    };
    let account = Account::open(&dir(&profile));
    if account.username().is_none() {
        return Ok(());
    }
    let mut merged = Vec::new();
    for (name, field, local) in locals {
        let bytes = serde_json::to_vec(&local).unwrap_or_default();
        let result = account.vault_sync("stream", name, &bytes, |mine, theirs| {
            let mine: Value = serde_json::from_slice(mine).unwrap_or(Value::Null);
            let theirs: Value = serde_json::from_slice(theirs).unwrap_or(Value::Null);
            serde_json::to_vec(&merge(&mine, &theirs, field, now)).unwrap_or_default()
        })?;
        let doc: Value = serde_json::from_slice(&result).unwrap_or(Value::Null);
        merged.push((name, doc));
    }
    let mut session = shared.lock().map_err(|_| lost())?;
    if session.profiles.current != profile {
        // Someone switched profiles meanwhile; the next sync will do it.
        return Ok(());
    }
    let mut state = SyncState::load(&profile);
    for (name, doc) in merged {
        let entries = doc
            .get("entries")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        if apply(&mut session, name, &entries) {
            state
                .known
                .insert(name.to_owned(), entries.keys().cloned().collect());
        }
    }
    state.last_sync = Some(now);
    state.save(&profile);
    Ok(())
}

/// Syncs on a thread of its own; a failure waits for the next one.
pub fn sync_soon(shared: &Shared) {
    let shared = std::sync::Arc::clone(shared);
    std::thread::spawn(move || {
        let _ = sync(&shared);
    });
}

/// Every few minutes while the program runs, and once at start.
pub fn keep_in_sync(shared: &Shared) {
    let shared = std::sync::Arc::clone(shared);
    std::thread::spawn(move || loop {
        let _ = sync(&shared);
        std::thread::sleep(std::time::Duration::from_secs(180));
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(id: &str, at: u64) -> Value {
        json!({"content_type": "movie", "id": id, "name": id, "at": at})
    }

    fn doc(items: &[Value], removed: &[(&str, u64)]) -> Value {
        let entries: Map<String, Value> = items
            .iter()
            .map(|i| (key_of(i).unwrap_or_default(), i.clone()))
            .collect();
        let removed: Map<String, Value> = removed
            .iter()
            .map(|(k, w)| ((*k).to_owned(), json!(w)))
            .collect();
        json!({"v": 1, "entries": entries, "removed": removed})
    }

    fn keys(d: &Value) -> Vec<String> {
        d["entries"]
            .as_object()
            .map(|m| m.keys().cloned().collect())
            .unwrap_or_default()
    }

    #[test]
    fn the_newer_copy_of_an_item_wins_from_either_side() {
        let a = doc(&[item("x", 10), item("y", 5)], &[]);
        let b = doc(&[item("x", 20), item("z", 1)], &[]);
        let m = merge(&a, &b, "at", 100);
        assert_eq!(keys(&m), ["movie:x", "movie:y", "movie:z"]);
        assert_eq!(m["entries"]["movie:x"]["at"], 20);
        assert_eq!(merge(&b, &a, "at", 100), m, "order does not matter");
    }

    #[test]
    fn a_removal_beats_older_items_and_loses_to_newer_ones() {
        let here = doc(&[], &[("movie:x", 50)]);
        let there = doc(&[item("x", 40)], &[]);
        assert!(keys(&merge(&here, &there, "at", 100)).is_empty());
        let rescored = doc(&[item("x", 60)], &[]);
        let m = merge(&here, &rescored, "at", 100);
        assert_eq!(keys(&m), ["movie:x"]);
        assert!(m["removed"].as_object().is_some_and(Map::is_empty));
    }

    #[test]
    fn removals_are_noticed_from_what_was_there_last_time() {
        let mut items = Map::new();
        items.insert("movie:y".into(), item("y", 5));
        let known: BTreeSet<String> = ["movie:x".to_owned(), "movie:y".to_owned()].into();
        let d = local_document(&items, Some(&known), 77);
        assert_eq!(d["removed"]["movie:x"], 77);
        assert!(d["removed"].get("movie:y").is_none());
        assert!(local_document(&items, None, 77)["removed"]
            .as_object()
            .is_some_and(Map::is_empty));
    }

    #[test]
    fn old_removals_are_forgotten() {
        let d = doc(&[], &[("movie:x", 1)]);
        let m = merge(&d, &doc(&[], &[]), "at", 1 + REMOVED_KEEP_SECS);
        assert!(m["removed"].as_object().is_some_and(Map::is_empty));
    }

    #[test]
    fn the_device_name_says_nothing_about_the_machine() {
        assert!(device_name().starts_with("EON Stream on "));
    }
}
