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
//! `async` and does its work on a blocking thread. A command that waits on an
//! addon does not hold the session lock while it waits: it copies out what the
//! request needs, asks with nothing locked, and locks again to fold the answer
//! in (`with_network`). Before v0.17 the lock was held for the whole request,
//! and one slow addon answering one row held up the whole window. The one
//! exception to answering only when done is playing a torrent,
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
    subs, torrents,
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

/// Run something that waits on addons, without the session lock held while
/// it waits.
///
/// `prepare` copies out what the request needs and `finish` folds the answer
/// back in, each with the session locked for as long as that takes; `fetch`
/// is the request, with nothing locked. See [`view::Net`] for why.
async fn with_network<N, R, T, P, F, B>(
    state: &State<'_, Shared>,
    prepare: P,
    fetch: F,
    finish: B,
) -> Out<T>
where
    P: FnOnce(&mut Session) -> Out<N> + Send + 'static,
    F: FnOnce(&N) -> R + Send + 'static,
    B: FnOnce(&mut Session, N, R) -> Out<T> + Send + 'static,
    N: Send + 'static,
    R: Send + 'static,
    T: Send + 'static,
{
    let shared = Arc::clone(state.inner());
    tauri::async_runtime::spawn_blocking(move || {
        let lost = || "the host lost track of its own state".to_owned();
        let request = {
            let mut session = shared.lock().map_err(|_| lost())?;
            prepare(&mut session)?
        };
        let answer = fetch(&request);
        let mut session = shared.lock().map_err(|_| lost())?;
        finish(&mut session, request, answer)
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
    extra: Option<(String, String)>,
) -> Out<Vec<view::ItemView>> {
    with_network(
        &state,
        move |session| {
            view::page_request(
                session,
                &addon_id,
                &content_type,
                &catalog_id,
                skip,
                query.as_deref(),
                extra,
            )
        },
        view::fetch_page,
        |session, _, previews| previews.map(|previews| view::page_items(session, previews)),
    )
    .await
}

#[tauri::command]
pub async fn search(state: State<'_, Shared>, query: String) -> Out<SearchView> {
    with_network(
        &state,
        move |session| view::search_request(session, &query),
        view::search_fetch,
        |session, _, found| Ok(view::search_view(session, found)),
    )
    .await
}

/// One page of a search, forty-two titles, read from the addons only as far
/// as that page needs.
#[tauri::command]
pub async fn search_page(
    state: State<'_, Shared>,
    query: String,
    page: usize,
) -> Out<crate::search::PageView> {
    with_network(
        &state,
        move |session| crate::search::request(session, &query, page),
        crate::search::fetch,
        move |session, _, search| Ok(crate::search::view(session, search, page)),
    )
    .await
}

#[tauri::command]
pub async fn detail(state: State<'_, Shared>, content_type: String, id: String) -> Out<DetailView> {
    let (ct, item) = (content_type.clone(), id.clone());
    with_network(
        &state,
        |session| Ok(view::Net::of(session)),
        move |net| view::fetch_meta(net, &ct, &item),
        move |session, _, fetched| view::detail(session, &content_type, &id, fetched),
    )
    .await
}

/// The calendar: episodes of the library's series from a month back to two
/// months ahead, and the new ones as home-screen cards. Kept for half an
/// hour unless the library's series change, or `fresh` asks again.
#[tauri::command]
pub async fn calendar(state: State<'_, Shared>, fresh: Option<bool>) -> Out<view::CalendarView> {
    if !fresh.unwrap_or(false) {
        if let Some(kept) = with_session(&state, |session| Ok(view::calendar_kept(session))).await?
        {
            return Ok(kept);
        }
    }
    with_network(
        &state,
        |session| Ok(view::calendar_request(session)),
        |(net, series)| view::fetch_calendar(net, series),
        |session, _, answers| Ok(view::calendar(session, answers)),
    )
    .await
}

/// More of the open title's genre, from an addon catalogue narrowed to it,
/// without the title itself. `None` when no addon can narrow by genre.
#[tauri::command]
pub async fn more_like(
    state: State<'_, Shared>,
    content_type: String,
    id: String,
) -> Out<Option<view::MoreView>> {
    let own = id.clone();
    with_network(
        &state,
        move |session| view::more_request(session, &content_type, &id),
        |request| {
            request
                .as_ref()
                .map(|(_, request)| view::fetch_page(request))
        },
        move |session, request, answer| {
            let (Some((genre, _)), Some(previews)) = (request, answer) else {
                return Ok(None);
            };
            let previews: Vec<_> = previews?
                .into_iter()
                .filter(|preview| preview.id != own)
                .collect();
            Ok(Some(view::MoreView {
                genre,
                items: view::page_items(session, previews),
            }))
        },
    )
    .await
}

/// Play the open title's trailer.
///
/// Inside the window when mpv can: a direct file always, YouTube only when
/// `yt-dlp` is there for mpv's ytdl hook to use. Otherwise the system's
/// browser opens it -- the one place this program hands a link to something
/// else, and only on a press, because a trailer button that does nothing
/// without a tool most people do not have is a broken button. Answers
/// `player` or `browser`.
#[tauri::command]
pub async fn trailer(state: State<'_, Shared>) -> Out<String> {
    let shared = Arc::clone(state.inner());
    tauri::async_runtime::spawn_blocking(move || -> Out<String> {
        let url = {
            let session = shared
                .lock()
                .map_err(|_| "the host lost track of its own state".to_owned())?;
            let meta = session
                .meta
                .as_ref()
                .ok_or_else(|| session.text("ui.error.notrailer"))?;
            meta.trailer_url()
                .ok_or_else(|| session.text("ui.error.notrailer"))?
        };
        let youtube = url.contains("youtube.com/") || url.contains("youtu.be/");
        if youtube && !has_ytdlp() {
            open_in_browser(&url)?;
            return Ok("browser".to_owned());
        }
        let ticket = shared
            .lock()
            .map_err(|_| "the host lost track of its own state".to_owned())?
            .job
            .lock()
            .map(|mut job| job.begin())
            .unwrap_or_default();
        let search = subs::Search {
            ticket,
            subject: None,
            filename: None,
            video_hash: None,
            size: None,
            torrent: None,
            file_index: None,
            source: None,
        };
        launch(
            &shared,
            PlaybackSource::Url(url),
            Vec::new(),
            Vec::new(),
            None,
            search,
            Start::Beginning,
        )?;
        Ok("player".to_owned())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Whether `yt-dlp` is on the `PATH` or beside this program, where mpv's
/// ytdl hook looks for it.
fn has_ytdlp() -> bool {
    let names = ["yt-dlp.exe", "yt-dlp"];
    let beside = names
        .iter()
        .any(|name| crate::state::beside_exe(name).is_file());
    beside
        || std::env::var_os("PATH").is_some_and(|path| {
            std::env::split_paths(&path)
                .any(|dir| names.iter().any(|name| dir.join(name).is_file()))
        })
}

/// Hand a web address to the system's browser.
fn open_in_browser(url: &str) -> Out<()> {
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return Err("not a web address".to_owned());
    }
    #[cfg(windows)]
    let result = std::process::Command::new("rundll32")
        .args(["url.dll,FileProtocolHandler", url])
        .spawn();
    #[cfg(target_os = "macos")]
    let result = std::process::Command::new("open").arg(url).spawn();
    #[cfg(all(unix, not(target_os = "macos")))]
    let result = std::process::Command::new("xdg-open").arg(url).spawn();
    result.map(|_| ()).map_err(|e| e.to_string())
}

/// The billboard on the home screen: one item's artwork and blurb, looked up
/// without becoming the item the viewer is looking at.
#[tauri::command]
pub async fn feature(
    state: State<'_, Shared>,
    content_type: String,
    id: String,
) -> Out<DetailView> {
    let (ct, item) = (content_type.clone(), id.clone());
    with_network(
        &state,
        |session| Ok(view::Net::of(session)),
        move |net| view::fetch_meta(net, &ct, &item),
        move |session, _, fetched| view::feature(session, &content_type, &id, fetched),
    )
    .await
}

#[tauri::command]
pub async fn sources(
    state: State<'_, Shared>,
    content_type: String,
    id: String,
    name: String,
) -> Out<SourcesView> {
    let (ct, item) = (content_type.clone(), id.clone());
    with_network(
        &state,
        view::sources_request,
        move |(net, _)| view::fetch_sources(net, &ct, &item),
        move |session, (_, request), merged| {
            view::sources(session, &content_type, &id, &name, &merged, request)
        },
    )
    .await
}

/// Which languages the viewer's subtitle addons have subtitles in for one
/// item, and how many of each.
///
/// Asked beside the source list rather than inside it, so a slow subtitle
/// addon never holds up the sources. A subtitle addon matches on the film,
/// not on the release, so this holds whichever source is picked.
#[tauri::command]
pub async fn subtitle_languages(
    state: State<'_, Shared>,
    content_type: String,
    id: String,
) -> Out<subs::LanguagesView> {
    with_network(
        &state,
        |session| Ok(view::Net::of(session)),
        move |net| subs::languages(net, &content_type, &id),
        |_, _, found| Ok(found),
    )
    .await
}

// ------------------------------------------------------------------- keeping

/// Whether a community server is set, and who is signed in to it.
#[tauri::command]
pub async fn community_state(state: State<'_, Shared>) -> Out<crate::community::CommunityView> {
    with_session(&state, |session| Ok(session.community.view())).await
}

/// Set the community server's address, or clear it with an empty one.
#[tauri::command]
pub async fn community_server(
    state: State<'_, Shared>,
    address: String,
) -> Out<crate::community::CommunityView> {
    with_session(&state, move |session| {
        session
            .community
            .set_server(&address)
            .map_err(|key| session.text(key))?;
        Ok(session.community.view())
    })
    .await
}

/// One request to the community server, by the contract's method and path.
///
/// Signing in and out is noticed here, so the session token is kept on the
/// host and never reaches the interface. Answers `{status, body}`.
#[tauri::command]
pub async fn community_request(
    state: State<'_, Shared>,
    method: String,
    path: String,
    body: Option<serde_json::Value>,
) -> Out<serde_json::Value> {
    let (m, p) = (method.clone(), path.clone());
    with_network(
        &state,
        move |session| {
            crate::community::prepare(&session.community, &method, &path, body)
                .map_err(|key| session.text(key))
        },
        crate::community::send,
        move |session, _, answer| {
            let mut answer = answer?;
            let ok = (200..300).contains(&answer.status);
            let path = p.split('?').next().unwrap_or_default().to_owned();
            if ok && m == "POST" && path == "/v0/sessions" {
                let token = answer
                    .body
                    .get("token")
                    .and_then(|v| v.as_str())
                    .map(ToOwned::to_owned);
                let username = answer
                    .body
                    .get("username")
                    .and_then(|v| v.as_str())
                    .map(ToOwned::to_owned);
                let role = answer
                    .body
                    .get("role")
                    .and_then(|v| v.as_str())
                    .map(ToOwned::to_owned);
                if let (Some(token), Some(username)) = (token, username) {
                    session.community.signed_in(token, username, role);
                }
                // The token stays here.
                if let Some(object) = answer.body.as_object_mut() {
                    object.remove("token");
                }
            }
            // A 401 means the session is gone -- except where it answers a
            // password that was typed wrong.
            let typed = path == "/v0/sessions" || path == "/v0/me/password";
            let signed_out = (ok && m == "DELETE" && (path == "/v0/sessions" || path == "/v0/me"))
                || (ok && path == "/v0/me/password")
                || (answer.status == 401 && !typed);
            if signed_out {
                session.community.signed_out();
            }
            Ok(serde_json::json!({ "status": answer.status, "body": answer.body }))
        },
    )
    .await
}

/// Download a source of the current list, to watch later without a
/// connection. Returns once the torrent is known and fetching; the downloads
/// page follows it from there.
#[tauri::command]
pub async fn download(state: State<'_, Shared>, index: usize) -> Out<()> {
    let shared = Arc::clone(state.inner());
    tauri::async_runtime::spawn_blocking(move || -> Out<()> {
        let lost = || "the host lost track of its own state".to_owned();
        let (engine, request, hex, wanted) = {
            let mut session = shared.lock().map_err(|_| lost())?;
            let ranked = session
                .sources
                .get(index)
                .cloned()
                .ok_or_else(|| session.text("ui.error.nosource"))?;
            let torrent = torrents::source_of(&ranked.stream)
                .ok_or_else(|| session.text("ui.download.torrentonly"))?;
            let kept = crate::downloads::Request::from(&torrent.request)
                .ok_or_else(|| session.text("ui.download.torrentonly"))?;
            let playing = now_playing(&session, &ranked);
            let poster = session.meta.as_ref().and_then(|meta| meta.poster.clone());
            let engine = session.torrent_engine()?;
            let hex = kept.hex.clone();
            let (content_type, id, name, series_id) = playing.map_or_else(
                || {
                    (
                        String::new(),
                        String::new(),
                        ranked.stream.display_label(),
                        None,
                    )
                },
                |p| (p.content_type, p.id, p.name, p.series_id),
            );
            session.downloads.add(crate::downloads::Download {
                request: kept,
                file: None,
                wanted_file: torrent.file_index,
                content_type,
                id,
                name,
                series_id,
                poster,
                path: None,
                length: None,
                added: eon_stream_core::history::now_unix(),
            });
            session.downloads.save();
            (engine, torrent.request, hex, torrent.file_index)
        };
        fetch_download(&shared, &engine, &request, &hex, wanted)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Add a download's torrent to the engine and fetch only its file.
fn fetch_download(
    shared: &Shared,
    engine: &eon_stream_engine::TorrentEngine,
    request: &eon_stream_engine::TorrentRequest,
    hex: &str,
    wanted: Option<usize>,
) -> Out<()> {
    let handle = engine.add(request).map_err(|e| e.to_string())?;
    let file = handle
        .choose_file(None, wanted)
        .map_err(|e| e.to_string())?
        .clone();
    engine
        .only_file(&handle, file.index)
        .map_err(|e| e.to_string())?;
    if let Ok(mut session) = shared.lock() {
        if let Some(download) = session
            .downloads
            .all_mut()
            .iter_mut()
            .find(|d| d.request.hex.eq_ignore_ascii_case(hex))
        {
            download.file = Some(file.index);
            download.length = Some(file.length);
        }
        session.downloads.save();
    }
    Ok(())
}

/// Carry on with downloads that were not finished, at startup.
pub fn resume_downloads(shared: &Shared) {
    let pending: Vec<crate::downloads::Download> = shared
        .lock()
        .map(|session| {
            session
                .downloads
                .all()
                .iter()
                .filter(|d| d.path.is_none())
                .cloned()
                .collect()
        })
        .unwrap_or_default();
    if pending.is_empty() {
        return;
    }
    let Ok(engine) = shared
        .lock()
        .map_err(|_| String::new())
        .and_then(|mut session| session.torrent_engine())
    else {
        return;
    };
    for download in pending {
        let wanted = download.file.or(download.wanted_file);
        let _ = fetch_download(
            shared,
            &engine,
            &download.request.to_request(),
            &download.request.hex,
            wanted,
        );
    }
}

/// Every download, with how far each has got. A download that has finished
/// is noticed here and remembered with its file.
#[tauri::command]
pub async fn downloads(state: State<'_, Shared>) -> Out<Vec<crate::downloads::DownloadView>> {
    with_session(&state, |session| {
        let engine = session.torrent.clone();
        let mut changed = false;
        let mut out = Vec::new();
        for download in session.downloads.all_mut().iter_mut() {
            let mut percent = 0u8;
            let mut down_bps = 0;
            if download.path.is_some() {
                percent = 100;
            } else if let (Some(engine), Some(index)) = (engine.as_ref(), download.file) {
                if let Ok(handle) = engine.get(&download.request.hex) {
                    let have = engine.file_progress(&handle, index).unwrap_or_default();
                    let length = download.length.unwrap_or(1).max(1);
                    #[allow(clippy::cast_possible_truncation)]
                    {
                        percent = ((have.min(length) * 100) / length) as u8;
                    }
                    down_bps = engine
                        .progress(&handle)
                        .map(|p| p.download_bps)
                        .unwrap_or_default();
                    if have >= length {
                        if let Ok(file) = handle.file(index) {
                            download.path = Some(
                                engine
                                    .options()
                                    .download_dir
                                    .join(download.request.hex.to_ascii_lowercase())
                                    .join(&file.path),
                            );
                            changed = true;
                        }
                    }
                }
            }
            out.push(crate::downloads::DownloadView {
                hex: download.request.hex.clone(),
                name: download.name.clone(),
                poster: download.poster.clone(),
                content_type: download.content_type.clone(),
                id: download.id.clone(),
                percent,
                done: download.path.is_some(),
                down_bps,
                length: download.length,
            });
        }
        if changed {
            session.downloads.save();
        }
        Ok(out)
    })
    .await
}

/// Play a finished download from its file: nothing on the network.
#[tauri::command]
pub async fn download_play(state: State<'_, Shared>, hex: String) -> Out<()> {
    let shared = Arc::clone(state.inner());
    tauri::async_runtime::spawn_blocking(move || -> Out<()> {
        let (download, ticket) = {
            let session = shared
                .lock()
                .map_err(|_| "the host lost track of its own state".to_owned())?;
            let download = session
                .downloads
                .get(&hex)
                .cloned()
                .ok_or_else(|| session.text("ui.download.gone"))?;
            let ticket = session
                .job
                .lock()
                .map(|mut job| job.begin())
                .unwrap_or_default();
            (download, ticket)
        };
        let Some(path) = download.path.clone().filter(|path| path.is_file()) else {
            return finish_with_error(&shared, ticket, "ui.download.notyet".to_owned());
        };
        let playing = (!download.id.is_empty()).then(|| NowPlaying {
            content_type: download.content_type.clone(),
            id: download.id.clone(),
            name: download.name.clone(),
            series_id: download.series_id.clone(),
            season: None,
            episode: None,
            binge_group: None,
            addon_id: None,
        });
        let search = subs::Search {
            ticket,
            subject: playing
                .as_ref()
                .map(|p| (p.content_type.clone(), p.id.clone())),
            filename: path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned()),
            video_hash: None,
            size: download.length,
            torrent: None,
            file_index: None,
            source: None,
        };
        launch(
            &shared,
            PlaybackSource::File(path),
            Vec::new(),
            Vec::new(),
            playing,
            search,
            Start::Resume,
        )
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Whether a file dropped on the window is something to play.
#[must_use]
pub fn is_video(path: &std::path::Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(str::to_lowercase)
        .is_some_and(|e| {
            matches!(
                e.as_str(),
                "mkv"
                    | "mp4"
                    | "m4v"
                    | "avi"
                    | "mov"
                    | "webm"
                    | "wmv"
                    | "flv"
                    | "ts"
                    | "m2ts"
                    | "mpg"
                    | "mpeg"
                    | "ogv"
                    | "3gp"
            )
        })
}

/// Play a video file from this computer -- one dropped on the window. Its
/// path stays here: the interface is told only that something plays, and
/// nothing is recorded in history, which keeps titles, not files.
pub fn play_local(shared: &Shared, path: std::path::PathBuf) -> Out<()> {
    let ticket = {
        let session = shared
            .lock()
            .map_err(|_| "the host lost track of its own state".to_owned())?;
        session
            .job
            .lock()
            .map(|mut job| job.begin())
            .unwrap_or_default()
    };
    let size = std::fs::metadata(&path).ok().map(|m| m.len());
    let search = subs::Search {
        ticket,
        subject: None,
        filename: path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned()),
        video_hash: None,
        size,
        torrent: None,
        file_index: None,
        source: None,
    };
    launch(
        shared,
        PlaybackSource::File(path),
        Vec::new(),
        Vec::new(),
        None,
        search,
        Start::Beginning,
    )
}

/// Delete a download, finished or not, and its file.
#[tauri::command]
pub async fn download_remove(state: State<'_, Shared>, hex: String) -> Out<()> {
    let shared = Arc::clone(state.inner());
    tauri::async_runtime::spawn_blocking(move || -> Out<()> {
        let (engine, folder) = {
            let mut session = shared
                .lock()
                .map_err(|_| "the host lost track of its own state".to_owned())?;
            let removed = session.downloads.remove(&hex);
            session.downloads.save();
            let engine = session.torrent.clone();
            let folder = torrents::options_from(&session.settings.torrent)
                .ok()
                .map(|options| options.download_dir.join(hex.to_ascii_lowercase()));
            (engine, removed.and(folder))
        };
        if let Some(engine) = engine {
            if let Ok(handle) = engine.get(&hex) {
                let _ = engine.remove(&handle, true);
            }
        }
        // The torrent's own folder, named by its info hash: this program
        // made it, for this download.
        if let Some(folder) = folder.filter(|folder| folder.is_dir()) {
            let _ = std::fs::remove_dir_all(folder);
        }
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Who can watch on this computer, who is, and whether the first-run setup
/// was done.
#[tauri::command]
pub async fn profiles(state: State<'_, Shared>) -> Out<crate::profiles::Profiles> {
    with_session(&state, |session| Ok(session.profiles.clone())).await
}

#[tauri::command]
pub async fn profile_add(
    state: State<'_, Shared>,
    name: String,
    color: u8,
) -> Out<crate::profiles::Profiles> {
    with_session(&state, move |session| {
        grown_up(session)?;
        session
            .profiles
            .add(&name, color)
            .map_err(|key| session.text(key))?;
        Ok(session.profiles.clone())
    })
    .await
}

/// Profiles are changed from a grown-up's profile, never from a child's.
fn grown_up(session: &Session) -> Result<(), String> {
    if session.profiles.kids() {
        Err(session.text("ui.profiles.kidsblocked"))
    } else {
        Ok(())
    }
}

/// Make a profile a child's, or not.
#[tauri::command]
pub async fn profile_kids(
    state: State<'_, Shared>,
    id: String,
    kids: bool,
) -> Out<crate::profiles::Profiles> {
    with_session(&state, move |session| {
        grown_up(session)?;
        let profile = session
            .profiles
            .profiles
            .iter_mut()
            .find(|p| p.id == id)
            .ok_or_else(|| "no such profile".to_owned())?;
        profile.kids = kids;
        session.profiles.save();
        session.feed = None;
        Ok(session.profiles.clone())
    })
    .await
}

/// Set, change or clear a profile's PIN. Changing or clearing one takes the
/// one it has.
#[tauri::command]
pub async fn profile_pin(
    state: State<'_, Shared>,
    id: String,
    pin: Option<String>,
    old: Option<String>,
) -> Out<crate::profiles::Profiles> {
    with_session(&state, move |session| {
        grown_up(session)?;
        if !crate::profiles::pin_opens(&id, old.as_deref()) {
            return Err(session.text("ui.profiles.wrongpin"));
        }
        crate::profiles::set_pin(&id, pin.as_deref()).map_err(|key| session.text(key))?;
        if let Some(profile) = session.profiles.profiles.iter_mut().find(|p| p.id == id) {
            profile.locked = pin.is_some();
        }
        session.profiles.save();
        Ok(session.profiles.clone())
    })
    .await
}

#[tauri::command]
pub async fn profile_switch(
    state: State<'_, Shared>,
    id: String,
    pin: Option<String>,
) -> Out<crate::profiles::Profiles> {
    with_session(&state, move |session| {
        if !crate::profiles::pin_opens(&id, pin.as_deref()) {
            return Err(session.text("ui.profiles.wrongpin"));
        }
        session
            .switch_profile(&id)
            .map_err(|key| session.text(key))?;
        Ok(session.profiles.clone())
    })
    .await
}

#[tauri::command]
pub async fn profile_edit(
    state: State<'_, Shared>,
    id: String,
    name: String,
    color: u8,
) -> Out<crate::profiles::Profiles> {
    with_session(&state, move |session| {
        grown_up(session)?;
        session
            .profiles
            .rename(&id, &name, color)
            .map_err(|key| session.text(key))?;
        Ok(session.profiles.clone())
    })
    .await
}

#[tauri::command]
pub async fn profile_remove(
    state: State<'_, Shared>,
    id: String,
) -> Out<crate::profiles::Profiles> {
    with_session(&state, move |session| {
        grown_up(session)?;
        let _ = crate::profiles::set_pin(&id, None);
        session
            .profiles
            .remove(&id)
            .map_err(|key| session.text(key))?;
        Ok(session.profiles.clone())
    })
    .await
}

/// How the window opens, and now.
#[tauri::command]
pub async fn window_mode_set(state: State<'_, Shared>, mode: String) -> Out<()> {
    if !crate::profiles::WINDOW_MODES.contains(&mode.as_str()) {
        return Err(format!("{mode} is not a window mode"));
    }
    let m = mode.clone();
    with_session(&state, move |session| {
        session.profiles.window = m;
        session.profiles.save();
        Ok(())
    })
    .await?;
    crate::surface::apply_window_mode(&mode)
}

/// The first-run setup is done, or was skipped: it is not shown again.
#[tauri::command]
pub async fn onboarding_done(state: State<'_, Shared>) -> Out<()> {
    with_session(&state, |session| {
        session.profiles.onboarded = true;
        session.profiles.save();
        Ok(())
    })
    .await
}

/// Ask the addons what the titles in continue watching look like, for those
/// that played before their look was remembered. Answers how many were
/// filled in; the home screen draws its row again when that is not zero.
#[tauri::command]
pub async fn fill_art(state: State<'_, Shared>) -> Out<usize> {
    with_network(
        &state,
        |session| {
            // Continue watching first -- those cards are on screen -- then
            // the rest of history and the library, for the recommendations.
            let mut missing: Vec<(String, String)> = session
                .history
                .continue_watching(12)
                .into_iter()
                .map(crate::art::title_of)
                .collect();
            let mut rest: Vec<&eon_stream_core::WatchEntry> = session.history.entries().collect();
            rest.sort_by_key(|e| std::cmp::Reverse(e.updated_at));
            missing.extend(rest.into_iter().map(crate::art::title_of));
            missing.extend(
                session
                    .library
                    .list(eon_stream_core::LibraryOrder::Recent)
                    .into_iter()
                    .map(|e| (e.content_type.clone(), e.id.clone())),
            );
            let mut asked = std::collections::HashSet::new();
            missing.retain(|(_, id)| session.art.get(id).is_none() && asked.insert(id.clone()));
            missing.truncate(24);
            Ok((view::Net::of(session), missing))
        },
        |(net, missing)| {
            missing
                .iter()
                .map(|(content_type, id)| (id.clone(), view::fetch_meta(net, content_type, id).0))
                .collect::<Vec<_>>()
        },
        |session, _, found| {
            let mut filled = 0;
            for (id, meta) in found {
                if let Some(meta) = meta {
                    session.art.put(&id, crate::art::Look::of(&meta));
                    filled += 1;
                }
            }
            if filled > 0 {
                let profile = session.profiles.current.clone();
                session.art.save(&profile);
            }
            Ok(filled)
        },
    )
    .await
}

/// One page of "Recommended for you": ranked on this computer from what
/// this profile watched, scored and saved. `fresh` starts the feed again.
#[tauri::command]
pub async fn recommend(
    state: State<'_, Shared>,
    page: usize,
    fresh: bool,
) -> Out<crate::recommend::FeedPage> {
    with_network(
        &state,
        move |session| Ok(crate::recommend::request(session, page, fresh)),
        crate::recommend::fetch,
        move |session, _, feed| Ok(crate::recommend::view(session, feed, page)),
    )
    .await
}

/// The viewer's own score of a title, if they gave one.
#[tauri::command]
pub async fn rating_get(
    state: State<'_, Shared>,
    content_type: String,
    id: String,
) -> Out<Option<crate::ratings::Rating>> {
    with_session(&state, move |session| {
        Ok(session.ratings.get(&content_type, &id).cloned())
    })
    .await
}

/// Score a title out of ten, with an optional review; no score takes it
/// away. Kept on this machine only.
#[tauri::command]
pub async fn rating_set(
    state: State<'_, Shared>,
    content_type: String,
    id: String,
    score: Option<u8>,
    review: Option<String>,
) -> Out<Option<crate::ratings::Rating>> {
    with_session(&state, move |session| {
        crate::ratings::set(session, &content_type, &id, score, review)
    })
    .await
}

/// The viewer's profile: every score, and what they add up to.
#[tauri::command]
pub async fn profile(state: State<'_, Shared>) -> Out<crate::ratings::ProfileView> {
    with_session(&state, |session| Ok(crate::ratings::profile(session))).await
}

/// The whole library, for its page.
#[tauri::command]
pub async fn library(state: State<'_, Shared>, order: String) -> Out<Vec<view::ItemView>> {
    with_session(&state, move |session| Ok(view::library(session, &order))).await
}

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

/// Take something off "continue watching". What was finished stays watched.
#[tauri::command]
pub async fn continue_remove(
    state: State<'_, Shared>,
    content_type: String,
    id: String,
) -> Out<usize> {
    with_session(&state, move |session| {
        let removed = session.history.stop_continuing(&content_type, &id);
        if removed > 0 {
            session.save_history();
        }
        Ok(removed)
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
pub async fn play(
    state: State<'_, Shared>,
    index: usize,
    file: Option<usize>,
    from_start: Option<bool>,
) -> Out<()> {
    let shared = Arc::clone(state.inner());
    let begin = if from_start.unwrap_or(false) {
        Start::Beginning
    } else {
        Start::Resume
    };
    tauri::async_runtime::spawn_blocking(move || start(&shared, index, file, begin, None))
        .await
        .map_err(|e| e.to_string())?
}

/// Where a playback starts.
#[derive(Clone, Copy)]
pub enum Start {
    /// Where the viewer left off, if anywhere.
    Resume,
    /// The beginning: "Start over".
    Beginning,
    /// A given second: a restart after a seek back past what the cache
    /// keeper gave back.
    At(f64),
}

/// Start source `index` of the current list, on the calling thread.
///
/// `begin` says where: where the viewer left off, the beginning ("Start
/// over"), or a given second. `ticket` is a start already under way -- see
/// [`play_next`] -- or `None` to begin one.
fn start(
    shared: &Shared,
    index: usize,
    file: Option<usize>,
    begin: Start,
    ticket: Option<u64>,
) -> Out<()> {
    // Everything that needs the lock happens first and the lock is then
    // dropped, because what follows can take minutes.
    let (ranked, playing, engine, job, ticket) = {
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
        session.starting_source = Some(index);
        if !session.tried.contains(&index) {
            session.tried.push(index);
        }
        let job = Arc::clone(&session.job);
        let ticket = match ticket {
            // A start that began earlier -- the next episode, whose sources
            // were being found under this ticket -- and may have been
            // cancelled meanwhile.
            Some(ticket) => {
                if !job.lock().map(|job| job.wanted(ticket)).unwrap_or(true) {
                    return Ok(());
                }
                ticket
            }
            None => job.lock().map(|mut job| job.begin()).unwrap_or_default(),
        };
        (ranked, playing, engine, job, ticket)
    };

    let subtitles: Vec<String> = ranked
        .stream
        .subtitles
        .iter()
        .map(|s| s.url.clone())
        .collect();
    let hints = ranked.stream.subtitle_match();
    let mut search = subs::Search {
        ticket,
        subject: playing
            .as_ref()
            .map(|p| (p.content_type.clone(), p.id.clone())),
        filename: hints.filename.map(ToOwned::to_owned),
        video_hash: hints.video_hash.map(ToOwned::to_owned),
        size: hints.video_size.or_else(|| ranked.stream.size_bytes()),
        torrent: None,
        file_index: None,
        source: Some((index, file)),
    };

    let (source, headers) = match (torrents::source_of(&ranked.stream), engine) {
        (Some(torrent), Some(engine)) => {
            match torrents::resolve(&engine, &torrent, file, &job, ticket) {
                // Subtitles that came with the stream still apply; the
                // headers do not, because the player is reading from
                // loopback rather than from the addon's host.
                Ok(resolved) => {
                    if let Ok(file) = resolved.handle.file(resolved.index) {
                        search.filename = Some(file.name().to_owned());
                        search.size = Some(file.length);
                    }
                    if let (Some(bps), Ok(mut session)) = (resolved.measured_bps, shared.lock()) {
                        session.speeds.record(resolved.handle.info_hash(), bps);
                        session.speeds.save();
                    }
                    search.torrent = Some((engine, resolved.handle));
                    search.file_index = Some(resolved.index);
                    (resolved.source, Vec::new())
                }
                Err(message) => return finish_with_error(shared, ticket, message),
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
                return finish_with_error(shared, ticket, message);
            }
        },
    };

    launch(shared, source, headers, subtitles, playing, search, begin)
}

/// Play the episode after the one playing, from the same release when the
/// addon groups its releases.
///
/// The work a person would do by hand -- open the series, pick the episode,
/// pick a source -- in the order they would do it, with one rule for the
/// last step: the source whose `bingeGroup` matches the one this series was
/// last watched with, then one from the same addon, then the best match.
/// Sources are found under the start's ticket, so "Cancel and go back" works
/// here too.
#[tauri::command]
pub async fn play_next(state: State<'_, Shared>) -> Out<()> {
    let shared = Arc::clone(state.inner());
    tauri::async_runtime::spawn_blocking(move || -> Out<()> {
        let lost = || "the host lost track of its own state".to_owned();
        let (net, request, next, name, wanted_group, addon, ticket) = {
            let mut session = shared.lock().map_err(|_| lost())?;
            let (next, name) =
                view::next_episode(&session).ok_or_else(|| session.text("ui.error.nonext"))?;
            let playing = session.playing.clone();
            let wanted_group = playing
                .as_ref()
                .and_then(|p| p.series_id.as_deref())
                .and_then(|series| session.history.binge_group_for_series(series))
                .map(ToOwned::to_owned)
                .or_else(|| playing.as_ref().and_then(|p| p.binge_group.clone()));
            let addon = playing.as_ref().and_then(|p| p.addon_id.clone());
            let (net, request) = view::sources_request(&mut session)?;
            let ticket = session
                .job
                .lock()
                .map(|mut job| {
                    let ticket = job.begin();
                    job.at("ui.job.next", name.clone());
                    ticket
                })
                .unwrap_or_default();
            (net, request, next, name, wanted_group, addon, ticket)
        };

        let merged = view::fetch_sources(&net, "series", &next);

        let index = {
            let mut session = shared.lock().map_err(|_| lost())?;
            let listed = view::sources(&mut session, "series", &next, &name, &merged, request);
            let found = match listed {
                Ok(found) => found,
                Err(message) => {
                    drop(session);
                    return finish_with_error(&shared, ticket, message);
                }
            };
            if found.sources.is_empty() {
                let message = session.text("ui.detail.nosources");
                drop(session);
                return finish_with_error(&shared, ticket, message);
            }
            let ranked = &session.sources;
            wanted_group
                .and_then(|group| {
                    ranked
                        .iter()
                        .position(|r| r.stream.binge_group() == Some(group.as_str()))
                })
                .or_else(|| addon.and_then(|addon| ranked.iter().position(|r| r.addon_id == addon)))
                .unwrap_or(0)
        };
        start(&shared, index, None, Start::Resume, Some(ticket))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Mark an episode or a film as watched, or as not watched.
///
/// Watched is written as a history entry that is finished; not watched
/// forgets the entry, which is also what takes it out of "Continue watching".
#[tauri::command]
pub async fn watched_set(
    state: State<'_, Shared>,
    content_type: String,
    id: String,
    watched: bool,
) -> Out<bool> {
    with_session(&state, move |session| {
        if !watched {
            session.history.forget(&content_type, &id);
            session.save_history();
            return Ok(false);
        }
        let meta = session.meta.as_ref();
        let video = meta.and_then(|meta| meta.videos.iter().find(|video| video.id == id));
        let name = meta.map_or_else(|| id.clone(), |meta| meta.display_name().to_owned());
        let series_id = meta
            .map(|meta| meta.id.clone())
            .filter(|series| *series != id);
        let entry = eon_stream_core::WatchEntry {
            content_type: content_type.clone(),
            id: id.clone(),
            name,
            series_id,
            season: video.and_then(|video| video.season),
            episode: video.and_then(|video| video.episode),
            // Finished by definition: the whole of a runtime nobody measured.
            position_secs: 1.0,
            duration_secs: Some(1.0),
            binge_group: None,
            addon_id: None,
            updated_at: eon_stream_core::history::now_unix(),
        };
        session.history.record(entry);
        session.save_history();
        Ok(true)
    })
    .await
}

/// Give up on the source that is starting or stalling, and start the fastest
/// one of the same list not tried yet: by measured speed where there is one,
/// otherwise by the addon's seeders; a direct link counts as fast.
#[tauri::command]
pub async fn play_faster(state: State<'_, Shared>) -> Out<()> {
    let shared = Arc::clone(state.inner());
    tauri::async_runtime::spawn_blocking(move || -> Out<()> {
        let next = {
            let session = shared
                .lock()
                .map_err(|_| "the host lost track of its own state".to_owned())?;
            let score = |ranked: &eon_stream_core::ranking::RankedStream| -> u64 {
                match torrents::source_of(&ranked.stream) {
                    Some(torrent) => torrent
                        .request
                        .info_hash()
                        .ok()
                        .and_then(|hash| session.speeds.get(&hash))
                        .unwrap_or_else(|| crate::speeds::estimate(ranked.facts.seeders)),
                    None => 8 * 1024 * 1024,
                }
            };
            session
                .sources
                .iter()
                .enumerate()
                .filter(|(index, _)| !session.tried.contains(index))
                .max_by_key(|(index, ranked)| (score(ranked), std::cmp::Reverse(*index)))
                .map(|(index, _)| index)
                .ok_or_else(|| session.text("ui.error.nofaster"))?
        };
        abandon(&shared)?;
        start(&shared, next, None, Start::Resume, None)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Give up on what is starting or playing, and on what it fetched.
///
/// The button under the spinner. Whatever stage the start is in -- waiting for
/// metadata, filling the prebuffer, or a player that opened and never got a
/// frame -- this ends it: the worker is told nobody wants its result, mpv is
/// closed, and the torrent is taken out of the engine with its pieces deleted.
/// A stalled start is usually a swarm that is not there, and keeping what it
/// managed to fetch would be keeping the cause.
///
/// Where it got to is still written down: a film that stalled after forty
/// minutes resumes at forty minutes from another source.
#[tauri::command]
pub async fn cancel(state: State<'_, Shared>) -> Out<()> {
    let shared = Arc::clone(state.inner());
    tauri::async_runtime::spawn_blocking(move || abandon(&shared))
        .await
        .map_err(|e| e.to_string())?
}

/// The work of [`cancel`], on the calling thread.
fn abandon(shared: &Shared) -> Out<()> {
    let (engine, hashes) = {
        let mut session = shared
            .lock()
            .map_err(|_| "the host lost track of its own state".to_owned())?;
        let mut hashes: Vec<String> = session
            .job
            .lock()
            .ok()
            .and_then(|mut job| job.cancel())
            .into_iter()
            .collect();
        session.remember_position();
        if let Some(player) = session.player.take() {
            let _ = player.quit();
        }
        session.playing = None;
        session.last_seen = None;
        if let Some(hash) = session.playing_torrent.take() {
            if !hashes.contains(&hash) {
                hashes.push(hash);
            }
        }
        // Ends the subtitle search for this playback too: its ticket no
        // longer matches. And the cache keeper's watch.
        session.subtitles = subs::Found::default();
        session.keeper.torrent = None;
        session.keeper.source = None;
        (session.torrent.clone(), hashes)
    };
    crate::surface::close_controls();
    subs::clear_folder();
    if let Some(engine) = engine {
        for hash in hashes {
            if let Ok(handle) = engine.get(&hash) {
                let _ = engine.remove(&handle, true);
            }
        }
    }
    Ok(())
}

/// What subtitles were found for what is playing.
#[tauri::command]
pub async fn subtitles(state: State<'_, Shared>) -> Out<subs::SubtitlesView> {
    with_session(&state, |session| Ok(subs::view(session))).await
}

/// Load one of the subtitles found, by its place in the list, and switch it
/// on.
#[tauri::command]
pub async fn subtitle_load(state: State<'_, Shared>, index: usize) -> Out<()> {
    let shared = Arc::clone(state.inner());
    tauri::async_runtime::spawn_blocking(move || {
        let ticket = shared
            .lock()
            .map(|session| session.subtitles.ticket)
            .map_err(|_| "the host lost track of its own state".to_owned())?;
        subs::load_one(&shared, ticket, index, true)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// A subtitle file the viewer picked, as its name and its bytes.
#[tauri::command]
pub async fn subtitle_file(state: State<'_, Shared>, name: String, bytes: Vec<u8>) -> Out<()> {
    with_session(&state, move |session| {
        subs::add_file(session, &name, &bytes)
    })
    .await
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

    let ticket = shared
        .lock()
        .map_err(|_| "the host lost track of its own state".to_owned())?
        .job
        .lock()
        .map(|mut job| job.begin())
        .unwrap_or_default();
    // No addon is involved, so there is nothing to ask a subtitle addon
    // about; what ships inside the torrent is still found.
    let mut search = subs::Search {
        ticket,
        subject: None,
        filename: None,
        video_hash: None,
        size: None,
        torrent: None,
        file_index: None,
        source: None,
    };

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
        return match torrents::resolve(&engine, &source, None, &job, ticket) {
            Ok(resolved) => {
                search.torrent = Some((engine, resolved.handle));
                search.file_index = Some(resolved.index);
                launch(
                    shared,
                    resolved.source,
                    Vec::new(),
                    Vec::new(),
                    None,
                    search,
                    Start::Resume,
                )
            }
            Err(message) => finish_with_error(shared, ticket, message),
        };
    }

    let path = std::path::Path::new(&link);
    let source = if path.is_file() {
        PlaybackSource::File(path.to_path_buf())
    } else {
        PlaybackSource::Url(link)
    };
    launch(
        shared,
        source,
        Vec::new(),
        Vec::new(),
        None,
        search,
        Start::Resume,
    )
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
    if action == "seek" || action == "seek-to" {
        let amount = value.unwrap_or_default();
        let absolute = action == "seek-to";
        let shared = Arc::clone(state.inner());
        let restart = with_session(&state, move |session| {
            let released = session.keeper.released_secs;
            if released <= 0.0 || session.keeper.source.is_none() {
                return Ok(None);
            }
            let target = if absolute {
                amount
            } else {
                let now = session
                    .player
                    .as_mut()
                    .and_then(|player| player.position().ok().flatten())
                    .unwrap_or_default();
                now + amount
            };
            Ok((target < released).then_some(target.max(0.0)))
        })
        .await?;
        if let Some(target) = restart {
            return tauri::async_runtime::spawn_blocking(move || restart_at(&shared, target))
                .await
                .map_err(|e| e.to_string())?;
        }
        return with_session(&state, move |session| {
            let Some(player) = session.player.as_mut() else {
                return Err(session.text("ui.error.noplayer"));
            };
            let mode = if absolute {
                SeekMode::Absolute
            } else {
                SeekMode::Relative
            };
            player.seek(amount, mode).map_err(|e| e.to_string())
        })
        .await;
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
            // Subtitle size, and how far subtitles and sound are moved
            // against the picture, in seconds. Absolute values: the controls
            // hold the number and send the new one.
            "sub-scale" => player.set_property("sub-scale", &amount.clamp(0.5, 3.0).to_string()),
            "sub-delay" => player.set_subtitle_delay(amount.clamp(-60.0, 60.0)),
            "audio-delay" => player.set_audio_delay(amount.clamp(-10.0, 10.0)),
            // How far above the bottom subtitles sit, in mpv's scaled pixels
            // (720 to the picture's height). The controls raise them while
            // they are up, so a line of dialogue is never under the bar.
            "sub-margin" => {
                player.set_property("sub-margin-y", &amount.clamp(0.0, 300.0).to_string())
            }
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

/// Subtitle size and the two delays, as the player has them now.
#[derive(Serialize)]
pub struct TuningView {
    pub sub_scale: f64,
    pub sub_delay: f64,
    pub audio_delay: f64,
}

#[tauri::command]
pub async fn tuning(state: State<'_, Shared>) -> Out<TuningView> {
    with_session(&state, |session| {
        let gone = session.text("ui.error.noplayer");
        let player = session.player.as_mut().ok_or(gone)?;
        let mut read = |name: &str| {
            player
                .get_property(name)
                .ok()
                .and_then(|value| value.parse::<f64>().ok())
        };
        Ok(TuningView {
            sub_scale: read("sub-scale").unwrap_or(1.0),
            sub_delay: read("sub-delay").unwrap_or_default(),
            audio_delay: read("audio-delay").unwrap_or_default(),
        })
    })
    .await
}

/// Play the same source again from `target`: the part of the file before it
/// was given back to keep the cache under its limit, so the torrent is
/// dropped with its pieces -- its record of what it has no longer matches
/// the disk -- and fetched again from there.
fn restart_at(shared: &Shared, target: f64) -> Out<()> {
    let lost = || "the host lost track of its own state".to_owned();
    let (index, file, engine, hash, ticket) = {
        let mut session = shared.lock().map_err(|_| lost())?;
        let (index, file) = session
            .keeper
            .source
            .ok_or_else(|| session.text("ui.error.noplayer"))?;
        // The start begins before the player goes, so no status poll sees
        // "nothing playing, nothing starting" in between and takes the
        // controls down.
        let ticket = session
            .job
            .lock()
            .map(|mut job| job.begin())
            .unwrap_or_default();
        session.remember_position();
        if let Some(player) = session.player.take() {
            let _ = player.quit();
        }
        session.playing = None;
        session.last_seen = None;
        let hash = session.playing_torrent.take();
        (index, file, session.torrent.clone(), hash, ticket)
    };
    if let (Some(engine), Some(hash)) = (engine, hash) {
        if let Ok(handle) = engine.get(&hash) {
            let _ = engine.remove(&handle, true);
        }
    }
    start(shared, index, file, Start::At(target), Some(ticket))
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
            "playback.subtitleScale" => parse(value, |v| candidate.playback.subtitle_scale = v),
            "playback.autoplayNext" => boolean(value, |v| candidate.playback.autoplay_next = v),
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
            "torrent.cacheLimitMegabytes" => {
                parse(value, |v| candidate.torrent.cache_limit_megabytes = v)
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
    search: subs::Search,
    begin: Start,
) -> Out<()> {
    let ticket = search.ticket;
    // Before the lock: on a `--play` start this can wait for the window to be
    // built, and nothing else should wait on that.
    let mut extra_args = crate::surface::mpv_arguments();

    let mut session = shared
        .lock()
        .map_err(|_| "the host lost track of its own state".to_owned())?;

    // Checked under the lock `cancel` takes as well, so a cancel lands either
    // wholly before this -- and nothing starts -- or wholly after, and closes
    // what did.
    let wanted = session
        .job
        .lock()
        .map(|job| job.wanted(ticket))
        .unwrap_or(true);
    if !wanted {
        return Ok(());
    }

    session.remember_position();
    if let Some(existing) = session.player.take() {
        let _ = existing.quit();
    }

    // The volume the viewer set, rather than mpv's own default of 100.
    extra_args.push(format!("--volume={}", session.settings.playback.volume));
    // Decode on the graphics card where that is known to work. The player
    // runs with `--no-config`, so mpv's own default applies, and that default
    // is the processor: a 4K HEVC release then takes a laptop's CPU whole,
    // while the torrent engine is hashing pieces on the same cores, and the
    // picture stutters. `auto-safe` only picks decoders mpv considers safe
    // and falls back to software on its own when none fits.
    extra_args.push("--hwdec=auto-safe".to_owned());
    // Stay on the last frame at the end rather than exiting: the controls
    // decide what comes next -- the next episode, or back to the catalogue --
    // and they cannot ask a player that has already gone.
    extra_args.push("--keep-open=yes".to_owned());
    extra_args.extend(crate::keeper::player_cache_args());
    // Subtitles that read over a bright sky as well as a dark room: mpv's
    // default outline is thin enough to vanish against white.
    extra_args.push("--sub-border-size=2.6".to_owned());
    extra_args.push("--sub-shadow-offset=1.2".to_owned());
    extra_args.push("--sub-shadow-color=#99000000".to_owned());
    // Embedded subtitle and audio tracks in the viewer's languages.
    extra_args.extend(subs::mpv_arguments(&session));
    // The viewer's subtitle size, and the two playback settings the window
    // used to accept and never apply: starting muted, and the player
    // arguments the settings file carries (validated by core: each a
    // `--flag`, never a bare value mpv would read as a file).
    extra_args.push(format!(
        "--sub-scale={}",
        session.settings.playback.subtitle_scale
    ));
    if session.settings.playback.start_muted {
        extra_args.push("--mute=yes".to_owned());
    }
    extra_args.extend(session.settings.playback.player_arguments.iter().cloned());

    let resume = match begin {
        Start::Resume => playing
            .as_ref()
            .and_then(|p| session.history.resume_position(&p.content_type, &p.id)),
        Start::Beginning => None,
        Start::At(seconds) => Some(seconds),
    };
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
            // What it looks like, for the continue-watching card.
            if let (Some(now), Some(meta)) = (playing.as_ref(), session.meta.clone()) {
                let title = now.series_id.clone().unwrap_or_else(|| now.id.clone());
                if meta.id == title {
                    session.art.put(&title, crate::art::Look::of(&meta));
                    let profile = session.profiles.current.clone();
                    session.art.save(&profile);
                }
            }
            session.playing = playing;
            session.playing_torrent = search
                .torrent
                .as_ref()
                .map(|(_, handle)| handle.info_hash().to_owned());
            let watched = search
                .torrent
                .clone()
                .zip(search.file_index)
                .map(|((engine, handle), index)| (engine, handle, index));
            let keep = watched.is_some();
            session.keeper.begin(ticket, watched, search.source);
            subs::reset(&mut session, ticket);
            if let Ok(mut job) = session.job.lock() {
                job.done();
            }
            // The controls go on after mpv's surface exists, so they land on
            // top of it -- see `surface`. Not under the lock: laying them on
            // waits for the window's own thread.
            drop(session);
            crate::surface::open_controls();
            if keep {
                let shared = Arc::clone(shared);
                std::thread::spawn(move || crate::keeper::watch(&shared, ticket));
            }
            let shared = Arc::clone(shared);
            std::thread::spawn(move || subs::find(&shared, search));
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
///
/// A start the viewer cancelled reports nothing: they asked for it to stop,
/// and a toast saying it stopped -- or that the swarm it was waiting on timed
/// out a minute later -- would be news about something they left.
fn finish_with_error(shared: &Shared, ticket: u64, message: String) -> Out<()> {
    if let Ok(session) = shared.lock() {
        if let Ok(mut job) = session.job.lock() {
            if !job.wanted(ticket) {
                return Ok(());
            }
            job.failed(message.clone());
        }
    }
    if message == torrents::CANCELLED {
        return Ok(());
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
