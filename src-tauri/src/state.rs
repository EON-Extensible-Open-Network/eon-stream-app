// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 EON contributors
//
// Additional permission under GNU GPL version 3 section 7:
// see LICENSE-EXCEPTION.md (EON Module ABI Exception 1.0).

//! What the window is looking at.
//!
//! One [`Session`], behind one mutex, owned by the host. The interface holds no
//! state that matters: it holds what is on screen, and asks for the rest by
//! name or by number.
//!
//! ## Why the sources stay on this side
//!
//! A source list is kept here and handed to the interface as labels and
//! indices, never as URLs. Two reasons, and the second is the one that makes it
//! a rule rather than a preference:
//!
//! 1. A stream URL can carry a credential — a debrid token, a signed path —
//!    exactly as an addon's configured address can (madde 22).
//! 2. `src/` is the layer that would carry a third-party module's code if the
//!    sandbox decision (madde 4) ever lands on an isolated webview. Anything
//!    reachable from there is reachable from a module, so the smaller that
//!    surface is now, the less there is to take away later.
//!
//! So `play 3` means the third source of what the host last resolved, and the
//! interface never learns what it points at. The headless build works the same
//! way for the same reason.
//!
//! ## Everything is written beside the executable
//!
//! Seven files and one directory, all in the executable's own folder, which is
//! the claim `v0.12` got wrong and `v0.13` fixed. Nothing is written to an OS
//! configuration directory, nothing is sent anywhere, and there is no account.

use std::{
    fs,
    path::PathBuf,
    sync::{Arc, Mutex},
};

use eon_stream_core::{
    i18n::{self, Messages},
    modules::ModuleStore,
    ranking::RankedStream,
    settings::Settings,
    signature::TrustStore,
    AddonClient, AddonRegistry, BuildProfile, Library, Meta, MetaPreview, WatchEntry, WatchHistory,
};
use eon_stream_engine::{MpvPlayer, TorrentEngine};
use serde::Serialize;

use crate::http::Http;

/// A file this program keeps, in the executable's own directory.
pub fn beside_exe(name: &str) -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join(name)))
        .unwrap_or_else(|| PathBuf::from(name))
}

/// What is being played, so its position can be written down and the next
/// episode found.
#[derive(Clone)]
pub struct NowPlaying {
    pub content_type: String,
    pub id: String,
    pub name: String,
    pub series_id: Option<String>,
    pub season: Option<u32>,
    pub episode: Option<u32>,
    pub binge_group: Option<String>,
    pub addon_id: Option<String>,
}

/// What the host is working on, for the interface to show while it waits.
///
/// Resolving a torrent takes as long as the swarm takes: metadata from
/// strangers, then a prebuffer. The headless build prints a line a second into
/// a terminal. A window has to do the same job without blocking, so the work
/// happens on a thread and reports through this — which the interface polls
/// rather than being pushed, because a poll that misses a tick is a stale
/// number and a push that misses one is a progress bar that never finishes.
///
/// `stage` is a message key, not a sentence: the host does not know which
/// language the window is in and should not have to (madde 40).
#[derive(Clone, Default, Serialize)]
pub struct Job {
    pub active: bool,
    pub stage: String,
    pub detail: String,
    /// How far the current stage is, 0 to 1, when that is known. The prebuffer
    /// is the stage it is known for, and the one a viewer waits on longest.
    pub progress: Option<f64>,
    pub error: Option<String>,
    /// Which start this is. Every start takes a new number and so does every
    /// cancel, so a worker that is still waiting on a swarm after the viewer
    /// gave up can tell that nobody wants its result -- and does not open a
    /// player over the page they went back to.
    #[serde(skip)]
    pub ticket: u64,
    /// The info hash of the torrent this start is resolving, once known, so a
    /// cancel can take it out of the engine and delete its pieces.
    #[serde(skip)]
    pub torrent: Option<String>,
}

impl Job {
    /// A new start. Returns its ticket.
    pub fn begin(&mut self) -> u64 {
        self.ticket = self.ticket.wrapping_add(1);
        self.torrent = None;
        self.at("ui.job.starting", String::new());
        self.ticket
    }

    /// Whether `ticket` is still the start someone is waiting for.
    #[must_use]
    pub fn wanted(&self, ticket: u64) -> bool {
        self.ticket == ticket
    }

    /// Give up on the current start. Answers the torrent it was resolving.
    pub fn cancel(&mut self) -> Option<String> {
        self.ticket = self.ticket.wrapping_add(1);
        self.done();
        self.torrent.take()
    }

