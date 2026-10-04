// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 EON contributors
//
// Additional permission under GNU GPL version 3 section 7:
// see LICENSE-EXCEPTION.md (EON Module ABI Exception 1.0).

//! Settings, updates and revocation commands.
//!
//! ## `set` takes a path and a value, and refuses the rest
//!
//! Settings are a typed document, so `set torrent.keepFiles true` either names
//! a setting of the right type or it does not. A path nobody recognises is
//! reported with the list, rather than stored and silently ignored — the same
//! rule the settings file itself follows.
//!
//! ## Update checks are a thing the user asks for
//!
//! `update` checks when typed, and `updates.checkOnStart` is off until turned
//! on. A check is a network request that tells a server this machine exists
//! and which version it runs, which is the one piece of information this
//! program would otherwise never send anywhere (madde 36). Revocation refresh
//! is separate and on, because declining update checks should not quietly also
//! decline finding out that an installed module turned out to be malicious.

use std::{fs, path::PathBuf};

use eon_stream_core::{
    http::HttpClient,
    modules::ModuleStore,
    ranking::Resolution,
    revocation::{RevocationFreshness, RevocationList, RevocationStore},
    settings::{find_forbidden_keys, Settings, UpdateChannel},
    signature::TrustStore,
    update::{ReleaseManifest, UpdateCheck, UpdateDecision},
    BuildProfile,
};

use crate::text::{self, say, Text};

/// Read settings from beside the executable, or start with the defaults.
pub fn load(path: &PathBuf, text: &Text) -> Settings {
    match fs::read_to_string(path) {
        Err(_) => Settings::default(),
        Ok(stored) => match Settings::parse(&stored) {
            Ok(settings) => settings,
            Err(e) => {
                // Named rather than replaced: starting from defaults without a
                // word means someone's preferences vanished silently.
                println!("{e}");
                say!(text, "cli.settings.unreadable");
                Settings::default()
            }
        },
    }
}

/// Write settings back.
pub fn save(settings: &Settings, path: &PathBuf, text: &Text) {
    match settings.to_json() {
        Ok(json) => {
            if let Err(e) = fs::write(path, json) {
                say!(
                    text,
                    "cli.settings.unsaveable",
                    &path.display().to_string(),
                    &e.to_string()
                );
            }
        }
        Err(e) => println!("{e}"),
    }
}

/// Print every setting.
pub fn show(settings: &Settings, text: &Text) {
    say!(text, "cli.settings.header");
    for (name, value) in settings.summary() {
        println!("  {name:<30} {value}");
    }
    // Stated out loud, because "I could not find the setting" and "there is no
    // such setting" look identical from a settings list.
    println!();
    say!(text, "cli.settings.sendsnothing");
}

/// Change one setting.
///
/// Returns whether anything changed, so the caller knows whether to save.
pub fn set(settings: &mut Settings, rest: &[&str], text: &Text) -> bool {
    let changed = set_quietly(settings, rest, text);
    if changed {
        if let (Some(path), Some(_)) = (rest.first(), rest.get(1)) {
            say!(text, "cli.settings.set", path, rest[1..].join(" ").trim());
        }
    }
    changed
}

