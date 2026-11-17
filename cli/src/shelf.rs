// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 EON contributors
//
// Additional permission under GNU GPL version 3 section 7:
// see LICENSE-EXCEPTION.md (EON Module ABI Exception 1.0).

//! The library, from the command line.
//!
//! Stremio's "My Library" is the most visible thing this project did not have,
//! and the difference is where it lives: theirs is tied to an account and
//! synced to a server, and this is a file beside the executable
//! ([`eon_stream_core::library`]).
//!
//! Everything here addresses items by the number last printed beside them, so
//! `save 3` keeps the third thing in whatever list is on screen — a search
//! result, a catalogue page, a continue-watching row. One rule, every list.

use eon_stream_core::{history::now_unix, types::MetaPreview, Library, LibraryOrder};

use crate::{
    paging::{Page, DEFAULT_PAGE_SIZE},
    text::{say, Text},
};

/// Keep the item at a one-based position in the last list shown.
///
/// Returns whether the library changed, so the caller knows to save.
pub fn save(library: &mut Library, items: &[MetaPreview], rest: &[&str], text: &Text) -> bool {
    let Some(item) = pick(items, rest, text) else {
        return false;
    };
    let name = item.display_name().to_owned();
    if library.contains(&item.content_type, &item.id) {
        say!(text, "cli.library.already", &name);
        return false;
    }
    library.save_preview(item, None, now_unix());
    say!(text, "cli.library.saved", &name);
    true
}

/// Stop keeping the item at a position in the last list shown.
pub fn unsave(library: &mut Library, items: &[MetaPreview], rest: &[&str], text: &Text) -> bool {
    let Some(item) = pick(items, rest, text) else {
        return false;
    };
    let name = item.display_name().to_owned();
    if library.remove(&item.content_type, &item.id) {
        say!(text, "cli.library.removed", &name);
        true
    } else {
        say!(text, "cli.library.notin", &name);
        false
    }
}

/// Resolve `rest` as a position in `items`.
fn pick<'a>(items: &'a [MetaPreview], rest: &[&str], text: &Text) -> Option<&'a MetaPreview> {
    let Some(first) = rest.first() else {
        say!(text, "cli.error.needsargument");
        return None;
    };
    let Ok(position) = first.parse::<usize>() else {
        say!(text, "cli.error.notanumber", first);
        return None;
    };
    match position.checked_sub(1).and_then(|index| items.get(index)) {
        Some(item) => Some(item),
        None => {
            if items.is_empty() {
                say!(text, "cli.error.nolist");
            } else {
                say!(text, "cli.error.outofrange", first);
            }
            None
        }
    }
}

/// Print a page of the library, and return what was listed.
///
/// The returned previews become the last list shown, so `open <n>` and
/// `unsave <n>` work on them exactly as they would on a catalogue page.
pub fn list(library: &Library, rest: &[&str], text: &Text) -> Vec<MetaPreview> {
    if library.is_empty() {
        say!(text, "cli.library.empty");
        return Vec::new();
    }

    // `library 2` is a page; `library series` is a kind. Both are natural to
    // type, and a number is never a content type, so there is no ambiguity to
    // resolve.
    let (kind, requested) = match rest.first() {
        None => (None, 1),
        Some(first) => match first.parse::<usize>() {
            Ok(page) => (None, page.max(1)),
            Err(_) => (
                Some((*first).to_owned()),
                crate::paging::requested(&rest[1..]),
            ),
        },
    };

    let entries = match &kind {
        None => library.list(LibraryOrder::Recent),
        Some(wanted) => {
            let found = library.of_kind(wanted, LibraryOrder::Recent);
            if found.is_empty() {
                say!(
                    text,
                    "cli.library.nosuchkind",
                    wanted,
                    &library.kinds().join(", ")
                );
                return Vec::new();
            }
            found
        }
    };

    let previews: Vec<MetaPreview> = entries
        .iter()
        .map(|entry| MetaPreview {
            id: entry.id.clone(),
            content_type: entry.content_type.clone(),
            name: Some(entry.name.clone()),
            poster: entry.poster.clone(),
            poster_shape: None,
            description: None,
            release_info: entry.release_info.clone(),
            imdb_rating: None,
        })
        .collect();

    let Some(page) = Page::of(previews.len(), requested, DEFAULT_PAGE_SIZE) else {
        say!(text, "cli.library.empty");
        return Vec::new();
    };

    println!();
    say!(text, "cli.library.header", &previews.len().to_string());
    println!();
    for (offset, entry) in page.slice(&entries).iter().enumerate() {
        println!("{:>3}. {}", page.start + offset + 1, entry.display_line());
    }
    println!();
    // The page command carries the kind when one was given, so turning a page
    // does not silently drop the filter.
    let command = match &kind {
        Some(wanted) => format!("library {wanted}"),
        None => "library".to_owned(),
    };
    println!("{}", page.footer(&command, text));
    say!(text, "cli.library.openhint");

    previews
}

