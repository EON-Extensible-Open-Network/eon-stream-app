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
    AddonClient, AddonFailure, AddonRegistry, InstalledAddon, Library, LibraryOrder, Merged, Meta,
    MetaPreview, Stream, Suggestion, WatchHistory,
};
use serde::Serialize;

use crate::{http::Http, state::Session};

/// What a request to the addons needs, copied out of the session.
///
/// Every command that waits on an addon works in three steps: copy this out
/// with the session locked, ask the addons with nothing locked, and fold the
/// answer back in with the session locked again. Holding the lock for the
/// whole request -- what this file used to do -- meant one slow addon
/// answering one row held up every other row, the billboard, and the status
/// poll that the loading screen and the player's controls are drawn from.
/// Both copies are cheap: the client is a handle on a shared connection pool,
/// and the registry is a few manifests.
pub struct Net {
    pub client: AddonClient<Http>,
    pub registry: AddonRegistry,
}

impl Net {
    pub fn of(session: &Session) -> Self {
        Self {
            client: session.client.clone(),
            registry: session.registry.clone(),
        }
    }
}

/// How many listed items are remembered for saving to the library. Paging a
/// few long rows would otherwise grow the list without end, and every save
/// walks it.
const PREVIEWS_KEPT: usize = 2_000;

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
#[derive(Serialize, Clone)]
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
    /// `trending` or `new`, when the catalogue's own id or name says it is
    /// one of those. Read from the addon's words, never decided here: this
    /// program does not rank or recommend anything itself.
    pub role: Option<String>,
    /// The one parameter a catalogue needs before it answers, and the value
    /// it is asked with -- the first the addon lists. Cinemeta's "New" needs
    /// a year.
    pub extra: Option<(String, String)>,
    /// The genres the catalogue can be narrowed to, as the addon lists them,
    /// for Discover. Empty when it lists none.
    pub genres: Vec<String>,
    /// Whether a genre must be picked: such a catalogue has no "all".
    pub genre_required: bool,
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
    /// Whether `title` is the episode's own name rather than `SxxEyy`, which
    /// is what an addon that names no episodes leaves.
    pub named: bool,
    pub overview: Option<String>,
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
    /// The same, as a number, for sorting by it.
    pub size_bytes: Option<u64>,
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
    /// At the end of the file, holding the last frame.
    pub ended: bool,
    /// The episode after this one, when there is one that has aired.
    pub next: Option<NextView>,
    /// Whether the next episode starts by itself.
    pub autoplay: bool,
}

/// The episode that comes next, for the player's "Next episode".
#[derive(Serialize, Clone)]
pub struct NextView {
    /// `S01E02`.
    pub label: String,
    /// The episode's own name, when the addon gives one.
    pub title: Option<String>,
}

/// The episode after the one playing: the next by season and episode, with
/// specials (season 0) only after everything else, and only once it has
/// aired. Answers its id and what to call it.
#[must_use]
pub fn next_episode(session: &Session) -> Option<(String, String)> {
    let (video, meta) = next_video(session)?;
    let label = episode_code(video);
    let name = match video.title.as_ref().or(video.name.as_ref()) {
        Some(title) if !title.trim().is_empty() => {
            format!("{} · {label} · {title}", meta.display_name())
        }
        _ => format!("{} · {label}", meta.display_name()),
    };
    Some((video.id.clone(), name))
}

fn next_video(session: &Session) -> Option<(&eon_stream_core::types::Video, &Meta)> {
    let playing = session.playing.as_ref()?;
    let series = playing.series_id.as_deref()?;
    let meta = session.meta.as_ref().filter(|meta| meta.id == series)?;
    let mut videos: Vec<&eon_stream_core::types::Video> = meta
        .videos
        .iter()
        .filter(|video| video.season.is_some() && video.episode.is_some())
        .collect();
    videos.sort_by_key(|video| (video.season == Some(0), video.season, video.episode));
    let at = videos.iter().position(|video| video.id == playing.id)?;
    let next = *videos.get(at + 1)?;
    let today = today();
    // An ISO date or timestamp compares as text: "2026-11-18T..." after
    // "2026-11-18" is later the same day, which counts as aired.
    let aired = next
        .released
        .as_deref()
        .is_none_or(|released| released.get(..10).unwrap_or(released) <= today.as_str());
    aired.then_some((next, meta))
}

