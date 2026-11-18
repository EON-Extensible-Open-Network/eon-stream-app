// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 EON contributors
//
// Additional permission under GNU GPL version 3 section 7:
// see LICENSE-EXCEPTION.md (EON Module ABI Exception 1.0).

//! The viewer's own scores and reviews: a film diary that needs no account.
//!
//! Madde 7 holds that the viewer works fully without an account, and that a
//! score shared with others comes with a separate sign-in. So scoring starts
//! here, on this machine, in `eon-ratings.json` beside the executable: what
//! was scored, out of ten, when, and what the viewer wrote about it. Nothing
//! here leaves the machine. When a community server exists and the viewer
//! signs in to it, these are what they may choose to share (`community`).
//!
//! A score keeps the title's name, poster address, year and genres as they
//! were when it was given, so the profile page draws without asking an addon
//! for every title again.

use serde::{Deserialize, Serialize};

use crate::state::Session;

const FILE: &str = "eon-ratings.json";
/// Long enough for a real review, short enough that a paste of a whole
/// script is refused rather than stored.
pub const REVIEW_MAX_CHARS: usize = 5000;

/// One title, scored.
#[derive(Clone, Serialize, Deserialize)]
pub struct Rating {
    pub content_type: String,
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub poster: Option<String>,
    #[serde(default)]
    pub year: Option<String>,
    #[serde(default)]
    pub genres: Vec<String>,
    /// 1 to 10.
    pub score: u8,
    #[serde(default)]
    pub review: Option<String>,
    /// When it was scored or last changed, Unix seconds.
    pub at: u64,
}

#[derive(Default, Serialize, Deserialize)]
pub struct Ratings {
    items: Vec<Rating>,
}

impl Ratings {
    /// A profile's scores.
    pub fn load(profile: &str) -> Self {
        std::fs::read_to_string(crate::profiles::file(profile, FILE))
            .ok()
            .and_then(|json| serde_json::from_str(&json).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, profile: &str) {
        if let Ok(json) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(crate::profiles::file(profile, FILE), json);
        }
    }

    #[must_use]
    pub fn get(&self, content_type: &str, id: &str) -> Option<&Rating> {
        self.items
            .iter()
            .find(|r| r.content_type == content_type && r.id == id)
    }

    /// Every score, newest first.
    #[must_use]
    pub fn all(&self) -> Vec<Rating> {
        let mut items = self.items.clone();
        items.sort_by_key(|item| std::cmp::Reverse(item.at));
        items
    }

    fn put(&mut self, rating: Rating) {
        self.items
            .retain(|r| !(r.content_type == rating.content_type && r.id == rating.id));
        self.items.push(rating);
    }

    fn remove(&mut self, content_type: &str, id: &str) {
        self.items
            .retain(|r| !(r.content_type == content_type && r.id == id));
    }
}

/// The profile page's numbers.
#[derive(Serialize)]
pub struct ProfileView {
    pub items: Vec<Rating>,
    pub count: usize,
    /// The mean score, to one decimal; `None` before the first.
    pub average: Option<f64>,
    /// How many titles got each score, 1 to 10.
    pub histogram: [usize; 10],
    /// The genres of the titles scored 7 or more, most frequent first.
    pub liked_genres: Vec<(String, usize)>,
    /// How many have a written review.
    pub reviews: usize,
}

