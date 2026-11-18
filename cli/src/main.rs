// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 EON contributors
//
// Additional permission under GNU GPL version 3 section 7:
// see LICENSE-EXCEPTION.md (EON Module ABI Exception 1.0).

//! EON Stream, headless — a terminal front end over the addon protocol.
//!
//! This is not the product; `eon-stream` is. Everything here was proven at a
//! prompt first and the window was written on top of it (madde 2), which is why
//! this build still ships: it is where the system is exercised, and it still
//! does things the window does not — modules, the update check, subtitle search
//! by hand, track selection, `eon://` deep links.
//!
//! Playback is mpv over JSON IPC (madde 2): HTTP and HTTPS streams — including
//! HLS and DASH, which mpv handles natively — local files, and BitTorrent
//! sources through the engine's loopback server.
//!
//! Numbered selection throughout, because typing `tt0903747:1:1` by hand is not a
//! user interface at any level of shallowness.

#![forbid(unsafe_code)]
#![allow(clippy::print_stdout, clippy::print_stderr)]

mod helptext;
mod modules;
mod paging;
mod prefs;
mod search;
mod shelf;
mod sources;
mod text;
mod torrents;

use std::{
    fs,
    io::{self, Read, Write},
    path::PathBuf,
    time::Duration,
};

use eon_stream_core::{
    history::{clock, now_unix},
    http::{HttpClient, HttpError, HttpResponse, Limits},
    modules::ModuleStore,
    rank,
    ranking::{format_size, RankedStream, Resolution},
    revocation::RevocationStore,
    settings::Settings,
    signature::TrustStore,
    types::AddonCatalogEntry,
    AddonClient, AddonRegistry, BuildProfile, HealthTracker, Library, Meta, MetaPreview, Stream,
    StreamSource, Subtitle, Video, WatchEntry, WatchHistory,
};
use eon_stream_engine::{MpvPlayer, PlaybackSource, PlayerOptions, SeekMode, TorrentEngine};

use crate::text::{say, Text};

const VERSION: &str = env!("CARGO_PKG_VERSION");

// ---------------------------------------------------------------- transport

/// A real HTTP client. Kept here rather than in the library, so the library
/// pulls in no network stack.
struct Http {
    agent: ureq::Agent,
    limits: Limits,
}

impl Http {
    fn new() -> Self {
        let limits = Limits::default();
        let agent = ureq::AgentBuilder::new()
            .timeout(Duration::from_millis(limits.timeout_ms))
            .redirects(u32::from(limits.max_redirects))
            .user_agent(concat!(
                "EON-Stream/",
                env!("CARGO_PKG_VERSION"),
                " (+https://github.com/EON-Extensible-Open-Network)"
            ))
            .build();
        Self { agent, limits }
    }
}

impl HttpClient for Http {
    fn get(&self, url: &str) -> Result<HttpResponse, HttpError> {
        let response = match self.agent.get(url).call() {
            Ok(response) => response,
            // A non-success status is a response, not a transport failure: the
            // caller needs the status the addon actually sent.
            Err(ureq::Error::Status(status, response)) => {
                let mut body = Vec::new();
                let _ = response.into_reader().read_to_end(&mut body);
                return Ok(HttpResponse { status, body });
            }
            // Never put the URL in here: an addon's configuration credential
            // lives in its path.
            Err(ureq::Error::Transport(t)) => {
                return Err(HttpError::new(
                    t.message()
                        .map_or_else(|| t.kind().to_string(), ToOwned::to_owned),
                ));
            }
        };
        let status = response.status();
        let mut body = Vec::new();
        let cap = self.limits.max_body_bytes as u64 + 1;
        response
            .into_reader()
            .take(cap)
            .read_to_end(&mut body)
            .map_err(|e| HttpError::new(e.to_string()))?;
        Ok(HttpResponse { status, body })
    }

    fn limits(&self) -> Limits {
        self.limits
    }
}

// ---------------------------------------------------------------- storage

/// Files live beside the executable, so the binary stays self-contained and
/// leaves nothing behind elsewhere. No account, no sync, nothing sent anywhere.
pub(crate) fn beside_exe(name: &str) -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join(name)))
        .unwrap_or_else(|| PathBuf::from(name))
}

// ---------------------------------------------------------------- state

/// A catalogue the user can pick by number.
struct CatalogRef {
    addon_id: String,
    content_type: String,
    catalog_id: String,
    label: String,
}

/// What the last listing showed, so the next command can say "3".
#[derive(Default)]
struct Selection {
    catalogs: Vec<CatalogRef>,
    items: Vec<MetaPreview>,
    ranked: Vec<RankedStream>,
    subtitles: Vec<Subtitle>,
    episodes: Vec<Video>,
    discovered: Vec<AddonCatalogEntry>,
    meta: Option<Meta>,
    /// What is currently being listed as sources: type and id. An episode is not
    /// its series, so this is tracked rather than assumed from `meta`.
    subject: Option<(String, String, String)>,
    /// Catalogue number, query and how many items were skipped, so `more` can
    /// ask for the next page.
    page: Option<(usize, String, usize)>,

    /// Which of `ranked` are on screen, by index into it.
    ///
    /// Narrowing by quality or reordering by a filter changes this and never
    /// `ranked`, which is what keeps `play <n>` meaning one source for the
    /// life of the listing.
    visible: Vec<usize>,
    /// The quality bucket currently selected, by position in the bucket list.
    quality: Option<usize>,
    /// How the visible sources are ordered beyond the ranking.
    filter: sources::Filter,
    /// The last search, kept so `search <n>` can page it and `search exactly`
    /// can repeat it uncorrected.
    search: search::Outcome,
    /// Subtitles matching the last `sub search`, by index into `subtitles`.
    subtitle_matches: Option<Vec<usize>>,
    /// What the last `sub search` was for, for the header when paging it.
    subtitle_query: String,
}

/// What is on screen, so its position can be remembered and the next episode
/// found.
#[derive(Clone)]
struct NowPlaying {
    content_type: String,
    id: String,
    name: String,
    series_id: Option<String>,
    season: Option<u32>,
    episode: Option<u32>,
    binge_group: Option<String>,
    addon_id: Option<String>,
}

struct App {
    client: AddonClient<Http>,
    registry: AddonRegistry,
    history: WatchHistory,
    health: HealthTracker,
    settings: Settings,
    text: Text,
    /// Installed modules. The build profile is `Stream`; an Edu build is a
    /// separate binary with a separate trust set (madde 22).
    modules: ModuleStore,
    /// What this machine trusts. Empty until keys exist (madde 30, 34).
    trust: TrustStore,
    revocations: RevocationStore,
    /// Things the viewer kept. Stremio calls this My Library; it is a file
    /// here rather than an account.
    library: Library,
    selection: Selection,
    player: Option<MpvPlayer>,
    playing: Option<NowPlaying>,
    /// Started the first time a torrent is played, not at launch: a DHT and a
    /// listening socket are not something to open on behalf of someone who
    /// only plays HTTP streams.
    torrent: Option<TorrentEngine>,
}

impl App {
    fn new() -> Self {
        let text = Text::new("en");
        let settings = prefs::load(&beside_exe("eon-settings.json"), &text);
        // The interface language is a setting, so the catalogues are reloaded
        // once it is known. Loading them twice costs a JSON parse of a file
        // compiled into the binary.
        let text = Text::new(&settings.language);
        let modules =
            modules::load_store(&beside_exe("eon-modules.json"), BuildProfile::Stream, &text);
        Self {
            client: AddonClient::new(Http::new()),
            registry: fs::read_to_string(beside_exe("eon-addons.json"))
                .ok()
                .and_then(|json| AddonRegistry::from_json(&json).ok())
                .unwrap_or_default(),
            history: fs::read_to_string(beside_exe("eon-history.json"))
                .ok()
                .and_then(|json| WatchHistory::from_json(&json).ok())
                .unwrap_or_default(),
            health: HealthTracker::new(),
            trust: modules::load_trust(&beside_exe("eon-trust.json")),
            revocations: fs::read_to_string(beside_exe("eon-revocations.json"))
                .ok()
                .and_then(|json| RevocationStore::from_json(&json).ok())
                .unwrap_or_default(),
            library: fs::read_to_string(beside_exe("eon-library.json"))
                .ok()
                .and_then(|json| Library::from_json(&json).ok())
                .unwrap_or_default(),
            modules,
            settings,
            text,
            selection: Selection::default(),
            player: None,
            playing: None,
            torrent: None,
        }
    }

    fn save_settings(&self) {
        prefs::save(&self.settings, &beside_exe("eon-settings.json"), &self.text);
    }

    fn save_modules(&self) {
        if let Ok(json) = self.modules.to_json() {
            if let Err(e) = fs::write(beside_exe("eon-modules.json"), json) {
                eprintln!("could not save the module list: {e}");
            }
        }
    }

    fn save_library(&self) {
        if let Ok(json) = self.library.to_json() {
            if let Err(e) = fs::write(beside_exe("eon-library.json"), json) {
                eprintln!("could not save the library: {e}");
            }
        }
    }

