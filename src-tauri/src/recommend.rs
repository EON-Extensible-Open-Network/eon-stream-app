// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 EON contributors
//
// Additional permission under GNU GPL version 3 section 7:
// see LICENSE-EXCEPTION.md (EON Module ABI Exception 1.0).

//! "Recommended for you", worked out on this computer.
//!
//! Nothing here leaves the device. The taste is read from what this profile
//! did -- how long it watched each title and how far it got, what it scored,
//! what it saved -- and every title it touched is described by its genres,
//! year and type (`art`, filled at playback). The candidates are the addons'
//! own catalogues, read page by page as the feed is scrolled, the way a
//! search is (`search.rs`).
//!
//! The ranking is a content-based score, the first stage of what the large
//! services do before their collaborative stage:
//!
//! * **genre affinity**, each genre weighted by the signals of the titles
//!   that carry it: a 9 for an isekai pulls isekai up harder than an hour of
//!   adventure, and a series abandoned after ten minutes pulls its genres
//!   down;
//! * **quality** -- the catalogue's IMDb score;
//! * **popularity** -- how high the catalogue placed it;
//! * **era** and **type** -- near the years and the films-or-series balance
//!   this profile watches.
//!
//! The list is then re-ranked for variety (maximal marginal relevance), so a
//! profile that loves one genre still sees the next one now and then. When a
//! community server is connected, its "people who liked this also liked"
//! answers are added as a boost (`boost`); only scores the viewer chose to
//! share ever reach it, and watch history never does.

use std::collections::{HashMap, HashSet};

use eon_stream_core::MetaPreview;
use serde::Serialize;

use crate::state::Session;
use crate::view::{ItemView, Net};

/// Titles per page of the feed.
pub const PAGE: usize = 24;

/// What one title says about this profile.
#[derive(Clone, Debug, Default)]
pub struct Signal {
    pub content_type: String,
    pub genres: Vec<String>,
    pub year: Option<i32>,
    /// Positive for liked, negative for disliked or abandoned.
    pub weight: f64,
}

/// The taste, as the scoring reads it.
#[derive(Clone, Debug, Default)]
pub struct Taste {
    /// -1 to 1 per genre, lower-cased.
    pub genres: HashMap<String, f64>,
    /// Share of positive weight per type.
    pub types: HashMap<String, f64>,
    pub year_mean: Option<f64>,
    pub year_spread: f64,
    /// Titles already seen, scored or saved -- not recommended again.
    pub known: HashSet<String>,
    /// Genres in the order this profile likes them, best first.
    pub ranked_genres: Vec<String>,
}

/// The first year in an addon's "2019", "2019–2023" or "2019-".
#[must_use]
pub fn year_of(text: Option<&str>) -> Option<i32> {
    let text = text?;
    let digits: String = text.chars().take_while(char::is_ascii_digit).collect();
    (digits.len() == 4).then(|| digits.parse().ok()).flatten()
}

fn key(content_type: &str, id: &str) -> String {
    format!("{content_type}:{id}")
}

