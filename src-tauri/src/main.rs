// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 EON contributors
//
// Additional permission under GNU GPL version 3 section 7:
// see LICENSE-EXCEPTION.md (EON Module ABI Exception 1.0).

//! EON Stream — the window.
//!
//! The interface was the last thing added — at `v0.14`, during the alpha
//! rather than at `v1` as madde 2 first had it. The half of that decision
//! which mattered held: every part of what this window does was proven at a
//! prompt first, and the headless build still ships beside it as
//! `eon-stream-cli`.
//!
//! ## What this binary is
//!
//! A Tauri host (madde 2). The webview is the system's own — WebView2 on
//! Windows — so no renderer is shipped and none has to be kept patched. The
//! interface in `src/` is static HTML, CSS and JavaScript: no bundler, no
//! Node, no lock file of its own. It knows nothing about torrents, mpv or
//! addon addresses, and reaches the host only through the commands enumerated
//! in [`commands`].
//!
//! ## Where the video goes
//!
//! Inside this window, since `v0.16` (madde 2, plan v0.7). mpv is still a
//! separate process over JSON IPC; it is handed the window's handle and draws
//! as a child of it, and the player's controls are a second, transparent
//! webview laid on top. [`surface`] holds the sequencing that makes that work,
//! and the fallback -- mpv's own window -- where it cannot.
//!
//! ## Nothing is reported anywhere
//!
//! No telemetry, no analytics, no crash reporting, no account (madde 36).
//! `--selftest` proves it rather than asserting it: it walks the stored
//! settings for any key that could send something somewhere, which is the
//! check the headless build's `audit` command makes.

// Release builds have no console: this is a desktop application, and a
// terminal flashing up behind the window would be an artefact of how it was
// compiled. Debug builds keep theirs, because a `println!` in a debug build is
// how the host gets read while it is being written.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
#![forbid(unsafe_code)]

mod buffer;
mod commands;
mod http;
mod keeper;
mod speeds;
mod state;
mod subs;
mod surface;
mod torrents;
mod view;

use std::sync::{Arc, Mutex};

use eon_stream_core::http::HttpClient as _;
use state::Session;

use crate::commands::Shared;

const VERSION: &str = env!("CARGO_PKG_VERSION");

fn main() {
    let mut arguments = std::env::args().skip(1);
    match arguments.next().as_deref() {
        Some("--version" | "-V") => println!("EON Stream {VERSION}"),
        // `eon-stream --play <magnet|url|path>` opens the window and starts on
        // it. Two reasons it exists: a published build can then be verified by
        // running it rather than by someone clicking through the window, which
        // is the check `v0.13` taught this project to make; and it is the shape
        // a file association and an `eon://` handler will need.
        Some("--play") => match arguments.next() {
            Some(link) => window(Some(link)),
            None => {
                eprintln!("--play needs a magnet link, info hash, URL or file path");
                std::process::exit(2);
            }
        },
        Some("--selftest") => {
            let report = selftest();
            // A release build has no console to print into, so the report goes
            // to a file when one is named. The exit code is the part a
            // workflow reads either way.
            match arguments.next() {
                Some(path) => {
                    if let Err(e) = std::fs::write(&path, &report) {
                        eprintln!("could not write {path}: {e}");
                        std::process::exit(2);
                    }
                }
                None => print!("{report}"),
            }
            if !report.ends_with("ok\n") {
                std::process::exit(1);
            }
        }
        _ => window(None),
    }
}

/// Open the window, optionally starting on something.
fn window(play: Option<String>) {
    let session: Shared = Arc::new(Mutex::new(Session::load()));

    if let Some(link) = play {
        // On its own thread, because resolving a torrent waits on strangers and
        // the window has to be up and answering while that happens. It reports
        // through the job like any other playback, so what a viewer sees is the
        // same buffering line they would see having pasted it themselves.
        let shared = Arc::clone(&session);
        std::thread::spawn(move || {
            if let Err(e) = commands::play_link_now(&shared, &link) {
                eprintln!("could not play that: {e}");
            }
        });
    }

    let app = tauri::Builder::default()
        .manage(Arc::clone(&session))
        .setup(|app| {
            surface::main_window(app.handle())?;
            surface::install(app.handle());
            Ok(())
        })
        // The whole IPC surface. Nothing is registered that is not in
        // `commands.rs`, which is the point of keeping it in one file.
        .invoke_handler(tauri::generate_handler![
            commands::strings,
            commands::language_set,
            commands::status,
            commands::about,
            commands::addons,
            commands::addon_add,
            commands::addon_remove,
            commands::addon_enabled,
            commands::addon_move,
            commands::addon_refresh,
            commands::rows,
            commands::row_items,
            commands::search,
            commands::detail,
            commands::feature,
            commands::sources,
            commands::library,
            commands::more_like,
            commands::calendar,
            commands::trailer,
            commands::library_save,
            commands::library_remove,
            commands::history_forget,
            commands::continue_remove,
            commands::play,
            commands::play_link,
            commands::cancel,
            commands::play_faster,
            commands::play_next,
            commands::watched_set,
            commands::subtitles,
            commands::subtitle_load,
            commands::subtitle_file,
            commands::subtitle_languages,
            commands::player,
            commands::tracks,
            commands::tuning,
            commands::buffer_clear,
            commands::settings,
            commands::settings_set,
        ])
        .build(tauri::generate_context!());

    match app {
        Ok(app) => app.run(move |_handle, event| {
            if matches!(event, tauri::RunEvent::Exit) {
                shutdown(&session);
            }
        }),
        Err(e) => {
            // There is no window to say this in, and on a release build no
            // console either. The exit code is the message.
            eprintln!("EON Stream could not start: {e}");
            std::process::exit(1);
        }
    }
}