    /// Say what is happening now.
    pub fn at(&mut self, stage: &str, detail: String) {
        self.active = true;
        self.stage = stage.to_owned();
        self.detail = detail;
        self.progress = None;
        self.error = None;
    }

    pub fn at_fraction(&mut self, stage: &str, detail: String, fraction: f64) {
        self.at(stage, detail);
        self.progress = Some(fraction.clamp(0.0, 1.0));
    }

    /// Finished, with nothing to report.
    pub fn done(&mut self) {
        self.active = false;
        self.stage = String::new();
        self.detail = String::new();
        self.progress = None;
        self.error = None;
    }

    /// Finished badly. Stays visible until the next job starts, because a
    /// failure that clears itself is a failure nobody read.
    pub fn failed(&mut self, message: String) {
        self.active = false;
        self.stage = String::new();
        self.detail = String::new();
        self.progress = None;
        self.error = Some(message);
    }
}

/// A shared handle to the job, for a worker thread that must not hold the
/// session lock while it waits on a swarm.
pub type JobHandle = Arc<Mutex<Job>>;

/// Everything the host owns.
pub struct Session {
    pub client: AddonClient<Http>,
    pub registry: AddonRegistry,
    pub history: WatchHistory,
    pub library: Library,
    pub settings: Settings,
    pub messages: Option<Messages>,
    /// Installed modules. The window does not manage them yet: with an empty
    /// trust set nothing installs anyway (madde 30), so module management --
    /// and with it the revocation list and the per-addon health backoff --
    /// stays in the headless build until there is a key to exercise it. The
    /// about page says how many are installed, which on every published alpha
    /// is none.
    pub modules: ModuleStore,
    pub trust: TrustStore,

    /// The last resolved source list, which `play <n>` indexes into.
    pub sources: Vec<RankedStream>,
    /// How many source lists have been asked for. Only the newest may
    /// replace `sources`: see `view::sources`.
    pub sources_wanted: u64,
    /// What those sources are for: type, id, display name.
    pub subject: Option<(String, String, String)>,
    /// The last opened item, for episodes and for the next-episode step.
    pub meta: Option<Meta>,
    /// The last listing the interface showed, so saving to the library has a
    /// poster and a name without another request.
    pub previews: Vec<MetaPreview>,

    pub player: Option<MpvPlayer>,
    pub playing: Option<NowPlaying>,
    /// The last position mpv reported while it was still answering.
    ///
    /// mpv exits by itself at the end of a file, and a process that has gone
    /// cannot be asked where it got to -- so asking only at the end means the
    /// one case everybody has (watching something to the end, or nearly) is
    /// the one case that records nothing, and nothing ever reaches continue
    /// watching. The position is therefore kept as it goes past.
    pub last_seen: Option<(f64, Option<f64>)>,
    /// Started the first time a torrent is played, not at launch: a DHT and a
    /// listening socket are not things to open on behalf of someone who only
    /// plays HTTP streams.
    pub torrent: Option<Arc<TorrentEngine>>,
    pub job: JobHandle,
    /// The info hash of the torrent the player is reading, so giving up on it
    /// can delete what it fetched.
    pub playing_torrent: Option<String>,
    /// Subtitles found for what is playing, loaded or not.
    pub subtitles: crate::subs::Found,
    /// The last calendar: when it was made, which series it covered, and
    /// what it said. Asking every series' addon on every visit to the home
    /// screen would be dozens of requests for an answer that changes daily.
    pub calendar: Option<(u64, String, crate::view::CalendarView)>,
    /// The cache limit's bookkeeping for what is playing.
    pub keeper: crate::keeper::Keeper,
    /// Measured torrent speeds, for sorting sources by speed.
    pub speeds: crate::speeds::Speeds,
    /// The source of the current list being started or played, and every
    /// source of this list already tried: "try a faster source" picks among
    /// the rest.
    pub starting_source: Option<usize>,
    pub tried: Vec<usize>,
    /// The viewer's own scores and reviews (madde 7: no account needed).
    pub ratings: crate::ratings::Ratings,
}

