// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 EON contributors
//
// Additional permission under GNU GPL version 3 section 7:
// see LICENSE-EXCEPTION.md (EON Module ABI Exception 1.0).

//! Module and theme commands.
//!
//! ## There are no project signing keys yet, and that is visible here
//!
//! The module manager is complete and it refuses everything, because nothing
//! has been signed: the key hierarchy's custody transfers to a legal entity
//! that does not exist yet (madde 30, and madde 34 for the hierarchy). So a
//! fresh install has an **empty trust set**, and an empty trust set accepts no
//! module at all.
//!
//! That is the correct behaviour rather than a gap, and the error says so
//! instead of leaving someone to wonder. What it is not is untested: the trust
//! set is read from `eon-trust.json` beside the executable, so anyone can mint
//! a key, sign a module and watch the whole chain work end to end. That file is
//! a statement of what this machine trusts, which is the same mechanism an
//! institution uses for its own key — not a development backdoor, and there
//! isn't one.

use std::{fs, path::PathBuf};

use eon_stream_core::{
    module::ModuleManifest,
    modules::{DisabledReason, ModuleStore, PreparedInstall},
    revocation::RevocationStore,
    signature::{TrustFreshness, TrustStore},
    theme::{ContrastFinding, ThemeBase, ThemeDocument},
    BuildProfile, ModuleKind,
};

use crate::text::{say, Text};

/// Read the trust set from beside the executable.
///
/// A missing file is an empty trust set, not an error: that is the state every
/// build ships in today.
pub fn load_trust(path: &PathBuf) -> TrustStore {
    fs::read(path)
        .ok()
        .and_then(|bytes| TrustStore::parse(&bytes).ok())
        .unwrap_or_else(TrustStore::empty)
}

/// Print what this build trusts and whether it is still valid.
pub fn trust(store: &TrustStore, now: i64, text: &Text) {
    if store.keys.is_empty() {
        say!(text, "cli.trust.empty");
        return;
    }
    say!(text, "cli.trust.header", &store.keys.len().to_string());
    for key in &store.keys {
        let state = match key.is_valid_at(now) {
            Ok(true) => text.get("cli.common.ok"),
            Ok(false) => text.format("cli.trust.outside", &[&key.not_after]),
            Err(e) => e.to_string(),
        };
        println!("  {:<20} {:<12} {state}", key.key_id, key.purpose.as_str());
        if let Some(comment) = &key.comment {
            println!("    {comment}");
        }
    }
    match store.freshness(now) {
        Ok(TrustFreshness::Fresh) => {}
        Ok(TrustFreshness::Stale { newest_expiry }) => {
            say!(text, "cli.trust.stale", &newest_expiry);
        }
        Ok(TrustFreshness::Empty) => say!(text, "cli.trust.empty"),
        Err(e) => println!("  {e}"),
    }
}

/// Print the installed modules.
pub fn list(store: &ModuleStore, text: &Text) {
    if store.is_empty() {
        say!(text, "cli.module.list.empty");
        return;
    }
    say!(text, "cli.module.list.header", &store.len().to_string());
    for (position, module) in store.all().iter().enumerate() {
        let state = if module.enabled {
            text.get("cli.common.enabled")
        } else {
            module
                .disabled_reason
                .as_ref()
                .map_or_else(|| text.get("cli.common.disabled"), |r| reason_text(r, text))
        };
        println!(
            "{:>3}. {} {}  [{}]  {state}",
            position + 1,
            module.manifest.localised_name(text.locale()),
            module.manifest.version,
            module.manifest.kind.as_str()
        );
        println!("     {}  ·  {}", module.id(), module.signed_by);
    }
    match store.load_order() {
        Ok(order) if order.len() > 1 => {
            say!(text, "cli.module.loadorder", &order.join(" -> "));
        }
        Ok(_) => {}
        Err(e) => println!("  {e}"),
    }
}