fn episode_code(video: &eon_stream_core::types::Video) -> String {
    format!(
        "S{:02}E{:02}",
        video.season.unwrap_or_default(),
        video.episode.unwrap_or_default()
    )
}

/// Today, as `YYYY-MM-DD` in UTC -- the form addons write release dates in.
fn today() -> String {
    #[allow(clippy::cast_possible_wrap)]
    let days = (now_unix() / 86_400) as i64;
    // Howard Hinnant's days-to-civil.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}")
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
    /// Something the host has to tell the viewer, once: the cache limit was
    /// lowered to fit the disk.
    pub notice: Option<String>,
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
    /// Free space on the disk the cache is on, where it can be measured.
    pub disk_free_bytes: Option<u64>,
    /// The cache limit in force: the setting, or less if the disk has less.
    pub cache_limit_bytes: u64,
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
            role: None,
            extra: None,
            genres: Vec::new(),
            genre_required: false,
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
            role: None,
            extra: None,
            genres: Vec::new(),
            genre_required: false,
            items: kept,
        });
    }

    for addon in session.registry.enabled() {
        let summary = addon.summary();
        for catalog in &addon.manifest.catalogs {
            let Some(extra) = required_extra(catalog) else {
                continue;
            };
            rows.push(RowView {
                kind: "catalog".to_owned(),
                title: catalog_title(catalog, &summary.name),
                addon_id: Some(summary.id.clone()),
                addon_name: Some(summary.name.clone()),
                catalog_id: Some(catalog.id.clone()),
                content_type: Some(catalog.content_type.clone()),
                searchable: catalog.is_searchable(),
                role: role_of(catalog).map(ToOwned::to_owned),
                extra,
                genres: catalog
                    .extra
                    .iter()
                    .find(|prop| prop.name == "genre")
                    .map(|prop| prop.options.clone())
                    .unwrap_or_default(),
                genre_required: catalog
                    .extra
                    .iter()
                    .any(|prop| prop.name == "genre" && prop.is_required),
                items: Vec::new(),
            });
        }
    }
    rows
}

/// One episode on the calendar.
#[derive(Serialize, Clone)]
pub struct CalendarEpisode {
    pub series_id: String,
    pub series_name: String,
    pub poster: Option<String>,
    pub video_id: String,
    /// `S01E02`.
    pub label: String,
    /// The episode's own name, when the addon gives one.
    pub title: Option<String>,
    /// `YYYY-MM-DD`, as the addon dated it.
    pub date: String,
    pub aired: bool,
    pub watched: bool,
}

/// The episodes of the library's series around today, and the new ones as
/// home-screen cards.
#[derive(Serialize, Clone)]
pub struct CalendarView {
    pub episodes: Vec<CalendarEpisode>,
    /// Aired in the last fortnight and not watched, newest first: the home
    /// screen's "New episodes".
    pub fresh: Vec<ItemView>,
    pub failures: Vec<String>,
}

/// How far back and ahead the calendar looks, in days.
const CALENDAR_BACK_DAYS: i64 = 30;
const CALENDAR_AHEAD_DAYS: i64 = 60;
/// How recent an episode must be to count as new on the home screen.
const FRESH_DAYS: i64 = 14;
/// How many series are asked about. A library of hundreds is asked about
/// its most recently kept, rather than sending hundreds of requests.
const CALENDAR_SERIES: usize = 40;
/// How long an answer is reused before the addons are asked again.
const CALENDAR_KEEP_SECS: u64 = 30 * 60;

/// What the calendar asks for: the library's series, newest kept first.
pub fn calendar_request(session: &Session) -> (Net, Vec<(String, String, Option<String>)>) {
    let series = session
        .library
        .list(LibraryOrder::Recent)
        .into_iter()
        .filter(|entry| entry.content_type == "series")
        .take(CALENDAR_SERIES)
        .map(|entry| (entry.id.clone(), entry.name.clone(), entry.poster.clone()))
        .collect();
    (Net::of(session), series)
}