/// Close everything that outlives a click.
///
/// The position is written down first, because what a viewer notices is
/// resuming where they left off. The engine is shut down after, which is what
/// removes the pieces when `torrent.keepFiles` is off: a program that leaves a
/// season pack on someone's disk after they closed it has not finished
/// running.
fn shutdown(session: &Shared) {
    let Ok(mut session) = session.lock() else {
        return;
    };
    session.remember_position();
    if let Some(player) = session.player.take() {
        let _ = player.quit();
    }
    subs::clear_folder();
    if let Some(engine) = session.torrent.take() {
        // A worker thread still resolving a torrent holds the other handle. It
        // is about to be killed with the process, and forcing the shutdown
        // here would race it, so in that case the pieces are left where they
        // are rather than half-removed.
        if let Ok(engine) = Arc::try_unwrap(engine) {
            let _ = engine.shutdown();
        }
    }
}

/// Everything that can be checked without a window, as a report.
///
/// This exists because of how `v0.13` happened: `v0.12` passed every test,
/// shipped, and could not play a torrent on a second run. A window is checked
/// by opening it — but the host underneath it can be checked by a machine, and
/// what a machine can check should not be waiting on someone to look.
///
/// It runs in CI against the same binary that gets published.
fn selftest() -> String {
    use std::fmt::Write as _;

    let mut report = String::new();
    let _ = writeln!(report, "EON Stream {VERSION} — self-test");

    let mut session = Session::load();

    // 1. The message catalogues. A key missing from Turkish falls back to
    //    English by design; a catalogue that does not load at all would mean
    //    every label in the window comes out as its own key.
    let english = eon_stream_core::i18n::shipped("en").map(|catalog| catalog.len());
    let turkish = eon_stream_core::i18n::shipped("tr").map(|catalog| catalog.len());
    let _ = writeln!(
        report,
        "catalogues: en={} tr={}",
        english.map_or_else(|e| e.to_string(), |n| n.to_string()),
        turkish.map_or_else(|e| e.to_string(), |n| n.to_string()),
    );

    // 2. Every string the interface draws must exist. The interface holds no
    //    text of its own, so a missing `ui.` key is a blank label (madde 40).
    let ui_keys = eon_stream_core::i18n::shipped("en")
        .map(|catalog| {
            catalog
                .keys()
                .into_iter()
                .filter(|key| key.starts_with("ui."))
                .count()
        })
        .unwrap_or_default();
    let _ = writeln!(report, "interface strings: {ui_keys}");

    // 3. What this build trusts, which in every published alpha is nothing at
    //    all, on purpose (madde 30). A build that started trusting a key by
    //    default would be a decision somebody had to have made, and this is
    //    where it would get noticed.
    let _ = writeln!(report, "trusted signing keys: {}", session.trust.keys.len());

    // 4. Nothing reports anywhere: the stored settings are walked for any key
    //    that could send something somewhere (madde 36).
    let settings = view::settings(&session);
    let _ = writeln!(
        report,
        "settings audit: {}",
        if settings.audit_clean {
            "nothing in this document sends anything anywhere".to_owned()
        } else {
            format!("FOUND {:?}", settings.audit_found)
        }
    );

    // 5. The transport is the real one, with the library's own limits.
    let limits = session.client.http().limits();
    let _ = writeln!(
        report,
        "transport: timeout {} ms, {} redirects, {} byte cap",
        limits.timeout_ms, limits.max_redirects, limits.max_body_bytes
    );

    // 6. The views the window opens on, built from whatever this machine has.
    //    An empty install answers all of them, which is the state a first run
    //    is in.
    let rows = view::rows(&session);
    let addons = view::addons(&session);
    let about = view::about(&session);
    let status = view::status(&mut session);
    let _ = writeln!(
        report,
        "views: {} rows, {} addons, {} kept, player running: {}",
        rows.len(),
        addons.len(),
        status.library,
        status.player.running
    );
    let _ = writeln!(report, "files this build writes: {}", about.files.join(" "));

    // 7. An addon address must not be reachable from the interface. That is
    //    the one line here which is a security boundary rather than a
    //    behaviour: a configured address can carry a credential in its path
    //    (madde 22). Checked against the serialised views, because that is
    //    literally what crosses the line — and against the addresses this
    //    machine actually has rather than against a pattern, since a poster
    //    URL is a URL too, and a check that cannot tell them apart is a check
    //    that ends up switched off.
    let serialised = format!(
        "{}{}{}",
        serde_json::to_string(&addons).unwrap_or_default(),
        serde_json::to_string(&rows).unwrap_or_default(),
        serde_json::to_string(&about).unwrap_or_default(),
    );
    let addresses: Vec<String> = session
        .registry
        .addons()
        .iter()
        .map(|addon| addon.address.base().to_owned())
        .collect();
    let leaked = addresses
        .iter()
        .any(|address| !address.is_empty() && serialised.contains(address.as_str()));
    let _ = writeln!(
        report,
        "addon addresses in the views: {}",
        match (addresses.len(), leaked) {
            (0, _) => "no addon installed, nothing to leak".to_owned(),
            (n, false) => format!("none of {n}"),
            (_, true) => "FOUND — this is a leak".to_owned(),
        }
    );

    // The last line is the verdict, and `main` turns it into an exit code.
    if settings.audit_clean && !leaked && ui_keys > 0 {
        let _ = writeln!(report, "ok");
    } else {
        let _ = writeln!(report, "FAILED");
    }
    report
}