#[must_use]
pub fn profile(session: &Session) -> ProfileView {
    let items = session.ratings.all();
    let count = items.len();
    let mut histogram = [0usize; 10];
    let mut total = 0u64;
    let mut liked: Vec<(String, usize)> = Vec::new();
    for item in &items {
        if let Some(slot) = histogram.get_mut(usize::from(item.score.clamp(1, 10)) - 1) {
            *slot += 1;
        }
        total += u64::from(item.score);
        if item.score >= 7 {
            for genre in &item.genres {
                match liked.iter_mut().find(|(g, _)| g == genre) {
                    Some((_, n)) => *n += 1,
                    None => liked.push((genre.clone(), 1)),
                }
            }
        }
    }
    liked.sort_by_key(|entry| std::cmp::Reverse(entry.1));
    liked.truncate(8);
    #[allow(clippy::cast_precision_loss)]
    let average = (count > 0).then(|| ((total as f64 / count as f64) * 10.0).round() / 10.0);
    let reviews = items
        .iter()
        .filter(|r| r.review.as_deref().is_some_and(|t| !t.trim().is_empty()))
        .count();
    ProfileView {
        items,
        count,
        average,
        histogram,
        liked_genres: liked,
        reviews,
    }
}

/// Score a title, change its score or review, or -- with no score -- take
/// the score away.
///
/// The title's name and artwork come from what the viewer has open: the
/// detail page's item, or a card they saw in a list.
///
/// # Errors
///
/// As text: a score outside 1 to 10, a review over the length limit, or a
/// title this session knows nothing about.
pub fn set(
    session: &mut Session,
    content_type: &str,
    id: &str,
    score: Option<u8>,
    review: Option<String>,
) -> Result<Option<Rating>, String> {
    let Some(score) = score else {
        session.ratings.remove(content_type, id);
        session.ratings.save(&session.profiles.current);
        return Ok(None);
    };
    if !(1..=10).contains(&score) {
        return Err(session.text("ui.rating.range"));
    }
    let review = review
        .map(|text| text.trim().to_owned())
        .filter(|text| !text.is_empty());
    if review
        .as_ref()
        .is_some_and(|text| text.chars().count() > REVIEW_MAX_CHARS)
    {
        return Err(session
            .text("ui.rating.toolong")
            .replace("{0}", &REVIEW_MAX_CHARS.to_string()));
    }
    let known =
        describe(session, content_type, id).ok_or_else(|| session.text("ui.rating.unknown"))?;
    let rating = Rating {
        content_type: content_type.to_owned(),
        id: id.to_owned(),
        score,
        review,
        at: eon_stream_core::history::now_unix(),
        ..known
    };
    session.ratings.put(rating.clone());
    session.ratings.save(&session.profiles.current);
    Ok(Some(rating))
}

/// What the session knows about a title: the open item first, then any card
/// shown, then an earlier score of it.
fn describe(session: &Session, content_type: &str, id: &str) -> Option<Rating> {
    let blank = |name: String| Rating {
        content_type: content_type.to_owned(),
        id: id.to_owned(),
        name,
        poster: None,
        year: None,
        genres: Vec::new(),
        score: 0,
        review: None,
        at: 0,
    };
    if let Some(meta) = session.meta.as_ref().filter(|meta| meta.id == id) {
        return Some(Rating {
            poster: meta.poster.clone(),
            year: meta.release_info.clone(),
            genres: meta.genres.clone(),
            ..blank(meta.display_name().to_owned())
        });
    }
    if let Some(preview) = session.previews.iter().find(|p| p.id == id) {
        return Some(Rating {
            poster: preview.poster.clone(),
            ..blank(preview.display_name().to_owned())
        });
    }
    session.ratings.get(content_type, id).cloned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rating(id: &str, score: u8, at: u64) -> Rating {
        Rating {
            content_type: "movie".into(),
            id: id.into(),
            name: id.into(),
            poster: None,
            year: None,
            genres: vec!["Drama".into()],
            score,
            review: None,
            at,
        }
    }

    #[test]
    fn a_title_has_one_score_and_the_newest_comes_first() {
        let mut ratings = Ratings::default();
        ratings.put(rating("a", 5, 1));
        ratings.put(rating("b", 9, 2));
        ratings.put(rating("a", 8, 3));
        let all = ratings.all();
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].id, "a");
        assert_eq!(all[0].score, 8);
        ratings.remove("movie", "a");
        assert_eq!(ratings.all().len(), 1);
    }
}