/// Ask for every series' episodes, eight at a time, with nothing locked.
pub fn fetch_calendar(
    net: &Net,
    series: &[(String, String, Option<String>)],
) -> Vec<(String, String, Option<String>, Fetched)> {
    let mut out = Vec::new();
    for chunk in series.chunks(8) {
        let answers: Vec<_> = std::thread::scope(|scope| {
            let handles: Vec<_> = chunk
                .iter()
                .map(|(id, _, _)| scope.spawn(move || fetch_meta(net, "series", id)))
                .collect();
            handles
                .into_iter()
                .map(|handle| handle.join().unwrap_or((None, Vec::new())))
                .collect()
        });
        for ((id, name, poster), fetched) in chunk.iter().zip(answers) {
            out.push((id.clone(), name.clone(), poster.clone(), fetched));
        }
    }
    out
}

/// Days from 1970-01-01 to a `YYYY-MM-DD` date.
fn days_of(date: &str) -> Option<i64> {
    let mut parts = date.get(..10)?.split('-');
    let year: i64 = parts.next()?.parse().ok()?;
    let month: i64 = parts.next()?.parse().ok()?;
    let day: i64 = parts.next()?.parse().ok()?;
    // Howard Hinnant's days-from-civil.
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let yoe = year - era * 400;
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some(era * 146_097 + doe - 719_468)
}

/// Fold the answers into the calendar, and remember it for a while.
pub fn calendar(
    session: &mut Session,
    answers: Vec<(String, String, Option<String>, Fetched)>,
) -> CalendarView {
    let today = days_of(&today()).unwrap_or_default();
    let mut episodes = Vec::new();
    let mut failures = Vec::new();
    for (series_id, series_name, poster, (meta, failed)) in answers {
        failures.extend(describe_failures(&failed));
        let Some(meta) = meta else { continue };
        let poster = poster.or_else(|| meta.poster.clone());
        for video in &meta.videos {
            let (Some(_), Some(_), Some(released)) = (video.season, video.episode, &video.released)
            else {
                continue;
            };
            let Some(day) = days_of(released) else {
                continue;
            };
            if day < today - CALENDAR_BACK_DAYS || day > today + CALENDAR_AHEAD_DAYS {
                continue;
            }
            episodes.push(CalendarEpisode {
                series_id: series_id.clone(),
                series_name: series_name.clone(),
                poster: poster.clone(),
                video_id: video.id.clone(),
                label: episode_code(video),
                title: video
                    .title
                    .as_ref()
                    .or(video.name.as_ref())
                    .filter(|title| !title.trim().is_empty())
                    .cloned(),
                date: released.get(..10).unwrap_or(released).to_owned(),
                aired: day <= today,
                watched: session
                    .history
                    .get("series", &video.id)
                    .is_some_and(eon_stream_core::WatchEntry::is_finished),
            });
        }
    }
    episodes.sort_by(|a, b| a.date.cmp(&b.date).then(a.series_name.cmp(&b.series_name)));
    // One card per series: its newest episode not yet watched.
    let mut carded = std::collections::HashSet::new();
    let fresh = episodes
        .iter()
        .rev()
        .filter(|episode| {
            episode.aired
                && !episode.watched
                && days_of(&episode.date).is_some_and(|day| day >= today - FRESH_DAYS)
        })
        .filter(|episode| carded.insert(episode.series_id.clone()))
        .map(|episode| ItemView {
            id: episode.series_id.clone(),
            content_type: "series".to_owned(),
            name: episode.series_name.clone(),
            poster: episode.poster.clone(),
            description: episode.title.clone(),
            year: Some(episode.date.clone()),
            rating: None,
            in_library: true,
            progress: None,
            resume_at: None,
            episode_label: Some(episode.label.clone()),
        })
        .collect();
    let view = CalendarView {
        episodes,
        fresh,
        failures,
    };
    session.calendar = Some((now_unix(), library_mark(session), view.clone()));
    view
}

/// The calendar remembered, while it is recent and the library has not
/// changed since.
pub fn calendar_kept(session: &Session) -> Option<CalendarView> {
    let (when, mark, view) = session.calendar.as_ref()?;
    (now_unix().saturating_sub(*when) < CALENDAR_KEEP_SECS && *mark == library_mark(session))
        .then(|| view.clone())
}

