// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 EON contributors
//
// Additional permission under GNU GPL version 3 section 7:
// see LICENSE-EXCEPTION.md (EON Module ABI Exception 1.0).

//! Subtitles, found rather than hoped for.
//!
//! Almost no source says which subtitles it carries, so a filter over what
//! sources say finds nothing. This looks instead, every time something
//! starts, in the three places a subtitle actually is:
//!
//! 1. **Inside the video.** mpv already offers those tracks; the only help it
//!    needs is being told which languages to pick by default (`--slang`).
//! 2. **Beside the video, in the same torrent.** Releases ship `.srt` files
//!    next to the film more often than they say so. They are served by the
//!    loopback server like the film itself.
//! 3. **From a subtitle addon** the viewer installed -- told the file's name
//!    and size, which is what turns "forty tracks for this film" into "the
//!    tracks for this release". No addon is built in (madde 9); with none
//!    installed this source is simply empty, and the player says so.
//!
//! What is found is listed in the player's subtitle menu. The best few in the
//! viewer's languages are loaded straight away, and one of them switched on
//! when that is what the viewer would want.
//!
//! A subtitle is fetched here, written to `eon-subtitles/` beside the
//! executable, and only then handed to mpv as a file. Handing mpv the URL
//! would be shorter, but mpv loads a subtitle on its playback thread and
//! answers only once it has: a slow subtitle server -- or a torrent piece not
//! yet downloaded -- would freeze the picture *and* hold the host's lock for
//! as long. The folder is this program's own and is emptied at every start.

use std::path::PathBuf;
use std::sync::Arc;

use eon_stream_core::http::HttpClient as _;
use eon_stream_core::{language, SubtitleMatch};
use eon_stream_engine::{TorrentEngine, TorrentHandle};
use serde::Serialize;

use crate::commands::Shared;
use crate::state::{beside_exe, Session};
use crate::view::Net;

/// Where fetched subtitles are written, beside the executable.
const FOLDER: &str = "eon-subtitles";

/// Extensions mpv reads as a subtitle on its own. `.sub` is left out: it is
/// usually half of a VobSub pair and useless without its `.idx`.
const EXTENSIONS: &[&str] = &["srt", "ass", "ssa", "vtt"];

/// How many tracks per wanted language are loaded before anyone asks. One is
/// usually right; a second covers the release whose timing the first misses.
const LOADED_PER_LANGUAGE: usize = 2;

/// Subtitles inside a torrent loaded without being asked for. A season pack
/// can carry hundreds.
const LOADED_FROM_TORRENT: usize = 4;

/// Where an offer came from.
#[derive(Clone)]
pub enum Origin {
    /// A file inside the torrent being played, by name.
    Torrent(String),
    /// A subtitle addon, by name.
    Addon(String),
    /// A file the viewer chose.
    File(String),
}

impl Origin {
    fn label(&self) -> &str {
        match self {
            Self::Torrent(name) | Self::Addon(name) | Self::File(name) => name,
        }
    }

    fn kind(&self) -> &'static str {
        match self {
            Self::Torrent(_) => "torrent",
            Self::Addon(_) => "addon",
            Self::File(_) => "file",
        }
    }
}

/// One subtitle that could be shown.
#[derive(Clone)]
pub struct Offer {
    /// A two-letter code when it could be told.
    pub language: Option<String>,
    pub origin: Origin,
    /// Where to fetch it. Host-side only: an addon's URL can carry its
    /// configuration, and a loopback URL carries the session token.
    url: String,
    /// The extension to write it under.
    extension: String,
    pub loaded: bool,
}

/// Everything found for one playback.
#[derive(Default)]
pub struct Found {
    /// The playback these belong to: the start's ticket.
    pub ticket: u64,
    pub searching: bool,
    /// How many enabled addons offer subtitles. Zero is worth saying in the
    /// menu, because it is the reason nothing came from that direction.
    pub addons: usize,
    pub offers: Vec<Offer>,
}

/// One offer, as the menu shows it.
#[derive(Serialize)]
pub struct OfferView {
    pub index: usize,
    pub language: Option<String>,
    /// `torrent`, `addon` or `file`.
    pub kind: &'static str,
    /// The addon's name or the file's.
    pub origin: String,
    pub loaded: bool,
}