    fn save_revocations(&self) {
        if let Ok(json) = self.revocations.to_json() {
            if let Err(e) = fs::write(beside_exe("eon-revocations.json"), json) {
                eprintln!("could not save the revocation list: {e}");
            }
        }
    }

    /// The torrent engine, started if it is not already.
    ///
    /// Returns `None` after reporting why, so a caller can simply give up on
    /// this source without inventing an error message of its own.
    fn torrent_engine(&mut self) -> Option<&TorrentEngine> {
        if self.torrent.is_none() {
            let options = match torrents::options_from(&self.settings.torrent) {
                Ok(options) => options,
                Err(e) => {
                    println!("{e}");
                    return None;
                }
            };
            match TorrentEngine::start(options) {
                Ok(engine) => self.torrent = Some(engine),
                Err(e) => {
                    println!("{e}");
                    return None;
                }
            }
        }
        self.torrent.as_ref()
    }

    fn save_addons(&self) {
        if let Ok(json) = self.registry.to_json() {
            if let Err(e) = fs::write(beside_exe("eon-addons.json"), json) {
                eprintln!("could not save the addon list: {e}");
            }
        }
    }

    fn save_history(&self) {
        if let Ok(json) = self.history.to_json() {
            if let Err(e) = fs::write(beside_exe("eon-history.json"), json) {
                eprintln!("could not save the history: {e}");
            }
        }
    }

    /// Write down where playback got to, before the player goes away.
    fn remember_position(&mut self) {
        let Some(playing) = self.playing.clone() else {
            return;
        };
        let Some(player) = self.player.as_mut() else {
            return;
        };
        let Some(position) = player.position().ok().flatten() else {
            return;
        };
        let duration = player.duration().ok().flatten();
        self.history.record(WatchEntry {
            content_type: playing.content_type,
            id: playing.id,
            name: playing.name,
            series_id: playing.series_id,
            season: playing.season,
            episode: playing.episode,
            position_secs: position,
            duration_secs: duration,
            binge_group: playing.binge_group,
            addon_id: playing.addon_id,
            updated_at: now_unix(),
        });
        self.save_history();
    }
}

// ---------------------------------------------------------------- entry

fn main() {
    let mut app = App::new();

    println!("EON Stream {VERSION}  ·  alpha  ·  the headless build");
    println!("An open ecosystem for everyone.  https://github.com/EON-Extensible-Open-Network");
    println!();
    println!("Plays HTTP, HLS, DASH, local files and BitTorrent through mpv.");
    println!("Torrents stream: sequential download into a loopback server the player reads.");
    println!();

    // A deep link on the command line is handled before the prompt appears.
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Some(link) = args.first().filter(|a| a.starts_with("eon://")) {
        deep_link(&mut app, link);
    }

    if app.registry.is_empty() {
        println!("No addons installed. EON Stream ships with none, ever.");
        println!("Try a first one:");
        println!("  add https://v3-cinemeta.strem.io/manifest.json");
    } else {
        println!(
            "{} addon(s) installed. `help` for commands.",
            app.registry.len()
        );
        let resume: Vec<String> = app
            .history
            .continue_watching(3)
            .into_iter()
            .map(WatchEntry::display_line)
            .collect();
        if !resume.is_empty() {
            println!("\nContinue watching:");
            for line in resume {
                println!("  {line}");
            }
            println!("  (`continue` for the full list)");
        }
    }

    loop {
        print!("\neon> ");
        if io::stdout().flush().is_err() {
            break;
        }
        let mut line = String::new();
        match io::stdin().read_line(&mut line) {
            Ok(0) => break,
            Ok(_) => {}
            Err(e) => {
                eprintln!("could not read input: {e}");
                break;
            }
        }

        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let mut parts = line.split_whitespace();
        let command = parts.next().unwrap_or("");
        let rest: Vec<&str> = parts.collect();

        if matches!(command, "quit" | "exit" | "q") {
            break;
        }
        dispatch(&mut app, command, &rest);
    }

    // Leaving must not lose where you got to.
    app.remember_position();
    if let Some(player) = app.player.take() {
        let _ = player.quit();
    }
    // The engine goes down last: it owns a runtime with disk writes in flight,
    // and it is what decides whether the downloaded pieces are kept.
    if let Some(engine) = app.torrent.take() {
        torrents::shutdown(engine, &app.text);
    }
}

fn dispatch(app: &mut App, command: &str, rest: &[&str]) {
    match command {
        "help" | "?" => helptext::section(rest, &app.text),

        // addons
        "add" => add(app, rest),
        "list" => list(app),
        "remove" | "rm" => remove(app, rest),
        "enable" => set_enabled(app, rest, true),
        "disable" => set_enabled(app, rest, false),
        "refresh" => refresh(app, rest),
        "order" => reorder(app, rest),
        "configure" => configure(app, rest),
        "discover" => discover(app, rest),
        "health" => health(app),

        // search
        "search" | "s" => search_command(app, rest),

        // browsing
        "catalogs" | "cat" => catalogs(app),
        "browse" | "b" => browse(app, rest),
        "more" => more(app),
        "open" | "o" => open(app, rest),
        "episodes" | "eps" => episodes(app, rest),
        "ep" => open_episode(app, rest),

        // playback
        "play" | "p" => play(app, rest),
        "next" => next_episode(app),
        "pause" | "resume" | "seek" | "where" | "stop" | "buffer" => control(app, command, rest),
        "tracks" => tracks(app),
        "audio" | "video" | "subtrack" => select_track(app, command, rest),
        "subs" => subtitles(app, rest),
        "sub" => sub_command(app, rest),
        "speed" | "adelay" | "sdelay" | "volume" => tune(app, command, rest),

        // sources
        "sources" | "src" => sources_command(app, rest),
        "res" | "quality" => resolution_command(app, rest),
        "filter" => filter_command(app, rest),
        "trailer" => trailer_command(app),

        // library
        "save" => {
            if shelf::save(&mut app.library, &app.selection.items, rest, &app.text) {
                app.save_library();
            }
        }
        "unsave" => {
            if shelf::unsave(&mut app.library, &app.selection.items, rest, &app.text) {
                app.save_library();
            }
        }
        "library" | "lib" => {
            let listed = shelf::list(&app.library, rest, &app.text);
            if !listed.is_empty() {
                // A library row is openable by its number like any other list.
                app.selection.items = listed;
            }
        }

        // torrents
        "torrent" | "torrents" => torrents::status(app.torrent.as_ref(), &app.text),
        "files" => torrent_files(app, rest),

        // modules and themes
        "modules" | "mods" => modules::list(&app.modules, &app.text),
        "install" => install_module(app, rest),
        "uninstall" | "enable-module" | "disable-module" | "order-module" => {
            module_operation(app, command, rest);
        }
        "theme" => theme_command(app, rest),
        "trust" => modules::trust(&app.trust, now_unix_signed(), &app.text),

        // settings and updates
        "settings" => prefs::show(&app.settings, &app.text),
        "set" => set_setting(app, rest),
        "audit" => prefs::audit_settings(&app.settings, &app.text),
        "lang" => set_language(app, rest),
        "update" => update_command(app, rest),
        "revocations" => revocations_command(app, rest),

        // history and preferences
        "continue" => continue_watching(app),
        "forget" => forget(app, rest),
        "prefer" => prefer(app, rest),

        other => say!(app.text, "cli.error.unknowncommand", other),
    }
}

// ---------------------------------------------------------------- search

fn search_command(app: &mut App, rest: &[&str]) {
    // `search exactly` is the headless equivalent of clicking "search instead
    // for" -- the offer is only real if something can be typed.
    if rest.first().is_some_and(|first| {
        first.eq_ignore_ascii_case("exactly") || first.eq_ignore_ascii_case("instead")
    }) {
        let Some(typed) = app.selection.search.typed.clone() else {
            say!(app.text, "cli.search.nothingtoundo");
            return;
        };
        say!(app.text, "cli.search.exactly", &typed);
        let outcome = search::run(
            &app.client,
            &app.registry,
            &app.library,
            &app.history,
            &typed,
            true,
            &app.text,
        );
        adopt_search(app, outcome, 1);
        return;
    }

    // A bare number pages the last search rather than starting a new one for
    // the word "2", which is what someone typing `search 2` means.
    if let Some(page) = rest
        .first()
        .filter(|_| rest.len() == 1)
        .and_then(|first| first.parse::<usize>().ok())
    {
        if app.selection.search.results.is_empty() {
            say!(app.text, "cli.search.needsquery");
            return;
        }
        let outcome = std::mem::take(&mut app.selection.search);
        adopt_search(app, outcome, page);
        return;
    }

    let query = rest.join(" ");
    let outcome = search::run(
        &app.client,
        &app.registry,
        &app.library,
        &app.history,
        &query,
        false,
        &app.text,
    );
    adopt_search(app, outcome, 1);
}

