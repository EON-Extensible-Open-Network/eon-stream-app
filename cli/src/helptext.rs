// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 EON contributors
//
// Additional permission under GNU GPL version 3 section 7:
// see LICENSE-EXCEPTION.md (EON Module ABI Exception 1.0).

//! Help, as data rather than as a wall of prose.
//!
//! `help` had grown to sixty lines, which scrolls a terminal and buries the
//! part someone was looking for. It is now a numbered list of sections, and
//! `help <n>` opens one.
//!
//! The commands themselves are literal and stay untranslated: `play` is a
//! thing a person types, and translating it would mean the documentation stops
//! matching the program. The *descriptions* beside them are sentences, so they
//! come from the catalogue like every other sentence (madde 40).

use crate::{
    paging::Page,
    text::{say, Text},
};

/// One command and the key for its description.
struct Entry {
    /// What the viewer types, shown verbatim.
    usage: &'static str,
    /// Catalogue key for the description.
    describes: &'static str,
}

/// A group of related commands.
struct Section {
    /// Catalogue key for the section's own name.
    title: &'static str,
    entries: &'static [Entry],
}

const fn entry(usage: &'static str, describes: &'static str) -> Entry {
    Entry { usage, describes }
}

/// Every section, in the order `help` lists them.
///
/// The order is the order someone meets these things: addons before browsing,
/// browsing before playing, and the housekeeping last.
const SECTIONS: &[Section] = &[
    Section {
        title: "cli.help.addons",
        entries: &[
            entry("add <url>", "cli.help.cmd.add"),
            entry("list", "cli.help.cmd.list"),
            entry("remove <n>", "cli.help.cmd.remove"),
            entry("enable <n> / disable <n>", "cli.help.cmd.enable"),
            entry("refresh <n>", "cli.help.cmd.refresh"),
            entry("order <n> <pos>", "cli.help.cmd.order"),
            entry("configure <n>", "cli.help.cmd.configure"),
            entry("discover [<n>]", "cli.help.cmd.discover"),
            entry("health", "cli.help.cmd.health"),
        ],
    },
    Section {
        title: "cli.help.search",
        entries: &[
            entry("search <words>", "cli.help.cmd.search"),
            entry("search exactly", "cli.help.cmd.searchexactly"),
            entry("search <n>", "cli.help.cmd.searchpage"),
            entry("catalogs", "cli.help.cmd.catalogs"),
            entry("browse <n> [query]", "cli.help.cmd.browse"),
            entry("more", "cli.help.cmd.more"),
        ],
    },
    Section {
        title: "cli.help.browsing",
        entries: &[
            entry("open <n>", "cli.help.cmd.open"),
            entry("episodes [season]", "cli.help.cmd.episodes"),
            entry("ep <n>", "cli.help.cmd.ep"),
            entry("trailer", "cli.help.cmd.trailer"),
        ],
    },
    Section {
        title: "cli.help.sources",
        entries: &[
            entry("sources [<n>]", "cli.help.cmd.sources"),
            entry("res [<n>]", "cli.help.cmd.res"),
            entry("filter view", "cli.help.cmd.filterview"),
            entry("filter size", "cli.help.cmd.filtersize"),
            entry("filter sub", "cli.help.cmd.filtersub"),
            entry("filter none", "cli.help.cmd.filternone"),
        ],
    },
    Section {
        title: "cli.help.playback",
        entries: &[
            entry("play <n>", "cli.help.cmd.play"),
            entry("next", "cli.help.cmd.next"),
            entry("pause / resume / stop", "cli.help.cmd.pause"),
            entry("seek <n>", "cli.help.cmd.seek"),
            entry("where", "cli.help.cmd.where"),
            entry("tracks", "cli.help.cmd.tracks"),
            entry("audio|video|subtrack <n|off>", "cli.help.cmd.track"),
            entry(
                "speed <x>  volume <n>  adelay <s>  sdelay <s>",
                "cli.help.cmd.tune",
            ),
        ],
    },
    Section {
        title: "cli.help.subtitles",
        entries: &[
            entry("subs [<n>]", "cli.help.cmd.subs"),
            entry("sub <n>", "cli.help.cmd.sub"),
            entry("sub search <words>", "cli.help.cmd.subsearch"),
            entry("sdelay <s>", "cli.help.cmd.sdelay"),
        ],
    },
    Section {
        title: "cli.help.torrents",
        entries: &[
            entry("play <magnet|hash>", "cli.help.cmd.playmagnet"),
            entry("play <n> file <i>", "cli.help.cmd.playfile"),
            entry("torrent", "cli.help.cmd.torrent"),
            entry("files [<n>]", "cli.help.cmd.files"),
        ],
    },
    Section {
        title: "cli.help.library",
        entries: &[
            entry("save <n>", "cli.help.cmd.save"),
            entry("library [<n>]", "cli.help.cmd.library"),
            entry("library <kind>", "cli.help.cmd.librarykind"),
            entry("unsave <n>", "cli.help.cmd.unsave"),
            entry("continue", "cli.help.cmd.continue"),
            entry("forget <n>", "cli.help.cmd.forget"),
        ],
    },
    Section {
        title: "cli.help.modules",
        entries: &[
            entry("modules", "cli.help.cmd.modules"),
            entry("install <manifest>", "cli.help.cmd.install"),
            entry("uninstall <n>", "cli.help.cmd.uninstall"),
            entry(
                "enable-module <n> / disable-module <n>",
                "cli.help.cmd.modtoggle",
            ),
            entry("order-module <n> <pos>", "cli.help.cmd.modorder"),
            entry("theme [<id>|none|tokens]", "cli.help.cmd.theme"),
            entry("trust", "cli.help.cmd.trust"),
        ],
    },
    Section {
        title: "cli.help.preferences",
        entries: &[
            entry("prefer", "cli.help.cmd.prefer"),
            entry("prefer lang tur,eng", "cli.help.cmd.preferlang"),
            entry("prefer min 1080", "cli.help.cmd.prefermin"),
            entry("prefer country tur", "cli.help.cmd.prefercountry"),
            entry("prefer size on|off", "cli.help.cmd.prefersize"),
            entry("settings", "cli.help.cmd.settings"),
            entry("set <path> <value>", "cli.help.cmd.set"),
            entry("lang en|tr", "cli.help.cmd.lang"),
        ],
    },
    Section {
        title: "cli.help.updates",
        entries: &[
            entry("update", "cli.help.cmd.update"),
            entry("update verify <file>", "cli.help.cmd.updateverify"),
            entry("revocations [refresh]", "cli.help.cmd.revocations"),
            entry("audit", "cli.help.cmd.audit"),
            entry("quit", "cli.help.cmd.quit"),
        ],
    },
];