/// Which series are kept, as one string: the calendar is asked again when it
/// changes.
fn library_mark(session: &Session) -> String {
    session
        .library
        .list(LibraryOrder::Recent)
        .into_iter()
        .filter(|entry| entry.content_type == "series")
        .take(CALENDAR_SERIES)
        .map(|entry| entry.id.as_str())
        .collect::<Vec<_>>()
        .join(",")
}

/// More of a genre, for a title's page.
#[derive(Serialize)]
pub struct MoreView {
    /// The genre, as the addon wrote it.
    pub genre: String,
    pub items: Vec<ItemView>,
}

/// Which catalogue to ask for more of an item's genre: the first catalogue of
/// the same type, on an enabled addon, that can be narrowed to that genre.
/// Tried for each of the item's genres in order. `None` when no addon can.
///
/// This is not a recommendation. The addon is asked for its own catalogue,
/// narrowed the way the addon says it can be; nothing here decides what is
/// "like" anything (madde 9).
pub fn more_request(
    session: &Session,
    content_type: &str,
    id: &str,
) -> Result<Option<(String, PageRequest)>, String> {
    let Some(meta) = session.meta.as_ref().filter(|meta| meta.id == id) else {
        return Ok(None);
    };
    for genre in &meta.genres {
        for addon in session.registry.enabled() {
            for catalog in &addon.manifest.catalogs {
                if catalog.content_type != content_type {
                    continue;
                }
                let offers = catalog.extra.iter().any(|prop| {
                    prop.name == "genre"
                        && (prop.options.is_empty() || prop.options.iter().any(|g| g == genre))
                });
                // A catalogue that needs some other parameter first is not
                // one this can ask.
                let other_required = catalog
                    .extra
                    .iter()
                    .any(|prop| prop.is_required && prop.name != "genre");
                if !offers || other_required {
                    continue;
                }
                let request = page_request(
                    session,
                    addon.id(),
                    content_type,
                    &catalog.id,
                    0,
                    None,
                    Some(("genre".to_owned(), genre.clone())),
                )?;
                return Ok(Some((genre.clone(), request)));
            }
        }
    }
    Ok(None)
}

/// Everything kept, in the order asked for: `recent` or `name`.
///
/// The home screen's row shows the newest two dozen; this is the whole
/// library, for its own page.
#[must_use]
pub fn library(session: &Session, order: &str) -> Vec<ItemView> {
    let order = if order == "name" {
        LibraryOrder::Name
    } else {
        LibraryOrder::Recent
    };
    session
        .library
        .list(order)
        .into_iter()
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
        .collect()
}

/// The parameter a catalogue needs before it answers, if any.
///
/// `Some(None)` is a catalogue that needs nothing. A catalogue that needs one
/// parameter and lists its values is asked with the first of them. One that
/// needs a value nobody listed -- the ids of the series someone follows, say
/// -- cannot be asked at all, and is `None`: left off the screen rather than
/// drawn as a row that only ever shows an error.
fn required_extra(catalog: &Catalog) -> Option<Option<(String, String)>> {
    let mut extra = None;
    for prop in catalog.extra.iter().filter(|prop| prop.is_required) {
        match (prop.options.first(), &extra) {
            (Some(value), None) => extra = Some((prop.name.clone(), value.clone())),
            _ => return None,
        }
    }
    Some(extra)
}

/// Whether a catalogue calls itself trending or new, in its own id or name.
fn role_of(catalog: &Catalog) -> Option<&'static str> {
    let words = format!(
        "{} {}",
        catalog.id,
        catalog.name.as_deref().unwrap_or_default()
    )
    .to_lowercase();
    let has = |list: &[&str]| {
        words
            .split(|c: char| !c.is_alphanumeric())
            .any(|word| list.contains(&word))
    };
    if has(&["top", "popular", "trending", "trend", "hot"]) {
        Some("trending")
    } else if has(&["new", "year", "latest", "recent", "releases"]) {
        Some("new")
    } else {
        None
    }
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

/// One catalogue page, ready to ask for.
pub struct PageRequest {
    client: AddonClient<Http>,
    addon: InstalledAddon,
    content_type: String,
    catalog_id: String,
    params: Vec<(String, String)>,
}