/// Show a search outcome and make its results the list numbers refer to.
fn adopt_search(app: &mut App, outcome: search::Outcome, page: usize) {
    search::print(&outcome, page, &app.text);
    if !outcome.results.is_empty() {
        app.selection.items = outcome.results.clone();
    }
    if outcome.was_corrected() {
        // Kept so `search exactly` has something to undo. A search that was
        // not corrected leaves nothing to offer back.
        app.selection.search = outcome;
    } else {
        app.selection.search = search::Outcome {
            typed: None,
            ..outcome
        };
    }
}

// ---------------------------------------------------------------- sources

/// Re-derive which sources are on screen, then show a page of them.
fn show_sources(app: &mut App, page: usize) {
    if app.selection.ranked.is_empty() {
        say!(app.text, "cli.sources.nolist");
        return;
    }
    let buckets = sources::qualities(&app.selection.ranked);
    let quality = app
        .selection
        .quality
        .and_then(|position| buckets.get(position).map(|(quality, _)| *quality));

    app.selection.visible = sources::visible(
        &app.selection.ranked,
        quality,
        app.selection.filter,
        &app.settings.playback.subtitle_languages,
    );

    let showing = quality.map_or_else(
        || app.text.get("cli.sources.allqualities"),
        |quality| quality.label().to_owned(),
    );
    sources::print_page(
        &app.selection.ranked,
        &app.selection.visible,
        &app.registry,
        page,
        &showing,
        app.selection.filter,
        &app.text,
    );
}

fn sources_command(app: &mut App, rest: &[&str]) {
    show_sources(app, paging::requested(rest));
}

fn resolution_command(app: &mut App, rest: &[&str]) {
    if app.selection.ranked.is_empty() {
        say!(app.text, "cli.sources.nolist");
        return;
    }
    match rest.first().copied() {
        // `res` on its own asks the question rather than answering it.
        None => sources::print_qualities(&app.selection.ranked, &app.text),
        Some("all" | "none" | "any") => {
            app.selection.quality = None;
            show_sources(app, 1);
        }
        Some(argument) => {
            let buckets = sources::qualities(&app.selection.ranked);
            match argument
                .parse::<usize>()
                .ok()
                .and_then(|n| n.checked_sub(1))
                .filter(|index| *index < buckets.len())
            {
                Some(index) => {
                    app.selection.quality = Some(index);
                    show_sources(app, 1);
                }
                None => say!(app.text, "cli.sources.nosuchres", argument),
            }
        }
    }
}

fn filter_command(app: &mut App, rest: &[&str]) {
    let Some(word) = rest.first() else {
        say!(app.text, "cli.filter.current", app.selection.filter.label());
        return;
    };
    let Some(filter) = sources::Filter::parse(word) else {
        say!(app.text, "cli.filter.unknown", word);
        return;
    };

    // Promoting subtitles needs to know which language to promote, and saying
    // so beats silently doing nothing.
    if filter == sources::Filter::Subtitles
        && app.settings.playback.subtitle_languages.is_empty()
        && app.settings.ranking.preferred_audio_languages.is_empty()
    {
        say!(app.text, "cli.filter.subnolang");
        return;
    }

    app.selection.filter = filter;
    match filter {
        sources::Filter::Subtitles => {
            let languages = subtitle_languages(app);
            say!(app.text, filter.message_key(), &languages.join(", "));
        }
        other => say!(app.text, other.message_key()),
    }
    show_sources(app, 1);
}

/// Which subtitle languages this session prefers.
///
/// Falls back to the audio-language preference, because someone who asked for
/// Turkish audio and never set a subtitle language almost certainly wants
/// Turkish subtitles too.
fn subtitle_languages(app: &App) -> Vec<String> {
    if app.settings.playback.subtitle_languages.is_empty() {
        app.settings.ranking.preferred_audio_languages.clone()
    } else {
        app.settings.playback.subtitle_languages.clone()
    }
}

fn trailer_command(app: &mut App) {
    let Some(meta) = app.selection.meta.clone() else {
        say!(app.text, "cli.trailer.noitem");
        return;
    };
    // Stremio shows a trailer from the metadata; addons put it in `trailers`
    // as a YouTube id, or in `trailerStreams`.
    let Some(url) = meta.trailer_url() else {
        say!(app.text, "cli.trailer.none");
        return;
    };
    say!(app.text, "cli.trailer.playing", meta.display_name());
    launch(app, PlaybackSource::Url(url), Vec::new(), Vec::new(), None);
}

/// Unix seconds as the signed value the signature and revocation layers use.
///
/// They take `i64` because a key's `notBefore` can legitimately precede the
/// epoch on an imported key, and the history layer uses `u64` because a watch
/// position cannot.
fn now_unix_signed() -> i64 {
    i64::try_from(now_unix()).unwrap_or(i64::MAX)
}

// ---------------------------------------------------------------- modules

fn install_module(app: &mut App, rest: &[&str]) {
    let Some(path) = rest.first() else {
        say!(app.text, "cli.error.needsargument");
        return;
    };
    let now = now_unix_signed();
    let installed = modules::install(
        &mut app.modules,
        &app.trust,
        &app.revocations,
        now,
        path,
        &app.text,
        // Installing is the one thing in this program that asks before acting,
        // because it is the one thing that grants something (madde 3).
        |_prepared, text| confirm(text),
    );
    if installed {
        app.save_modules();
    }
}

fn module_operation(app: &mut App, command: &str, rest: &[&str]) {
    let operation = match command {
        "uninstall" => "remove",
        "enable-module" => "enable",
        "disable-module" => "disable",
        _ => "order",
    };
    if modules::operate(&mut app.modules, operation, rest, &app.text) {
        app.save_modules();
    }
}

fn theme_command(app: &mut App, rest: &[&str]) {
    match rest.first().copied() {
        // `theme none` goes back to the built-in palette.
        Some("none") => {
            app.settings.theme = None;
            app.save_settings();
            modules::theme(&app.modules, None, &rest[1..], &app.text);
        }
        Some("tokens") | None => {
            let selected = app.settings.theme.clone();
            modules::theme(&app.modules, selected.as_deref(), rest, &app.text);
        }
        Some(id) => {
            // Resolve it before storing it, so a name that does not work is
            // not written into the settings.
            let candidate = id.to_owned();
            modules::theme(&app.modules, Some(&candidate), &rest[1..], &app.text);
            if app.modules.get(&candidate).is_some() {
                app.settings.theme = Some(candidate);
                app.save_settings();
            }
        }
    }
}

// ---------------------------------------------------------------- settings

fn set_setting(app: &mut App, rest: &[&str]) {
    if prefs::set(&mut app.settings, rest, &app.text) {
        app.save_settings();
        // The language takes effect at once rather than at the next launch.
        if rest.first().copied() == Some("language") {
            app.text = Text::new(&app.settings.language);
        }
    }
}

fn set_language(app: &mut App, rest: &[&str]) {
    let Some(locale) = rest.first() else {
        println!("  {}", app.text.locale());
        return;
    };
    // Changed quietly, catalogues reloaded, then reported -- so the
    // confirmation arrives in the language that was just chosen rather than
    // the one being left behind.
    if prefs::set_quietly(&mut app.settings, &["language", locale], &app.text) {
        app.text = Text::new(&app.settings.language);
        app.save_settings();
        say!(app.text, "cli.settings.set", "language", locale);
    }
}

// ---------------------------------------------------------------- updates

fn update_command(app: &mut App, rest: &[&str]) {
    let now = now_unix_signed();
    // `update verify <file>` checks a download against the signed manifest
    // rather than looking for a newer one.
    if rest.first().copied() == Some("verify") {
        let Some(path) = rest.get(1) else {
            say!(app.text, "cli.error.needsargument");
            return;
        };
        prefs::verify_download(app.client.http(), &app.trust, path, now, &app.text);
        return;
    }
    prefs::check_update(
        app.client.http(),
        &app.trust,
        VERSION,
        &app.settings,
        now,
        &app.text,
    );
    if app.settings.updates.refresh_revocations {
        refresh_revocations(app);
    }
}

fn revocations_command(app: &mut App, rest: &[&str]) {
    match rest.first().copied() {
        Some("refresh") => refresh_revocations(app),
        _ => {
            prefs::revocation_status(&app.revocations, now_unix_signed(), &app.text);
            let now = now_unix_signed();
            prefs::apply_revocations(&app.revocations, &mut app.modules, now, &app.text);
        }
    }
}

fn refresh_revocations(app: &mut App) {
    let now = now_unix_signed();
    let refreshed = prefs::refresh_revocations(
        app.client.http(),
        &mut app.revocations,
        &mut app.modules,
        &app.trust,
        now,
        &app.text,
    );
    if refreshed {
        app.save_revocations();
        app.save_modules();
    }
    prefs::revocation_status(&app.revocations, now, &app.text);
}

// ---------------------------------------------------------------- torrents

fn torrent_files(app: &mut App, rest: &[&str]) {
    let Some(engine) = app.torrent.as_ref() else {
        say!(app.text, "cli.torrent.nosession");
        return;
    };
    let torrents = engine.list();
    if torrents.is_empty() {
        say!(app.text, "cli.torrent.nosession");
        return;
    }
    // One torrent at a time pages cleanly; several would make a page number
    // ambiguous, so with more than one the listing stays whole and each
    // torrent is short enough to read.
    let page = paging::requested(rest);
    for handle in torrents {
        println!(
            "
{}",
            handle.name().unwrap_or(handle.info_hash())
        );
        torrents::list_files(&handle, page, &app.text);
    }
}

