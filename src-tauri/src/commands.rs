// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 EON contributors
//
// Additional permission under GNU GPL version 3 section 7:
// see LICENSE-EXCEPTION.md (EON Module ABI Exception 1.0).

//! The IPC surface. All of it, in one file, on purpose.
//!
//! `docs/architecture.md` asks for this: every command named, typed and
//! enumerated in one place, because
//!
//! 1. it is the security boundary between the interface and the host;
//! 2. modules reach the same surface through the Module ABI, and two ways in
//!    would mean two permission models;
//! 3. an enumerated surface can be diffed in review and an ad-hoc one cannot.
//!
//! The list the host registers is at the bottom of this file, and
//! [`crate::main`] registers nothing that is not in it.
//!
//! ## Nothing here blocks the window
//!
//! Every command fetches over the network or talks to mpv, so every command is
//! `async` and does its work on a blocking thread with the session lock held
//! only for as long as the work takes. The one exception is playing a torrent,
//! which waits on strangers: it returns as soon as the work is *started* and
//! reports through [`crate::state::Job`], which the interface polls. A command
//! that held the lock for three minutes would freeze every other command,
//! including the one drawing the progress bar.

use std::sync::{Arc, Mutex};

use eon_stream_core::{
    i18n::{self, Messages},
    ranking::Resolution,
    settings::{Settings, UpdateChannel},
};
use eon_stream_engine::{MpvPlayer, PlaybackSource, PlayerOptions, SeekMode};
use serde::Serialize;
use tauri::State;

use crate::{
    state::{NowPlaying, Session},
    torrents,
    view::{
        self, AboutView, AddonView, DetailView, RowView, SearchView, SettingsView, SourcesView,
        TrackView,
    },
};

/// The session, as the host holds it.
pub type Shared = Arc<Mutex<Session>>;

/// Every command answers either a view or a sentence that says what went
/// wrong. The interface never sees an error type it has to interpret.
type Out<T> = Result<T, String>;

