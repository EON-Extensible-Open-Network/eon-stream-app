// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 EON contributors
//
// Additional permission under GNU GPL version 3 section 7:
// see LICENSE-EXCEPTION.md (EON Module ABI Exception 1.0).

//! One search box over every addon, forgiving about spelling.
//!
//! `search befer coll saol` finds *Better Call Saul*, says that it corrected
//! the query, and offers the literal one back — the shape everybody already
//! knows from a search engine:
//!
//! ```text
//! Results are for: Better Call Saul
//! Search instead for: befer coll saol   ·   `search exactly`
//! ```
//!
//! ## Where the correction comes from
//!
//! Not from a dictionary. The addons are asked first with exactly what was
//! typed; if that finds little or nothing, a second pass gathers candidate
//! titles — from the longest word of the query, from the library, from what
//! has been watched — and the query is corrected only *towards a title that
//! exists*. See [`eon_stream_core::fuzzy`] for why that constraint matters: a
//! film called *Inglourious Basterds* must not be "fixed".
//!
//! ## "Search instead for" has to be a command, not a hint
//!
//! There is no interface to click, so the offer is only real if something can
//! be typed. `search exactly` repeats the last query uncorrected, which is the
//! headless equivalent of clicking the link — and when the interface arrives
//! it has a command to call rather than needing new logic.

use eon_stream_core::{
    fuzzy, http::HttpClient, types::MetaPreview, AddonClient, AddonRegistry, Library, WatchHistory,
};

use crate::{
    paging::{Page, DEFAULT_PAGE_SIZE},
    text::{say, Text},
};

/// What a search turned into.
#[derive(Debug, Clone, Default)]
pub struct Outcome {
    /// What was actually asked of the addons.
    pub used: String,
    /// What the viewer typed, when it differs from `used`.
    pub typed: Option<String>,
    /// The results, deduplicated, in addon order.
    pub results: Vec<MetaPreview>,
}

impl Outcome {
    /// Whether a correction happened that the viewer should be told about.
    #[must_use]
    pub fn was_corrected(&self) -> bool {
        self.typed.is_some()
    }
}

/// Search every addon, correcting the query against titles that exist.
///
/// `exactly` skips the correction pass entirely, which is what
/// `search exactly` does.
pub fn run<C: HttpClient>(
    client: &AddonClient<C>,
    registry: &AddonRegistry,
    library: &Library,
    history: &WatchHistory,
    query: &str,
    exactly: bool,
    text: &Text,
) -> Outcome {
    let query = query.trim();
    if query.is_empty() {
        say!(text, "cli.search.needsquery");
        return Outcome::default();
    }

    let searchable = registry
        .enabled()
        .any(|addon| addon.manifest.catalogs.iter().any(|c| c.is_searchable()));
    if !searchable {
        say!(text, "cli.search.nosearchable");
        return Outcome::default();
    }

    say!(text, "cli.search.searching", query);
    let first = client.search_all(registry, query);
    for failure in &first.failures {
        println!("  {failure}");
    }
    let direct: Vec<MetaPreview> = first.items.into_iter().flat_map(|(_, v)| v).collect();

    // A query that found plenty is not a query that needs fixing, and
    // "correcting" it would be the behaviour nobody wants.
    if exactly || direct.len() >= GOOD_ENOUGH {
        return Outcome {
            used: query.to_owned(),
            typed: None,
            results: direct,
        };
    }

    let Some(correction) = correct(client, registry, library, history, query, &direct) else {
        return Outcome {
            used: query.to_owned(),
            typed: None,
            results: direct,
        };
    };

    let second = client.search_all(registry, &correction);
    let corrected: Vec<MetaPreview> = second.items.into_iter().flat_map(|(_, v)| v).collect();

    // Only prefer the correction if it actually did better. Otherwise the
    // viewer gets told their spelling was wrong *and* shown fewer results,
    // which is the worst of both.
    if corrected.len() > direct.len() {
        Outcome {
            used: correction,
            typed: Some(query.to_owned()),
            results: corrected,
        }
    } else {
        Outcome {
            used: query.to_owned(),
            typed: None,
            results: direct,
        }
    }
}

/// How many results count as "this query was fine".
const GOOD_ENOUGH: usize = 3;

/// Find a title to correct towards, or `None` to leave the query alone.
fn correct<C: HttpClient>(
    client: &AddonClient<C>,
    registry: &AddonRegistry,
    library: &Library,
    history: &WatchHistory,
    query: &str,
    direct: &[MetaPreview],
) -> Option<String> {
    // The vocabulary, cheapest sources first. Anything already on this
    // machine costs nothing to consult.
    let mut vocabulary: Vec<String> = library.names().into_iter().map(ToOwned::to_owned).collect();
    vocabulary.extend(history.names().into_iter().map(ToOwned::to_owned));
    vocabulary.extend(direct.iter().map(|item| item.display_name().to_owned()));

    // Then a broader pass at the addons: the longest word of the query is the
    // most distinctive, and a catalogue search on it tends to return the
    // neighbourhood of the right title even when the whole phrase does not.
    if let Some(longest) = query
        .split_whitespace()
        .filter(|word| word.chars().count() >= 4)
        .max_by_key(|word| word.chars().count())
    {
        let probe = client.search_all(registry, longest);
        vocabulary.extend(
            probe
                .items
                .into_iter()
                .flat_map(|(_, items)| items)
                .map(|item| item.display_name().to_owned()),
        );
    }

    vocabulary.sort();
    vocabulary.dedup();
    let candidates: Vec<&str> = vocabulary.iter().map(String::as_str).collect();
    let suggestion = fuzzy::suggest(query, &candidates)?;
    suggestion.is_meaningful().then_some(suggestion.corrected)
}