/// Ask before granting something.
fn confirm(text: &Text) -> bool {
    print!("{} ", text.get("cli.common.confirm"));
    let _ = io::stdout().flush();
    let mut line = String::new();
    if io::stdin().read_line(&mut line).is_err() {
        return false;
    }
    let answer = line.trim().to_ascii_lowercase();
    answer == "yes" || answer == text.get("cli.common.yes").to_ascii_lowercase()
}

// ---------------------------------------------------------------- addons

fn add(app: &mut App, rest: &[&str]) {
    let Some(url) = rest.first() else {
        println!("usage: add <manifest url>");
        return;
    };
    print!("fetching... ");
    let _ = io::stdout().flush();
    match app.client.install(&mut app.registry, url) {
        Ok(addon) => {
            app.save_addons();
            let s = addon.summary();
            println!("\radded {} {}", s.name, s.version);
            println!("  id         {}", s.id);
            println!("  types      {}", s.types.join(", "));
            println!("  resources  {}", s.resources.join(", "));
            println!("  catalogues {}", s.catalog_count);
            if s.configuration_required {
                println!("  note: this addon does nothing useful until configured");
                println!("        `configure {}` for where", app.registry.len());
            }
        }
        Err(e) => println!("\rcould not add the addon: {e}"),
    }
}

fn list(app: &App) {
    if app.registry.is_empty() {
        println!("no addons installed");
        return;
    }
    let now = now_unix();
    for (index, addon) in app.registry.addons().iter().enumerate() {
        let s = addon.summary();
        let state = if s.enabled { "" } else { "  [disabled]" };
        let health = app
            .health
            .get(&s.id)
            .map(|h| h.display_state(now))
            .filter(|state| state != "ok")
            .map_or_else(String::new, |state| format!("  [{state}]"));
        println!("{:>3}. {} {}{state}{health}", index + 1, s.name, s.version);
        println!("     {}  ·  {}", s.id, s.resources.join(", "));
    }
}

fn nth_addon_id(app: &App, arg: Option<&&str>) -> Option<String> {
    let n: usize = arg?.parse().ok()?;
    app.registry
        .addons()
        .get(n.checked_sub(1)?)
        .map(|a| a.id().to_owned())
}

fn remove(app: &mut App, rest: &[&str]) {
    let Some(id) = nth_addon_id(app, rest.first()) else {
        println!("usage: remove <number from `list`>");
        return;
    };
    match app.registry.remove(&id) {
        Ok(addon) => {
            app.save_addons();
            println!("removed {}", addon.manifest.name);
        }
        Err(e) => println!("{e}"),
    }
}

fn set_enabled(app: &mut App, rest: &[&str], enabled: bool) {
    let word = if enabled { "enable" } else { "disable" };
    let Some(id) = nth_addon_id(app, rest.first()) else {
        println!("usage: {word} <number from `list`>");
        return;
    };
    match app.registry.set_enabled(&id, enabled) {
        Ok(()) => {
            app.save_addons();
            println!("{id} {word}d");
        }
        Err(e) => println!("{e}"),
    }
}

fn refresh(app: &mut App, rest: &[&str]) {
    let Some(id) = nth_addon_id(app, rest.first()) else {
        println!("usage: refresh <number from `list`>");
        return;
    };
    match app.client.refresh(&mut app.registry, &id) {
        Ok(addon) => {
            app.save_addons();
            println!("{} is now {}", addon.manifest.name, addon.manifest.version);
        }
        Err(e) => println!("could not refresh: {e}"),
    }
}

fn reorder(app: &mut App, rest: &[&str]) {
    let (Some(id), Some(to)) = (
        nth_addon_id(app, rest.first()),
        rest.get(1).and_then(|n| n.parse::<usize>().ok()),
    ) else {
        println!("usage: order <number from `list`> <new position>");
        println!("  order decides priority: the first addon wins a tie between sources");
        return;
    };
    match app.registry.reorder(&id, to.saturating_sub(1)) {
        Ok(()) => {
            app.save_addons();
            list(app);
        }
        Err(e) => println!("{e}"),
    }
}

fn configure(app: &App, rest: &[&str]) {
    let Some(id) = nth_addon_id(app, rest.first()) else {
        println!("usage: configure <number from `list`>");
        return;
    };
    let Some(addon) = app.registry.get(&id) else {
        println!("no such addon");
        return;
    };
    let hints = &addon.manifest.behavior_hints;
    if !hints.configurable && !hints.configuration_required {
        println!(
            "{} does not offer a configuration page.",
            addon.manifest.name
        );
        return;
    }
    // The configure page is the addon's own. Using it produces a new manifest URL
    // with the configuration in its path, which installs as a separate addon --
    // that is how the protocol carries per-user addon settings.
    println!("{} is configured on its own page:", addon.manifest.name);
    println!("  {}/configure", addon.address.base());
    println!();
    println!("Open that in a browser, choose your settings, and it hands you a new");
    println!("manifest URL with the configuration in it. Install that with `add <url>`.");
    println!("The configured addon is a separate install, so remove this one if you no");
    println!("longer want the unconfigured version.");
    if hints.configuration_required {
        println!("\nThis addon says it does nothing useful until configured.");
    }
}

fn discover(app: &mut App, rest: &[&str]) {
    // Install one that was already listed.
    if let Some(index) = rest.first().and_then(|n| n.parse::<usize>().ok()) {
        let Some(entry) = index
            .checked_sub(1)
            .and_then(|i| app.selection.discovered.get(i))
        else {
            println!("no advertised addon number {index}; run `discover` first");
            return;
        };
        let url = entry.transport_url.clone();
        // The advertised manifest is a hint. Installing re-fetches from the addon
        // itself, so an addon cannot lie about what another addon does.
        println!("installing from its own manifest, not the advertised copy...");
        add(app, &[url.as_str()]);
        return;
    }

    let advertisers: Vec<(String, String, Vec<String>)> = app
        .registry
        .addons()
        .iter()
        .filter(|a| {
            a.manifest
                .resources
                .iter()
                .any(|r| r.name() == "addon_catalog")
        })
        .map(|a| {
            (
                a.id().to_owned(),
                a.manifest.name.clone(),
                a.manifest.types.clone(),
            )
        })
        .collect();

    if advertisers.is_empty() {
        println!("None of the installed addons advertises others.");
        println!("An addon offering the `addon_catalog` resource can list more addons;");
        println!("until then, add addons by URL.");
        return;
    }

    app.selection.discovered.clear();
    for (addon_id, name, types) in &advertisers {
        for content_type in types {
            // Catalogue ids are addon-defined; these are the usual ones, and a
            // wrong guess is a plain error rather than a problem.
            for catalog_id in ["all", "community", "official"] {
                let Some(addon) = app.registry.get(addon_id) else {
                    continue;
                };
                match app.client.addon_catalog(addon, content_type, catalog_id) {
                    Ok(entries) if !entries.is_empty() => {
                        app.health.record_success(addon_id);
                        println!("from {name} ({content_type}/{catalog_id}):");
                        for entry in entries {
                            println!(
                                "{:>3}. {} {}",
                                app.selection.discovered.len() + 1,
                                entry.manifest.name,
                                entry.manifest.version
                            );
                            app.selection.discovered.push(entry);
                        }
                        break;
                    }
                    Ok(_) => {}
                    Err(e) => {
                        app.health
                            .record_failure(addon_id, now_unix(), &e.to_string());
                    }
                }
            }
        }
    }

    if app.selection.discovered.is_empty() {
        println!("nothing advertised, or the catalogue ids are not the usual ones");
    } else {
        println!("\n`discover <n>` to install one");
    }
}

fn health(app: &App) {
    let now = now_unix();
    let resting = app.health.resting(now);
    if resting.is_empty() {
        println!("no addon is currently being rested");
    } else {
        println!("resting after repeated failures:");
        for (id, remaining) in resting {
            println!("  {id}  for another {remaining}s");
        }
    }
    for addon in app.registry.addons() {
        if let Some(health) = app.health.get(addon.id()) {
            println!(
                "  {}  {} ok / {} failed  ·  {}",
                addon.manifest.name,
                health.total_successes,
                health.total_failures,
                health.display_state(now)
            );
        }
    }
}

// ---------------------------------------------------------------- browsing

