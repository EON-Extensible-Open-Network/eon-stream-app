// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 EON contributors
//
// Additional permission under GNU GPL version 3 section 7:
// see LICENSE-EXCEPTION.md (EON Module ABI Exception 1.0).

//! What the window is allowed to know.
//!
//! Every type here is a *view*: what the interface needs in order to draw
//! something, and nothing else. The shapes the library works in —
//! [`eon_stream_core::Stream`], [`eon_stream_core::ranking::RankedStream`],
//! [`eon_stream_core::registry::InstalledAddon`] — never cross the IPC
//! boundary, and that is not tidiness.
//!
//! * An addon's configured address can carry a credential in its path, so
//!   [`AddonView`] has an id, a name and a version and no URL. `addons.read`
//!   in the Module ABI makes the same promise to a module (madde 22), and the
//!   interface gets no more than a module does.
//! * A stream URL can carry a debrid token or a signed path. [`SourceView`]
//!   has an index and a label; `play(index)` is how it is used.
//! * A torrent's loopback URL carries the session token. [`crate::torrents`]
//!   reports the address without it.
//!
//! The rule in one line: **a view carries what a person reads, not what a
//! request needs.**

use eon_stream_core::{
    history::{clock, now_unix},
    language,
    manifest::Catalog,
    ranking::{format_size, rank, RankedStream, Resolution},
    types::StreamSource,
    AddonRegistry, Library, LibraryOrder, MetaPreview, Suggestion, WatchHistory,
};
use serde::Serialize;

use crate::state::Session;

/// An installed addon, as the interface sees it: no address, ever.
#[derive(Serialize)]
pub struct AddonView {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: Option<String>,
    pub types: Vec<String>,
    pub resources: Vec<String>,
    pub catalogs: usize,
    pub enabled: bool,
    pub needs_configuration: bool,
}

/// One thing in a row or a grid.
#[derive(Serialize)]
pub struct ItemView {
    pub id: String,
    pub content_type: String,
    pub name: String,
    pub poster: Option<String>,
    /// The catalogue's own blurb, for the billboard on the home screen.
    pub description: Option<String>,
    pub year: Option<String>,
    pub rating: Option<String>,
    pub in_library: bool,
    /// How far through it someone got, 0.0 to 1.0, when there is a position.
    pub progress: Option<f64>,
    /// Where to resume, as a clock, for the label on a continue-watching card.
    pub resume_at: Option<String>,
    /// Season and episode, for a continue-watching card of a series.
    pub episode_label: Option<String>,
}

/// A row on the home screen.
///
/// Catalogue rows arrive empty and are filled by a second call, one row at a
/// time. A home screen that waits for eight addons to answer before it draws
/// anything is a home screen that looks broken on a slow connection; one that
/// fills in as the answers arrive is the same requests in a different order.
#[derive(Serialize)]
pub struct RowView {
    /// `continue`, `library` or `catalog`.
    pub kind: String,
    /// What to call it. For a catalogue row this is the addon's own wording,
    /// which is not translatable — it is their title, not ours.
    pub title: String,
    pub addon_id: Option<String>,
    pub addon_name: Option<String>,
    pub catalog_id: Option<String>,
    pub content_type: Option<String>,
    pub searchable: bool,
    pub items: Vec<ItemView>,
}

/// The result of one search box.
#[derive(Serialize)]
pub struct SearchView {
    pub query: String,
    /// What was typed, when the query that ran was a correction of it.
    pub corrected_from: Option<String>,
    pub items: Vec<ItemView>,
    pub failures: Vec<String>,
}

/// One episode of a series.
#[derive(Serialize)]
pub struct EpisodeView {
    pub id: String,
    pub title: String,
    pub season: Option<u32>,
    pub episode: Option<u32>,
    pub released: Option<String>,
    pub thumbnail: Option<String>,
    pub progress: Option<f64>,
    pub finished: bool,
}