/// Step one of a catalogue page: what to ask, and of whom.
///
/// A parameter the interface passes is used only when the catalogue declares
/// it, and with a value from the catalogue's own list when it has one: the
/// interface chooses among what the addon offers, it does not write queries.
pub fn page_request(
    session: &Session,
    addon_id: &str,
    content_type: &str,
    catalog_id: &str,
    skip: usize,
    query: Option<&str>,
    extra: Option<(String, String)>,
) -> Result<PageRequest, String> {
    let addon = session
        .registry
        .get(addon_id)
        .cloned()
        .ok_or_else(|| session.text("ui.error.noaddon"))?;

    let mut params: Vec<(String, String)> = Vec::new();
    if let Some((name, value)) = extra {
        let declared = addon
            .manifest
            .catalogs
            .iter()
            .find(|c| c.id == catalog_id && c.content_type == content_type)
            .and_then(|c| c.extra.iter().find(|prop| prop.name == name))
            .is_some_and(|prop| prop.options.is_empty() || prop.options.contains(&value));
        if declared {
            params.push((name, value));
        }
    }
    if skip > 0 {
        params.push(("skip".to_owned(), skip.to_string()));
    }
    if let Some(query) = query.filter(|q| !q.is_empty()) {
        params.push(("search".to_owned(), query.to_owned()));
    }
    Ok(PageRequest {
        client: session.client.clone(),
        addon,
        content_type: content_type.to_owned(),
        catalog_id: catalog_id.to_owned(),
        params,
    })
}

/// Step two: ask, with nothing locked.
pub fn fetch_page(request: &PageRequest) -> Result<Vec<MetaPreview>, String> {
    let extra: Vec<(&str, &str)> = request
        .params
        .iter()
        .map(|(name, value)| (name.as_str(), value.as_str()))
        .collect();
    request
        .client
        .catalog(
            &request.addon,
            &request.content_type,
            &request.catalog_id,
            &extra,
        )
        .map_err(|e| e.to_string())
}

/// Step three: cards, and the listing remembered for saving to the library.
pub fn page_items(session: &mut Session, previews: Vec<MetaPreview>) -> Vec<ItemView> {
    let items = previews
        .iter()
        .map(|preview| item_from_preview(preview, &session.library, &session.history))
        .collect();
    remember(session, previews);
    items
}

/// Keep a listing, so saving to the library has a poster and a name without
/// another request -- the newest listings, up to [`PREVIEWS_KEPT`].
fn remember(session: &mut Session, previews: Vec<MetaPreview>) {
    session.previews.extend(previews);
    let over = session.previews.len().saturating_sub(PREVIEWS_KEPT);
    if over > 0 {
        session.previews.drain(..over);
    }
}

/// One search box over every searchable catalogue of every enabled addon,
/// ready to ask.
///
/// A query that comes back with almost nothing is tried again against titles
/// that exist — what is in the library, what has been watched — and the
/// interface is told what was typed so it can offer the literal query back.
/// Corrections are never invented: the candidate list is the vocabulary, so
/// this cannot turn a correctly spelled title into a different one.
pub struct SearchRequest {
    net: Net,
    query: String,
    vocabulary: Vec<String>,
}

/// What a search found, before it is turned into cards.
pub struct Found {
    query: String,
    corrected_from: Option<String>,
    previews: Vec<MetaPreview>,
    failures: Vec<String>,
}

/// Step one of a search.
pub fn search_request(session: &Session, query: &str) -> Result<SearchRequest, String> {
    let query = query.trim();
    if query.is_empty() {
        return Err(session.text("ui.error.emptyquery"));
    }
    if session.registry.enabled().count() == 0 {
        return Err(session.text("ui.error.noaddons"));
    }
    let mut vocabulary: Vec<String> = session
        .library
        .names()
        .into_iter()
        .map(ToOwned::to_owned)
        .collect();
    vocabulary.extend(session.history.names().into_iter().map(ToOwned::to_owned));
    Ok(SearchRequest {
        net: Net::of(session),
        query: query.to_owned(),
        vocabulary,
    })
}