/// Install a module from a manifest file.
///
/// The manifest's `runtime.entry` names the content the signature covers, and
/// it is read from beside the manifest. Taking the content from anywhere else
/// would make the content hash describe something other than what gets
/// installed.
pub fn install(
    store: &mut ModuleStore,
    trust: &TrustStore,
    revocations: &RevocationStore,
    now: i64,
    path: &str,
    text: &Text,
    confirm: impl FnOnce(&PreparedInstall, &Text) -> bool,
) -> bool {
    let manifest_path = PathBuf::from(path);
    let bytes = match fs::read(&manifest_path) {
        Ok(bytes) => bytes,
        Err(e) => {
            say!(text, "cli.module.unreadable", path, &e.to_string());
            return false;
        }
    };
    let manifest = match ModuleManifest::parse(&bytes) {
        Ok(manifest) => manifest,
        Err(e) => {
            println!("{e}");
            return false;
        }
    };

    // Owned, so the manifest can be moved into `prepare` afterwards.
    let Some(entry) = manifest.runtime.as_ref().and_then(|r| r.entry.clone()) else {
        say!(text, "cli.module.noentry", &manifest.id);
        return false;
    };
    let content_path = manifest_path
        .parent()
        .unwrap_or(std::path::Path::new("."))
        .join(&entry);
    let content = match fs::read(&content_path) {
        Ok(content) => content,
        Err(e) => {
            say!(
                text,
                "cli.module.unreadable",
                &content_path.display().to_string(),
                &e.to_string()
            );
            return false;
        }
    };

    let prepared = match store.prepare(manifest, &content, trust, revocations, now) {
        Ok(prepared) => prepared,
        Err(e) => {
            println!("{e}");
            if trust.keys.is_empty() {
                // The most likely reason anything is refused today, and the
                // one nobody would guess from "the trust set is stale".
                say!(text, "cli.module.nokeysyet");
            }
            return false;
        }
    };

    // The permission prompt, before anything is installed (madde 3). In v1 the
    // list is always empty because only declarative modules install, and the
    // prompt still happens: the habit is the point.
    describe_prepared(&prepared, text);
    if !confirm(&prepared, text) {
        say!(text, "cli.common.cancelled");
        return false;
    }

    let replaced = prepared.replaces().map(ToString::to_string);
    let signed_by = prepared.signed_by().to_owned();
    let version = prepared.manifest().version.clone();
    let name = prepared.manifest().localised_name(text.locale()).to_owned();

    let module_id = prepared.manifest().id.clone();

    match store.commit(prepared) {
        Ok(installed) => {
            match replaced {
                Some(previous) => {
                    say!(text, "cli.module.updated", &name, &previous, &version);
                }
                None => say!(text, "cli.module.installed", &name, &version, &signed_by),
            }
            if let Some(reason) = &installed.disabled_reason {
                println!("  {}", reason_text(reason, text));
            }
            // The content is copied in only after the manifest is committed,
            // and only the bytes that were hashed and verified -- not the
            // directory it came from. A module's installed content is
            // therefore exactly what the signature covers, which is what makes
            // re-checking it later meaningful.
            if let Err(e) = store_content(&module_id, &entry, &content) {
                say!(text, "cli.module.unreadable", &entry, &e.to_string());
            }
            true
        }
        Err(e) => {
            println!("{e}");
            false
        }
    }
}

/// Where an installed module's own files live.
fn module_directory(id: &str) -> PathBuf {
    crate::beside_exe("eon-modules").join(id)
}

/// Write the verified content into the module's directory.
///
/// `entry` has already been checked by `ModuleManifest::validate` to be a
/// relative path that does not leave the module root, which is what makes this
/// join safe.
fn store_content(id: &str, entry: &str, content: &[u8]) -> std::io::Result<()> {
    let target = module_directory(id).join(entry);
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(target, content)
}

/// One disabled reason, rendered through the catalogue.
///
/// `DisabledReason::describe` is English and is a diagnostic; this is the path
/// for text a person reads (madde 40).
pub fn reason_text(reason: &DisabledReason, text: &Text) -> String {
    let arguments = reason.message_arguments();
    let borrowed: Vec<&str> = arguments.iter().map(String::as_str).collect();
    text.format(reason.message_key(), &borrowed)
}

/// Show what the user is agreeing to.
pub fn describe_prepared(prepared: &PreparedInstall, text: &Text) {
    let manifest = prepared.manifest();
    let name = manifest.localised_name(text.locale());
    println!("\n{name} {}", manifest.version);
    println!("  {}  ·  {}", manifest.id, manifest.kind.as_str());
    if let Some(description) = manifest.localised_description(text.locale()) {
        println!("  {description}");
    }
    say!(text, "cli.module.signature.verified", prepared.signed_by());
    if let Some(previous) = prepared.replaces() {
        say!(text, "cli.module.replaces", &previous.to_string());
    }

    let permissions = prepared.permissions();
    if permissions.is_empty() {
        say!(text, "cli.module.permissions.none", name);
    } else {
        say!(text, "cli.module.permissions.header", name);
        for (permission, rationale) in permissions {
            println!("  - {}", text.get(permission.message_key()));
            if let Some(rationale) = rationale {
                say!(text, "cli.module.permissions.rationale", rationale);
            }
        }
    }

    for (dependency, range) in prepared.missing_required() {
        say!(text, "cli.module.needs", name, dependency, range);
    }
    if !prepared.notes().is_empty() {
        say!(text, "cli.module.review.header");
        for note in prepared.notes() {
            println!("  - {}", note.detail);
        }
    }
}