/// Everything about one item.
#[derive(Serialize)]
pub struct DetailView {
    pub id: String,
    pub content_type: String,
    pub name: String,
    pub description: Option<String>,
    pub poster: Option<String>,
    pub background: Option<String>,
    pub logo: Option<String>,
    pub year: Option<String>,
    pub runtime: Option<String>,
    pub genres: Vec<String>,
    pub cast: Vec<String>,
    pub director: Vec<String>,
    pub in_library: bool,
    pub resume_at: Option<String>,
    pub seasons: Vec<u32>,
    pub episodes: Vec<EpisodeView>,
    pub has_trailer: bool,
    pub failures: Vec<String>,
}

/// One playable source.
#[derive(Serialize)]
pub struct SourceView {
    /// Its position in the host's list. This is what `play` takes, and it
    /// stays the same for as long as the list stands — narrowing by quality
    /// changes which rows are on screen and never what a row is called.
    pub index: usize,
    pub label: String,
    pub addon_name: String,
    pub quality: String,
    pub size: Option<String>,
    pub seeders: Option<u32>,
    /// Audio languages the source says it carries -- what a dub is. Two-letter
    /// codes where the language table knows the label, so the interface can
    /// filter on them and name them in its own language; the addon's own
    /// spelling where it does not.
    pub languages: Vec<String>,
    /// Languages of the subtitle files that come with the source, the same
    /// way. Empty does not mean "no subtitles": a release can carry them
    /// inside the file, which `has_subtitles` reports.
    pub subtitle_languages: Vec<String>,
    pub is_torrent: bool,
    pub has_subtitles: bool,
    pub preferred_audio: bool,
    pub needs_headers: bool,
}

/// The sources for one item.
#[derive(Serialize)]
pub struct SourcesView {
    pub content_type: String,
    pub id: String,
    pub name: String,
    pub sources: Vec<SourceView>,
    /// Quality buckets present, best first, with how many sources each holds.
    pub qualities: Vec<(String, usize)>,
    pub failures: Vec<String>,
}

/// What the player is doing.
#[derive(Serialize, Default)]
pub struct PlayerView {
    pub running: bool,
    pub title: String,
    pub paused: bool,
    pub position: Option<f64>,
    pub duration: Option<f64>,
    pub position_clock: Option<String>,
    pub duration_clock: Option<String>,
    pub buffered_seconds: Option<f64>,
    /// 0 to 100, as mpv reports it.
    pub volume: Option<f64>,
    pub muted: bool,
    pub speed: Option<f64>,
    /// Waiting on the network rather than paused by a person.
    pub buffering: bool,
}

/// One audio or subtitle track inside what is playing.
#[derive(Serialize)]
pub struct TrackView {
    pub id: u32,
    /// `audio` or `sub`. Video tracks are not offered: a file with two is
    /// rare enough that a menu for it would be clutter.
    pub kind: String,
    /// A two-letter code when the language table knows the file's label.
    pub language: Option<String>,
    pub title: Option<String>,
    pub selected: bool,
}

/// One poll of everything that changes on its own.
#[derive(Serialize)]
pub struct StatusView {
    pub version: String,
    pub locale: String,
    pub job: crate::state::Job,
    pub player: PlayerView,
    pub torrents: Vec<crate::torrents::TorrentView>,
    pub addons: usize,
    pub library: usize,
}

/// The settings page.
#[derive(Serialize)]
pub struct SettingsView {
    /// The whole settings document, as it is stored.
    pub settings: serde_json::Value,
    /// Key to value, in the same words the headless build uses.
    pub summary: std::collections::BTreeMap<String, String>,
    /// Whether anything in the stored document could send anything anywhere
    /// (madde 36). Walked rather than asserted.
    pub audit_clean: bool,
    pub audit_found: Vec<String>,
    /// Languages a picker may offer, as two-letter codes. A list rather than
    /// a text box: a language typed free-hand is one nothing will ever match.
    pub languages: Vec<String>,
    /// Bytes of downloaded pieces on disk.
    pub buffer_bytes: u64,
}

/// The about page: what this build is and what it trusts.
#[derive(Serialize)]
pub struct AboutView {
    pub version: String,
    pub locale: String,
    /// How many signing keys this build trusts. Zero, in every published
    /// alpha, and the window says why rather than failing obscurely later
    /// (madde 30).
    pub trusted_keys: usize,
    /// The files this program writes, all beside the executable.
    pub files: Vec<String>,
    pub modules: usize,
}