/// Step two: ask, with nothing locked -- twice, when the spelling is in doubt.
pub fn search_fetch(request: &SearchRequest) -> Found {
    let net = &request.net;
    let merged = net.client.search_all(&net.registry, &request.query);
    let failures = describe_failures(&merged.failures);
    let mut previews: Vec<MetaPreview> = merged.items.into_iter().flat_map(|(_, v)| v).collect();
    let mut query = request.query.clone();
    let mut corrected_from = None;

    // Three results is not a query that needs its spelling questioned; one or
    // none is.
    if previews.len() < 2 {
        let candidates: Vec<&str> = request.vocabulary.iter().map(String::as_str).collect();
        let suggestion = eon_stream_core::suggest(&request.query, &candidates);
        if let Some(suggestion) = suggestion.filter(Suggestion::is_meaningful) {
            let retry = net.client.search_all(&net.registry, &suggestion.corrected);
            let retried: Vec<MetaPreview> = retry.items.into_iter().flat_map(|(_, v)| v).collect();
            if retried.len() > previews.len() {
                corrected_from = Some(request.query.clone());
                query = suggestion.corrected;
                previews = retried;
            }
        }
    }
    Found {
        query,
        corrected_from,
        previews,
        failures,
    }
}

/// Step three: cards.
pub fn search_view(session: &mut Session, found: Found) -> SearchView {
    let items = found
        .previews
        .iter()
        .map(|preview| item_from_preview(preview, &session.library, &session.history))
        .collect();
    remember(session, found.previews);
    SearchView {
        query: found.query,
        corrected_from: found.corrected_from,
        items,
        failures: found.failures,
    }
}

/// What every addon that knows about one item says about it.
pub type Fetched = (Option<Meta>, Vec<AddonFailure>);

/// Step two of `detail` and `feature`: ask, with nothing locked.
pub fn fetch_meta(net: &Net, content_type: &str, id: &str) -> Fetched {
    let (meta, _answered, failures) = net.client.meta_merged(&net.registry, content_type, id);
    (meta, failures)
}

/// Step three of `detail`: the page, and the item remembered as the one being
/// looked at, which is what playing a source and keeping it in the library
/// work from.
pub fn detail(
    session: &mut Session,
    content_type: &str,
    id: &str,
    fetched: Fetched,
) -> Result<DetailView, String> {
    let (view, meta) = detail_of(session, content_type, id, fetched)?;
    session.meta = Some(meta);
    Ok(view)
}

/// The same, for the billboard on the home screen -- and *not* remembered.
/// The billboard is drawn while a viewer may already be opening something
/// else, and it must not become the item a source gets attributed to.
pub fn feature(
    session: &Session,
    content_type: &str,
    id: &str,
    fetched: Fetched,
) -> Result<DetailView, String> {
    detail_of(session, content_type, id, fetched).map(|(view, _)| view)
}