/// Run something with the session, off the thread that draws the window.
async fn with_session<T, F>(state: &State<'_, Shared>, work: F) -> Out<T>
where
    F: FnOnce(&mut Session) -> Out<T> + Send + 'static,
    T: Send + 'static,
{
    let shared = Arc::clone(state.inner());
    tauri::async_runtime::spawn_blocking(move || {
        let mut session = shared
            .lock()
            .map_err(|_| "the host lost track of its own state".to_owned())?;
        work(&mut session)
    })
    .await
    .map_err(|e| e.to_string())?
}

// ----------------------------------------------------------------- the words

/// Every user-visible string the interface draws, in the chosen language.
#[derive(Serialize)]
pub struct StringsView {
    pub locale: String,
    pub messages: std::collections::BTreeMap<String, String>,
    pub locales: Vec<String>,
}

/// The `ui.*` half of the message catalogue.
///
/// The interface holds no English text of its own — not a label, not a button,
/// not an error (madde 40). It asks for this once at startup and again when the
/// language changes. The key list comes from the English catalogue because that
/// is the fallback: a key missing from Turkish yields the English sentence
/// rather than a blank, which is the behaviour the audit in `eon-stream-core`
/// exists to keep honest.
fn strings_for(locale: &str) -> StringsView {
    let messages = Messages::for_locale(locale)
        .or_else(|_| Messages::for_locale(i18n::FALLBACK_LOCALE))
        .ok();
    let mut out = std::collections::BTreeMap::new();
    if let Ok(english) = i18n::shipped(i18n::FALLBACK_LOCALE) {
        for key in english.keys() {
            if let Some(key) = key.strip_prefix("ui.").map(|_| key) {
                let text = messages
                    .as_ref()
                    .map_or_else(|| key.to_owned(), |m| m.get(key).to_owned());
                out.insert(key.to_owned(), text);
            }
        }
    }
    StringsView {
        locale: messages
            .as_ref()
            .map_or("none", Messages::locale)
            .to_owned(),
        messages: out,
        locales: i18n::SHIPPED_LOCALES
            .iter()
            .map(|l| (*l).to_owned())
            .collect(),
    }
}

#[tauri::command]
pub async fn strings(state: State<'_, Shared>) -> Out<StringsView> {
    with_session(&state, |session| {
        Ok(strings_for(&session.settings.language))
    })
    .await
}

#[tauri::command]
pub async fn language_set(state: State<'_, Shared>, code: String) -> Out<StringsView> {
    with_session(&state, move |session| {
        if !i18n::SHIPPED_LOCALES.contains(&code.as_str()) {
            return Err(session.text("ui.error.unknownlanguage"));
        }
        session.settings.language = code;
        session.reload_messages();
        session.save_settings();
        Ok(strings_for(&session.settings.language))
    })
    .await
}

// --------------------------------------------------------------- what is on

#[tauri::command]
pub async fn status(state: State<'_, Shared>) -> Out<view::StatusView> {
    let status = with_session(&state, |session| Ok(view::status(session))).await?;
    // mpv closes itself at the end of a file, or when it fails. Whichever
    // webview notices first takes the controls down, so the catalogue is
    // uncovered rather than left under a player with nothing in it.
    if !status.player.running && !status.job.active {
        crate::surface::close_controls();
    }
    Ok(status)
}

#[tauri::command]
pub async fn about(state: State<'_, Shared>) -> Out<AboutView> {
    with_session(&state, |session| Ok(view::about(session))).await
}

// ------------------------------------------------------------------- addons

#[tauri::command]
pub async fn addons(state: State<'_, Shared>) -> Out<Vec<AddonView>> {
    with_session(&state, |session| Ok(view::addons(session))).await
}

/// Install an addon from the address someone pasted.
///
/// The address is the one thing that travels *into* the host from the
/// interface, and it stops here: it is stored, used, and never handed back
/// (madde 22).
#[tauri::command]
pub async fn addon_add(state: State<'_, Shared>, address: String) -> Out<Vec<AddonView>> {
    with_session(&state, move |session| {
        let client = &session.client;
        let installed = client
            .install(&mut session.registry, address.trim())
            .map_err(|e| e.to_string())?;
        let _ = installed;
        session.save_addons();
        Ok(view::addons(session))
    })
    .await
}

#[tauri::command]
pub async fn addon_remove(state: State<'_, Shared>, id: String) -> Out<Vec<AddonView>> {
    with_session(&state, move |session| {
        session.registry.remove(&id).map_err(|e| e.to_string())?;
        session.save_addons();
        Ok(view::addons(session))
    })
    .await
}

#[tauri::command]
pub async fn addon_enabled(
    state: State<'_, Shared>,
    id: String,
    enabled: bool,
) -> Out<Vec<AddonView>> {
    with_session(&state, move |session| {
        session
            .registry
            .set_enabled(&id, enabled)
            .map_err(|e| e.to_string())?;
        session.save_addons();
        Ok(view::addons(session))
    })
    .await
}

/// Move an addon up or down the list, which is also its priority: the first
/// addon that answers a lookup wins a tie.
#[tauri::command]
pub async fn addon_move(state: State<'_, Shared>, id: String, by: i32) -> Out<Vec<AddonView>> {
    with_session(&state, move |session| {
        let at = view::position_of(&session.registry, &id)
            .ok_or_else(|| session.text("ui.error.noaddon"))?;
        let to = usize::try_from(i64::from(by) + at as i64).unwrap_or(0);
        session
            .registry
            .reorder(&id, to.min(session.registry.len().saturating_sub(1)))
            .map_err(|e| e.to_string())?;
        session.save_addons();
        Ok(view::addons(session))
    })
    .await
}

/// Ask an addon for its manifest again, which is how a renamed catalogue or a
/// new resource is picked up.
#[tauri::command]
pub async fn addon_refresh(state: State<'_, Shared>, id: String) -> Out<Vec<AddonView>> {
    with_session(&state, move |session| {
        let client = &session.client;
        client
            .refresh(&mut session.registry, &id)
            .map_err(|e| e.to_string())?;
        session.save_addons();
        Ok(view::addons(session))
    })
    .await
}

// ------------------------------------------------------------------ browsing

#[tauri::command]
pub async fn rows(state: State<'_, Shared>) -> Out<Vec<RowView>> {
    with_session(&state, |session| Ok(view::rows(session))).await
}

#[tauri::command]
pub async fn row_items(
    state: State<'_, Shared>,
    addon_id: String,
    content_type: String,
    catalog_id: String,
    skip: usize,
    query: Option<String>,
) -> Out<Vec<view::ItemView>> {
    with_session(&state, move |session| {
        view::row_items(
            session,
            &addon_id,
            &content_type,
            &catalog_id,
            skip,
            query.as_deref(),
        )
    })
    .await
}

#[tauri::command]
pub async fn search(state: State<'_, Shared>, query: String) -> Out<SearchView> {
    with_session(&state, move |session| view::search(session, &query)).await
}

#[tauri::command]
pub async fn detail(state: State<'_, Shared>, content_type: String, id: String) -> Out<DetailView> {
    with_session(&state, move |session| {
        view::detail(session, &content_type, &id)
    })
    .await
}

/// The billboard on the home screen: one item's artwork and blurb, looked up
/// without becoming the item the viewer is looking at.
#[tauri::command]
pub async fn feature(
    state: State<'_, Shared>,
    content_type: String,
    id: String,
) -> Out<DetailView> {
    with_session(&state, move |session| {
        view::feature(session, &content_type, &id)
    })
    .await
}

#[tauri::command]
pub async fn sources(
    state: State<'_, Shared>,
    content_type: String,
    id: String,
    name: String,
) -> Out<SourcesView> {
    with_session(&state, move |session| {
        view::sources(session, &content_type, &id, &name)
    })
    .await
}

// ------------------------------------------------------------------- keeping

#[tauri::command]
pub async fn library_save(state: State<'_, Shared>, content_type: String, id: String) -> Out<bool> {
    with_session(&state, move |session| {
        view::library_save(session, &content_type, &id)
    })
    .await
}

#[tauri::command]
pub async fn library_remove(
    state: State<'_, Shared>,
    content_type: String,
    id: String,
) -> Out<bool> {
    with_session(&state, move |session| {
        let removed = session.library.remove(&content_type, &id);
        session.save_library();
        Ok(removed)
    })
    .await
}

#[tauri::command]
pub async fn history_forget(
    state: State<'_, Shared>,
    content_type: String,
    id: String,
) -> Out<bool> {
    with_session(&state, move |session| {
        let forgotten = session.history.forget(&content_type, &id);
        session.save_history();
        Ok(forgotten)
    })
    .await
}

// ------------------------------------------------------------------ playback

/// Play the `index`th source of the list the host last resolved.
///
/// Returns as soon as the work has started. A direct URL is playing by then; a
/// torrent needs its metadata and a prebuffer first, and the interface follows
/// that through [`status`].
#[tauri::command]
pub async fn play(state: State<'_, Shared>, index: usize, file: Option<usize>) -> Out<()> {
    let shared = Arc::clone(state.inner());
    tauri::async_runtime::spawn_blocking(move || -> Out<()> {
        // Everything that needs the lock happens first and the lock is then
        // dropped, because what follows can take minutes.
        let (ranked, playing, engine) = {
            let mut session = shared
                .lock()
                .map_err(|_| "the host lost track of its own state".to_owned())?;
            let ranked = session
                .sources
                .get(index)
                .cloned()
                .ok_or_else(|| session.text("ui.error.nosource"))?;
            let playing = now_playing(&session, &ranked);
            let engine = if torrents::source_of(&ranked.stream).is_some() {
                Some(session.torrent_engine()?)
            } else {
                None
            };
            if let Ok(mut job) = session.job.lock() {
                job.at("ui.job.starting", String::new());
            }
            (ranked, playing, engine)
        };

        let subtitles: Vec<String> = ranked
            .stream
            .subtitles
            .iter()
            .map(|s| s.url.clone())
            .collect();

        let (source, headers) = match (torrents::source_of(&ranked.stream), engine) {
            (Some(torrent), Some(engine)) => {
                let job = {
                    let session = shared
                        .lock()
                        .map_err(|_| "the host lost track of its own state".to_owned())?;
                    Arc::clone(&session.job)
                };
                match torrents::resolve(&engine, &torrent, file, &job) {
                    // Subtitles that came with the stream still apply; the
                    // headers do not, because the player is reading from
                    // loopback rather than from the addon's host.
                    Ok((source, _index)) => (source, Vec::new()),
                    Err(message) => return finish_with_error(&shared, message),
                }
            }
            _ => match to_playback_source(&ranked.stream) {
                Ok(source) => (
                    source,
                    ranked
                        .stream
                        .request_headers()
                        .into_iter()
                        .map(|(k, v)| (k.to_owned(), v.to_owned()))
                        .collect(),
                ),
                Err(message) => {
                    let message = {
                        let session = shared
                            .lock()
                            .map_err(|_| "the host lost track of its own state".to_owned())?;
                        session.text(&message)
                    };
                    return finish_with_error(&shared, message);
                }
            },
        };

        launch(&shared, source, headers, subtitles, playing)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Play a magnet link, an info hash, a URL or a file path, with no addon
/// involved.
///
/// This is the path a torrent someone sent you takes, and the one the engine
/// was tested on.
#[tauri::command]
pub async fn play_link(state: State<'_, Shared>, link: String) -> Out<()> {
    let shared = Arc::clone(state.inner());
    tauri::async_runtime::spawn_blocking(move || play_link_now(&shared, &link))
        .await
        .map_err(|e| e.to_string())?
}

/// Play a link, on the thread that calls this.
///
/// Shared with the startup path: `eon-stream <link>` is the same work, and
/// having one function means the way a release gets verified is the way a
/// viewer's paste is handled, rather than something that only resembles it.
///
/// # Errors
///
/// As text: nothing to play, a torrent that will not resolve, or mpv refusing
/// to start.
pub fn play_link_now(shared: &Shared, link: &str) -> Out<()> {
    let link = link.trim().to_owned();
    if link.is_empty() {
        return Err("nothing to play".to_owned());
    }

    if let Some(request) = torrents::request_from_argument(&link) {
        let (engine, job) = {
            let mut session = shared
                .lock()
                .map_err(|_| "the host lost track of its own state".to_owned())?;
            let engine = session.torrent_engine()?;
            (engine, Arc::clone(&session.job))
        };
        let source = torrents::TorrentSource {
            request,
            file_index: None,
        };
        return match torrents::resolve(&engine, &source, None, &job) {
            Ok((source, _index)) => launch(shared, source, Vec::new(), Vec::new(), None),
            Err(message) => finish_with_error(shared, message),
        };
    }

    let path = std::path::Path::new(&link);
    let source = if path.is_file() {
        PlaybackSource::File(path.to_path_buf())
    } else {
        PlaybackSource::Url(link)
    };
    launch(shared, source, Vec::new(), Vec::new(), None)
}

/// Pause, seek, stop, and the rest of what a player does.
///
/// One command rather than eight, because the set is small, fixed, and every
/// member of it is "tell mpv one thing". `action` is checked against the list
/// here; an unknown one is an error rather than a silent no-op.
#[tauri::command]
pub async fn player(state: State<'_, Shared>, action: String, value: Option<f64>) -> Out<()> {
    if action == "fullscreen" {
        // The window's business, not mpv's: the picture is a child of the
        // window and follows its size.
        return crate::surface::toggle_fullscreen().map(|_| ());
    }
    let stopping = action == "stop";
    let result = with_session(&state, move |session| {
        if action == "stop" {
            session.remember_position();
            if let Some(player) = session.player.take() {
                let _ = player.quit();
            }
            session.playing = None;
            return Ok(());
        }

        let Some(player) = session.player.as_mut() else {
            return Err(session.text("ui.error.noplayer"));
        };
        let amount = value.unwrap_or_default();
        let result = match action.as_str() {
            "pause" => player.set_paused(true),
            "resume" => player.set_paused(false),
            "toggle" => {
                let paused = player
                    .get_property("pause")
                    .map(|value| value == "true")
                    .unwrap_or_default();
                player.set_paused(!paused)
            }
            "seek" => player.seek(amount, SeekMode::Relative),
            "seek-to" => player.seek(amount, SeekMode::Absolute),
            "volume" => player.set_volume(amount),
            "speed" => player.set_speed(amount),
            "subtitles-on" => player.set_subtitles_visible(true),
            "subtitles-off" => player.set_subtitles_visible(false),
            "mute" => {
                let muted = player
                    .get_property("mute")
                    .map(|value| value == "true")
                    .unwrap_or_default();
                player.set_property("mute", if muted { "no" } else { "yes" })
            }
            // A negative id is "none", which for subtitles means off.
            "audio" | "sub" => {
                let id = if amount < 0.0 {
                    None
                } else {
                    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                    Some(amount as u32)
                };
                player.select_track(&action, id)
            }
            other => return Err(format!("{other} is not a player action")),
        };
        result.map_err(|e| e.to_string())
    })
    .await;
    if stopping {
        // After the lock, because it waits on the window's own thread.
        crate::surface::close_controls();
    }
    result
}

/// The audio and subtitle tracks of what is playing, for the player's menu.
#[tauri::command]
pub async fn tracks(state: State<'_, Shared>) -> Out<Vec<TrackView>> {
    with_session(&state, view::tracks).await
}

/// Delete the torrent pieces on disk. Answers how many bytes that freed.
#[tauri::command]
pub async fn buffer_clear(state: State<'_, Shared>) -> Out<u64> {
    with_session(&state, crate::buffer::clear).await
}

// ------------------------------------------------------------------ settings

#[tauri::command]
pub async fn settings(state: State<'_, Shared>) -> Out<SettingsView> {
    with_session(&state, |session| Ok(view::settings(session))).await
}

/// Change one setting, by the same path the headless build uses.
///
/// The candidate is built, validated and only then adopted. Validating after
/// assigning would leave an out-of-range value in memory on the failure path,
/// which is how a rejected setting still takes effect.
#[tauri::command]
pub async fn settings_set(
    state: State<'_, Shared>,
    path: String,
    value: String,
) -> Out<SettingsView> {
    with_session(&state, move |session| {
        let mut candidate: Settings = session.settings.clone();
        let value = value.trim();
        let applied = match path.as_str() {
            "playback.volume" => parse(value, |v| candidate.playback.volume = v),
            "playback.subtitleLanguages" => {
                candidate.playback.subtitle_languages = list(value);
                true
            }
            "playback.startMuted" => boolean(value, |v| candidate.playback.start_muted = v),
            "ranking.audioLanguages" => {
                candidate.ranking.preferred_audio_languages = list(value);
                true
            }
            "ranking.minimumResolution" => match resolution(value) {
                Some(resolution) => {
                    candidate.ranking.minimum_resolution = resolution;
                    true
                }
                None => false,
            },
            "ranking.preferHigherResolution" => {
                boolean(value, |v| candidate.ranking.prefer_higher_resolution = v)
            }
            "ranking.preferHighDynamicRange" => {
                boolean(value, |v| candidate.ranking.prefer_high_dynamic_range = v)
            }
            "ranking.preferSourcesWithSubtitles" => boolean(value, |v| {
                candidate.ranking.prefer_sources_with_subtitles = v;
            }),
            "ranking.preferLarger" => boolean(value, |v| candidate.ranking.prefer_larger = v),
            "torrent.downloadDirectory" => {
                candidate.torrent.download_directory = if value.is_empty() {
                    None
                } else {
                    Some(value.to_owned())
                };
                true
            }
            "torrent.peerLimit" => parse(value, |v| candidate.torrent.peer_limit = v),
            "torrent.downloadLimitKbps" => {
                optional(value, |v| candidate.torrent.download_limit_kbps = v)
            }
            "torrent.uploadLimitKbps" => {
                optional(value, |v| candidate.torrent.upload_limit_kbps = v)
            }
            "torrent.prebufferMegabytes" => {
                parse(value, |v| candidate.torrent.prebuffer_megabytes = v)
            }
            "torrent.keepFiles" => boolean(value, |v| candidate.torrent.keep_files = v),
            "torrent.seedAfterPlayback" => {
                boolean(value, |v| candidate.torrent.seed_after_playback = v)
            }
            "updates.checkOnStart" => boolean(value, |v| candidate.updates.check_on_start = v),
            "updates.refreshRevocations" => {
                boolean(value, |v| candidate.updates.refresh_revocations = v)
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
                _ => false,
            },
            // Deliberately not here: `language`, which has its own command
            // because it also reloads the catalogues.
            _ => return Err(format!("{path} is not a setting")),
        };
        if !applied {
            return Err(session.text("ui.error.badvalue"));
        }
        candidate.validate().map_err(|e| e.to_string())?;
        session.settings = candidate;
        session.save_settings();
        Ok(view::settings(session))
    })
    .await
}

fn parse<T: std::str::FromStr>(value: &str, mut set: impl FnMut(T)) -> bool {
    match value.parse::<T>() {
        Ok(parsed) => {
            set(parsed);
            true
        }
        Err(_) => false,
    }
}

/// An empty box means "no limit", which is not the same as zero.
fn optional<T: std::str::FromStr>(value: &str, mut set: impl FnMut(Option<T>)) -> bool {
    if value.is_empty() {
        set(None);
        return true;
    }
    match value.parse::<T>() {
        Ok(parsed) => {
            set(Some(parsed));
            true
        }
        Err(_) => false,
    }
}

fn boolean(value: &str, mut set: impl FnMut(bool)) -> bool {
    match value {
        "true" | "yes" | "on" | "1" => {
            set(true);
            true
        }
        "false" | "no" | "off" | "0" => {
            set(false);
            true
        }
        _ => false,
    }
}

fn resolution(value: &str) -> Option<Option<Resolution>> {
    match value {
        "none" | "" => Some(None),
        "720" => Some(Some(Resolution::Hd)),
        "1080" => Some(Some(Resolution::FullHd)),
        "1440" => Some(Some(Resolution::QuadHd)),
        "2160" => Some(Some(Resolution::UltraHd)),
        _ => None,
    }
}

fn list(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(|part| part.trim().to_ascii_lowercase())
        .filter(|part| !part.is_empty())
        .collect()
}

// ------------------------------------------------------------------ playback
// helpers, shared by `play` and `play_link`.

/// Start the player on a source that is already resolved.
///
/// One place, so the torrent path -- which arrives here several seconds later
/// and through a different route -- gets exactly the same launch: the old
/// position written down, the resume point applied, the job closed.
fn launch(
    shared: &Shared,
    source: PlaybackSource,
    headers: Vec<(String, String)>,
    subtitles: Vec<String>,
    playing: Option<NowPlaying>,
) -> Out<()> {
    // Before the lock: on a `--play` start this can wait for the window to be
    // built, and nothing else should wait on that.
    let mut extra_args = crate::surface::mpv_arguments();

    let mut session = shared
        .lock()
        .map_err(|_| "the host lost track of its own state".to_owned())?;

    session.remember_position();
    if let Some(existing) = session.player.take() {
        let _ = existing.quit();
    }

    // The volume the viewer set, rather than mpv's own default of 100.
    extra_args.push(format!("--volume={}", session.settings.playback.volume));

    let resume = playing
        .as_ref()
        .and_then(|p| session.history.resume_position(&p.content_type, &p.id));
    // The name, not the URL: `v0.13` shipped because a window title read
    // `Big%20Buck%20Bunny.mp4`.
    let title = playing
        .as_ref()
        .map_or_else(|| source.display_hint(), |p| p.name.clone());

    let options = PlayerOptions {
        title: Some(format!("EON Stream — {title}")),
        http_headers: headers,
        subtitle_urls: subtitles,
        start_at_secs: resume,
        extra_args,
        ..PlayerOptions::default()
    };

    match MpvPlayer::launch(&source, &options) {
        Ok(started) => {
            session.player = Some(started);
            session.playing = playing;
            if let Ok(mut job) = session.job.lock() {
                job.done();
            }
            // The controls go on after mpv's surface exists, so they land on
            // top of it -- see `surface`. Not under the lock: laying them on
            // waits for the window's own thread.
            drop(session);
            crate::surface::open_controls();
            Ok(())
        }
        Err(e) => {
            let message = e.to_string();
            if let Ok(mut job) = session.job.lock() {
                job.failed(message.clone());
            }
            Err(message)
        }
    }
}

/// Record a failure where the interface will see it, and report it as well.
///
/// Both, because the two are read at different moments: the job survives the
/// command returning, and the returned error is what the button that was just
/// pressed shows.
fn finish_with_error(shared: &Shared, message: String) -> Out<()> {
    if let Ok(session) = shared.lock() {
        if let Ok(mut job) = session.job.lock() {
            job.failed(message.clone());
        }
    }
    Err(message)
}

/// What to remember about the thing being played.
fn now_playing(
    session: &Session,
    ranked: &eon_stream_core::ranking::RankedStream,
) -> Option<NowPlaying> {
    let (content_type, id, name) = session.subject.clone()?;
    let episode = session
        .meta
        .as_ref()
        .and_then(|meta| meta.videos.iter().find(|video| video.id == id));
    let series_id = session
        .meta
        .as_ref()
        .map(|meta| meta.id.clone())
        .filter(|series| *series != id);

    Some(NowPlaying {
        content_type,
        id,
        name,
        series_id,
        season: episode.and_then(|video| video.season),
        episode: episode.and_then(|video| video.episode),
        binge_group: ranked.stream.binge_group().map(ToOwned::to_owned),
        addon_id: Some(ranked.addon_id.clone()),
    })
}

/// Classify a source. A torrent never reaches here: `play` intercepts one
/// before this is called, because resolving it needs the engine and blocks for
/// as long as the swarm takes. Returns a message *key*, so the sentence is
/// chosen in the interface's language.
fn to_playback_source(stream: &eon_stream_core::Stream) -> Result<PlaybackSource, String> {
    use eon_stream_core::types::StreamSource;
    match stream.source() {
        Some(StreamSource::Direct(url)) => Ok(PlaybackSource::Url(url.to_owned())),
        // mpv plays this through its ytdl hook, which needs yt-dlp present.
        Some(StreamSource::YouTube(id)) => Ok(PlaybackSource::Url(format!(
            "https://www.youtube.com/watch?v={id}"
        ))),
        Some(StreamSource::Torrent { .. }) => Err("ui.error.torrentelsewhere".to_owned()),
        Some(StreamSource::External(_)) => Err("ui.error.externalsource".to_owned()),
        None => Err("ui.error.nothingplayable".to_owned()),
    }
}