/// Read the signals out of a profile.
#[must_use]
pub fn signals(session: &Session, now: u64) -> (Vec<Signal>, HashSet<String>) {
    let mut by_title: HashMap<String, (Signal, f64)> = HashMap::new();
    let mut known = HashSet::new();
    let day = 86_400.0;

    // Watching. A title is its series for an episode; episodes add up, but
    // with diminishing returns, so forty episodes of one show do not drown
    // everything else.
    for entry in session.history.entries() {
        let (content_type, id) = crate::art::title_of(entry);
        known.insert(key(&content_type, &id));
        let look = session.art.get(&id);
        let watched = entry
            .duration_secs
            .map_or(entry.position_secs, |d| entry.position_secs.min(d));
        let minutes = watched / 60.0;
        let completion = entry.progress().unwrap_or(0.5);
        #[allow(clippy::cast_precision_loss)]
        let age_days = now.saturating_sub(entry.updated_at) as f64 / day;
        let recency = 0.5 + 0.5 * (-age_days / 90.0).exp();
        let mut weight = (minutes / 45.0).min(1.5) * (0.4 + 0.6 * completion);
        if entry.is_finished() {
            weight += 0.3;
        }
        // Given up on: a few minutes, a week ago, never back.
        if completion < 0.1 && minutes < 15.0 && age_days > 7.0 && entry.series_id.is_none() {
            weight = -0.4;
        }
        let slot = by_title.entry(key(&content_type, &id)).or_insert_with(|| {
            (
                Signal {
                    content_type: content_type.clone(),
                    genres: look.map(|l| l.genres.clone()).unwrap_or_default(),
                    year: look.and_then(|l| l.year),
                    weight: 0.0,
                },
                0.0,
            )
        });
        slot.1 += weight * recency;
    }
    let mut out: Vec<Signal> = by_title
        .into_values()
        .map(|(mut signal, sum)| {
            signal.weight = if sum >= 0.0 {
                1.6 * (1.0 - (-sum / 1.5).exp())
            } else {
                sum
            };
            signal
        })
        .collect();

    // Scores speak loudest: a 10 is +2, a 1 is -2.
    for rating in session.ratings.all() {
        known.insert(key(&rating.content_type, &rating.id));
        out.push(Signal {
            content_type: rating.content_type.clone(),
            genres: rating.genres.clone(),
            year: year_of(rating.year.as_deref()),
            weight: (f64::from(rating.score) - 5.5) / 2.25,
        });
    }

    // Saving for later is a quieter yes.
    for saved in session.library.list(eon_stream_core::LibraryOrder::Recent) {
        known.insert(key(&saved.content_type, &saved.id));
        if let Some(look) = session.art.get(&saved.id) {
            out.push(Signal {
                content_type: saved.content_type.clone(),
                genres: look.genres.clone(),
                year: look.year.or_else(|| year_of(saved.release_info.as_deref())),
                weight: 0.5,
            });
        }
    }
    (out, known)
}