/// Remove, enable, disable or reorder by list position.
pub fn operate(store: &mut ModuleStore, operation: &str, rest: &[&str], text: &Text) -> bool {
    let Some(id) = resolve_target(store, rest.first().copied(), text) else {
        return false;
    };
    let name = store.get(&id).map_or_else(
        || id.clone(),
        |m| m.manifest.localised_name(text.locale()).to_owned(),
    );

    let outcome = match operation {
        "remove" => store.remove(&id).map(|_| {
            // Its own files go with it. Leaving them behind would mean a
            // reinstall silently reusing content that was never re-verified.
            let _ = fs::remove_dir_all(module_directory(&id));
            "cli.module.removed"
        }),
        "enable" => store.enable(&id).map(|()| "cli.module.enabled"),
        "disable" => store.disable(&id).map(|()| "cli.module.disabled"),
        "order" => {
            let Some(position) = rest.get(1).and_then(|p| p.parse::<usize>().ok()) else {
                say!(text, "cli.error.needsargument");
                return false;
            };
            store
                .reorder(&id, position.saturating_sub(1))
                .map(|()| "cli.module.reordered")
        }
        _ => {
            say!(text, "cli.error.unknowncommand", operation);
            return false;
        }
    };

    match outcome {
        Ok(key) if key == "cli.module.reordered" => {
            let position = rest.get(1).copied().unwrap_or("1");
            say!(text, key, &name, position);
            true
        }
        Ok(key) => {
            say!(text, key, &name);
            true
        }
        Err(e) => {
            println!("{e}");
            false
        }
    }
}

/// Turn a list position, or an identifier, into an identifier.
fn resolve_target(store: &ModuleStore, argument: Option<&str>, text: &Text) -> Option<String> {
    let Some(argument) = argument else {
        say!(text, "cli.error.needsargument");
        return None;
    };
    // A number is a position in the last listing, which is how everything else
    // in this program is addressed. An identifier also works, because a script
    // should not have to count.
    if let Ok(position) = argument.parse::<usize>() {
        let module = position
            .checked_sub(1)
            .and_then(|index| store.all().get(index));
        return match module {
            Some(module) => Some(module.id().to_owned()),
            None => {
                say!(text, "cli.error.outofrange", argument);
                None
            }
        };
    }
    if store.get(argument).is_some() {
        return Some(argument.to_owned());
    }
    say!(text, "cli.module.notfound", argument);
    None
}

// ---------------------------------------------------------------- themes

/// Resolve the active theme and report its contrast.
///
/// Checking contrast from a command line is the only way it can be checked at
/// all right now, and basic accessibility is a v1 exit criterion (madde 35).
/// Doing it before the interface exists means the interface inherits a palette
/// that already passes rather than one that has to be fixed later.
pub fn theme(store: &ModuleStore, selected: Option<&str>, rest: &[&str], text: &Text) {
    let document = match selected {
        None => ThemeDocument::plain(ThemeBase::Dark),
        Some(id) => match load_theme_document(store, id, text) {
            Some(document) => document,
            None => return,
        },
    };

    let resolved = document.resolve();
    match selected {
        Some(id) => say!(text, "cli.theme.applied", id),
        None => say!(text, "cli.theme.cleared"),
    }

    if rest.first().copied() == Some("tokens") {
        say!(text, "cli.theme.tokens.header");
        for (name, value) in resolved.tokens() {
            println!("  {name:<22} {value}");
        }
        return;
    }

    say!(text, "cli.theme.contrast.header");
    for finding in resolved.contrast_report() {
        let minimum = finding
            .requirement
            .minimum()
            .map_or_else(String::new, |m| format!("{m:.1}"));
        let verdict = text.format(finding.verdict_key(), &[&minimum]);
        println!(
            "  {}",
            text.format(
                "cli.theme.contrast.line",
                &[
                    finding.foreground.as_str(),
                    finding.background.as_str(),
                    &format!("{:.2}", finding.ratio),
                    &verdict,
                ],
            )
        );
    }
    let failures: Vec<ContrastFinding> = resolved.contrast_failures();
    if failures.is_empty() {
        say!(text, "cli.theme.contrast.pass");
    } else {
        say!(text, "cli.theme.contrast.fail", &failures.len().to_string());
    }
}