// ------------------------------------------------------------------ building

/// Year and rating as a person reads them.
fn item_from_preview(preview: &MetaPreview, library: &Library, history: &WatchHistory) -> ItemView {
    let entry = history.get(&preview.content_type, &preview.id);
    ItemView {
        id: preview.id.clone(),
        content_type: preview.content_type.clone(),
        name: preview.display_name().to_owned(),
        poster: preview.poster.clone(),
        description: preview.description.clone(),
        year: preview.release_info.clone(),
        rating: preview.imdb_rating.clone(),
        in_library: library.contains(&preview.content_type, &preview.id),
        progress: entry.and_then(eon_stream_core::WatchEntry::progress),
        resume_at: entry
            .and_then(eon_stream_core::WatchEntry::resume_position)
            .map(clock),
        episode_label: None,
    }
}

/// Addons, in the viewer's order.
pub fn addons(session: &Session) -> Vec<AddonView> {
    session
        .registry
        .summaries()
        .into_iter()
        .map(|summary| AddonView {
            id: summary.id,
            name: summary.name,
            version: summary.version,
            description: summary.description,
            types: summary.types,
            resources: summary.resources,
            catalogs: summary.catalog_count,
            enabled: summary.enabled,
            needs_configuration: summary.configuration_required,
        })
        .collect()
}

/// The home screen's rows, in order, with the catalogue rows still empty.
pub fn rows(session: &Session) -> Vec<RowView> {
    let mut rows = Vec::new();

    let watching: Vec<ItemView> = session
        .history
        .continue_watching(12)
        .into_iter()
        .map(|entry| {
            // The history keeps no poster — it records what happened, not what
            // something looks like. The library often has one for the same
            // item, so it is borrowed when it is there and the card draws
            // without one when it is not.
            let poster = session
                .library
                .get(&entry.content_type, &entry.id)
                .and_then(|kept| kept.poster.clone())
                .or_else(|| {
                    entry.series_id.as_ref().and_then(|series| {
                        session
                            .library
                            .get("series", series)
                            .and_then(|kept| kept.poster.clone())
                    })
                });
            ItemView {
                id: entry.id.clone(),
                content_type: entry.content_type.clone(),
                name: entry.name.clone(),
                poster,
                description: None,
                year: None,
                rating: None,
                in_library: session.library.contains(&entry.content_type, &entry.id),
                progress: entry.progress(),
                resume_at: entry.resume_position().map(clock),
                episode_label: match (entry.season, entry.episode) {
                    (Some(season), Some(episode)) => Some(format!("S{season:02}E{episode:02}")),
                    _ => None,
                },
            }
        })
        .collect();
    if !watching.is_empty() {
        rows.push(RowView {
            kind: "continue".to_owned(),
            title: session.text("ui.row.continue"),
            addon_id: None,
            addon_name: None,
            catalog_id: None,
            content_type: None,
            searchable: false,
            items: watching,
        });
    }

    let kept: Vec<ItemView> = session
        .library
        .list(LibraryOrder::Recent)
        .into_iter()
        .take(24)
        .map(|entry| ItemView {
            id: entry.id.clone(),
            content_type: entry.content_type.clone(),
            name: entry.name.clone(),
            poster: entry.poster.clone(),
            description: None,
            year: entry.release_info.clone(),
            rating: None,
            in_library: true,
            progress: session
                .history
                .get(&entry.content_type, &entry.id)
                .and_then(eon_stream_core::WatchEntry::progress),
            resume_at: None,
            episode_label: None,
        })
        .collect();
    if !kept.is_empty() {
        rows.push(RowView {
            kind: "library".to_owned(),
            title: session.text("ui.row.library"),
            addon_id: None,
            addon_name: None,
            catalog_id: None,
            content_type: None,
            searchable: false,
            items: kept,
        });
    }

    for addon in session.registry.enabled() {
        let summary = addon.summary();
        for catalog in &addon.manifest.catalogs {
            rows.push(RowView {
                kind: "catalog".to_owned(),
                title: catalog_title(catalog, &summary.name),
                addon_id: Some(summary.id.clone()),
                addon_name: Some(summary.name.clone()),
                catalog_id: Some(catalog.id.clone()),
                content_type: Some(catalog.content_type.clone()),
                searchable: catalog.is_searchable(),
                items: Vec::new(),
            });
        }
    }
    rows
}