/// Change one setting without announcing it.
///
/// Separate because changing the *language* has to announce itself in the new
/// language, which means reloading the catalogues between the change and the
/// message. A caller that reports for itself uses this.
pub fn set_quietly(settings: &mut Settings, rest: &[&str], text: &Text) -> bool {
    let (Some(path), Some(_)) = (rest.first(), rest.get(1)) else {
        say!(text, "cli.error.needsargument");
        return false;
    };
    // The whole tail, so a value containing spaces -- a directory path, a
    // comma-and-space separated list -- arrives intact.
    let value = rest[1..].join(" ");
    let value = value.trim();

    // A candidate is built, validated, and only then adopted. Validating after
    // assigning would leave an out-of-range value in memory on the failure
    // path, which is how a rejected setting still takes effect.
    let mut candidate = settings.clone();
    let applied = match *path {
        "language" => {
            candidate.language = value.to_owned();
            true
        }
        "theme" => {
            candidate.theme = if value.eq_ignore_ascii_case("none") {
                None
            } else {
                Some(value.to_owned())
            };
            true
        }
        "playback.volume" => parse_into(value, text, |v| candidate.playback.volume = v),
        "playback.startMuted" => parse_bool(value, text, |v| candidate.playback.start_muted = v),
        "playback.resumeAfterSeconds" => {
            parse_into(value, text, |v| candidate.playback.resume_after_seconds = v)
        }
        "playback.finishedWithinSeconds" => parse_into(value, text, |v| {
            candidate.playback.finished_within_seconds = v;
        }),
        "playback.subtitleLanguages" => {
            candidate.playback.subtitle_languages = split_list(value);
            true
        }
        "torrent.downloadDirectory" => {
            candidate.torrent.download_directory = if value.eq_ignore_ascii_case("none") {
                None
            } else {
                Some(value.to_owned())
            };
            true
        }
        "torrent.peerLimit" => parse_into(value, text, |v| candidate.torrent.peer_limit = v),
        "torrent.downloadLimitKbps" => {
            parse_optional(value, text, |v| candidate.torrent.download_limit_kbps = v)
        }
        "torrent.uploadLimitKbps" => {
            parse_optional(value, text, |v| candidate.torrent.upload_limit_kbps = v)
        }
        "torrent.seedAfterPlayback" => {
            parse_bool(value, text, |v| candidate.torrent.seed_after_playback = v)
        }
        "torrent.keepFiles" => parse_bool(value, text, |v| candidate.torrent.keep_files = v),
        "torrent.prebufferMegabytes" => {
            parse_into(value, text, |v| candidate.torrent.prebuffer_megabytes = v)
        }
        "updates.checkOnStart" => parse_bool(value, text, |v| candidate.updates.check_on_start = v),
        "updates.refreshRevocations" => {
            parse_bool(value, text, |v| candidate.updates.refresh_revocations = v)
        }
        "updates.channel" => match value {
            "stable" => {
                candidate.updates.channel = UpdateChannel::Stable;
                true
            }
            "alpha" => {
                candidate.updates.channel = UpdateChannel::Alpha;
                true
            }
            other => {
                say!(text, "cli.settings.unknownvalue", other, "stable, alpha");
                false
            }
        },
        "ranking.audioLanguages" => {
            candidate.ranking.preferred_audio_languages = split_list(value);
            true
        }
        "ranking.minimumResolution" => match resolution_of(value) {
            Some(resolution) => {
                candidate.ranking.minimum_resolution = resolution;
                true
            }
            None => {
                say!(
                    text,
                    "cli.settings.unknownvalue",
                    value,
                    "none, 720, 1080, 1440, 2160"
                );
                false
            }
        },
        "ranking.country" => {
            candidate.ranking.country = if value.eq_ignore_ascii_case("none") {
                None
            } else {
                Some(value.to_ascii_lowercase())
            };
            true
        }
        "ranking.preferLarger" => parse_bool(value, text, |v| candidate.ranking.prefer_larger = v),
        "ranking.preferHdr" => parse_bool(value, text, |v| {
            candidate.ranking.prefer_high_dynamic_range = v;
        }),
        "ranking.preferSubtitles" => parse_bool(value, text, |v| {
            candidate.ranking.prefer_sources_with_subtitles = v;
        }),
        other => {
            say!(text, "cli.settings.unknown", other);
            false
        }
    };

    if !applied {
        return false;
    }
    if let Err(e) = candidate.validate() {
        println!("{e}");
        return false;
    }
    *settings = candidate;
    true
}

fn parse_into<T: std::str::FromStr>(value: &str, text: &Text, mut apply: impl FnMut(T)) -> bool {
    match value.parse::<T>() {
        Ok(parsed) => {
            apply(parsed);
            true
        }
        Err(_) => {
            say!(text, "cli.error.notanumber", value);
            false
        }
    }
}

fn parse_optional<T: std::str::FromStr>(
    value: &str,
    text: &Text,
    mut apply: impl FnMut(Option<T>),
) -> bool {
    if value.eq_ignore_ascii_case("none") || value.eq_ignore_ascii_case("unlimited") {
        apply(None);
        return true;
    }
    match value.parse::<T>() {
        Ok(parsed) => {
            apply(Some(parsed));
            true
        }
        Err(_) => {
            say!(text, "cli.error.notanumber", value);
            false
        }
    }
}

fn parse_bool(value: &str, text: &Text, mut apply: impl FnMut(bool)) -> bool {
    // `on`/`off` as well as `true`/`false`: both appear in the rest of this
    // program's commands, and refusing one of them would be arbitrary.
    match value.to_ascii_lowercase().as_str() {
        "true" | "on" | "yes" | "1" => {
            apply(true);
            true
        }
        "false" | "off" | "no" | "0" => {
            apply(false);
            true
        }
        other => {
            say!(text, "cli.settings.unknownvalue", other, "on, off");
            false
        }
    }
}

fn split_list(value: &str) -> Vec<String> {
    if value.eq_ignore_ascii_case("none") {
        return Vec::new();
    }
    value
        .split([',', ' '])
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .map(|part| part.to_ascii_lowercase())
        .collect()
}