fn catalogs(app: &mut App) {
    app.selection.catalogs.clear();
    for addon in app.registry.addons() {
        for catalog in &addon.manifest.catalogs {
            let required = catalog.required_extra();
            let mut marks = Vec::new();
            if catalog.is_searchable() {
                marks.push("searchable".to_owned());
            }
            if !required.is_empty() {
                marks.push(format!("requires {}", required.join(", ")));
            }
            let suffix = if marks.is_empty() {
                String::new()
            } else {
                format!("  ({})", marks.join(", "))
            };
            app.selection.catalogs.push(CatalogRef {
                addon_id: addon.id().to_owned(),
                content_type: catalog.content_type.clone(),
                catalog_id: catalog.id.clone(),
                label: format!(
                    "{} · {} / {}{suffix}",
                    addon.manifest.name,
                    catalog.content_type,
                    catalog.display_name()
                ),
            });
        }
    }
    if app.selection.catalogs.is_empty() {
        println!("none of the installed addons offers a browsable catalogue");
        return;
    }
    for (index, catalog) in app.selection.catalogs.iter().enumerate() {
        println!("{:>3}. {}", index + 1, catalog.label);
    }
    println!("\n`browse <n>` to open one, or `browse <n> <search terms>`");
}

fn browse(app: &mut App, rest: &[&str]) {
    if app.selection.catalogs.is_empty() {
        println!("run `catalogs` first");
        return;
    }
    let Some(index) = rest.first().and_then(|n| n.parse::<usize>().ok()) else {
        println!("usage: browse <number from `catalogs`> [search terms]");
        return;
    };
    let query = rest[1..].join(" ");
    fetch_page(app, index, &query, 0);
}

fn more(app: &mut App) {
    let Some((index, query, skip)) = app.selection.page.clone() else {
        println!("browse a catalogue first");
        return;
    };
    let next = skip + app.selection.items.len().max(1);
    fetch_page(app, index, &query, next);
}

fn fetch_page(app: &mut App, index: usize, query: &str, skip: usize) {
    let Some(catalog) = index
        .checked_sub(1)
        .and_then(|i| app.selection.catalogs.get(i))
    else {
        println!("no catalogue number {index}");
        return;
    };
    let addon_id = catalog.addon_id.clone();
    let content_type = catalog.content_type.clone();
    let catalog_id = catalog.catalog_id.clone();
    println!("{}", catalog.label);

    let Some(addon) = app.registry.get(&addon_id) else {
        println!("that addon is no longer installed");
        return;
    };

    let skip_text = skip.to_string();
    let mut extra: Vec<(&str, &str)> = Vec::new();
    if !query.is_empty() {
        extra.push(("search", query));
    }
    if skip > 0 {
        extra.push(("skip", skip_text.as_str()));
    }

    match app
        .client
        .catalog(addon, &content_type, &catalog_id, &extra)
    {
        Ok(page) => {
            app.health.record_success(&addon_id);
            if page.is_empty() {
                println!("  (no more items)");
                return;
            }
            app.selection.items = page;
            app.selection.page = Some((index, query.to_owned(), skip));
            for (n, item) in app.selection.items.iter().enumerate() {
                let year = item.release_info.as_deref().unwrap_or("    ");
                let rating = item
                    .imdb_rating
                    .as_deref()
                    .map_or_else(String::new, |r| format!("  {r}"));
                let seen = seen_marker(app, &item.content_type, &item.id);
                println!(
                    "{:>3}. {year}  {}{rating}{seen}",
                    n + 1,
                    item.display_name()
                );
            }
            println!("\n`open <n>` for details and sources · `more` for the next page");
        }
        Err(e) => {
            app.health
                .record_failure(&addon_id, now_unix(), &e.to_string());
            println!("  catalogue request failed: {e}");
        }
    }
}

/// A short mark showing whether something has been watched.
fn seen_marker(app: &App, content_type: &str, id: &str) -> String {
    app.history
        .get(content_type, id)
        .map_or_else(String::new, |entry| {
            if entry.is_finished() {
                "  watched".to_owned()
            } else {
                entry
                    .progress()
                    .map_or_else(String::new, |p| format!("  {:.0}%", p * 100.0))
            }
        })
}

fn open(app: &mut App, rest: &[&str]) {
    let Some(index) = rest.first().and_then(|n| n.parse::<usize>().ok()) else {
        println!("usage: open <number from the last listing>");
        return;
    };
    let Some(item) = index
        .checked_sub(1)
        .and_then(|i| app.selection.items.get(i))
        .cloned()
    else {
        println!("no item number {index} in the last listing");
        return;
    };
    let name = item.display_name().to_owned();
    show_item(app, &item.content_type, &item.id, &name);
}

fn show_item(app: &mut App, content_type: &str, id: &str, fallback_name: &str) {
    // Metadata is merged across every addon that has some: the first addon in the
    // user's order is authoritative, later ones fill gaps, and episode lists are
    // unioned.
    let (meta, contributors, failures) = app.client.meta_merged(&app.registry, content_type, id);
    for addon_id in &contributors {
        app.health.record_success(addon_id);
    }
    for failure in &failures {
        app.health
            .record_failure(&failure.addon_id, now_unix(), &failure.error.to_string());
        println!("  metadata addon failed: {failure}");
    }

    let name = match &meta {
        Some(meta) => {
            print_meta(meta);
            if contributors.len() > 1 {
                println!("  (merged from {} addons)", contributors.len());
            }
            // Say so when this is already kept, so `save` is not offered for
            // something that is in the library and `unsave` is discoverable.
            if let Some(mark) = shelf::marker(&app.library, content_type, id, &app.text) {
                println!("  ★ {mark}");
            }
            if meta.trailer_url().is_some() {
                say!(app.text, "cli.trailer.available");
            }
            app.selection.episodes = meta.videos.clone();
            meta.display_name().to_owned()
        }
        None => {
            println!("\n{fallback_name}");
            println!("  (no installed addon provides metadata for this item)");
            app.selection.episodes.clear();
            fallback_name.to_owned()
        }
    };
    app.selection.meta = meta;

    if let Some(entry) = app.history.get(content_type, id) {
        println!("\n  {}", entry.display_line());
    }

    list_sources(app, content_type, id, &name);
}

fn list_sources(app: &mut App, content_type: &str, id: &str, name: &str) {
    app.selection.subject = Some((content_type.to_owned(), id.to_owned(), name.to_owned()));

    let found = app.client.streams_from_all(&app.registry, content_type, id);
    for (addon_id, _) in &found.items {
        app.health.record_success(addon_id);
    }
    for failure in &found.failures {
        app.health
            .record_failure(&failure.addon_id, now_unix(), &failure.error.to_string());
    }

    app.selection.ranked = rank(&found.items, &app.settings.ranking);
    // A new subject starts unnarrowed: carrying a quality or a filter over
    // from the last thing opened would silently hide most of this one.
    app.selection.quality = None;
    app.selection.filter = sources::Filter::None;
    app.selection.visible = (0..app.selection.ranked.len()).collect();

    println!();
    if app.selection.ranked.is_empty() {
        if found.failures.is_empty() {
            say!(app.text, "cli.sources.none");
            say!(app.text, "cli.sources.nonehint");
        }
        for failure in &found.failures {
            println!("  addon failed: {failure}");
        }
        return;
    }

    // The useful question about a hundred sources is narrow -- which quality,
    // and then which of those -- so that is what gets asked first. The old
    // behaviour printed all of them, which is a wall of text.
    println!(
        "{}",
        app.text.format(
            "cli.sources.header",
            &[&app.selection.ranked.len().to_string(), name],
        )
    );
    println!();
    sources::print_qualities(&app.selection.ranked, &app.text);
    for failure in &found.failures {
        println!("  addon failed: {failure}");
    }
}

pub(crate) fn source_kind(stream: &Stream) -> &'static str {
    match stream.source() {
        Some(StreamSource::Direct(_)) => "http",
        Some(StreamSource::Torrent { .. }) => "torrent",
        Some(StreamSource::YouTube(_)) => "youtube",
        Some(StreamSource::External(_)) => "external",
        None => "unusable",
    }
}

fn print_meta(meta: &Meta) {
    println!("\n{}", meta.display_name());
    if let Some(year) = &meta.release_info {
        println!("  year     {year}");
    }
    if let Some(runtime) = &meta.runtime {
        println!("  runtime  {runtime}");
    }
    if !meta.genres.is_empty() {
        println!("  genres   {}", meta.genres.join(", "));
    }
    if !meta.cast.is_empty() {
        println!("  cast     {}", meta.cast.join(", "));
    }
    if let Some(description) = &meta.description {
        println!("\n  {description}");
    }
    let seasons = meta.seasons();
    if !seasons.is_empty() {
        println!(
            "\n  {} season(s), {} episode(s). `episodes [season]`",
            seasons.len(),
            meta.videos.len()
        );
    }
}

fn episodes(app: &mut App, rest: &[&str]) {
    let Some(meta) = app.selection.meta.clone() else {
        println!("open a series first");
        return;
    };
    let wanted = rest.first().and_then(|n| n.parse::<u32>().ok());
    let episodes: Vec<Video> = match wanted {
        Some(season) => meta.season(season).into_iter().cloned().collect(),
        None => meta.videos.clone(),
    };
    if episodes.is_empty() {
        match wanted {
            Some(season) => println!("no episodes in season {season}"),
            None => println!("this item has no episodes"),
        }
        return;
    }
    app.selection.episodes = episodes;
    let lines: Vec<String> = app
        .selection
        .episodes
        .iter()
        .enumerate()
        .map(|(n, video)| {
            let seen = seen_marker(app, &meta.content_type, &video.id);
            format!("{:>3}. {}{seen}", n + 1, video.display_title())
        })
        .collect();
    for line in lines {
        println!("{line}");
    }
    println!("\n`ep <n>` for sources");
}