/// What to call a catalogue.
///
/// Addons in the wild leave `name` out, and `top` on its own tells a viewer
/// nothing. The type is appended because one addon offers "Top" for films and
/// "Top" for series and they are two different rows.
fn catalog_title(catalog: &Catalog, addon_name: &str) -> String {
    let base = catalog
        .name
        .clone()
        .unwrap_or_else(|| catalog.id.replace(['_', '-'], " "));
    // Only the catalogue's own name. The type and the addon are separate
    // fields of the row, and the interface names them in its own language --
    // `movie` joined into a title is a word nobody can translate.
    let _ = addon_name;
    base
}

/// One page of one catalogue.
pub fn row_items(
    session: &mut Session,
    addon_id: &str,
    content_type: &str,
    catalog_id: &str,
    skip: usize,
    query: Option<&str>,
) -> Result<Vec<ItemView>, String> {
    let addon = session
        .registry
        .get(addon_id)
        .ok_or_else(|| session.text("ui.error.noaddon"))?;

    let skip_text = skip.to_string();
    let mut extra: Vec<(&str, &str)> = Vec::new();
    if skip > 0 {
        extra.push(("skip", skip_text.as_str()));
    }
    if let Some(query) = query.filter(|q| !q.is_empty()) {
        extra.push(("search", query));
    }

    let previews = session
        .client
        .catalog(addon, content_type, catalog_id, &extra)
        .map_err(|e| e.to_string())?;

    let items = previews
        .iter()
        .map(|preview| item_from_preview(preview, &session.library, &session.history))
        .collect();
    // Kept so saving to the library has a poster and a name without another
    // request.
    session.previews.extend(previews);
    Ok(items)
}

/// One search box over every searchable catalogue of every enabled addon.
///
/// A query that comes back with almost nothing is tried again against titles
/// that exist — what is in the library, what has been watched — and the
/// interface is told what was typed so it can offer the literal query back.
/// Corrections are never invented: the candidate list is the vocabulary, so
/// this cannot turn a correctly spelled title into a different one.
pub fn search(session: &mut Session, query: &str) -> Result<SearchView, String> {
    let query = query.trim();
    if query.is_empty() {
        return Err(session.text("ui.error.emptyquery"));
    }
    if session.registry.enabled().count() == 0 {
        return Err(session.text("ui.error.noaddons"));
    }

    let merged = session.client.search_all(&session.registry, query);
    let failures = describe_failures(&merged.failures);
    let mut previews: Vec<MetaPreview> = merged.items.into_iter().flat_map(|(_, v)| v).collect();
    let mut corrected_from = None;

    // Three results is not a query that needs its spelling questioned; one or
    // none is.
    if previews.len() < 2 {
        // Scoped so the candidate list -- which borrows the library and the
        // history -- is gone before anything is written back.
        let suggestion = {
            let mut candidates: Vec<&str> = session.library.names();
            candidates.extend(session.history.names());
            eon_stream_core::suggest(query, &candidates)
        };
        if let Some(suggestion) = suggestion.filter(Suggestion::is_meaningful) {
            let retry = session
                .client
                .search_all(&session.registry, &suggestion.corrected);
            let retried: Vec<MetaPreview> = retry.items.into_iter().flat_map(|(_, v)| v).collect();
            if retried.len() > previews.len() {
                corrected_from = Some(query.to_owned());
                previews = retried;
            }
        }
    }

    let items = previews
        .iter()
        .map(|preview| item_from_preview(preview, &session.library, &session.history))
        .collect();
    session.previews.extend(previews);

    Ok(SearchView {
        query: query.to_owned(),
        corrected_from,
        items,
        failures,
    })
}