#[derive(Serialize)]
pub struct SubtitlesView {
    pub searching: bool,
    pub addons: usize,
    pub offers: Vec<OfferView>,
}

/// The subtitle menu's contents.
#[must_use]
pub fn view(session: &Session) -> SubtitlesView {
    let found = &session.subtitles;
    SubtitlesView {
        searching: found.searching,
        addons: found.addons,
        offers: found
            .offers
            .iter()
            .enumerate()
            .map(|(index, offer)| OfferView {
                index,
                language: offer.language.clone(),
                kind: offer.origin.kind(),
                origin: offer.origin.label().to_owned(),
                loaded: offer.loaded,
            })
            .collect(),
    }
}

/// Subtitle languages on offer for one item, from the viewer's addons.
#[derive(Serialize)]
pub struct LanguagesView {
    /// How many enabled addons offer subtitles for this type.
    pub addons: usize,
    /// Two-letter code (or the label as written) and how many tracks.
    pub languages: Vec<(String, usize)>,
}

/// Ask every subtitle addon about one item, with nothing locked.
#[must_use]
pub fn languages(net: &Net, content_type: &str, id: &str) -> LanguagesView {
    let addons = net
        .registry
        .enabled()
        .filter(|addon| {
            addon
                .manifest
                .resource_for("subtitles", content_type)
                .is_some()
        })
        .count();
    if addons == 0 {
        return LanguagesView {
            addons,
            languages: Vec::new(),
        };
    }
    let merged = net
        .client
        .subtitles_from_all(&net.registry, content_type, id);
    let mut counts: Vec<(String, usize)> = Vec::new();
    for subtitle in merged.items.into_iter().flat_map(|(_, list)| list) {
        let Some(code) = subtitle.lang.as_deref().map(code_of) else {
            continue;
        };
        match counts.iter_mut().find(|(c, _)| *c == code) {
            Some((_, n)) => *n += 1,
            None => counts.push((code, 1)),
        }
    }
    counts.sort_by_key(|entry| std::cmp::Reverse(entry.1));
    LanguagesView {
        addons,
        languages: counts,
    }
}

/// What a search for one playback starts from.
pub struct Search {
    pub ticket: u64,
    /// Type and id of what is playing -- the episode's id for a series.
    pub subject: Option<(String, String)>,
    pub filename: Option<String>,
    pub video_hash: Option<String>,
    pub size: Option<u64>,
    pub torrent: Option<(Arc<TorrentEngine>, TorrentHandle)>,
    /// Which file of the torrent is playing.
    pub file_index: Option<usize>,
    /// Which source of the current list, and which file was asked for: what
    /// the cache keeper restarts when a seek goes back past what it gave
    /// back.
    pub source: Option<(usize, Option<usize>)>,
}

/// The languages a viewer wants subtitles in, best first, and whether one
/// should be switched on without asking.
///
/// The setting when there is one. Without it, the interface's language --
/// someone who reads the interface in Turkish wants Turkish subtitles on a
/// film in English -- but not English, because an English interface says
/// nothing about wanting subtitles on an English film.
#[must_use]
pub fn wanted(session: &Session) -> (Vec<String>, bool) {
    let set = &session.settings.playback.subtitle_languages;
    if !set.is_empty() {
        return (set.iter().map(|code| code_of(code)).collect(), true);
    }
    let interface = code_of(&session.settings.language);
    let on = interface != "en";
    (vec![interface], on)
}

/// The arguments that make mpv pick an embedded subtitle in a wanted
/// language by itself, and an audio track in a wanted language when a
/// release carries several.
#[must_use]
pub fn mpv_arguments(session: &Session) -> Vec<String> {
    let mut out = Vec::new();
    let (languages, on) = wanted(session);
    if on && !languages.is_empty() {
        out.push(format!("--slang={}", spellings(&languages).join(",")));
    }
    let audio = &session.settings.ranking.preferred_audio_languages;
    if !audio.is_empty() {
        let audio: Vec<String> = audio.iter().map(|code| code_of(code)).collect();
        out.push(format!("--alang={}", spellings(&audio).join(",")));
    }
    out
}