impl Session {
    /// Load everything this machine already knows.
    pub fn load() -> Self {
        let settings = read_json(&beside_exe("eon-settings.json"), Settings::parse)
            .unwrap_or_else(Settings::default);
        let messages = Messages::for_locale(&settings.language)
            .or_else(|_| Messages::for_locale(i18n::FALLBACK_LOCALE))
            .ok();
        let modules = read_json(&beside_exe("eon-modules.json"), |json| {
            ModuleStore::from_json(json, BuildProfile::Stream)
        })
        .unwrap_or_else(|| ModuleStore::new(BuildProfile::Stream));

        Self {
            client: AddonClient::new(Http::new()),
            registry: read_json(&beside_exe("eon-addons.json"), AddonRegistry::from_json)
                .unwrap_or_default(),
            history: read_json(&beside_exe("eon-history.json"), WatchHistory::from_json)
                .unwrap_or_default(),
            library: read_json(&beside_exe("eon-library.json"), Library::from_json)
                .unwrap_or_default(),
            // Read, never written: the trust set is a statement about this
            // machine, and this program is not the one that gets to change it
            // (madde 30).
            trust: fs::read(beside_exe("eon-trust.json"))
                .ok()
                .and_then(|bytes| TrustStore::parse(&bytes).ok())
                .unwrap_or_else(TrustStore::empty),
            modules,
            settings,
            messages,
            sources: Vec::new(),
            sources_wanted: 0,
            subject: None,
            meta: None,
            previews: Vec::new(),
            player: None,
            playing: None,
            last_seen: None,
            torrent: None,
            job: Arc::new(Mutex::new(Job::default())),
            playing_torrent: None,
            subtitles: crate::subs::Found::default(),
            calendar: None,
            keeper: crate::keeper::Keeper::default(),
            speeds: crate::speeds::Speeds::load(),
            starting_source: None,
            tried: Vec::new(),
            ratings: crate::ratings::Ratings::load(),
        }
    }

    /// One message, in the interface's language.
    pub fn text(&self, key: &str) -> String {
        self.messages
            .as_ref()
            .map_or_else(|| key.to_owned(), |m| m.get(key).to_owned())
    }

    /// Reload the catalogues after the language changes.
    pub fn reload_messages(&mut self) {
        self.messages = Messages::for_locale(&self.settings.language)
            .or_else(|_| Messages::for_locale(i18n::FALLBACK_LOCALE))
            .ok();
    }

    /// The torrent engine, started if it is not already.
    ///
    /// # Errors
    ///
    /// Whatever the engine reports, as text: a download directory that cannot
    /// be created, or a port that cannot be bound.
    pub fn torrent_engine(&mut self) -> Result<Arc<TorrentEngine>, String> {
        if let Some(engine) = &self.torrent {
            return Ok(Arc::clone(engine));
        }
        let options = crate::torrents::options_from(&self.settings.torrent)
            .map_err(|e: eon_stream_engine::Error| e.to_string())?;
        let engine = Arc::new(TorrentEngine::start(options).map_err(|e| e.to_string())?);
        self.torrent = Some(Arc::clone(&engine));
        Ok(engine)
    }

    /// Write down where playback got to.
    ///
    /// Asks the player first and falls back to the last position it reported,
    /// so this works both while mpv is running and after it has closed itself.
    pub fn remember_position(&mut self) {
        let Some(playing) = self.playing.clone() else {
            return;
        };
        let live = self.player.as_mut().and_then(|player| {
            player
                .position()
                .ok()
                .flatten()
                .map(|position| (position, player.duration().ok().flatten()))
        });
        let Some((position, duration)) = live.or(self.last_seen) else {
            return;
        };
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
            updated_at: eon_stream_core::history::now_unix(),
        });
        self.save_history();
    }

    pub fn save_addons(&self) {
        write_json(&beside_exe("eon-addons.json"), self.registry.to_json());
    }

    pub fn save_history(&self) {
        write_json(&beside_exe("eon-history.json"), self.history.to_json());
    }

    pub fn save_library(&self) {
        write_json(&beside_exe("eon-library.json"), self.library.to_json());
    }

    pub fn save_settings(&self) {
        write_json(&beside_exe("eon-settings.json"), self.settings.to_json());
    }
}

/// Read a document this program wrote earlier.
///
/// A file that will not parse is treated as absent rather than fatal: the
/// alternative is a window that will not open because a settings file got
/// truncated by a power cut. The default is always a usable state.
fn read_json<T, F>(path: &PathBuf, parse: F) -> Option<T>
where
    F: Fn(&str) -> eon_stream_core::Result<T>,
{
    fs::read_to_string(path)
        .ok()
        .and_then(|json| parse(&json).ok())
}

/// Write one, reporting a failure to the log rather than to the window.
///
/// There is nowhere useful to show "could not write the history file" while
/// someone is watching something, and failing the command they asked for
/// would be worse than the thing that failed.
fn write_json(path: &PathBuf, json: eon_stream_core::Result<String>) {
    match json {
        Ok(json) => {
            if let Err(e) = fs::write(path, json) {
                eprintln!("could not write {}: {e}", path.display());
            }
        }
        Err(e) => eprintln!("could not serialise {}: {e}", path.display()),
    }
}