fn resolution_of(value: &str) -> Option<Option<Resolution>> {
    match value.to_ascii_lowercase().as_str() {
        "none" | "any" => Some(None),
        "480" | "low" | "sd" => Some(Some(Resolution::Low)),
        "720" | "hd" => Some(Some(Resolution::Hd)),
        "1080" | "fullhd" => Some(Some(Resolution::FullHd)),
        "1440" | "quadhd" => Some(Some(Resolution::QuadHd)),
        "2160" | "4k" | "uhd" => Some(Some(Resolution::UltraHd)),
        _ => None,
    }
}

/// Prove the claim rather than only printing it.
///
/// Walks the serialised settings for anything that looks like a reporting key.
/// A sentence promising that nothing is reported is worth exactly as much as
/// the check behind it.
pub fn audit_settings(settings: &Settings, text: &Text) {
    match settings.to_json().and_then(|json| {
        let found = find_forbidden_keys(&json)?;
        Ok(found)
    }) {
        Ok(found) if found.is_empty() => say!(text, "cli.settings.auditclean"),
        Ok(found) => say!(text, "cli.settings.auditdirty", &found.join(", ")),
        Err(e) => println!("{e}"),
    }
}

// ---------------------------------------------------------------- updates

/// Where the signed release manifest and revocation list are published.
///
/// Fetched only when asked. Both are signed, so where they came from matters
/// less than who signed them — but they are still only read over HTTPS.
const RELEASE_MANIFEST_URL: &str =
    "https://raw.githubusercontent.com/EON-Extensible-Open-Network/eon-stream-app/main/release.json";
const REVOCATION_LIST_URL: &str =
    "https://raw.githubusercontent.com/EON-Extensible-Open-Network/eon-stream-core/main/revocations.json";

/// Check for a newer release.
pub fn check_update(
    http: &impl HttpClient,
    trust: &TrustStore,
    current: &str,
    settings: &Settings,
    now: i64,
    text: &Text,
) {
    say!(text, "cli.update.checking");
    let bytes = match fetch(http, RELEASE_MANIFEST_URL, text) {
        Some(bytes) => bytes,
        None => return,
    };
    let manifest = match ReleaseManifest::parse(&bytes) {
        Ok(manifest) => manifest,
        Err(e) => {
            println!("{e}");
            return;
        }
    };

    // The running build's own release date is not knowable from inside it, so
    // the check falls back to semver ordering -- correct from v1 onwards, and
    // during the alpha the tag order and semver order disagree. See
    // `eon_stream_core::update`.
    let check = match UpdateCheck::new("eon-stream", current, None, settings.updates.channel) {
        Ok(check) => check,
        Err(e) => {
            println!("{e}");
            return;
        }
    };

    match check.evaluate(&manifest, trust, BuildProfile::Stream, now) {
        Ok(UpdateDecision::UpToDate { current }) => say!(text, "cli.update.uptodate", &current),
        Ok(UpdateDecision::Available {
            version,
            tag,
            artifact,
            notes_url,
        }) => {
            say!(text, "cli.update.available", &tag, current);
            say!(
                text,
                "cli.update.artifact",
                &artifact.filename,
                &text::bytes(artifact.size)
            );
            println!("  {}", artifact.url);
            if let Some(notes) = notes_url {
                say!(text, "cli.update.notes", &notes);
            }
            let _ = version;
        }
        Ok(refused) => say!(text, "cli.update.refused", &refused.describe()),
        Err(e) => {
            println!("{e}");
            if trust.keys.is_empty() {
                say!(text, "cli.module.nokeysyet");
            }
        }
    }
}