fn detail_of(
    session: &Session,
    content_type: &str,
    id: &str,
    (meta, failures): Fetched,
) -> Result<(DetailView, Meta), String> {
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
                named: [&video.title, &video.name]
                    .into_iter()
                    .flatten()
                    .any(|title| !title.trim().is_empty()),
                overview: video
                    .overview
                    .clone()
                    .filter(|text| !text.trim().is_empty()),
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

/// Step one of a source list: who to ask, and a number saying this is now the
/// newest request -- see [`sources`].
pub fn sources_request(session: &mut Session) -> Result<(Net, u64), String> {
    if session.registry.enabled().count() == 0 {
        return Err(session.text("ui.error.noaddons"));
    }
    session.sources_wanted += 1;
    Ok((Net::of(session), session.sources_wanted))
}

/// Step two: ask, with nothing locked.
pub fn fetch_sources(net: &Net, content_type: &str, id: &str) -> Merged<Stream> {
    net.client.streams_from_all(&net.registry, content_type, id)
}

/// Step three: every source for one item, ranked by the viewer's preferences.
///
/// The ranked list stays in the session. What comes back is labels and
/// numbers, and `play(index)` is the only way to act on one -- which is why
/// only the newest request may replace the list. Two lists asked for in quick
/// succession can come back in either order, and an index the window read
/// off one list must never be played from the other.
pub fn sources(
    session: &mut Session,
    content_type: &str,
    id: &str,
    name: &str,
    merged: &Merged<Stream>,
    request: u64,
) -> Result<SourcesView, String> {
    if request != session.sources_wanted {
        return Err(session.text("ui.error.superseded"));
    }
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
            size_bytes: source.facts.size_bytes,
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

    let next = next_video(session).map(|(video, _)| NextView {
        label: episode_code(video),
        title: video
            .title
            .as_ref()
            .or(video.name.as_ref())
            .filter(|title| !title.trim().is_empty())
            .cloned(),
    });
    let mut player = PlayerView::default();
    let mut seen = None;
    if let Some(running) = session.player.as_mut() {
        if running.is_running() {
            let position = running.position().ok().flatten();
            let duration = running.duration().ok().flatten();
            // Kept for when mpv closes itself at the end of the file: see
            // Session::last_seen.
            seen = position.map(|position| (position, duration));
            let paused = running
                .get_property("pause")
                .map(|value| value == "true")
                .unwrap_or_default();
            // Waiting on the network: mpv says so once it has started and
            // runs dry (`paused-for-cache`). Before the first frame it does
            // not -- it is idle, not paused by anyone, and that is the wait a
            // stalled torrent leaves someone staring at.
            let buffering = running.is_buffering().unwrap_or_default()
                || (!paused
                    && running
                        .get_property("core-idle")
                        .is_ok_and(|value| value == "true"));
            player = PlayerView {
                running: true,
                title: session
                    .playing
                    .as_ref()
                    .map_or_else(String::new, |p| p.name.clone()),
                paused,
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
                buffering,
                ended: running
                    .get_property("eof-reached")
                    .is_ok_and(|value| value == "true"),
                next,
                autoplay: session.settings.playback.autoplay_next,
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
        notice: session.keeper.notice.take(),
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
        disk_free_bytes: crate::buffer::free(session),
        cache_limit_bytes: crate::keeper::budget(
            session.settings.torrent.cache_limit_megabytes,
            crate::buffer::free(session),
            crate::buffer::size(session),
        )
        .0,
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
            "eon-subtitles/",
            "eon-webview/",
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

#[cfg(test)]
mod tests {
    use super::*;

    fn catalog(json: &str) -> Catalog {
        serde_json::from_str(json).expect("a catalogue")
    }

    #[test]
    fn a_catalogue_that_needs_a_listed_value_is_asked_with_the_first() {
        // Cinemeta's "New": a year is required, and this year is listed first.
        let new = catalog(
            r#"{"type":"movie","id":"year","name":"New","extra":[
                {"name":"genre","isRequired":true,"options":["2026","2025"]},
                {"name":"skip"}]}"#,
        );
        assert_eq!(
            required_extra(&new),
            Some(Some(("genre".to_owned(), "2026".to_owned())))
        );
        assert_eq!(role_of(&new), Some("new"));
    }

    #[test]
    fn a_catalogue_that_needs_an_unlisted_value_is_left_out() {
        let followed = catalog(
            r#"{"type":"series","id":"last-videos","name":"Last videos","extra":[
                {"name":"lastVideosIds","isRequired":true}]}"#,
        );
        assert_eq!(required_extra(&followed), None);
    }

    #[test]
    fn a_catalogue_that_needs_nothing_is_asked_as_it_is() {
        let popular = catalog(r#"{"type":"movie","id":"top","name":"Popular"}"#);
        assert_eq!(required_extra(&popular), Some(None));
        assert_eq!(role_of(&popular), Some("trending"));
        let featured = catalog(r#"{"type":"movie","id":"imdbRating","name":"Featured"}"#);
        assert_eq!(role_of(&featured), None);
    }
}

#[cfg(test)]
mod date_tests {
    #[test]
    fn today_is_a_date() {
        let today = super::today();
        assert_eq!(today.len(), 10);
        assert!(today.as_str() > "2026-01-01");
    }

    #[test]
    fn a_date_counts_days_from_1970() {
        assert_eq!(super::days_of("1970-01-01"), Some(0));
        assert_eq!(super::days_of("2000-03-01T00:00:00.000Z"), Some(11_017));
        assert_eq!(
            super::days_of(&super::today()),
            Some(i64::try_from(super::now_unix() / 86_400).unwrap_or_default())
        );
        assert_eq!(super::days_of("not a date"), None);
    }
}