fn open_episode(app: &mut App, rest: &[&str]) {
    let Some(index) = rest.first().and_then(|n| n.parse::<usize>().ok()) else {
        println!("usage: ep <number from `episodes`>");
        return;
    };
    let Some(video) = index
        .checked_sub(1)
        .and_then(|i| app.selection.episodes.get(i))
        .cloned()
    else {
        println!("no episode number {index}");
        return;
    };
    let Some(meta) = app.selection.meta.clone() else {
        println!("open a series first");
        return;
    };
    println!("\n{}", video.display_title());
    if let Some(entry) = app.history.get(&meta.content_type, &video.id) {
        println!("  {}", entry.display_line());
    }
    let name = format!("{} — {}", meta.display_name(), video.display_title());
    list_sources(app, &meta.content_type, &video.id, &name);
}

// ---------------------------------------------------------------- playback

fn play(app: &mut App, rest: &[&str]) {
    let Some(first) = rest.first() else {
        println!("usage: play <number from the last listing>   |   play <url or file path>");
        return;
    };

    // A number selects from the last listing; anything else is taken literally,
    // which is how a local file or a direct URL is tried without an addon.
    let prepared = if let Ok(index) = first.parse::<usize>() {
        let Some(ranked) = index
            .checked_sub(1)
            .and_then(|i| app.selection.ranked.get(i))
            .cloned()
        else {
            // Two different situations, and conflating them sends someone
            // looking for a source number that was never going to be there.
            if app.selection.ranked.is_empty() {
                say!(app.text, "cli.error.nolist");
            } else {
                say!(app.text, "cli.error.outofrange", first);
            }
            return;
        };

        // A torrent is intercepted here, before classification, because
        // resolving one means starting the engine and waiting on the swarm.
        // `play <n> file <i>` overrides which file inside it gets played.
        if let Some(torrent) = torrents::source_of(&ranked.stream) {
            let explicit = rest
                .iter()
                .position(|argument| *argument == "file")
                .and_then(|at| rest.get(at + 1))
                .and_then(|value| value.parse::<usize>().ok());
            // Started first, then borrowed: `torrent_engine` needs `&mut app`
            // to start one, and resolving needs `&app.text` alongside it.
            if app.torrent_engine().is_none() {
                return;
            }
            let resolved = {
                let Some(engine) = app.torrent.as_ref() else {
                    return;
                };
                torrents::resolve(engine, &torrent, explicit, &app.text)
            };
            match resolved {
                Ok((source, _handle, _index)) => {
                    // Subtitles that came with the stream still apply; the
                    // headers do not, since the player is reading from
                    // loopback rather than from the addon's host.
                    let subtitles: Vec<String> = ranked
                        .stream
                        .subtitles
                        .iter()
                        .map(|s| s.url.clone())
                        .collect();
                    let playing = now_playing(app, &ranked);
                    launch(app, source, Vec::new(), subtitles, playing);
                }
                Err(message) => println!("{message}"),
            }
            return;
        }

        match to_playback_source(&ranked.stream) {
            Ok(source) => {
                let headers: Vec<(String, String)> = ranked
                    .stream
                    .request_headers()
                    .into_iter()
                    .map(|(k, v)| (k.to_owned(), v.to_owned()))
                    .collect();
                let subtitles: Vec<String> = ranked
                    .stream
                    .subtitles
                    .iter()
                    .map(|s| s.url.clone())
                    .collect();
                (source, headers, subtitles, now_playing(app, &ranked))
            }
            Err(message) => {
                println!("{message}");
                return;
            }
        }
    } else {
        let joined = rest.join(" ");

        // A magnet link or a bare info hash goes through the engine, which is
        // also the only way to reach it without an addon -- useful for trying
        // a torrent someone sent you, and the path the engine was tested on.
        if let Some(request) = torrents::request_from_argument(first) {
            let explicit = rest
                .iter()
                .position(|argument| *argument == "file")
                .and_then(|at| rest.get(at + 1))
                .and_then(|value| value.parse::<usize>().ok());
            if app.torrent_engine().is_none() {
                return;
            }
            let resolved = {
                let Some(engine) = app.torrent.as_ref() else {
                    return;
                };
                torrents::resolve(
                    engine,
                    &torrents::TorrentSource {
                        request,
                        file_index: None,
                    },
                    explicit,
                    &app.text,
                )
            };
            match resolved {
                Ok((source, _handle, _index)) => {
                    launch(app, source, Vec::new(), Vec::new(), None);
                }
                Err(message) => println!("{message}"),
            }
            return;
        }

        let path = std::path::Path::new(&joined);
        let source = if path.is_file() {
            PlaybackSource::File(path.to_path_buf())
        } else {
            PlaybackSource::Url(joined)
        };
        (source, Vec::new(), Vec::new(), None)
    };
    let (source, headers, subtitles, playing) = prepared;
    launch(app, source, headers, subtitles, playing);
}

/// Start the player on a source that is already resolved.
///
/// Split out of `play` so the torrent path, which resolves its source over
/// several seconds and through a different route, ends up in exactly the same
/// launch: one place that remembers the old position, applies the resume point
/// and reports what mpv said.
fn launch(
    app: &mut App,
    source: PlaybackSource,
    headers: Vec<(String, String)>,
    subtitles: Vec<String>,
    playing: Option<NowPlaying>,
) {
    // Replace whatever was running, keeping its position first.
    app.remember_position();
    if let Some(existing) = app.player.take() {
        let _ = existing.quit();
    }

    let resume = playing
        .as_ref()
        .and_then(|p| app.history.resume_position(&p.content_type, &p.id));
    if let Some(at) = resume {
        println!("resuming at {}", clock(at));
    }

    println!("starting mpv on {}...", source.display_hint());
    if !headers.is_empty() {
        println!("  sending {} header(s) the addon asked for", headers.len());
    }
    if !subtitles.is_empty() {
        println!(
            "  {} subtitle track(s) came with the source",
            subtitles.len()
        );
    }
    let options = PlayerOptions {
        title: Some(format!("EON Stream — {}", source.display_hint())),
        http_headers: headers,
        subtitle_urls: subtitles,
        start_at_secs: resume,
        ..PlayerOptions::default()
    };
    match MpvPlayer::launch(&source, &options) {
        Ok(mut started) => {
            match started.duration() {
                Ok(Some(seconds)) => println!("  duration {}", clock(seconds)),
                _ => println!("  (mpv is loading; duration not known yet)"),
            }
            println!("  mpv has its own window. `pause` `seek` `tracks` `subs` `stop`");
            app.player = Some(started);
            app.playing = playing;
        }
        Err(e) => println!("could not start playback: {e}"),
    }
}

/// What to remember about the thing being played.
///
/// The subject is whatever `list_sources` was last called for, which is the
/// episode rather than the series when an episode was opened — so each episode
/// resumes on its own, while the series is recorded alongside for
/// continue-watching and for keeping the same release.
fn now_playing(app: &App, ranked: &RankedStream) -> Option<NowPlaying> {
    let (content_type, id, name) = app.selection.subject.clone()?;
    let episode = app.selection.episodes.iter().find(|v| v.id == id);
    let series_id = app
        .selection
        .meta
        .as_ref()
        .map(|m| m.id.clone())
        .filter(|series| *series != id);

    Some(NowPlaying {
        content_type,
        id,
        name,
        series_id,
        season: episode.and_then(|v| v.season),
        episode: episode.and_then(|v| v.episode),
        binge_group: ranked.stream.binge_group().map(ToOwned::to_owned),
        addon_id: Some(ranked.addon_id.clone()),
    })
}

/// Sentinel for the one source kind `to_playback_source` cannot resolve on its
/// own. `play` intercepts torrents before this is ever returned; it exists so
/// that path cannot silently become reachable without someone noticing.
const TORRENT_HANDLED_ELSEWHERE: &str = "torrent source: resolved by the engine";

fn to_playback_source(stream: &Stream) -> Result<PlaybackSource, String> {
    match stream.source() {
        Some(StreamSource::Direct(url)) => Ok(PlaybackSource::Url(url.to_owned())),
        Some(StreamSource::YouTube(id)) => {
            // mpv plays this through its ytdl hook, which needs yt-dlp present.
            Ok(PlaybackSource::Url(format!(
                "https://www.youtube.com/watch?v={id}"
            )))
        }
        // A torrent is resolved by the caller rather than here: it needs the
        // engine, and it blocks for as long as the swarm takes. This function
        // is a pure classification, and stays that way.
        Some(StreamSource::Torrent { .. }) => Err(TORRENT_HANDLED_ELSEWHERE.to_owned()),
        Some(StreamSource::External(url)) => Err(format!(
            "This source is a link to open in a browser, not a stream:\n  {url}"
        )),
        None => Err("That source has nothing playable in it.".to_owned()),
    }
}

