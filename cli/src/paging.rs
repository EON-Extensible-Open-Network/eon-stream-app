// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 EON contributors
//
// Additional permission under GNU GPL version 3 section 7:
// see LICENSE-EXCEPTION.md (EON Module ABI Exception 1.0).

//! Pages, for lists that are longer than a terminal.
//!
//! This exists because of a specific defect: subtitle listings printed the
//! first forty tracks and then `... 38 more`, and there was no way to reach
//! the thirty-eight. A truncation notice without a way past it is worse than
//! either showing everything or showing less — it tells the viewer that what
//! they want exists and that they cannot have it.
//!
//! ## Numbers are absolute, never per page
//!
//! `sub 47` has to mean the forty-seventh subtitle whatever page is on screen.
//! Restarting at 1 on each page would make every number ambiguous the moment
//! two pages have been seen, and the whole program addresses things by the
//! number last printed beside them. So a page knows where it starts in the
//! whole list, and the caller prints `start + offset + 1`.
//!
//! ## An out-of-range page is clamped, not refused
//!
//! Asking for page 9 of 3 gets page 3, with the footer saying so. Someone
//! paging forward does not know where the end is, and "there is no page 9" is
//! a worse answer than the last page when the last page is obviously what
//! they wanted.

use crate::text::Text;

/// How many rows a page holds by default.
///
/// Twenty fits a terminal window with the header, the footer and the prompt
/// still visible, which is the number that matters rather than a round one.
pub const DEFAULT_PAGE_SIZE: usize = 20;

/// One page of a list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Page {
    /// Which page this is, counting from 1.
    pub number: usize,
    /// How many pages there are in total, at least 1.
    pub total: usize,
    /// Index of the first item on this page within the whole list.
    pub start: usize,
    /// Index one past the last item on this page.
    pub end: usize,
    /// How many items the whole list holds.
    pub items: usize,
}

impl Page {
    /// The page containing `requested`, clamped into range.
    ///
    /// Returns `None` only for an empty list, which has no pages at all — a
    /// caller showing "page 1 of 1" above nothing is a caller that should have
    /// said "none" instead.
    #[must_use]
    pub fn of(items: usize, requested: usize, size: usize) -> Option<Self> {
        if items == 0 {
            return None;
        }
        let size = size.max(1);
        let total = items.div_ceil(size);
        let number = requested.clamp(1, total);
        let start = (number - 1) * size;
        Some(Self {
            number,
            total,
            start,
            end: (start + size).min(items),
            items,
        })
    }

    /// The first page at the default size, for the common case.
    #[cfg(test)]
    #[must_use]
    fn first(items: usize) -> Option<Self> {
        Self::of(items, 1, DEFAULT_PAGE_SIZE)
    }

    /// The slice of indices this page covers.
    #[must_use]
    pub fn range(&self) -> std::ops::Range<usize> {
        self.start..self.end
    }

    /// Whether there is nothing after this page.
    #[must_use]
    pub fn is_last(&self) -> bool {
        self.number >= self.total
    }

    /// Whether the whole list fits on this one page.
    #[must_use]
    pub fn is_only(&self) -> bool {
        self.total == 1
    }

    /// The page after this one, or this one if it is the last.
    #[must_use]
    pub fn next(&self) -> usize {
        (self.number + 1).min(self.total)
    }

    /// Take this page's items out of a slice.
    #[must_use]
    pub fn slice<'a, T>(&self, all: &'a [T]) -> &'a [T] {
        all.get(self.range()).unwrap_or(&[])
    }

    /// The line printed under a paged list.
    ///
    /// Says where you are, how much there is, and the exact command that moves
    /// forward — because a page indicator that does not tell you how to turn
    /// the page is decoration.
    #[must_use]
    pub fn footer(&self, command: &str, text: &Text) -> String {
        if self.is_only() {
            return text.format("cli.page.only", &[&self.items.to_string()]);
        }
        if self.is_last() {
            return text.format(
                "cli.page.last",
                &[
                    &self.number.to_string(),
                    &self.total.to_string(),
                    &self.items.to_string(),
                    &format!("{command} 1"),
                ],
            );
        }
        text.format(
            "cli.page.more",
            &[
                &self.number.to_string(),
                &self.total.to_string(),
                &self.items.to_string(),
                &format!("{command} {}", self.next()),
            ],
        )
    }
}

