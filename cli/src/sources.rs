// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 EON contributors
//
// Additional permission under GNU GPL version 3 section 7:
// see LICENSE-EXCEPTION.md (EON Module ABI Exception 1.0).

//! Choosing a source out of a hundred of them.
//!
//! A popular film returns well over a hundred sources across a few addons,
//! and printing all of them was the whole of the old behaviour. That is a wall
//! of text where the useful question is narrow: *which quality, and then which
//! of those*.
//!
//! So opening something now shows the qualities and how many sources each has.
//! `res <n>` picks one, and the sources inside it page like everything else.
//!
//! ## Numbers stay absolute, and that is what makes this safe
//!
//! Every source keeps the number it had in the full ranked list for as long as
//! the list stands. Picking a quality, applying a filter and turning a page all
//! change *which* rows are on screen and never what any row is called. So
//! `play 47` is the forty-seventh source whatever has been narrowed since, and
//! a number read off the screen a minute ago still means what it meant.
//!
//! Renumbering per view would have been easier to print and would make every
//! number a lie the moment a filter changed.

use eon_stream_core::{
    language,
    ranking::{RankedStream, Resolution},
    AddonRegistry,
};

use crate::{
    paging::{Page, DEFAULT_PAGE_SIZE},
    text::{say, Text},
};

/// How the visible sources are ordered, beyond the default ranking.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Filter {
    /// The ranking the preferences produced, untouched.
    #[default]
    None,
    /// Most widely shared first — the closest thing to "most watched" that a
    /// torrent actually tells us.
    Views,
    /// Smallest first, for a slow connection or a small disk.
    Size,
    /// Sources carrying a subtitle in a preferred language first.
    Subtitles,
}

impl Filter {
    /// Read a filter out of what the viewer typed.
    #[must_use]
    pub fn parse(word: &str) -> Option<Self> {
        match word.to_ascii_lowercase().as_str() {
            "none" | "off" | "clear" | "default" => Some(Self::None),
            // `view`, `views`, `seeds`, `popular` all mean the same thing to
            // the person typing it.
            "view" | "views" | "seed" | "seeds" | "seeders" | "popular" => Some(Self::Views),
            "size" | "small" | "smallest" => Some(Self::Size),
            "sub" | "subs" | "subtitle" | "subtitles" => Some(Self::Subtitles),
            _ => None,
        }
    }

    /// Catalogue key describing what this filter did.
    #[must_use]
    pub const fn message_key(self) -> &'static str {
        match self {
            Self::None => "cli.filter.none",
            Self::Views => "cli.filter.view",
            Self::Size => "cli.filter.size",
            Self::Subtitles => "cli.filter.sub",
        }
    }

    /// Short label for the status line.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::None => "default",
            Self::Views => "most shared",
            Self::Size => "smallest",
            Self::Subtitles => "subtitled",
        }
    }
}

/// A quality bucket, coarser than a resolution so "unknown" has a home.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Quality(Option<Resolution>);

impl Quality {
    /// Which bucket a source falls in.
    #[must_use]
    pub fn of(source: &RankedStream) -> Self {
        Self(source.facts.resolution)
    }

    /// What to call it on screen.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self.0 {
            Some(Resolution::UltraHd) => "4K",
            Some(Resolution::QuadHd) => "1440p",
            Some(Resolution::FullHd) => "1080p",
            Some(Resolution::Hd) => "720p",
            Some(Resolution::Low) => "low / cam",
            // Plenty of addons write no quality at all, and those sources are
            // often fine. They get a bucket rather than being hidden.
            None => "unlabelled",
        }
    }
}

/// Quality buckets present, best first, with how many sources each holds.
#[must_use]
pub fn qualities(sources: &[RankedStream]) -> Vec<(Quality, usize)> {
    let mut buckets: Vec<(Quality, usize)> = Vec::new();
    for source in sources {
        let quality = Quality::of(source);
        match buckets
            .iter_mut()
            .find(|(existing, _)| *existing == quality)
        {
            Some((_, count)) => *count += 1,
            None => buckets.push((quality, 1)),
        }
    }
    // Best first, with the unlabelled bucket last rather than first: `None`
    // sorts below `Some` naturally, and reversing would put it at the top
    // where it would look like the best option.
    buckets.sort_by_key(|(quality, _)| std::cmp::Reverse(*quality));
    buckets
}