/// Fold signals into a taste.
#[must_use]
pub fn taste(signals: &[Signal], known: HashSet<String>) -> Taste {
    let mut genres: HashMap<String, f64> = HashMap::new();
    let mut types: HashMap<String, f64> = HashMap::new();
    let (mut year_sum, mut year_weight, mut year_sq) = (0.0, 0.0, 0.0);
    for signal in signals {
        if !signal.genres.is_empty() {
            #[allow(clippy::cast_precision_loss)]
            let share = signal.weight / (signal.genres.len() as f64).sqrt();
            for genre in &signal.genres {
                *genres.entry(genre.to_lowercase()).or_default() += share;
            }
        }
        if signal.weight > 0.0 {
            *types.entry(signal.content_type.clone()).or_default() += signal.weight;
            if let Some(year) = signal.year {
                let y = f64::from(year);
                year_sum += y * signal.weight;
                year_sq += y * y * signal.weight;
                year_weight += signal.weight;
            }
        }
    }
    let top = genres.values().fold(0.0_f64, |m, v| m.max(v.abs()));
    if top > 0.0 {
        for value in genres.values_mut() {
            *value /= top;
        }
    }
    let total: f64 = types.values().sum();
    if total > 0.0 {
        for value in types.values_mut() {
            *value /= total;
        }
    }
    let (year_mean, year_spread) = if year_weight > 0.0 {
        let mean = year_sum / year_weight;
        (
            Some(mean),
            (year_sq / year_weight - mean * mean).max(0.0).sqrt(),
        )
    } else {
        (None, 0.0)
    };
    let mut ranked: Vec<(String, f64)> = genres
        .iter()
        .filter(|(_, v)| **v > 0.0)
        .map(|(g, v)| (g.clone(), *v))
        .collect();
    ranked.sort_by(|a, b| b.1.total_cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    Taste {
        genres,
        types,
        year_mean,
        year_spread,
        known,
        ranked_genres: ranked.into_iter().map(|(g, _)| g).collect(),
    }
}

/// One title the feed could show, and where its catalogue placed it.
#[derive(Clone, Debug)]
pub struct Candidate {
    pub preview: MetaPreview,
    pub rank: usize,
}

/// How much this profile should want a title.
#[must_use]
pub fn score(taste: &Taste, candidate: &Candidate, boost: f64) -> f64 {
    let preview = &candidate.preview;
    let genre = if preview.genres.is_empty() {
        0.0
    } else {
        #[allow(clippy::cast_precision_loss)]
        let n = (preview.genres.len() as f64).sqrt();
        preview
            .genres
            .iter()
            .map(|g| taste.genres.get(&g.to_lowercase()).copied().unwrap_or(0.0))
            .sum::<f64>()
            / n
    };
    let quality = preview
        .imdb_rating
        .as_deref()
        .and_then(|r| r.trim().parse::<f64>().ok())
        .map_or(0.0, |r| ((r - 6.0) / 2.5).clamp(-1.0, 1.0));
    #[allow(clippy::cast_precision_loss)]
    let popularity = 1.0 / (1.0 + candidate.rank as f64 / 15.0);
    let era = match (taste.year_mean, year_of(preview.release_info.as_deref())) {
        (Some(mean), Some(year)) => {
            (-(f64::from(year) - mean).abs() / (taste.year_spread + 6.0)).exp()
        }
        _ => 0.5,
    };
    let kind = taste
        .types
        .get(&preview.content_type)
        .copied()
        .unwrap_or(0.5);
    1.4 * genre + 0.45 * quality + 0.35 * popularity + 0.2 * era + 0.15 * kind + boost
}

/// Rank for variety: each pick is the best score after a penalty for
/// repeating the leading genre or the type of the last few picks.
#[must_use]
pub fn diversify(mut scored: Vec<(f64, Candidate)>, take: usize) -> Vec<Candidate> {
    scored.sort_by(|a, b| b.0.total_cmp(&a.0));
    let mut picked: Vec<Candidate> = Vec::new();
    while picked.len() < take && !scored.is_empty() {
        let recent = &picked[picked.len().saturating_sub(8)..];
        let mut best = 0;
        let mut best_value = f64::MIN;
        // Only the leading part of the list can win; the rest is too far
        // behind for a penalty to matter.
        for (at, (value, candidate)) in scored.iter().enumerate().take(60) {
            let lead = candidate.preview.genres.first();
            #[allow(clippy::cast_precision_loss)]
            let same_genre = recent
                .iter()
                .filter(|p| lead.is_some() && p.preview.genres.first() == lead)
                .count() as f64;
            #[allow(clippy::cast_precision_loss)]
            let same_type = recent
                .iter()
                .rev()
                .take(4)
                .filter(|p| p.preview.content_type == candidate.preview.content_type)
                .count() as f64;
            let adjusted = value - 0.18 * same_genre - if same_type >= 4.0 { 0.1 } else { 0.0 };
            if adjusted > best_value {
                best_value = adjusted;
                best = at;
            }
        }
        picked.push(scored.remove(best).1);
    }
    picked
}

/// A catalogue the feed reads from, and how far.
#[derive(Clone, Debug)]
struct Cursor {
    addon_id: String,
    content_type: String,
    catalog_id: String,
    genre: Option<String>,
    skip: usize,
    done: bool,
}

/// The feed so far: what was read, what was shown.
#[derive(Clone, Default)]
pub struct Feed {
    taste: Taste,
    cursors: Vec<Cursor>,
    pool: Vec<Candidate>,
    seen: HashSet<String>,
    shown: Vec<Candidate>,
    /// From a community server, when one is connected: title key → boost.
    pub boost: HashMap<String, f64>,
    community_asked: bool,
    /// A child's profile: only family and animation.
    kids: bool,
    profile: String,
}

/// One page of the feed, and the rows made from it.
#[derive(Serialize)]
pub struct FeedPage {
    pub page: usize,
    pub items: Vec<ItemView>,
    pub more: bool,
    /// The genres this profile likes, best first, for the home screen's
    /// genre rows.
    pub genres: Vec<String>,
    /// Titles made in the viewer's preferred audio language, from what was
    /// read so far.
    pub in_language: Vec<ItemView>,
    pub language: Option<String>,
}

/// What a page needs, out of the session.
pub struct Request {
    net: Net,
    feed: Feed,
    page: usize,
    /// "People who liked what you liked", when signed in to a community.
    community: Option<crate::community::Call>,
}

/// Step one: the feed in progress, or a new one.
pub fn request(session: &Session, page: usize, fresh: bool) -> Request {
    let profile = session.profiles.current.clone();
    let feed = match &session.feed {
        Some(kept) if !fresh && kept.profile == profile => kept.clone(),
        _ => new_feed(session, profile),
    };
    // Asked once per feed: the server's answer only changes with scores.
    let community = (feed.boost.is_empty()
        && !feed.community_asked
        && session.community.username.is_some())
    .then(|| {
        crate::community::prepare(&session.community, "GET", "/v0/me/recommendations", None).ok()
    })
    .flatten();
    Request {
        net: Net::of(session),
        feed,
        page,
        community,
    }
}

/// The community's recommendations: a boost for each, and each a candidate
/// in its own right.
fn add_community(feed: &mut Feed, call: &crate::community::Call) {
    feed.community_asked = true;
    let Ok(answer) = crate::community::send(call) else {
        return;
    };
    if answer.status != 200 {
        return;
    }
    let items = answer
        .body
        .get("items")
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default();
    let top = items
        .first()
        .and_then(|i| i.get("score"))
        .and_then(serde_json::Value::as_f64)
        .unwrap_or(1.0)
        .max(f64::EPSILON);
    for (rank, item) in items.iter().enumerate() {
        let text = |v: Option<&serde_json::Value>| {
            v.and_then(serde_json::Value::as_str).map(ToOwned::to_owned)
        };
        let (Some(kind), Some(id)) = (text(item.get("type")), text(item.get("id"))) else {
            continue;
        };
        let score = item
            .get("score")
            .and_then(serde_json::Value::as_f64)
            .unwrap_or(0.0);
        let k = key(&kind, &id);
        feed.boost.insert(k.clone(), 0.8 * score / top);
        if feed.taste.known.contains(&k) || !feed.seen.insert(k) {
            continue;
        }
        let title = item.get("title");
        if feed.kids {
            let genres: Vec<String> = title
                .and_then(|t| t.get("genres"))
                .and_then(serde_json::Value::as_array)
                .map(|g| {
                    g.iter()
                        .filter_map(|v| v.as_str().map(ToOwned::to_owned))
                        .collect()
                })
                .unwrap_or_default();
            if !crate::profiles::for_kids(&genres) {
                continue;
            }
        }
        feed.pool.push(Candidate {
            preview: MetaPreview {
                id,
                content_type: kind,
                name: text(title.and_then(|t| t.get("name"))),
                poster: text(title.and_then(|t| t.get("poster"))),
                poster_shape: None,
                description: None,
                release_info: text(title.and_then(|t| t.get("year"))),
                imdb_rating: None,
                genres: title
                    .and_then(|t| t.get("genres"))
                    .and_then(serde_json::Value::as_array)
                    .map(|g| {
                        g.iter()
                            .filter_map(|v| v.as_str().map(ToOwned::to_owned))
                            .collect()
                    })
                    .unwrap_or_default(),
                country: None,
                language: None,
            },
            rank,
        });
    }
}

fn new_feed(session: &Session, profile: String) -> Feed {
    let (signals, known) = signals(session, eon_stream_core::history::now_unix());
    let taste = taste(&signals, known);
    let mut cursors = Vec::new();
    for addon in session.registry.enabled() {
        for catalog in &addon.manifest.catalogs {
            if !matches!(catalog.content_type.as_str(), "movie" | "series") {
                continue;
            }
            let required = catalog.required_extra();
            let genres: Vec<String> = catalog
                .extra
                .iter()
                .find(|prop| prop.name == "genre")
                .map(|prop| prop.options.clone())
                .unwrap_or_default();
            if required.is_empty() {
                cursors.push(Cursor {
                    addon_id: addon.id().to_owned(),
                    content_type: catalog.content_type.clone(),
                    catalog_id: catalog.id.clone(),
                    genre: None,
                    skip: 0,
                    done: false,
                });
            }
            // The genres this profile likes most, read on their own: the
            // general catalogues alone would be the same popular titles for
            // everyone.
            if required.iter().all(|r| *r == "genre") {
                for liked in taste.ranked_genres.iter().take(3) {
                    if let Some(genre) = genres.iter().find(|g| g.to_lowercase() == *liked) {
                        cursors.push(Cursor {
                            addon_id: addon.id().to_owned(),
                            content_type: catalog.content_type.clone(),
                            catalog_id: catalog.id.clone(),
                            genre: Some(genre.clone()),
                            skip: 0,
                            done: false,
                        });
                    }
                }
            }
        }
    }
    // Two catalogues that page by genre the same way are read once.
    let mut unique = HashSet::new();
    cursors.retain(|c| {
        unique.insert((
            c.addon_id.clone(),
            c.content_type.clone(),
            c.catalog_id.clone(),
            c.genre.clone(),
        ))
    });
    // Keep the request count sane for someone with many addons.
    cursors.truncate(24);
    Feed {
        taste,
        cursors,
        profile,
        kids: session.profiles.kids(),
        ..Feed::default()
    }
}

/// Step two, with nothing locked: read until the page can be ranked from a
/// pool three pages deep.
#[must_use]
pub fn fetch(request: &Request) -> Feed {
    let mut feed = request.feed.clone();
    if let Some(call) = &request.community {
        add_community(&mut feed, call);
    }
    let wanted = (request.page + 1) * PAGE;
    for _ in 0..6 {
        if feed.shown.len() >= wanted {
            break;
        }
        let need = wanted - feed.shown.len();
        let open = feed.cursors.iter().any(|c| !c.done);
        if feed.pool.len() < need * 3 && open {
            read_round(&request.net, &mut feed);
        }
        if feed.pool.is_empty() {
            break;
        }
        let scored: Vec<(f64, Candidate)> = std::mem::take(&mut feed.pool)
            .into_iter()
            .map(|c| {
                let boost = feed
                    .boost
                    .get(&key(&c.preview.content_type, &c.preview.id))
                    .copied()
                    .unwrap_or(0.0);
                (score(&feed.taste, &c, boost), c)
            })
            .collect();
        let picked = diversify(scored.clone(), need);
        let chosen: HashSet<String> = picked
            .iter()
            .map(|c| key(&c.preview.content_type, &c.preview.id))
            .collect();
        feed.pool = scored
            .into_iter()
            .map(|(_, c)| c)
            .filter(|c| !chosen.contains(&key(&c.preview.content_type, &c.preview.id)))
            .collect();
        feed.shown.extend(picked);
    }
    feed
}

/// One page from every catalogue that is not finished, at once.
fn read_round(net: &Net, feed: &mut Feed) {
    let open: Vec<(usize, Cursor)> = feed
        .cursors
        .iter()
        .enumerate()
        .filter(|(_, c)| !c.done)
        .map(|(at, c)| (at, c.clone()))
        .collect();
    let answers: Vec<(usize, Option<Vec<MetaPreview>>)> = std::thread::scope(|scope| {
        let handles: Vec<_> = open
            .iter()
            .map(|(at, cursor)| {
                scope.spawn(move || {
                    let Some(addon) = net.registry.get(&cursor.addon_id) else {
                        return (*at, None);
                    };
                    let skip = cursor.skip.to_string();
                    let mut extra: Vec<(&str, &str)> = Vec::new();
                    if let Some(genre) = &cursor.genre {
                        extra.push(("genre", genre.as_str()));
                    }
                    if cursor.skip > 0 {
                        extra.push(("skip", skip.as_str()));
                    }
                    (
                        *at,
                        net.client
                            .catalog(addon, &cursor.content_type, &cursor.catalog_id, &extra)
                            .ok(),
                    )
                })
            })
            .collect();
        handles.into_iter().filter_map(|h| h.join().ok()).collect()
    });
    for (at, page) in answers {
        let Some(page) = page else {
            feed.cursors[at].done = true;
            continue;
        };
        let count = page.len();
        let mut fresh = 0;
        for (rank, preview) in page.into_iter().enumerate() {
            let k = key(&preview.content_type, &preview.id);
            if feed.taste.known.contains(&k) || !feed.seen.insert(k) {
                continue;
            }
            fresh += 1;
            if feed.kids && !crate::profiles::for_kids(&preview.genres) {
                continue;
            }
            feed.pool.push(Candidate {
                preview,
                rank: feed.cursors[at].skip + rank,
            });
        }
        feed.cursors[at].skip += count;
        if fresh == 0 && count == 0 {
            feed.cursors[at].done = true;
        }
        // An addon that ignores `skip` sends the same page again: nothing
        // new twice in a row is the end of it.
        if count > 0 && fresh == 0 && feed.cursors[at].skip > count {
            feed.cursors[at].done = true;
        }
    }
}

/// The countries a language is mostly made in -- for "made in Turkish",
/// read from the catalogue's first country of production.
fn countries(language: &str) -> &'static [&'static str] {
    match language {
        "tr" => &["Turkey", "Türkiye"],
        "de" => &["Germany", "Austria", "Switzerland"],
        "fr" => &["France", "Belgium"],
        "es" => &["Spain", "Mexico", "Argentina", "Colombia", "Chile"],
        "it" => &["Italy"],
        "ja" => &["Japan"],
        "ko" => &["South Korea", "Korea"],
        "zh" => &["China", "Taiwan", "Hong Kong"],
        "ru" => &["Russia"],
        "pt" => &["Brazil", "Portugal"],
        "hi" => &["India"],
        "ar" => &[
            "Egypt",
            "Saudi Arabia",
            "United Arab Emirates",
            "Lebanon",
            "Jordan",
        ],
        "nl" => &["Netherlands"],
        "sv" => &["Sweden"],
        "da" => &["Denmark"],
        "no" => &["Norway"],
        "fi" => &["Finland"],
        "pl" => &["Poland"],
        "el" => &["Greece"],
        "he" => &["Israel"],
        "fa" => &["Iran"],
        "th" => &["Thailand"],
        "id" => &["Indonesia"],
        "uk" => &["Ukraine"],
        "cs" => &["Czech Republic", "Czechia"],
        "hu" => &["Hungary"],
        "ro" => &["Romania"],
        "az" => &["Azerbaijan"],
        _ => &[],
    }
}