fn next_episode(app: &mut App) {
    let Some(meta) = app.selection.meta.clone() else {
        println!("open a series first");
        return;
    };
    let Some(playing) = app.playing.clone() else {
        println!("nothing is playing, so there is no next episode");
        return;
    };
    let Some(next) = meta
        .videos
        .iter()
        .position(|v| v.id == playing.id)
        .and_then(|i| meta.videos.get(i + 1))
        .cloned()
    else {
        println!("that was the last episode listed");
        return;
    };

    println!("next: {}", next.display_title());
    let name = format!("{} — {}", meta.display_name(), next.display_title());
    list_sources(app, &meta.content_type, &next.id, &name);

    // Keep the same release when the addon groups them; that is what bingeGroup
    // is for, and it saves choosing a source for every episode.
    let wanted = app
        .history
        .binge_group_for_series(&meta.id)
        .map(ToOwned::to_owned)
        .or(playing.binge_group);
    let choice = wanted.and_then(|group| {
        app.selection
            .ranked
            .iter()
            .position(|r| r.stream.binge_group() == Some(group.as_str()))
    });
    match choice {
        Some(index) => {
            println!("keeping the same release as last time");
            play(app, &[&(index + 1).to_string()]);
        }
        None if !app.selection.ranked.is_empty() => println!("`play <n>` to choose a source"),
        None => {}
    }
}

fn control(app: &mut App, command: &str, rest: &[&str]) {
    if command == "stop" {
        app.remember_position();
        match app.player.take() {
            Some(active) => {
                let _ = active.quit();
                app.playing = None;
                println!("stopped");
            }
            None => println!("nothing is playing"),
        }
        return;
    }

    if !ensure_playing(app) {
        return;
    }

    if matches!(command, "where" | "buffer") {
        report_position(app);
        return;
    }

    let Some(active) = app.player.as_mut() else {
        return;
    };
    let outcome = match command {
        "pause" => active.set_paused(true),
        "resume" => active.set_paused(false),
        _ => match rest.first().and_then(|s| s.parse::<f64>().ok()) {
            Some(seconds) => active.seek(seconds, SeekMode::Relative),
            None => {
                println!("usage: seek <seconds, negative to go back>");
                return;
            }
        },
    };
    match outcome {
        Ok(()) => println!("  ok"),
        Err(e) => println!("  {e}"),
    }
}

fn report_position(app: &mut App) {
    let Some(active) = app.player.as_mut() else {
        return;
    };
    let position = active.position().ok().flatten();
    let duration = active.duration().ok().flatten();
    let buffered = active.buffered_secs().ok().flatten();
    let speed = active.network_speed_bytes().ok().flatten();
    let stalled = active.is_buffering().unwrap_or(false);

    match (position, duration) {
        (Some(p), Some(d)) => println!("  {} / {}", clock(p), clock(d)),
        (Some(p), None) => println!("  {}", clock(p)),
        _ => println!("  position not known yet"),
    }
    if let Some(buffered) = buffered {
        println!("  buffered {buffered:.1}s");
    }
    if let Some(speed) = speed {
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let bytes = speed.max(0.0) as u64;
        println!("  reading  {}/s", format_size(bytes));
    }
    if stalled {
        println!("  stalled waiting for data");
    }
    app.remember_position();
}

fn ensure_playing(app: &mut App) -> bool {
    let running = match app.player.as_mut() {
        None => {
            println!("nothing is playing. `play <n>` first.");
            return false;
        }
        Some(active) => active.is_running(),
    };
    if !running {
        println!("mpv has exited.");
        app.remember_position();
        app.player = None;
        app.playing = None;
        return false;
    }
    true
}

fn tracks(app: &mut App) {
    if !ensure_playing(app) {
        return;
    }
    let Some(active) = app.player.as_mut() else {
        return;
    };
    match active.tracks() {
        Ok(tracks) if tracks.is_empty() => println!("mpv reports no tracks yet"),
        Ok(tracks) => {
            for kind in ["video", "audio", "sub"] {
                let of_kind: Vec<_> = tracks.iter().filter(|t| t.kind == kind).collect();
                if of_kind.is_empty() {
                    continue;
                }
                println!("{kind}:");
                for track in of_kind {
                    println!("  {}", track.display_line());
                }
            }
            println!("\n`audio <n>` `video <n>` `subtrack <n>` to select, `off` to disable");
        }
        Err(e) => println!("{e}"),
    }
}

fn select_track(app: &mut App, command: &str, rest: &[&str]) {
    if !ensure_playing(app) {
        return;
    }
    let kind = match command {
        "audio" => "audio",
        "video" => "video",
        _ => "sub",
    };
    let Some(arg) = rest.first() else {
        println!("usage: {command} <track number from `tracks`>  |  {command} off");
        return;
    };
    let id = if arg.eq_ignore_ascii_case("off") {
        None
    } else if let Ok(id) = arg.parse::<u32>() {
        Some(id)
    } else {
        println!("usage: {command} <track number>  |  {command} off");
        return;
    };
    let Some(active) = app.player.as_mut() else {
        return;
    };
    match active.select_track(kind, id) {
        Ok(()) => println!("  ok"),
        Err(e) => println!("  {e}"),
    }
}

fn subtitles(app: &mut App, rest: &[&str]) {
    // A page number re-shows what is already fetched rather than asking every
    // subtitle addon again: `subs 3` is turning a page, not a new search.
    if !app.selection.subtitles.is_empty() {
        if let Some(page) = rest
            .first()
            .filter(|_| rest.len() == 1)
            .and_then(|first| first.parse::<usize>().ok())
        {
            print_subtitles(app, page);
            return;
        }
    }

    let Some(playing) = app.playing.clone() else {
        say!(app.text, "cli.subs.nolist");
        return;
    };

    // Tell the subtitle addons what file is playing. Name, hash and size are what
    // turn "many tracks for this film" into "the right track for this release".
    let matching = app
        .selection
        .ranked
        .iter()
        .find(|r| Some(&r.addon_id) == playing.addon_id.as_ref())
        .map(|r| r.stream.subtitle_match())
        .unwrap_or_default();
    let matched_on_file = !matching.is_empty();

    let found = app.client.subtitles_for_source_from_all(
        &app.registry,
        &playing.content_type,
        &playing.id,
        matching,
    );
    for (addon_id, _) in &found.items {
        app.health.record_success(addon_id);
    }
    for failure in &found.failures {
        app.health
            .record_failure(&failure.addon_id, now_unix(), &failure.error.to_string());
        println!("  subtitle addon failed: {failure}");
    }

    app.selection.subtitles = found.items.into_iter().flat_map(|(_, v)| v).collect();
    app.selection.subtitle_matches = None;
    app.selection.subtitle_query.clear();
    if app.selection.subtitles.is_empty() {
        say!(app.text, "cli.subs.none");
        return;
    }
    if matched_on_file {
        say!(app.text, "cli.subs.matched");
    }
    print_subtitles(app, paging::requested(rest));
}

/// Print one page of the subtitle tracks, or of the last `sub search` result.
///
/// Numbers are absolute positions in the full track list, so `sub 47` means
/// the same track whichever page or search narrowed the view. This is the
/// defect this whole command was rewritten for: it used to print forty tracks
/// and say `... 38 more`, with no way to reach the thirty-eight.
fn print_subtitles(app: &App, requested: usize) {
    let all = &app.selection.subtitles;
    let shown: Vec<usize> = match &app.selection.subtitle_matches {
        Some(matches) => matches.clone(),
        None => (0..all.len()).collect(),
    };

    let Some(page) = paging::Page::of(shown.len(), requested, paging::DEFAULT_PAGE_SIZE) else {
        say!(app.text, "cli.subs.none");
        return;
    };

    println!();
    if app.selection.subtitle_matches.is_some() {
        println!(
            "{}",
            app.text.format(
                "cli.subs.searchheader",
                &[
                    &shown.len().to_string(),
                    &all.len().to_string(),
                    &app.selection.subtitle_query,
                ],
            )
        );
    } else {
        say!(app.text, "cli.subs.header", &all.len().to_string());
    }
    println!();

    for index in page.slice(&shown) {
        let Some(subtitle) = all.get(*index) else {
            continue;
        };
        println!(
            "{:>4}. {:<6} {}",
            index + 1,
            subtitle.lang.as_deref().unwrap_or("??"),
            subtitle.id
        );
    }

    println!();
    // Paging a search keeps the search; paging the whole list keeps the list.
    let command = if app.selection.subtitle_matches.is_some() {
        format!("sub search {}", app.selection.subtitle_query)
    } else {
        "subs".to_owned()
    };
    println!("{}", page.footer(&command, &app.text));
    say!(app.text, "cli.subs.loadhint");
    if app.selection.subtitle_matches.is_none() && all.len() > paging::DEFAULT_PAGE_SIZE {
        say!(app.text, "cli.subs.searchhint");
    }
}