/// Which of `sources` to show, by index into the full list.
///
/// Returns indices rather than references so the caller can keep printing the
/// absolute number — the thing that makes `play <n>` mean one source for the
/// life of the listing.
#[must_use]
pub fn visible(
    sources: &[RankedStream],
    quality: Option<Quality>,
    filter: Filter,
    subtitle_languages: &[String],
) -> Vec<usize> {
    let mut chosen: Vec<usize> = sources
        .iter()
        .enumerate()
        .filter(|(_, source)| quality.is_none_or(|wanted| Quality::of(source) == wanted))
        .map(|(index, _)| index)
        .collect();

    match filter {
        Filter::None => {}
        Filter::Views => {
            // Unknown seeder counts sink rather than sorting as zero among
            // real zeroes: "we do not know" is not "nobody has it".
            chosen.sort_by(|a, b| {
                let seeds = |i: &usize| sources[*i].facts.seeders;
                seeds(b)
                    .unwrap_or(0)
                    .cmp(&seeds(a).unwrap_or(0))
                    .then_with(|| seeds(b).is_some().cmp(&seeds(a).is_some()))
                    .then_with(|| a.cmp(b))
            });
        }
        Filter::Size => {
            // A source with no stated size cannot be called smallest, so it
            // goes after every source that does state one.
            chosen.sort_by(|a, b| {
                let size = |i: &usize| sources[*i].facts.size_bytes;
                match (size(a), size(b)) {
                    (Some(x), Some(y)) => x.cmp(&y).then_with(|| a.cmp(b)),
                    (Some(_), None) => std::cmp::Ordering::Less,
                    (None, Some(_)) => std::cmp::Ordering::Greater,
                    (None, None) => a.cmp(b),
                }
            });
        }
        Filter::Subtitles => {
            // Promote, never remove. A source without the language might still
            // be the only one that plays, and the viewer can load a subtitle
            // separately — so this reorders rather than filtering.
            chosen.sort_by_key(|index| {
                let source = &sources[*index];
                let score = subtitle_score(source, subtitle_languages);
                (std::cmp::Reverse(score), *index)
            });
        }
    }
    chosen
}

/// How well a source's own subtitles match the languages asked for.
///
/// 2 for a named language match, 1 for subtitles of some unknown language,
/// 0 for none. Three levels rather than a boolean because "has subtitles, but
/// we cannot tell which" is genuinely better than "has none".
fn subtitle_score(source: &RankedStream, languages: &[String]) -> u8 {
    if source.stream.subtitles.is_empty() {
        return 0;
    }
    if languages.is_empty() {
        return 1;
    }
    // Through the language table, not a prefix comparison. Addons write `tr`,
    // `tur` and `Turkish` for one language, and `tur` is not a prefix of `tr`
    // -- so the naive version failed for exactly the language this project
    // cares most about. See `eon_stream_core::language`.
    let matches = source.stream.subtitles.iter().any(|subtitle| {
        subtitle
            .lang
            .as_deref()
            .is_some_and(|lang| language::matches_any(lang, languages))
    });
    if matches {
        2
    } else {
        1
    }
}

/// Print the quality buckets and how to pick one.
pub fn print_qualities(sources: &[RankedStream], text: &Text) {
    say!(text, "cli.sources.qualities");
    for (position, (quality, count)) in qualities(sources).iter().enumerate() {
        println!(
            "{:>3}. {:<12} {}",
            position + 1,
            quality.label(),
            text.format("cli.sources.count", &[&count.to_string()])
        );
    }
    println!();
    say!(text, "cli.sources.pick");
}