/// Everything about one item, from every addon that knows about it.
///
/// Remembered as the item being looked at, which is what playing a source and
/// keeping it in the library work from.
pub fn detail(session: &mut Session, content_type: &str, id: &str) -> Result<DetailView, String> {
    let (view, meta) = detail_of(session, content_type, id)?;
    session.meta = Some(meta);
    Ok(view)
}

/// The same, for the billboard on the home screen -- and *not* remembered.
/// The billboard is drawn while a viewer may already be opening something
/// else, and it must not become the item a source gets attributed to.
pub fn feature(session: &Session, content_type: &str, id: &str) -> Result<DetailView, String> {
    detail_of(session, content_type, id).map(|(view, _)| view)
}

fn detail_of(
    session: &Session,
    content_type: &str,
    id: &str,
) -> Result<(DetailView, eon_stream_core::Meta), String> {
    let (meta, _answered, failures) =
        session
            .client
            .meta_merged(&session.registry, content_type, id);
    let Some(meta) = meta else {
        let described = describe_failures(&failures);
        return Err(if described.is_empty() {
            session.text("ui.error.nometa")
        } else {
            described.join("\n")
        });
    };

    let seasons = meta.seasons();
    let episodes = meta
        .videos
        .iter()
        .map(|video| {
            let entry = session.history.get(content_type, &video.id);
            EpisodeView {
                id: video.id.clone(),
                title: video.display_title(),
                season: video.season,
                episode: video.episode,
                released: video.released.clone(),
                thumbnail: video.thumbnail.clone(),
                progress: entry.and_then(eon_stream_core::WatchEntry::progress),
                finished: entry.is_some_and(eon_stream_core::WatchEntry::is_finished),
            }
        })
        .collect();

    let view = DetailView {
        id: meta.id.clone(),
        content_type: meta.content_type.clone(),
        name: meta.display_name().to_owned(),
        description: meta.description.clone(),
        poster: meta.poster.clone(),
        background: meta.background.clone(),
        logo: meta.logo.clone(),
        year: meta.release_info.clone(),
        runtime: meta.runtime.clone(),
        genres: meta.genres.clone(),
        cast: meta.cast.clone(),
        director: meta.director.clone(),
        in_library: session.library.contains(content_type, id),
        resume_at: session.history.resume_position(content_type, id).map(clock),
        seasons,
        episodes,
        has_trailer: meta.trailer_url().is_some(),
        failures: describe_failures(&failures),
    };
    Ok((view, meta))
}

/// A language label as a code when the table knows it, and as written when it
/// does not -- a filter can still offer a language it has never heard of.
fn language_code(label: &str) -> Option<String> {
    let trimmed = label.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(language::canonical(trimmed).map_or_else(|| trimmed.to_lowercase(), ToOwned::to_owned))
}

/// Codes, in first-seen order, without repeats.
fn language_codes<'a>(labels: impl Iterator<Item = &'a str>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for code in labels.filter_map(language_code) {
        if !out.contains(&code) {
            out.push(code);
        }
    }
    out
}