/// `sub <n>` loads a track; `sub search <words>` narrows the list.
fn sub_command(app: &mut App, rest: &[&str]) {
    if rest.first().is_some_and(|first| {
        first.eq_ignore_ascii_case("search") || first.eq_ignore_ascii_case("find")
    }) {
        subtitle_search(app, &rest[1..]);
        return;
    }
    load_subtitle(app, rest);
}

/// Narrow the subtitle list by language or release name, forgiving spelling.
fn subtitle_search(app: &mut App, rest: &[&str]) {
    if app.selection.subtitles.is_empty() {
        say!(app.text, "cli.subs.nolist");
        return;
    }
    // A bare number pages the last search rather than searching for "2".
    if let Some(page) = rest
        .first()
        .filter(|_| rest.len() == 1)
        .and_then(|first| first.parse::<usize>().ok())
    {
        if app.selection.subtitle_matches.is_some() {
            print_subtitles(app, page);
            return;
        }
    }

    let query = rest.join(" ");
    if query.trim().is_empty() {
        // An empty search is a request to see everything again.
        app.selection.subtitle_matches = None;
        app.selection.subtitle_query.clear();
        print_subtitles(app, 1);
        return;
    }

    // Each track is matched on its language and its id, because the id is
    // usually the release name and that is what someone looking for a specific
    // rip will type. Approximate, so `turkish`, `turkce`, `tr` and `tur` all
    // land on the same tracks.
    let matches: Vec<usize> = app
        .selection
        .subtitles
        .iter()
        .enumerate()
        .filter(|(_, subtitle)| {
            let language = subtitle.lang.as_deref().unwrap_or_default();
            let haystacks = [language, subtitle.id.as_str()];
            haystacks.iter().any(|field| {
                let normalized = eon_stream_core::fuzzy::normalize(field);
                let wanted = eon_stream_core::fuzzy::normalize(&query);
                normalized.contains(&wanted)
                    || eon_stream_core::fuzzy::similarity(&query, field)
                        >= eon_stream_core::fuzzy::MIN_SIMILARITY
            })
        })
        .map(|(index, _)| index)
        .collect();

    if matches.is_empty() {
        say!(
            app.text,
            "cli.subs.searchnone",
            &query,
            &app.selection.subtitles.len().to_string()
        );
        return;
    }
    app.selection.subtitle_query = query;
    app.selection.subtitle_matches = Some(matches);
    print_subtitles(app, 1);
}

fn load_subtitle(app: &mut App, rest: &[&str]) {
    if !ensure_playing(app) {
        return;
    }
    let Some(index) = rest.first().and_then(|n| n.parse::<usize>().ok()) else {
        println!("usage: sub <number from `subs`>");
        return;
    };
    let Some(subtitle) = index
        .checked_sub(1)
        .and_then(|i| app.selection.subtitles.get(i))
        .cloned()
    else {
        println!("no subtitle number {index}");
        return;
    };
    let title = subtitle.lang.clone().unwrap_or_else(|| subtitle.id.clone());
    let Some(active) = app.player.as_mut() else {
        return;
    };
    match active.add_subtitle(&subtitle.url, Some(&title)) {
        Ok(()) => println!("  loaded and selected: {title}"),
        Err(e) => println!("  {e}"),
    }
}

fn tune(app: &mut App, command: &str, rest: &[&str]) {
    if !ensure_playing(app) {
        return;
    }
    let Some(value) = rest.first().and_then(|v| v.parse::<f64>().ok()) else {
        println!("usage: {command} <number>");
        return;
    };
    let Some(active) = app.player.as_mut() else {
        return;
    };
    let outcome = match command {
        "speed" => active.set_speed(value),
        "adelay" => active.set_audio_delay(value),
        "sdelay" => active.set_subtitle_delay(value),
        _ => active.set_volume(value),
    };
    match outcome {
        Ok(()) => println!("  ok"),
        Err(e) => println!("  {e}"),
    }
}

// ---------------------------------------------------------------- history

fn continue_watching(app: &mut App) {
    let entries: Vec<WatchEntry> = app
        .history
        .continue_watching(20)
        .into_iter()
        .cloned()
        .collect();
    if entries.is_empty() {
        println!("nothing part-watched");
        return;
    }
    // Reuse the item list so `open <n>` works on these too.
    app.selection.items = entries
        .iter()
        .map(|entry| MetaPreview {
            id: entry.series_id.clone().unwrap_or_else(|| entry.id.clone()),
            content_type: entry.content_type.clone(),
            name: Some(entry.name.clone()),
            poster: None,
            poster_shape: None,
            description: None,
            release_info: None,
            imdb_rating: None,
            genres: Vec::new(),
            country: None,
            language: None,
        })
        .collect();
    for (n, entry) in entries.iter().enumerate() {
        println!("{:>3}. {}", n + 1, entry.display_line());
    }
    println!("\n`open <n>` to pick up where you left off");
}

fn forget(app: &mut App, rest: &[&str]) {
    let Some(index) = rest.first().and_then(|n| n.parse::<usize>().ok()) else {
        println!("usage: forget <number from `continue`>");
        return;
    };
    let Some(item) = index
        .checked_sub(1)
        .and_then(|i| app.selection.items.get(i))
        .cloned()
    else {
        println!("no item number {index}");
        return;
    };
    if app.history.forget(&item.content_type, &item.id) {
        app.save_history();
        println!("forgotten: {}", item.display_name());
    } else {
        println!("that item was not in the history");
    }
}

// ---------------------------------------------------------------- preferences

fn prefer(app: &mut App, rest: &[&str]) {
    let Some(what) = rest.first() else {
        println!("ranking preferences:");
        println!(
            "  audio languages  {}",
            if app.settings.ranking.preferred_audio_languages.is_empty() {
                "none".to_owned()
            } else {
                app.settings.ranking.preferred_audio_languages.join(", ")
            }
        );
        println!(
            "  minimum res      {}",
            app.settings
                .ranking
                .minimum_resolution
                .map_or_else(|| "none".to_owned(), |r| format!("{r:?}"))
        );
        println!(
            "  country          {}",
            app.settings.ranking.country.as_deref().unwrap_or("not set")
        );
        println!("  favour larger    {}", app.settings.ranking.prefer_larger);
        println!("\nSession-only for now; they re-rank the next listing.");
        return;
    };
    match *what {
        "lang" => {
            app.settings.ranking.preferred_audio_languages = rest[1..]
                .join(",")
                .split(',')
                .map(|s| s.trim().to_ascii_lowercase())
                .filter(|s| !s.is_empty())
                .collect();
            println!(
                "  audio languages: {}",
                app.settings.ranking.preferred_audio_languages.join(", ")
            );
        }
        "min" => {
            app.settings.ranking.minimum_resolution = match rest.get(1).copied() {
                Some("2160" | "4k") => Some(Resolution::UltraHd),
                Some("1440") => Some(Resolution::QuadHd),
                Some("1080") => Some(Resolution::FullHd),
                Some("720") => Some(Resolution::Hd),
                Some("480" | "none" | "off") => None,
                _ => {
                    println!("usage: prefer min 480|720|1080|1440|2160|none");
                    return;
                }
            };
            println!("  minimum resolution set");
        }
        "country" => {
            app.settings.ranking.country = rest.get(1).map(|c| c.to_ascii_lowercase());
            println!(
                "  country: {}",
                app.settings.ranking.country.as_deref().unwrap_or("not set")
            );
        }
        "size" => {
            app.settings.ranking.prefer_larger =
                matches!(rest.get(1).copied(), Some("on" | "yes" | "true"));
            println!(
                "  favour larger files: {}",
                app.settings.ranking.prefer_larger
            );
        }
        other => println!("unknown preference '{other}'  (`prefer` to see them)"),
    }
    // These are stored settings now, not session state, so they are written
    // through. A preference that forgets itself on restart is a preference
    // nobody sets twice.
    app.save_settings();
}

// ---------------------------------------------------------------- deep links

/// Handle an `eon://` link.
///
/// * `eon://addon/<manifest url>` — install an addon
/// * `eon://open/<type>/<id>` — show an item and its sources
fn deep_link(app: &mut App, link: &str) {
    let rest = link.trim_start_matches("eon://");
    let mut parts = rest.splitn(2, '/');
    match (parts.next(), parts.next()) {
        (Some("addon"), Some(url)) => {
            println!("deep link: installing from {}", host_of(url));
            add(app, &[url]);
        }
        (Some("open"), Some(tail)) => {
            let mut tail = tail.splitn(2, '/');
            match (tail.next(), tail.next()) {
                (Some(content_type), Some(id)) => {
                    println!("deep link: opening {content_type}/{id}");
                    show_item(app, content_type, id, id);
                }
                _ => println!("a link to open needs eon://open/<type>/<id>"),
            }
        }
        _ => {
            println!("unrecognised link: {link}");
            println!("  eon://addon/<manifest url>   install an addon");
            println!("  eon://open/<type>/<id>       open an item");
        }
    }
}

/// Just the host of a URL, so a printed link carries no credentials.
fn host_of(url: &str) -> String {
    url.split("://")
        .nth(1)
        .and_then(|rest| rest.split('/').next())
        .unwrap_or(url)
        .to_owned()
}