/// Whether an item is kept, for the marker shown when it is opened.
#[must_use]
pub fn marker(library: &Library, content_type: &str, id: &str, text: &Text) -> Option<String> {
    library
        .contains(content_type, id)
        .then(|| text.get("cli.library.inlibrary"))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    fn preview(id: &str, name: &str, kind: &str) -> MetaPreview {
        MetaPreview {
            id: id.to_owned(),
            content_type: kind.to_owned(),
            name: Some(name.to_owned()),
            poster: None,
            poster_shape: None,
            description: None,
            release_info: Some("2015".to_owned()),
            imdb_rating: None,
        }
    }

    fn text() -> Text {
        Text::new("en")
    }

    #[test]
    fn saving_by_the_number_on_screen() {
        let items = vec![
            preview("tt1", "Better Call Saul", "series"),
            preview("tt2", "Breaking Bad", "series"),
        ];
        let mut library = Library::new();
        assert!(save(&mut library, &items, &["2"], &text()));
        assert!(library.contains("series", "tt2"));
        assert!(!library.contains("series", "tt1"));
    }

    #[test]
    fn saving_the_same_thing_twice_says_so_and_changes_nothing() {
        let items = vec![preview("tt1", "Better Call Saul", "series")];
        let mut library = Library::new();
        assert!(save(&mut library, &items, &["1"], &text()));
        assert!(
            !save(&mut library, &items, &["1"], &text()),
            "a second save is not a change"
        );
        assert_eq!(library.len(), 1);
    }

    #[test]
    fn unsaving_removes_it() {
        let items = vec![preview("tt1", "Better Call Saul", "series")];
        let mut library = Library::new();
        save(&mut library, &items, &["1"], &text());
        assert!(unsave(&mut library, &items, &["1"], &text()));
        assert!(library.is_empty());
        // And again is not a change.
        assert!(!unsave(&mut library, &items, &["1"], &text()));
    }

    #[test]
    fn a_bad_position_is_reported_without_panicking() {
        let items = vec![preview("tt1", "A", "movie")];
        let mut library = Library::new();
        assert!(!save(&mut library, &items, &["0"], &text()));
        assert!(!save(&mut library, &items, &["9"], &text()));
        assert!(!save(&mut library, &items, &["apple"], &text()));
        assert!(!save(&mut library, &items, &[], &text()));
        // And with nothing listed at all, which is a different message.
        assert!(!save(&mut library, &[], &["1"], &text()));
        assert!(library.is_empty());
    }

    #[test]
    fn listing_an_empty_library_says_how_to_fill_it() {
        let library = Library::new();
        let listed = list(&library, &[], &text());
        assert!(listed.is_empty());
    }

    #[test]
    fn listing_returns_previews_so_open_works_on_them() {
        // The point: a library row has to be openable by its number, like any
        // other list in the program.
        let items = vec![
            preview("tt1", "Better Call Saul", "series"),
            preview("tt2", "The Godfather", "movie"),
        ];
        let mut library = Library::new();
        save(&mut library, &items, &["1"], &text());
        save(&mut library, &items, &["2"], &text());

        let listed = list(&library, &[], &text());
        assert_eq!(listed.len(), 2);
        assert!(listed.iter().any(|p| p.id == "tt1"));
        assert!(listed.iter().any(|p| p.content_type == "movie"));
    }

    #[test]
    fn a_kind_narrows_the_listing_and_a_number_pages_it() {
        let items = vec![
            preview("tt1", "A Show", "series"),
            preview("tt2", "A Film", "movie"),
        ];
        let mut library = Library::new();
        save(&mut library, &items, &["1"], &text());
        save(&mut library, &items, &["2"], &text());

        assert_eq!(list(&library, &["series"], &text()).len(), 1);
        assert_eq!(list(&library, &["movie"], &text()).len(), 1);
        // A number is a page, not a kind -- and page 1 of two items is both.
        assert_eq!(list(&library, &["1"], &text()).len(), 2);
        // A kind nobody kept lists nothing rather than everything.
        assert!(list(&library, &["music"], &text()).is_empty());
    }

    #[test]
    fn the_marker_only_appears_for_kept_items() {
        let items = vec![preview("tt1", "A", "movie")];
        let mut library = Library::new();
        assert!(marker(&library, "movie", "tt1", &text()).is_none());
        save(&mut library, &items, &["1"], &text());
        assert!(marker(&library, "movie", "tt1", &text()).is_some());
        assert!(marker(&library, "series", "tt1", &text()).is_none());
    }

    #[test]
    fn library_messages_exist_in_both_languages() {
        for locale in ["en", "tr"] {
            let text = Text::new(locale);
            for key in [
                "cli.library.saved",
                "cli.library.already",
                "cli.library.removed",
                "cli.library.notin",
                "cli.library.empty",
                "cli.library.header",
                "cli.library.openhint",
                "cli.library.nosuchkind",
                "cli.library.inlibrary",
            ] {
                assert_ne!(text.get(key), key, "{locale} is missing {key}");
            }
        }
    }
}