/// Every source for one item, ranked by the viewer's preferences.
///
/// The ranked list stays in the session. What comes back is labels and
/// numbers, and `play(index)` is the only way to act on one.
pub fn sources(
    session: &mut Session,
    content_type: &str,
    id: &str,
    name: &str,
) -> Result<SourcesView, String> {
    if session.registry.enabled().count() == 0 {
        return Err(session.text("ui.error.noaddons"));
    }
    let merged = session
        .client
        .streams_from_all(&session.registry, content_type, id);
    let failures = describe_failures(&merged.failures);
    let ranked = rank(&merged.items, &session.settings.ranking);

    let names: std::collections::BTreeMap<String, String> = session
        .registry
        .summaries()
        .into_iter()
        .map(|summary| (summary.id, summary.name))
        .collect();
    let wanted = &session.settings.playback.subtitle_languages;

    let sources: Vec<SourceView> = ranked
        .iter()
        .enumerate()
        .map(|(index, source)| SourceView {
            index,
            label: source.stream.display_label(),
            addon_name: names
                .get(&source.addon_id)
                .cloned()
                .unwrap_or_else(|| source.addon_id.clone()),
            quality: quality_label(source),
            size: source.facts.size_bytes.map(format_size),
            seeders: source.facts.seeders,
            languages: language_codes(source.facts.audio_languages.iter().map(String::as_str)),
            subtitle_languages: language_codes(
                source
                    .stream
                    .subtitles
                    .iter()
                    .filter_map(|subtitle| subtitle.lang.as_deref()),
            ),
            is_torrent: matches!(source.stream.source(), Some(StreamSource::Torrent { .. })),
            has_subtitles: source.facts.has_subtitles || !source.stream.subtitles.is_empty(),
            // Whether this source speaks a language the viewer asked for, so
            // the interface can mark it. Through the language table, because
            // `tur` is not a prefix of `tr` and prefix matching was the bug.
            preferred_audio: language::matches_any(&source.facts.audio_languages.join(" "), wanted),
            needs_headers: source.facts.needs_headers,
        })
        .collect();

    let qualities = buckets(&ranked);
    session.sources = ranked;
    session.subject = Some((content_type.to_owned(), id.to_owned(), name.to_owned()));

    Ok(SourcesView {
        content_type: content_type.to_owned(),
        id: id.to_owned(),
        name: name.to_owned(),
        sources,
        qualities,
        failures,
    })
}

/// What to call a source's quality.
fn quality_label(source: &RankedStream) -> String {
    match source.facts.resolution {
        Some(Resolution::UltraHd) => "4K",
        Some(Resolution::QuadHd) => "1440p",
        Some(Resolution::FullHd) => "1080p",
        Some(Resolution::Hd) => "720p",
        Some(Resolution::Low) => "low",
        // Plenty of addons write no quality at all and those sources are often
        // fine. They get a bucket rather than being hidden.
        None => "—",
    }
    .to_owned()
}

/// Quality buckets present, best first, with how many sources each holds.
fn buckets(sources: &[RankedStream]) -> Vec<(String, usize)> {
    let mut out: Vec<(Option<Resolution>, String, usize)> = Vec::new();
    for source in sources {
        let resolution = source.facts.resolution;
        if let Some(found) = out.iter_mut().find(|(r, _, _)| *r == resolution) {
            found.2 += 1;
        } else {
            out.push((resolution, quality_label(source), 1));
        }
    }
    // `None` sorts below `Some`, so descending puts the unlabelled bucket last
    // rather than at the top where it would look like the best option.
    out.sort_by_key(|(resolution, _, _)| std::cmp::Reverse(*resolution));
    out.into_iter()
        .map(|(_, label, count)| (label, count))
        .collect()
}

/// Addon failures, as sentences a person can act on.
///
/// The addon's name and what went wrong. Never its address: that is where a
/// configuration credential lives (madde 22).
fn describe_failures(failures: &[eon_stream_core::AddonFailure]) -> Vec<String> {
    failures
        .iter()
        .map(|failure| format!("{}: {}", failure.addon_name, failure.error))
        .collect()
}

/// What is on screen that changes on its own.
pub fn status(session: &mut Session) -> StatusView {
    let job = session
        .job
        .lock()
        .map(|job| job.clone())
        .unwrap_or_default();

    let mut player = PlayerView::default();
    let mut seen = None;
    if let Some(running) = session.player.as_mut() {
        if running.is_running() {
            let position = running.position().ok().flatten();
            let duration = running.duration().ok().flatten();
            // Kept for when mpv closes itself at the end of the file: see
            // Session::last_seen.
            seen = position.map(|position| (position, duration));
            player = PlayerView {
                running: true,
                title: session
                    .playing
                    .as_ref()
                    .map_or_else(String::new, |p| p.name.clone()),
                paused: running
                    .get_property("pause")
                    .map(|value| value == "true")
                    .unwrap_or_default(),
                position,
                duration,
                position_clock: position.map(clock),
                duration_clock: duration.map(clock),
                buffered_seconds: running.buffered_secs().ok().flatten(),
                volume: running
                    .get_property("volume")
                    .ok()
                    .and_then(|value| value.parse().ok()),
                muted: running
                    .get_property("mute")
                    .map(|value| value == "true")
                    .unwrap_or_default(),
                speed: running
                    .get_property("speed")
                    .ok()
                    .and_then(|value| value.parse().ok()),
                buffering: running.is_buffering().unwrap_or_default(),
            };
        }
    }
    if seen.is_some() {
        session.last_seen = seen;
    }
    // The player has gone: remember where it got to, once, and stop asking.
    if !player.running && session.player.is_some() {
        session.remember_position();
        session.player = None;
        session.playing = None;
        session.last_seen = None;
    }

    StatusView {
        version: env!("CARGO_PKG_VERSION").to_owned(),
        locale: session
            .messages
            .as_ref()
            .map_or("none", eon_stream_core::i18n::Messages::locale)
            .to_owned(),
        job,
        player,
        torrents: crate::torrents::snapshot(session.torrent.as_deref()),
        addons: session.registry.len(),
        library: session.library.len(),
    }
}

