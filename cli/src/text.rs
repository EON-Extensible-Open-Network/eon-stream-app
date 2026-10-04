// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 EON contributors
//
// Additional permission under GNU GPL version 3 section 7:
// see LICENSE-EXCEPTION.md (EON Module ABI Exception 1.0).

//! User-visible text.
//!
//! Every line this program prints that is a *sentence* comes from here, which
//! is madde 40's rule applied to the alpha: no user-visible text embedded in
//! code, translation files separate. The catalogues themselves live in
//! `eon-stream-core/i18n/`, because the eventual interface needs the same
//! strings and duplicating them is how two translations drift apart.
//!
//! What does **not** come from here: command names, numbers, file names,
//! identifiers, and the fixed-width table scaffolding they sit in. `play <n>`
//! is a thing the user types, not a thing they read, and translating a command
//! name would mean the documentation stops matching the program.

use eon_stream_core::i18n::Messages;

/// The loaded catalogues.
///
/// The catalogues are `Option`, not a value with a hard-coded emergency
/// fallback. The only way to have none is for a catalogue compiled into this
/// binary to fail to parse — a build problem, caught by the test suite in
/// `eon-stream-core` — and the honest behaviour then is for every message to
/// come out as its own key. That is ugly and completely diagnostic, which is
/// what is wanted from something that cannot happen.
pub struct Text {
    messages: Option<Messages>,
}

impl Text {
    /// Load the catalogues for a locale, falling back to English.
    #[must_use]
    pub fn new(locale: &str) -> Self {
        let messages = Messages::for_locale(locale)
            .or_else(|_| Messages::for_locale("en"))
            .ok();
        Self { messages }
    }

    /// Which locale is in use, or `"none"` when no catalogue loaded.
    #[must_use]
    pub fn locale(&self) -> &str {
        self.messages.as_ref().map_or("none", Messages::locale)
    }

    /// A message with no substitutions.
    #[must_use]
    pub fn get(&self, key: &str) -> String {
        self.messages
            .as_ref()
            .map_or_else(|| key.to_owned(), |m| m.get(key).to_owned())
    }

    /// A message with `{0}`-style substitutions.
    #[must_use]
    pub fn format(&self, key: &str, arguments: &[&str]) -> String {
        self.messages
            .as_ref()
            .map_or_else(|| key.to_owned(), |m| m.format(key, arguments))
    }
}

/// Print a message.
macro_rules! say {
    ($text:expr, $key:expr) => {
        println!("{}", $text.get($key))
    };
    ($text:expr, $key:expr, $($arg:expr),+ $(,)?) => {
        println!("{}", $text.format($key, &[$($arg),+]))
    };
}

pub(crate) use say;

/// Human-readable byte count.
///
/// Not from the catalogue: the unit names are the same in both languages and
/// the number is a number.
#[must_use]
pub fn bytes(count: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    #[allow(clippy::cast_precision_loss)]
    let mut value = count as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{count} {}", UNITS[0])
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

/// Human-readable duration, for a wait rather than a playhead.
///
/// `clock` in `eon-stream-core` formats a playback position; this formats a
/// span, which reads differently: nobody says a download has "0:03:20" left.
#[must_use]
pub fn duration(seconds: u64) -> String {
    match seconds {
        0..60 => format!("{seconds}s"),
        60..3600 => format!("{}m {}s", seconds / 60, seconds % 60),
        _ => format!("{}h {}m", seconds / 3600, (seconds % 3600) / 60),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn byte_counts_read_naturally() {
        assert_eq!(bytes(0), "0 B");
        assert_eq!(bytes(512), "512 B");
        assert_eq!(bytes(1024), "1.0 KB");
        assert_eq!(bytes(1536), "1.5 KB");
        assert_eq!(bytes(8 * 1024 * 1024 * 1024), "8.0 GB");
        // Saturates at the largest unit rather than inventing one.
        assert!(bytes(u64::MAX).ends_with(" TB"));
    }

    #[test]
    fn durations_read_as_spans_not_as_clocks() {
        assert_eq!(duration(0), "0s");
        assert_eq!(duration(45), "45s");
        assert_eq!(duration(200), "3m 20s");
        assert_eq!(duration(3600), "1h 0m");
        assert_eq!(duration(5400), "1h 30m");
    }

    #[test]
    fn both_locales_load_and_differ() {
        let english = Text::new("en");
        let turkish = Text::new("tr");
        assert_eq!(english.locale(), "en");
        assert_eq!(turkish.locale(), "tr");
        assert_ne!(
            english.get("cli.help.header"),
            turkish.get("cli.help.header")
        );
    }

    #[test]
    fn an_unknown_locale_falls_back_rather_than_failing() {
        let text = Text::new("kl-GL");
        assert_eq!(text.locale(), "en");
        assert!(!text.get("cli.help.header").is_empty());
    }

    #[test]
    fn substitutions_land_in_both_languages() {
        for locale in ["en", "tr"] {
            let text = Text::new(locale);
            let line = text.format("cli.module.list.header", &["3"]);
            assert!(line.contains('3'), "{locale}: {line}");
            assert!(!line.contains("{0}"), "{locale}: {line}");
        }
    }
}