/// Print one page of the visible sources.
///
/// `visible` holds indices into `sources`; the number printed is the index
/// plus one, which is the absolute position in the full ranked list.
pub fn print_page(
    sources: &[RankedStream],
    visible: &[usize],
    registry: &AddonRegistry,
    requested: usize,
    showing: &str,
    filter: Filter,
    text: &Text,
) {
    let Some(page) = Page::of(visible.len(), requested, DEFAULT_PAGE_SIZE) else {
        say!(text, "cli.sources.none");
        return;
    };

    println!();
    let mut status = text.format("cli.sources.showing", &[showing]);
    if filter != Filter::None {
        status.push_str("   ");
        status.push_str(&text.format("cli.filter.current", &[filter.label()]));
    }
    println!("{status}");
    println!();

    for index in page.slice(visible) {
        let Some(source) = sources.get(*index) else {
            continue;
        };
        let addon = registry
            .get(&source.addon_id)
            .map_or(source.addon_id.as_str(), |a| a.manifest.name.as_str());
        let mut extras = Vec::new();
        if let Some(seeders) = source.facts.seeders {
            extras.push(text.format("cli.sources.seeders", &[&seeders.to_string()]));
        }
        if !source.stream.subtitles.is_empty() {
            extras.push(text.format(
                "cli.sources.subcount",
                &[&source.stream.subtitles.len().to_string()],
            ));
        }
        let tail = if extras.is_empty() {
            String::new()
        } else {
            format!("  ·  {}", extras.join("  "))
        };
        println!(
            "{:>4}. [{:>7}] {}  ·  {addon}{tail}",
            index + 1,
            crate::source_kind(&source.stream),
            source.display_label()
        );
    }

    println!();
    println!("{}", page.footer("sources", text));
    say!(text, "cli.sources.playhint");
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use eon_stream_core::{ranking::StreamFacts, Stream};

    fn source(label: &str, subtitles: &[&str]) -> RankedStream {
        let subs: Vec<String> = subtitles
            .iter()
            .map(|lang| format!(r#"{{"id":"s","url":"https://x/s.srt","lang":"{lang}"}}"#))
            .collect();
        let json = format!(
            r#"{{"name":"{label}","url":"https://example.org/v.mkv","subtitles":[{}]}}"#,
            subs.join(",")
        );
        let stream: Stream = serde_json::from_str(&json).expect("a stream");
        let facts = StreamFacts::read(&stream, 0);
        RankedStream {
            addon_id: "test.addon".to_owned(),
            stream,
            facts,
        }
    }

    #[test]
    fn qualities_are_grouped_best_first_with_unlabelled_last() {
        let sources = vec![
            source("Movie 720p", &[]),
            source("Movie 2160p HDR", &[]),
            source("Movie", &[]),
            source("Movie 1080p", &[]),
            source("Another 1080p", &[]),
        ];
        let buckets = qualities(&sources);
        let labels: Vec<&str> = buckets.iter().map(|(q, _)| q.label()).collect();
        assert_eq!(labels, vec!["4K", "1080p", "720p", "unlabelled"]);
        // Counts are right, and 1080p has two.
        assert_eq!(buckets[1].1, 2);
    }

    #[test]
    fn picking_a_quality_keeps_absolute_numbers() {
        // The property that makes `play 47` safe after narrowing.
        let sources = vec![
            source("A 720p", &[]),
            source("B 1080p", &[]),
            source("C 720p", &[]),
            source("D 1080p", &[]),
        ];
        let full_hd = Quality(Some(Resolution::FullHd));
        let shown = visible(&sources, Some(full_hd), Filter::None, &[]);
        // Indices 1 and 3, which print as 2 and 4 -- their position in the
        // whole list, not 1 and 2.
        assert_eq!(shown, vec![1, 3]);
    }

    #[test]
    fn no_quality_selected_shows_everything() {
        let sources = vec![source("A 720p", &[]), source("B", &[])];
        assert_eq!(visible(&sources, None, Filter::None, &[]), vec![0, 1]);
    }

    #[test]
    fn the_views_filter_sinks_unknown_seeder_counts() {
        // "We do not know" is not "nobody has it".
        let sources = vec![
            source("A 1080p", &[]), // no seeders stated
            source("B 1080p 👤 120", &[]),
            source("C 1080p 👤 5", &[]),
        ];
        let shown = visible(&sources, None, Filter::Views, &[]);
        assert_eq!(shown.first(), Some(&1), "120 seeders should lead");
        assert_eq!(shown.last(), Some(&0), "the unknown one goes last");
    }

    #[test]
    fn the_size_filter_puts_unstated_sizes_after_stated_ones() {
        // A source with no size cannot be called the smallest.
        let sources = vec![
            source("A 1080p", &[]), // no size stated
            source("B 1080p 8 GB", &[]),
            source("C 1080p 1.2 GB", &[]),
        ];
        let shown = visible(&sources, None, Filter::Size, &[]);
        assert_eq!(shown, vec![2, 1, 0]);
    }

    #[test]
    fn the_subtitle_filter_promotes_and_never_removes() {
        // A source without the language might be the only one that plays.
        let sources = vec![
            source("A 1080p", &[]),
            source("B 1080p", &["eng"]),
            source("C 1080p", &["tur"]),
        ];
        let wanted = vec!["tur".to_owned()];
        let shown = visible(&sources, None, Filter::Subtitles, &wanted);
        assert_eq!(shown.len(), 3, "nothing is dropped");
        assert_eq!(shown[0], 2, "the Turkish one leads");
        // Subtitles of another language still beat none at all.
        assert_eq!(shown[1], 1);
        assert_eq!(shown[2], 0);
    }

    #[test]
    fn subtitle_languages_match_across_the_spellings_addons_use() {
        // Addons write `tr`, `tur` and sometimes more for one language.
        let two_letter = source("A", &["tr"]);
        let three_letter = source("B", &["tur"]);
        assert_eq!(subtitle_score(&two_letter, &["tur".to_owned()]), 2);
        assert_eq!(subtitle_score(&three_letter, &["tr".to_owned()]), 2);
        assert_eq!(subtitle_score(&two_letter, &["eng".to_owned()]), 1);
        assert_eq!(subtitle_score(&source("C", &[]), &["tur".to_owned()]), 0);
        // With no preference set, any subtitles are better than none.
        assert_eq!(subtitle_score(&two_letter, &[]), 1);
    }

    #[test]
    fn filters_are_named_the_way_people_type_them() {
        assert_eq!(Filter::parse("view"), Some(Filter::Views));
        assert_eq!(Filter::parse("views"), Some(Filter::Views));
        assert_eq!(Filter::parse("seeds"), Some(Filter::Views));
        assert_eq!(Filter::parse("SIZE"), Some(Filter::Size));
        assert_eq!(Filter::parse("smallest"), Some(Filter::Size));
        assert_eq!(Filter::parse("sub"), Some(Filter::Subtitles));
        assert_eq!(Filter::parse("subtitles"), Some(Filter::Subtitles));
        assert_eq!(Filter::parse("none"), Some(Filter::None));
        assert_eq!(Filter::parse("clear"), Some(Filter::None));
        assert_eq!(Filter::parse("sideways"), None);
    }

    #[test]
    fn every_filter_has_text_in_both_languages() {
        for locale in ["en", "tr"] {
            let text = Text::new(locale);
            for filter in [Filter::None, Filter::Views, Filter::Size, Filter::Subtitles] {
                let key = filter.message_key();
                assert_ne!(text.get(key), key, "{locale} is missing {key}");
            }
        }
    }

    #[test]
    fn an_empty_source_list_has_no_qualities_and_no_page() {
        let sources: Vec<RankedStream> = Vec::new();
        assert!(qualities(&sources).is_empty());
        assert!(visible(&sources, None, Filter::Views, &[]).is_empty());
    }

    #[test]
    fn filtering_is_stable_so_a_listing_does_not_shuffle() {
        // Two sources that tie must keep their relative order, or turning a
        // page back shows something different.
        let sources = vec![
            source("A 1080p 2 GB", &[]),
            source("B 1080p 2 GB", &[]),
            source("C 1080p 2 GB", &[]),
        ];
        let first = visible(&sources, None, Filter::Size, &[]);
        let again = visible(&sources, None, Filter::Size, &[]);
        assert_eq!(first, again);
        assert_eq!(first, vec![0, 1, 2]);
    }
}