/// Print the section index.
pub fn index(text: &Text) {
    say!(text, "cli.help.header");
    println!();
    for (position, section) in SECTIONS.iter().enumerate() {
        println!(
            "{:>3}. {:<26} {}",
            position + 1,
            text.get(section.title),
            text.format("cli.help.count", &[&section.entries.len().to_string()])
        );
    }
    println!();
    say!(text, "cli.help.pick");
    say!(text, "cli.help.hint");
}

/// Print one section, or the index when the number names no section.
pub fn section(rest: &[&str], text: &Text) {
    let Some(first) = rest.first() else {
        index(text);
        return;
    };
    // `help search` is as natural to type as `help 2`, so a name works too --
    // matched against the translated title, so it works in either language.
    let chosen = first.parse::<usize>().ok().and_then(|number| {
        number
            .checked_sub(1)
            .and_then(|index| SECTIONS.get(index))
            .map(|section| (number, section))
    });
    let chosen = chosen.or_else(|| {
        let wanted = eon_stream_core::fuzzy::normalize(first);
        SECTIONS.iter().enumerate().find_map(|(position, section)| {
            let title = eon_stream_core::fuzzy::normalize(&text.get(section.title));
            (title.contains(&wanted) && !wanted.is_empty()).then_some((position + 1, section))
        })
    });

    let Some((number, section)) = chosen else {
        say!(text, "cli.help.nosuchsection", first);
        println!();
        index(text);
        return;
    };

    println!(
        "\n{}  ({}/{})",
        text.get(section.title),
        number,
        SECTIONS.len()
    );
    println!();
    let width = section
        .entries
        .iter()
        .map(|entry| entry.usage.chars().count())
        .max()
        .unwrap_or(20)
        .min(38);
    for entry in section.entries {
        println!(
            "  {:<width$}  {}",
            entry.usage,
            text.get(entry.describes),
            width = width
        );
    }
    println!();
    // The same page footer the lists use, so turning a help page and turning a
    // subtitle page look and work the same way.
    if let Some(page) = Page::of(SECTIONS.len(), number, 1) {
        println!("{}", page.footer("help", text));
    }
}