/// Check a file that has already been downloaded against the signed manifest.
///
/// This is the half of the updater that matters once a release exists: the
/// release notes tell whoever cut the build to download the asset and check it
/// before calling it ready, and a hash published next to the file it describes
/// proves only that nobody corrupted it in transit. This checks against the
/// hash inside the *signed* manifest instead.
pub fn verify_download(
    http: &impl HttpClient,
    trust: &TrustStore,
    path: &str,
    now: i64,
    text: &Text,
) {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(e) => {
            say!(text, "cli.module.unreadable", path, &e.to_string());
            return;
        }
    };
    let manifest_bytes = match fetch(http, RELEASE_MANIFEST_URL, text) {
        Some(bytes) => bytes,
        None => return,
    };
    let manifest = match ReleaseManifest::parse(&manifest_bytes) {
        Ok(manifest) => manifest,
        Err(e) => {
            println!("{e}");
            return;
        }
    };

    // The manifest's own signature first. Checking a file against an unsigned
    // manifest would be checking it against whatever the network handed over.
    let envelope = match manifest.signature.as_ref() {
        Some(envelope) => envelope,
        None => {
            println!(
                "{}",
                eon_stream_core::Error::SignatureMissing(manifest.product)
            );
            return;
        }
    };
    let subject = match manifest.signing_subject() {
        Ok(subject) => subject,
        Err(e) => {
            println!("{e}");
            return;
        }
    };
    if let Err(e) = trust.verify(
        envelope,
        &subject,
        eon_stream_core::signature::Artefact::Release,
        BuildProfile::Stream,
        now,
    ) {
        println!("{e}");
        if trust.keys.is_empty() {
            say!(text, "cli.module.nokeysyet");
        }
        return;
    }

    // Matched on the file's own name, so verifying the wrong file for the
    // platform is reported as a mismatch rather than passing against some
    // other artefact in the same release.
    let name = std::path::Path::new(path)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let Some(artifact) = manifest.artifacts.iter().find(|a| a.filename == name) else {
        say!(text, "cli.update.mismatch");
        println!("  {name}");
        for a in &manifest.artifacts {
            println!("  {} is in {}", a.filename, manifest.tag);
        }
        return;
    };

    match artifact.verify(&bytes) {
        Ok(()) => {
            say!(text, "cli.update.verified");
            println!("  {} {}", artifact.filename, artifact.sha256);
        }
        Err(e) => {
            say!(text, "cli.update.mismatch");
            println!("  {e}");
        }
    }
}

/// Fetch and apply the revocation list.
pub fn refresh_revocations(
    http: &impl HttpClient,
    store: &mut RevocationStore,
    modules: &mut ModuleStore,
    trust: &TrustStore,
    now: i64,
    text: &Text,
) -> bool {
    let bytes = match fetch(http, REVOCATION_LIST_URL, text) {
        Some(bytes) => bytes,
        None => return false,
    };
    let list = match RevocationList::parse(&bytes) {
        Ok(list) => list,
        Err(e) => {
            println!("{e}");
            return false;
        }
    };
    match store.accept(list, trust, BuildProfile::Stream, now) {
        Ok(_revoked_keys) => {
            apply_revocations(store, modules, now, text);
            true
        }
        Err(e) => {
            println!("{e}");
            if trust.keys.is_empty() {
                say!(text, "cli.module.nokeysyet");
            }
            false
        }
    }
}

/// Disable anything the held list revokes, and say so.
pub fn apply_revocations(
    store: &RevocationStore,
    modules: &mut ModuleStore,
    now: i64,
    text: &Text,
) {
    match modules.apply_revocations(store, now) {
        Ok(actions) if actions.is_empty() => say!(text, "cli.revocation.clean"),
        Ok(actions) => {
            say!(text, "cli.revocation.applied", &actions.len().to_string());
            for action in actions {
                // Rendered here rather than taking `action.describe()`, which
                // is English and is a diagnostic (madde 40).
                println!(
                    "  {}",
                    text.format(
                        "cli.revocation.line",
                        &[
                            &action.module_id,
                            &action.version,
                            &text.get(action.reason.message_key()),
                        ],
                    )
                );
                if let Some(advisory) = &action.advisory {
                    say!(text, "cli.revocation.advisory", advisory);
                }
            }
        }
        Err(e) => println!("{e}"),
    }
}

/// Print the state of the revocation data.
pub fn revocation_status(store: &RevocationStore, now: i64, text: &Text) {
    match store.freshness(now) {
        Ok(RevocationFreshness::Absent) => say!(text, "cli.revocation.absent"),
        Ok(RevocationFreshness::Fresh { next_update }) => {
            let count = store.held().map_or(0, |list| list.revoked.len());
            say!(
                text,
                "cli.revocation.fresh",
                &count.to_string(),
                &next_update
            );
        }
        Ok(RevocationFreshness::Stale {
            overdue_seconds, ..
        }) => say!(
            text,
            "cli.revocation.stale",
            &(overdue_seconds / 86_400).to_string()
        ),
        Err(e) => println!("{e}"),
    }
}