/// Whether a title was made in a language.
#[must_use]
pub fn made_in(preview: &MetaPreview, language: &str) -> bool {
    if let Some(spoken) = &preview.language {
        let spoken = spoken.to_lowercase();
        if spoken.starts_with(language) {
            return true;
        }
    }
    let Some(country) = &preview.country else {
        return false;
    };
    let first = country.split(',').next().unwrap_or("").trim();
    countries(language)
        .iter()
        .any(|c| c.eq_ignore_ascii_case(first))
}

/// Step three: keep the feed, and draw the page.
pub fn view(session: &mut Session, feed: Feed, page: usize) -> FeedPage {
    let start = page * PAGE;
    let slice: Vec<MetaPreview> = feed
        .shown
        .iter()
        .skip(start)
        .take(PAGE)
        .map(|c| c.preview.clone())
        .collect();
    let items = slice
        .iter()
        .map(|p| crate::view::item_from_preview(p, &session.library, &session.history))
        .collect();
    crate::view::remember(session, slice);
    let more = feed.shown.len() > start + PAGE
        || !feed.pool.is_empty()
        || feed.cursors.iter().any(|c| !c.done);
    // The language row: the viewer's first audio language, unless it is the
    // one most catalogues are already in.
    let language = session
        .settings
        .ranking
        .preferred_audio_languages
        .first()
        .map(|l| l.to_lowercase())
        .filter(|l| l != "en");
    let in_language = language.as_ref().map_or_else(Vec::new, |lang| {
        let mut found: Vec<&Candidate> = feed
            .shown
            .iter()
            .chain(feed.pool.iter())
            .filter(|c| made_in(&c.preview, lang))
            .collect();
        found.sort_by(|a, b| score(&feed.taste, b, 0.0).total_cmp(&score(&feed.taste, a, 0.0)));
        found
            .into_iter()
            .take(30)
            .map(|c| crate::view::item_from_preview(&c.preview, &session.library, &session.history))
            .collect()
    });
    let out = FeedPage {
        page,
        items,
        more,
        genres: feed.taste.ranked_genres.clone(),
        in_language,
        language,
    };
    session.feed = Some(feed);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn preview(id: &str, kind: &str, genres: &[&str], year: &str, rating: &str) -> MetaPreview {
        MetaPreview {
            id: id.to_owned(),
            content_type: kind.to_owned(),
            name: Some(id.to_owned()),
            poster: None,
            poster_shape: None,
            description: None,
            release_info: Some(year.to_owned()),
            imdb_rating: Some(rating.to_owned()),
            genres: genres.iter().map(|g| (*g).to_owned()).collect(),
            country: None,
            language: None,
        }
    }

    fn signal(genres: &[&str], weight: f64) -> Signal {
        Signal {
            content_type: "series".into(),
            genres: genres.iter().map(|g| (*g).to_owned()).collect(),
            year: Some(2020),
            weight,
        }
    }

    #[test]
    fn a_high_score_outweighs_more_watching() {
        // Watches a lot of adventure, scores isekai high.
        let taste = taste(
            &[
                signal(&["Adventure", "Animation"], 1.2),
                signal(&["Adventure", "Action"], 1.2),
                signal(&["Isekai", "Fantasy"], 2.0),
                signal(&["Isekai", "Animation"], 2.0),
            ],
            HashSet::new(),
        );
        assert_eq!(
            taste.ranked_genres.first().map(String::as_str),
            Some("isekai")
        );
        let isekai = Candidate {
            preview: preview("a", "series", &["Isekai"], "2021", "7.5"),
            rank: 10,
        };
        let western = Candidate {
            preview: preview("b", "series", &["Western"], "2021", "7.5"),
            rank: 10,
        };
        assert!(score(&taste, &isekai, 0.0) > score(&taste, &western, 0.0));
    }

    #[test]
    fn an_abandoned_genre_is_pushed_down() {
        let taste = taste(
            &[signal(&["Horror"], -0.4), signal(&["Comedy"], 1.0)],
            HashSet::new(),
        );
        let horror = Candidate {
            preview: preview("h", "movie", &["Horror"], "2020", "7.0"),
            rank: 0,
        };
        let neutral = Candidate {
            preview: preview("n", "movie", &["Documentary"], "2020", "7.0"),
            rank: 0,
        };
        assert!(score(&taste, &horror, 0.0) < score(&taste, &neutral, 0.0));
    }

    #[test]
    fn with_no_history_the_popular_and_well_rated_come_first() {
        let taste = Taste::default();
        let top = Candidate {
            preview: preview("t", "movie", &["Drama"], "2020", "8.5"),
            rank: 0,
        };
        let low = Candidate {
            preview: preview("l", "movie", &["Drama"], "2020", "5.5"),
            rank: 80,
        };
        assert!(score(&taste, &top, 0.0) > score(&taste, &low, 0.0));
    }

    #[test]
    fn variety_breaks_up_a_run_of_one_genre() {
        let mut scored = Vec::new();
        for n in 0..10 {
            scored.push((
                2.0 - f64::from(n) * 0.01,
                Candidate {
                    preview: preview(&format!("d{n}"), "movie", &["Drama"], "2020", "7"),
                    rank: 0,
                },
            ));
        }
        scored.push((
            1.7,
            Candidate {
                preview: preview("c", "movie", &["Comedy"], "2020", "7"),
                rank: 0,
            },
        ));
        let picked = diversify(scored, 6);
        assert!(
            picked.iter().any(|c| c.preview.id == "c"),
            "a comedy should break the drama run"
        );
        assert_eq!(picked[0].preview.id, "d0");
    }

    #[test]
    fn a_title_is_made_in_the_language_of_its_first_country() {
        let mut p = preview("x", "series", &[], "2020", "7");
        p.country = Some("Turkey, Germany".into());
        assert!(made_in(&p, "tr"));
        assert!(!made_in(&p, "de"));
        p.country = None;
        p.language = Some("Turkish".into());
        assert!(!made_in(&p, "tr"));
        p.language = Some("tr".into());
        assert!(made_in(&p, "tr"));
    }

    #[test]
    fn a_year_is_read_from_a_range() {
        assert_eq!(year_of(Some("2019–2023")), Some(2019));
        assert_eq!(year_of(Some("abc")), None);
    }
}