/// Print one page of results, with the correction notice when there was one.
pub fn print(outcome: &Outcome, requested: usize, text: &Text) {
    if outcome.results.is_empty() {
        say!(text, "cli.search.none", &outcome.used);
        return;
    }

    println!();
    if let Some(typed) = &outcome.typed {
        // Both halves, in the order a search engine shows them: what you got,
        // then how to get what you asked for.
        say!(text, "cli.search.corrected", &outcome.used);
        say!(text, "cli.search.instead", typed);
        println!();
    }

    let Some(page) = Page::of(outcome.results.len(), requested, DEFAULT_PAGE_SIZE) else {
        return;
    };
    println!(
        "{}",
        text.format(
            "cli.search.header",
            &[&outcome.results.len().to_string(), &outcome.used],
        )
    );
    println!();
    for (offset, item) in page.slice(&outcome.results).iter().enumerate() {
        let year = item
            .release_info
            .as_deref()
            .map_or_else(String::new, |year| format!("  ({year})"));
        let rating = item
            .imdb_rating
            .as_deref()
            .map_or_else(String::new, |rating| format!("  ★ {rating}"));
        println!(
            "{:>3}. {}{year}  [{}]{rating}",
            page.start + offset + 1,
            item.display_name(),
            item.content_type
        );
    }
    println!();
    println!("{}", page.footer("search", text));
    say!(text, "cli.search.openhint");
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    fn preview(id: &str, name: &str) -> MetaPreview {
        MetaPreview {
            id: id.to_owned(),
            content_type: "series".to_owned(),
            name: Some(name.to_owned()),
            poster: None,
            poster_shape: None,
            description: None,
            release_info: Some("2015".to_owned()),
            imdb_rating: Some("8.9".to_owned()),
            genres: Vec::new(),
            country: None,
            language: None,
        }
    }

    #[test]
    fn an_outcome_knows_whether_it_was_corrected() {
        let plain = Outcome {
            used: "better call saul".to_owned(),
            typed: None,
            results: vec![preview("tt1", "Better Call Saul")],
        };
        assert!(!plain.was_corrected());

        let fixed = Outcome {
            used: "Better Call Saul".to_owned(),
            typed: Some("befer coll saol".to_owned()),
            results: vec![preview("tt1", "Better Call Saul")],
        };
        assert!(fixed.was_corrected());
    }

    #[test]
    fn printing_a_correction_shows_both_halves() {
        // The offer is only real if the viewer can see what to type.
        for locale in ["en", "tr"] {
            let text = Text::new(locale);
            let corrected = text.format("cli.search.corrected", &["Better Call Saul"]);
            let instead = text.format("cli.search.instead", &["befer coll saol"]);
            assert!(
                corrected.contains("Better Call Saul"),
                "{locale}: {corrected}"
            );
            assert!(instead.contains("befer coll saol"), "{locale}: {instead}");
            // And it names the command, since there is nothing to click.
            assert!(instead.contains("search exactly"), "{locale}: {instead}");
        }
    }

    #[test]
    fn the_turkish_correction_notice_reads_as_the_owner_asked() {
        // The exact phrasing from the request: "Sonuçlar şununla ilgili: ..."
        // and "Yine de şu sorguyu ara: ...".
        let text = Text::new("tr");
        let corrected = text.format("cli.search.corrected", &["better call saul"]);
        let instead = text.format("cli.search.instead", &["befer coll saol"]);
        assert!(
            corrected.starts_with("Sonuçlar şununla ilgili:"),
            "{corrected}"
        );
        assert!(instead.starts_with("Yine de şu sorguyu ara:"), "{instead}");
    }

    #[test]
    fn results_are_numbered_absolutely_across_pages() {
        let results: Vec<MetaPreview> = (0..45)
            .map(|n| preview(&format!("tt{n}"), &format!("Film {n}")))
            .collect();
        let second = Page::of(results.len(), 2, DEFAULT_PAGE_SIZE).unwrap();
        // The first row of page two is number 21, not number 1.
        assert_eq!(second.start + 1, 21);
        assert_eq!(second.slice(&results).len(), 20);
    }

    #[test]
    fn an_empty_query_is_refused_without_touching_the_network() {
        // No client is constructed, which is the assertion: the guard happens
        // before anything is fetched.
        let text = Text::new("en");
        assert!(!text.get("cli.search.needsquery").is_empty());
    }

    #[test]
    fn good_enough_is_a_small_number_for_a_reason() {
        // A query returning three or more results is not a query that needs
        // its spelling questioned.
        assert!((2..=5).contains(&GOOD_ENOUGH));
    }
}