/// Read a page number out of a command's arguments.
///
/// Anything unparseable is page 1 rather than an error: `subs` and `subs
/// please` should both show the first page, and a viewer who typed something
/// odd wanted a list either way.
#[must_use]
pub fn requested(rest: &[&str]) -> usize {
    rest.first()
        .and_then(|first| first.parse::<usize>().ok())
        .filter(|page| *page > 0)
        .unwrap_or(1)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_list_has_no_pages() {
        // Better than "page 1 of 1" printed above nothing.
        assert!(Page::of(0, 1, 20).is_none());
        assert!(Page::first(0).is_none());
    }

    #[test]
    fn a_short_list_is_one_page() {
        let page = Page::of(5, 1, 20).unwrap();
        assert_eq!(page.number, 1);
        assert_eq!(page.total, 1);
        assert_eq!(page.range(), 0..5);
        assert!(page.is_only());
        assert!(page.is_last());
    }

    #[test]
    fn the_case_this_was_written_for() {
        // 78 subtitles: the listing used to show 40 and say "... 38 more",
        // with no way to reach them.
        let first = Page::of(78, 1, 20).unwrap();
        assert_eq!(first.total, 4);
        assert_eq!(first.range(), 0..20);
        assert!(!first.is_last());
        assert_eq!(first.next(), 2);

        let last = Page::of(78, 4, 20).unwrap();
        assert_eq!(last.range(), 60..78);
        assert!(last.is_last());
        // Every item is reachable, which is the whole point.
        let covered: usize = (1..=4)
            .map(|n| Page::of(78, n, 20).unwrap().range().len())
            .sum();
        assert_eq!(covered, 78);
    }

    #[test]
    fn an_exact_multiple_does_not_produce_a_trailing_empty_page() {
        let page = Page::of(40, 1, 20).unwrap();
        assert_eq!(page.total, 2);
        assert_eq!(Page::of(40, 2, 20).unwrap().range(), 20..40);
    }

    #[test]
    fn an_out_of_range_page_is_clamped_to_the_last() {
        // Someone paging forward does not know where the end is.
        let page = Page::of(78, 99, 20).unwrap();
        assert_eq!(page.number, 4);
        assert!(page.is_last());
        // And zero clamps up rather than underflowing.
        assert_eq!(Page::of(78, 0, 20).unwrap().number, 1);
    }

    #[test]
    fn numbers_are_absolute_across_pages() {
        // `sub 47` must mean the forty-seventh subtitle whatever is on screen.
        let third = Page::of(78, 3, 20).unwrap();
        assert_eq!(third.start, 40);
        // The caller prints start + offset + 1, so the first row here is 41.
        assert_eq!(third.start + 1, 41);
        assert_eq!(third.start + 6 + 1, 47);
    }

    #[test]
    fn slicing_never_panics_on_a_stale_page() {
        // A page computed before the list shrank must not index out of bounds.
        let page = Page::of(78, 4, 20).unwrap();
        let shrunk: Vec<u8> = vec![1, 2, 3];
        assert!(page.slice(&shrunk).is_empty());

        let full: Vec<usize> = (0..78).collect();
        assert_eq!(page.slice(&full).len(), 18);
    }

    #[test]
    fn a_page_size_of_zero_does_not_divide_by_zero() {
        let page = Page::of(10, 1, 0).unwrap();
        assert_eq!(page.total, 10);
        assert_eq!(page.range(), 0..1);
    }

    #[test]
    fn a_requested_page_is_read_loosely() {
        assert_eq!(requested(&["3"]), 3);
        assert_eq!(requested(&[]), 1);
        assert_eq!(requested(&["please"]), 1);
        assert_eq!(requested(&["0"]), 1);
        assert_eq!(requested(&["-2"]), 1);
        assert_eq!(requested(&["2", "extra"]), 2);
    }

    #[test]
    fn the_footer_names_the_command_that_turns_the_page() {
        // A page indicator that does not say how to move is decoration.
        let text = Text::new("en");
        let first = Page::of(78, 1, 20).unwrap();
        let line = first.footer("subs", &text);
        assert!(line.contains("subs 2"), "{line}");
        assert!(line.contains("78"), "{line}");

        let last = Page::of(78, 4, 20).unwrap();
        let line = last.footer("subs", &text);
        assert!(line.contains("subs 1"), "{line}");

        // One page says how many there are and nothing about paging.
        let only = Page::of(5, 1, 20).unwrap();
        let line = only.footer("subs", &text);
        assert!(!line.contains("subs"), "{line}");
    }

    #[test]
    fn the_footer_is_translated() {
        let english = Page::of(78, 1, 20)
            .unwrap()
            .footer("subs", &Text::new("en"));
        let turkish = Page::of(78, 1, 20)
            .unwrap()
            .footer("subs", &Text::new("tr"));
        assert_ne!(english, turkish);
        // Both still carry the command, which is not translated.
        assert!(english.contains("subs 2"));
        assert!(turkish.contains("subs 2"));
    }
}