/// Every way a file is likely to label these languages: `tr` and `tur`, `de`,
/// `ger` and `deu`. mpv compares the label as written.
fn spellings(codes: &[String]) -> Vec<String> {
    const THREE: &[&str] = &[
        "eng", "tur", "ger", "deu", "fre", "fra", "spa", "ita", "por", "rus", "jpn", "kor", "chi",
        "zho", "ara", "dut", "nld", "pol", "swe", "nor", "dan", "fin", "gre", "ell", "heb", "hin",
        "hun", "cze", "ces", "rum", "ron", "bul", "ukr", "per", "fas", "tha", "vie", "ind", "may",
        "msa", "hrv", "srp", "slv", "slk", "slo", "aze", "kur",
    ];
    let mut out: Vec<String> = Vec::new();
    for code in codes {
        out.push(code.clone());
        for three in THREE {
            if language::canonical(three) == Some(code.as_str()) {
                out.push((*three).to_owned());
            }
        }
    }
    out.dedup();
    out
}

fn code_of(label: &str) -> String {
    language::canonical(label).map_or_else(|| label.trim().to_lowercase(), ToOwned::to_owned)
}

/// A language read out of a file name: `Movie.2019.tr.srt`,
/// `Subs/3_Turkish.srt`, `English(SDH).srt`.
fn language_in_name(path: &str) -> Option<String> {
    let lower = path.to_lowercase();
    let stem = lower
        .rsplit_once('.')
        .map_or(lower.as_str(), |(stem, _)| stem);
    // From the end: the language is almost always the last word, and a title
    // word early in the name ("The Italian Job") must not win.
    stem.split(|c: char| !c.is_alphanumeric())
        .rev()
        .filter(|word| word.len() >= 2)
        .find_map(|word| language::canonical(word).map(ToOwned::to_owned))
}

fn extension_of(path: &str) -> Option<String> {
    let name = path.rsplit(['/', '\\']).next().unwrap_or(path);
    let (_, extension) = name.rsplit_once('.')?;
    let extension = extension.to_ascii_lowercase();
    EXTENSIONS
        .contains(&extension.as_str())
        .then_some(extension)
}

/// The folder fetched subtitles go in.
fn folder() -> PathBuf {
    beside_exe(FOLDER)
}

/// Empty the folder. It is this program's own: nothing else writes there.
pub fn clear_folder() {
    let folder = folder();
    if folder.exists() {
        let _ = std::fs::remove_dir_all(folder);
    }
}

/// Start over for a new playback.
pub fn reset(session: &mut Session, ticket: u64) {
    session.subtitles = Found {
        ticket,
        searching: true,
        ..Found::default()
    };
    clear_folder();
}