fn load_theme_document(store: &ModuleStore, id: &str, text: &Text) -> Option<ThemeDocument> {
    let Some(module) = store.get(id) else {
        say!(text, "cli.module.notfound", id);
        return None;
    };
    if module.manifest.kind != ModuleKind::Theme {
        say!(text, "cli.theme.notatheme", id);
        return None;
    }
    // The theme document lives where the manifest's entry says, beneath the
    // module's own directory beside the executable.
    let entry = module
        .manifest
        .runtime
        .as_ref()
        .and_then(|r| r.entry.as_deref());
    let Some(entry) = entry else {
        say!(text, "cli.module.noentry", id);
        return None;
    };
    let path = module_directory(id).join(entry);
    match fs::read(&path) {
        Ok(bytes) => match ThemeDocument::parse(&bytes) {
            Ok(document) => Some(document),
            Err(e) => {
                println!("{e}");
                None
            }
        },
        Err(e) => {
            say!(
                text,
                "cli.module.unreadable",
                &path.display().to_string(),
                &e.to_string()
            );
            None
        }
    }
}

/// Read the stored module list, or start an empty one.
pub fn load_store(path: &PathBuf, build: BuildProfile, text: &Text) -> ModuleStore {
    match fs::read_to_string(path) {
        Err(_) => ModuleStore::new(build),
        Ok(stored) => match ModuleStore::from_json(&stored, build) {
            Ok(store) => store,
            Err(e) => {
                // Said rather than silently replaced: a store that will not
                // load is either corrupt or from another build, and starting
                // empty without a word looks like the modules were removed.
                println!("{e}");
                say!(text, "cli.module.storeunreadable");
                ModuleStore::new(build)
            }
        },
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_trust_file_is_an_empty_set_and_not_an_error() {
        // Which is the state every build ships in today.
        let store = load_trust(&PathBuf::from("no-such-trust-file.json"));
        assert!(store.keys.is_empty());
    }

    #[test]
    fn a_position_resolves_to_an_identifier() {
        let text = Text::new("en");
        let store = ModuleStore::new(BuildProfile::Stream);
        // Nothing installed, so every position is out of range -- and says so
        // rather than panicking on the subtraction.
        assert!(resolve_target(&store, Some("1"), &text).is_none());
        assert!(resolve_target(&store, Some("0"), &text).is_none());
        assert!(resolve_target(&store, None, &text).is_none());
        assert!(resolve_target(&store, Some("community.example.theme"), &text).is_none());
    }

    #[test]
    fn the_built_in_theme_passes_its_own_contrast_check() {
        // The same assertion the library makes, from the side a user sees.
        let text = Text::new("en");
        let store = ModuleStore::new(BuildProfile::Stream);
        theme(&store, None, &[], &text);
        let resolved = ThemeDocument::plain(ThemeBase::Dark).resolve();
        assert!(resolved.meets_contrast_requirements());
    }

    #[test]
    fn a_store_from_another_build_is_reported_rather_than_replaced() {
        let text = Text::new("en");
        let directory = std::env::temp_dir().join("eon-cli-module-store-test");
        fs::create_dir_all(&directory).unwrap();
        let path = directory.join("eon-modules.json");
        let edu = ModuleStore::new(BuildProfile::Edu);
        fs::write(&path, edu.to_json().unwrap()).unwrap();

        // Reading an Edu store into a Stream build must not carry Edu modules
        // across (madde 22); it starts empty and says why.
        let store = load_store(&path, BuildProfile::Stream, &text);
        assert_eq!(store.build(), BuildProfile::Stream);
        assert!(store.is_empty());
        let _ = fs::remove_dir_all(directory);
    }

    #[test]
    fn an_absent_store_file_starts_empty_quietly() {
        let text = Text::new("en");
        let store = load_store(
            &PathBuf::from("no-such-module-store.json"),
            BuildProfile::Stream,
            &text,
        );
        assert!(store.is_empty());
    }
}