/// The settings page, including the proof that nothing in it reports anywhere.
pub fn settings(session: &Session) -> SettingsView {
    let json = session
        .settings
        .to_json()
        .unwrap_or_else(|_| "{}".to_owned());
    let found = eon_stream_core::settings::find_forbidden_keys(&json).unwrap_or_default();
    SettingsView {
        settings: serde_json::from_str(&json).unwrap_or(serde_json::Value::Null),
        summary: session.settings.summary(),
        audit_clean: found.is_empty(),
        audit_found: found,
        languages: language::known().map(ToOwned::to_owned).collect(),
        buffer_bytes: crate::buffer::size(session),
    }
}

/// What this build is, and what it trusts.
pub fn about(session: &Session) -> AboutView {
    AboutView {
        version: env!("CARGO_PKG_VERSION").to_owned(),
        locale: session
            .messages
            .as_ref()
            .map_or("none", eon_stream_core::i18n::Messages::locale)
            .to_owned(),
        trusted_keys: session.trust.keys.len(),
        files: [
            "eon-addons.json",
            "eon-history.json",
            "eon-library.json",
            "eon-settings.json",
            "eon-modules.json",
            "eon-revocations.json",
            "eon-trust.json",
            "eon-torrents/",
        ]
        .iter()
        .map(|name| (*name).to_owned())
        .collect(),
        modules: session.modules.len(),
    }
}

/// Keep something, or stop keeping it.
pub fn library_save(session: &mut Session, content_type: &str, id: &str) -> Result<bool, String> {
    // From the last listing, which is where the poster and the name are. An
    // item nobody listed cannot be saved, which is not a case the window can
    // reach.
    let preview = session
        .previews
        .iter()
        .find(|p| p.content_type == content_type && p.id == id)
        .cloned()
        .or_else(|| {
            session
                .meta
                .as_ref()
                .filter(|m| m.id == id && m.content_type == content_type)
                .map(|m| MetaPreview {
                    id: m.id.clone(),
                    content_type: m.content_type.clone(),
                    name: m.name.clone(),
                    poster: m.poster.clone(),
                    poster_shape: None,
                    description: m.description.clone(),
                    release_info: m.release_info.clone(),
                    imdb_rating: None,
                })
        })
        .ok_or_else(|| session.text("ui.error.unknownitem"))?;

    let saved = session.library.save_preview(&preview, None, now_unix());
    session.save_library();
    Ok(saved)
}

/// Registry order, as a number the interface can hand back.
pub fn position_of(registry: &AddonRegistry, id: &str) -> Option<usize> {
    registry.addons().iter().position(|addon| addon.id() == id)
}

/// The audio and subtitle tracks of what is playing.
pub fn tracks(session: &mut Session) -> Result<Vec<TrackView>, String> {
    let Some(player) = session.player.as_mut() else {
        return Err(session.text("ui.error.noplayer"));
    };
    let tracks = player.tracks().map_err(|e| e.to_string())?;
    Ok(tracks
        .into_iter()
        .filter(|track| track.kind == "audio" || track.kind == "sub")
        .map(|track| TrackView {
            id: track.id,
            kind: track.kind,
            language: track.language.as_deref().and_then(language_code),
            title: track.title,
            selected: track.selected,
        })
        .collect())
}