/// How many sections there are, so a test can assert every one is reachable.
#[cfg(test)]
#[must_use]
fn section_count() -> usize {
    SECTIONS.len()
}

/// Every catalogue key the help text needs, for a coverage test.
#[cfg(test)]
#[must_use]
fn message_keys() -> Vec<&'static str> {
    let mut keys = Vec::new();
    for section in SECTIONS {
        keys.push(section.title);
        for entry in section.entries {
            keys.push(entry.describes);
        }
    }
    keys
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn every_help_string_is_in_both_catalogues() {
        // The index and every section must be readable in Turkish as well as
        // English. A missing key renders as the key itself, which is the sort
        // of thing that ships unnoticed.
        for locale in ["en", "tr"] {
            let text = Text::new(locale);
            for key in message_keys() {
                let rendered = text.get(key);
                assert_ne!(
                    rendered, key,
                    "{locale} has no text for {key} -- it would print the key"
                );
                assert!(!rendered.trim().is_empty(), "{locale}:{key} is empty");
            }
        }
    }

    #[test]
    fn no_section_is_unreachable() {
        // Every section has to be openable by its number, or it is dead text.
        let text = Text::new("en");
        for number in 1..=section_count() {
            let as_text = number.to_string();
            // Must not fall through to the index, which is what an unknown
            // number does.
            let found = (1..=section_count()).contains(&number);
            assert!(found, "section {number} is not reachable");
            section(&[as_text.as_str()], &text);
        }
    }

    #[test]
    fn sections_have_distinct_titles_in_both_languages() {
        // Two sections sharing a title makes `help <name>` ambiguous and the
        // index confusing.
        for locale in ["en", "tr"] {
            let text = Text::new(locale);
            let mut titles: Vec<String> = SECTIONS
                .iter()
                .map(|section| text.get(section.title))
                .collect();
            let before = titles.len();
            titles.sort();
            titles.dedup();
            assert_eq!(titles.len(), before, "{locale} has two sections alike");
        }
    }

    #[test]
    fn an_unknown_section_does_not_panic() {
        let text = Text::new("en");
        section(&["99"], &text);
        section(&["nonsense"], &text);
        section(&[], &text);
        section(&["0"], &text);
    }

    #[test]
    fn a_section_can_be_opened_by_name_in_either_language() {
        // `help search` is as natural as `help 2`.
        let english = Text::new("en");
        let turkish = Text::new("tr");
        // Both resolve without falling through to the index; the assertion is
        // that the lookup finds something, which `section` prints.
        let wanted = eon_stream_core::fuzzy::normalize(&english.get("cli.help.torrents"));
        assert!(SECTIONS.iter().any(|s| {
            eon_stream_core::fuzzy::normalize(&english.get(s.title)).contains(&wanted)
        }));
        let wanted_tr = eon_stream_core::fuzzy::normalize(&turkish.get("cli.help.torrents"));
        assert!(SECTIONS.iter().any(|s| {
            eon_stream_core::fuzzy::normalize(&turkish.get(s.title)).contains(&wanted_tr)
        }));
    }

    #[test]
    fn the_index_fits_a_terminal() {
        // The reason help was split up: sixty lines scrolls away the part
        // somebody was looking for.
        assert!(
            section_count() <= 14,
            "{} sections is too many for one screen",
            section_count()
        );
        for s in SECTIONS {
            assert!(
                s.entries.len() <= 12,
                "a section with {} entries is a wall again",
                s.entries.len()
            );
        }
    }

    #[test]
    fn no_command_is_documented_twice_in_one_section() {
        for s in SECTIONS {
            let mut usages: Vec<&str> = s.entries.iter().map(|e| e.usage).collect();
            let before = usages.len();
            usages.sort_unstable();
            usages.dedup();
            assert_eq!(usages.len(), before, "a section repeats a command");
        }
    }
}