/// Look everywhere, then load the best few. Runs on its own thread, after
/// the player is up: nothing here is worth delaying the picture for.
pub fn find(shared: &Shared, search: Search) {
    let ticket = search.ticket;

    // 1. Beside the video in the torrent.
    let mut offers: Vec<Offer> = Vec::new();
    if let Some((engine, handle)) = &search.torrent {
        for file in handle.files() {
            let Some(extension) = extension_of(&file.path) else {
                continue;
            };
            let Ok(url) = engine.stream_url(handle, file.index) else {
                continue;
            };
            offers.push(Offer {
                language: language_in_name(&file.path),
                origin: Origin::Torrent(file.name().to_owned()),
                url,
                extension,
                loaded: false,
            });
        }
    }

    // 2. From the viewer's subtitle addons.
    let Some((net, languages, choose)) = with(shared, ticket, |session| {
        let (languages, choose) = wanted(session);
        (Net::of(session), languages, choose)
    }) else {
        return;
    };
    let content_type = search.subject.as_ref().map(|(t, _)| t.as_str());
    let addons: Vec<_> = net
        .registry
        .enabled()
        .filter(|addon| {
            content_type.is_some_and(|t| addon.manifest.resource_for("subtitles", t).is_some())
        })
        .map(|addon| (addon.id().to_owned(), addon.summary().name))
        .collect();
    let mut from_addons: Vec<Offer> = Vec::new();
    if let (Some((content_type, id)), false) = (&search.subject, addons.is_empty()) {
        let matching = SubtitleMatch {
            filename: search.filename.as_deref(),
            video_hash: search.video_hash.as_deref(),
            video_size: search.size,
        };
        let merged =
            net.client
                .subtitles_for_source_from_all(&net.registry, content_type, id, matching);
        for (addon_id, subtitles) in merged.items {
            let name = addons
                .iter()
                .find(|(id, _)| *id == addon_id)
                .map_or_else(|| addon_id.clone(), |(_, name)| name.clone());
            let mut seen = std::collections::HashSet::new();
            for subtitle in subtitles {
                // Some addons list the same file twice under two ids.
                if !seen.insert(subtitle.url.clone()) {
                    continue;
                }
                from_addons.push(Offer {
                    language: subtitle.lang.as_deref().map(code_of),
                    origin: Origin::Addon(name.clone()),
                    extension: extension_of(&subtitle.url).unwrap_or_else(|| "srt".to_owned()),
                    url: subtitle.url,
                    loaded: false,
                });
            }
        }
    }
    // The viewer's languages first, in their order; within one language, the
    // addon's own order, which is its idea of best.
    let rank = |offer: &Offer| {
        offer
            .language
            .as_ref()
            .and_then(|code| languages.iter().position(|wanted| wanted == code))
            .unwrap_or(usize::MAX)
    };
    from_addons.sort_by_key(rank);
    offers.sort_by_key(rank);
    offers.extend(from_addons);

    // Which to load now: subtitles shipped with the release in a wanted
    // language or none stated, and the first few per wanted language from
    // the addons.
    let mut load: Vec<usize> = Vec::new();
    let mut per_language: std::collections::HashMap<String, usize> =
        std::collections::HashMap::new();
    let mut from_torrent = 0;
    for (index, offer) in offers.iter().enumerate() {
        let in_wanted = offer
            .language
            .as_ref()
            .is_some_and(|code| languages.contains(code));
        match &offer.origin {
            Origin::Torrent(_) => {
                if (in_wanted || offer.language.is_none()) && from_torrent < LOADED_FROM_TORRENT {
                    from_torrent += 1;
                    load.push(index);
                }
            }
            _ => {
                if let (true, Some(code)) = (in_wanted, &offer.language) {
                    let count = per_language.entry(code.clone()).or_default();
                    if *count < LOADED_PER_LANGUAGE {
                        *count += 1;
                        load.push(index);
                    }
                }
            }
        }
    }

    let addon_count = addons.len();
    if with(shared, ticket, move |session| {
        session.subtitles.offers = offers;
        session.subtitles.addons = addon_count;
    })
    .is_none()
    {
        return;
    }

    // The first one in the viewer's first language is switched on -- unless
    // they already have a subtitle on (an embedded one `--slang` chose, or
    // one they picked while this ran), or the film is already being heard in
    // that language.
    let mut switched = !choose;
    for index in load {
        let select = !switched
            && with(shared, ticket, |session| {
                let offer = session.subtitles.offers.get(index)?;
                let code = offer.language.clone()?;
                if languages.first() != Some(&code) {
                    return None;
                }
                let player = session.player.as_mut()?;
                let tracks = player.tracks().ok()?;
                let sub_on = tracks.iter().any(|t| t.kind == "sub" && t.selected);
                let heard = tracks.iter().any(|t| {
                    t.kind == "audio"
                        && t.selected
                        && t.language.as_deref().map(code_of).as_deref() == Some(code.as_str())
                });
                Some(!sub_on && !heard)
            })
            .flatten()
            .unwrap_or(false);
        if load_one(shared, ticket, index, select).is_ok() && select {
            switched = true;
        }
    }

    let _ = with(shared, ticket, |session| {
        session.subtitles.searching = false
    });
}

/// Run `work` on the session if `ticket` is still the playback the session
/// holds subtitles for. `None` when it is not -- the viewer moved on.
fn with<T>(shared: &Shared, ticket: u64, work: impl FnOnce(&mut Session) -> T) -> Option<T> {
    let mut session = shared.lock().ok()?;
    if session.subtitles.ticket != ticket {
        return None;
    }
    Some(work(&mut session))
}