fn fetch(http: &impl HttpClient, url: &str, text: &Text) -> Option<Vec<u8>> {
    match http.get(url) {
        Ok(response) if (200..300).contains(&response.status) => Some(response.body),
        Ok(response) => {
            say!(text, "cli.update.httpstatus", &response.status.to_string());
            None
        }
        Err(e) => {
            say!(text, "cli.update.unreachable", &e.message);
            None
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    fn text() -> Text {
        Text::new("en")
    }

    #[test]
    fn setting_a_value_validates_before_adopting_it() {
        // The failure path must not leave the out-of-range value in place.
        let mut settings = Settings::default();
        assert!(!set(&mut settings, &["playback.volume", "900"], &text()));
        assert_eq!(settings.playback.volume, 100);

        assert!(set(&mut settings, &["playback.volume", "80"], &text()));
        assert_eq!(settings.playback.volume, 80);
    }

    #[test]
    fn an_unknown_path_changes_nothing() {
        let mut settings = Settings::default();
        let before = settings.clone();
        assert!(!set(&mut settings, &["playback.vlume", "80"], &text()));
        assert_eq!(settings, before);
    }

    #[test]
    fn booleans_accept_the_words_the_rest_of_the_program_uses() {
        let mut settings = Settings::default();
        for (written, expected) in [
            ("on", true),
            ("off", false),
            ("true", true),
            ("false", false),
            ("yes", true),
            ("no", false),
            ("1", true),
            ("0", false),
        ] {
            assert!(
                set(&mut settings, &["torrent.keepFiles", written], &text()),
                "{written} should be accepted"
            );
            assert_eq!(settings.torrent.keep_files, expected, "{written}");
        }
        assert!(!set(
            &mut settings,
            &["torrent.keepFiles", "maybe"],
            &text()
        ));
    }

    #[test]
    fn none_clears_an_optional_setting_rather_than_storing_the_word() {
        let mut settings = Settings::default();
        assert!(set(
            &mut settings,
            &["torrent.uploadLimitKbps", "512"],
            &text()
        ));
        assert_eq!(settings.torrent.upload_limit_kbps, Some(512));
        assert!(set(
            &mut settings,
            &["torrent.uploadLimitKbps", "none"],
            &text()
        ));
        assert_eq!(settings.torrent.upload_limit_kbps, None);
        // And "unlimited" means the same thing, since that is what the display
        // calls it.
        assert!(set(
            &mut settings,
            &["torrent.downloadLimitKbps", "unlimited"],
            &text()
        ));
        assert_eq!(settings.torrent.download_limit_kbps, None);
    }

    #[test]
    fn a_zero_rate_limit_is_refused_by_validation() {
        // Zero would stall rather than mean unlimited.
        let mut settings = Settings::default();
        assert!(!set(
            &mut settings,
            &["torrent.downloadLimitKbps", "0"],
            &text()
        ));
        assert_eq!(settings.torrent.download_limit_kbps, None);
    }

    #[test]
    fn resolutions_accept_the_numbers_people_say() {
        assert_eq!(resolution_of("1080"), Some(Some(Resolution::FullHd)));
        assert_eq!(resolution_of("4k"), Some(Some(Resolution::UltraHd)));
        assert_eq!(resolution_of("none"), Some(None));
        assert_eq!(resolution_of("800"), None);
    }

    #[test]
    fn lists_split_on_commas_and_spaces_and_lowercase() {
        assert_eq!(split_list("tur, eng"), vec!["tur", "eng"]);
        assert_eq!(split_list("TUR ENG"), vec!["tur", "eng"]);
        assert_eq!(split_list("none"), Vec::<String>::new());
        assert_eq!(split_list(" , "), Vec::<String>::new());
    }

    #[test]
    fn a_language_setting_is_validated_as_a_language_tag() {
        let mut settings = Settings::default();
        assert!(set(&mut settings, &["language", "tr"], &text()));
        assert_eq!(settings.language, "tr");
        assert!(!set(&mut settings, &["language", "Turkish!"], &text()));
        assert_eq!(settings.language, "tr");
    }

    #[test]
    fn a_multiword_value_is_kept_whole() {
        // `set torrent.downloadDirectory D:\My Videos\eon` is one path, not
        // two arguments.
        let mut settings = Settings::default();
        assert!(set(
            &mut settings,
            &["torrent.downloadDirectory", "D:\\My", "Videos"],
            &text()
        ));
        assert_eq!(
            settings.torrent.download_directory.as_deref(),
            Some("D:\\My Videos")
        );
    }

    #[test]
    fn the_settings_audit_passes_on_the_real_document() {
        let settings = Settings::default();
        let json = settings.to_json().unwrap();
        assert!(find_forbidden_keys(&json).unwrap().is_empty());
    }

    #[test]
    fn the_published_urls_are_https() {
        // A signature is the real protection, but there is no reason to hand
        // anyone on the path the attempt.
        assert!(RELEASE_MANIFEST_URL.starts_with("https://"));
        assert!(REVOCATION_LIST_URL.starts_with("https://"));
    }
}