/// Fetch one offer, write it down, and hand it to mpv.
///
/// # Errors
///
/// As text: the playback moved on, the fetch failed, or mpv refused it.
pub fn load_one(shared: &Shared, ticket: u64, index: usize, select: bool) -> Result<(), String> {
    let (offer, gone) = {
        let session = shared
            .lock()
            .map_err(|_| "the host lost track of its own state".to_owned())?;
        let gone = session.text("ui.error.noplayer");
        if session.subtitles.ticket != ticket {
            return Err(gone);
        }
        let offer = session
            .subtitles
            .offers
            .get(index)
            .cloned()
            .ok_or_else(|| session.text("ui.error.nosubtitle"))?;
        (offer, gone)
    };

    // Not under the lock: a subtitle server, or a piece of the torrent, can
    // take its time.
    let response = crate::http::Http::new()
        .get(&offer.url)
        .map_err(|e| e.message)?;
    if !(200..300).contains(&response.status) {
        return Err(format!("HTTP {}", response.status));
    }
    let path = write(ticket, index, &offer.extension, &response.body)?;

    let mut session = shared.lock().map_err(|_| gone.clone())?;
    if session.subtitles.ticket != ticket {
        return Err(gone);
    }
    add_to_player(&mut session, &path, &offer, select)?;
    if let Some(stored) = session.subtitles.offers.get_mut(index) {
        stored.loaded = true;
    }
    Ok(())
}

/// A subtitle file the viewer chose, read by the interface and sent as
/// bytes: the webview has no file paths to give, by design.
///
/// # Errors
///
/// As text: nothing is playing, the bytes could not be written, or mpv
/// refused them.
pub fn add_file(session: &mut Session, name: &str, bytes: &[u8]) -> Result<(), String> {
    if session.player.is_none() {
        return Err(session.text("ui.error.noplayer"));
    }
    let extension = extension_of(name).unwrap_or_else(|| "srt".to_owned());
    let ticket = session.subtitles.ticket;
    let index = session.subtitles.offers.len();
    let path = write(ticket, index, &extension, bytes)?;
    let offer = Offer {
        language: language_in_name(name),
        origin: Origin::File(name.to_owned()),
        url: String::new(),
        extension,
        loaded: true,
    };
    add_to_player(session, &path, &offer, true)?;
    session.subtitles.offers.push(offer);
    Ok(())
}

fn write(ticket: u64, index: usize, extension: &str, bytes: &[u8]) -> Result<PathBuf, String> {
    let folder = folder();
    std::fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
    let path = folder.join(format!("{ticket}-{index}.{extension}"));
    std::fs::write(&path, bytes).map_err(|e| e.to_string())?;
    Ok(path)
}

fn add_to_player(
    session: &mut Session,
    path: &std::path::Path,
    offer: &Offer,
    select: bool,
) -> Result<(), String> {
    let gone = session.text("ui.error.noplayer");
    let player = session.player.as_mut().ok_or(gone)?;
    let path = path.to_string_lossy().into_owned();
    let flag = if select { "select" } else { "auto" };
    let title = offer.origin.label().to_owned();
    let language = offer.language.clone().unwrap_or_default();
    player
        .command(&["sub-add", &path, flag, &title, &language])
        .map(|_| ())
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_language_is_read_from_the_end_of_a_file_name() {
        assert_eq!(language_in_name("Movie.2019.tr.srt").as_deref(), Some("tr"));
        assert_eq!(
            language_in_name("Subs/3_Turkish.srt").as_deref(),
            Some("tr")
        );
        assert_eq!(
            language_in_name("Subs/English(SDH).srt").as_deref(),
            Some("en")
        );
        assert_eq!(language_in_name("Movie.2019.1080p.srt"), None);
    }

    #[test]
    fn only_formats_mpv_reads_alone_are_subtitles() {
        assert_eq!(extension_of("a/b/Film.SRT").as_deref(), Some("srt"));
        assert_eq!(extension_of("Film.ass").as_deref(), Some("ass"));
        assert_eq!(extension_of("Film.sub"), None);
        assert_eq!(extension_of("Film.mkv"), None);
        assert_eq!(extension_of("no-extension"), None);
    }

    #[test]
    fn a_language_is_offered_to_mpv_in_every_spelling() {
        let out = spellings(&["tr".to_owned()]);
        assert!(out.contains(&"tr".to_owned()));
        assert!(out.contains(&"tur".to_owned()));
    }
}
