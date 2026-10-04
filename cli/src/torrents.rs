// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 EON contributors
//
// Additional permission under GNU GPL version 3 section 7:
// see LICENSE-EXCEPTION.md (EON Module ABI Exception 1.0).

//! Turning a torrent source into something mpv can open.
//!
//! The chain, which is v1 item 4 end to end:
//!
//! ```text
//! addon stream { infoHash, fileIdx, sources }
//!   -> TorrentRequest                      (this file)
//!   -> session.add, wait for metadata       (eon-stream-engine::torrent)
//!   -> pick the file                        (this file, or the user by index)
//!   -> prebuffer ahead of the playhead      (eon-stream-engine::torrent)
//!   -> http://127.0.0.1:<port>/<token>/...  (eon-stream-engine::server)
//!   -> mpv, which never learns it is a torrent
//! ```
//!
//! ## The engine starts on first use, not at launch
//!
//! Starting it means a tokio runtime, a DHT, and a listening socket. Someone
//! who only plays HTTP streams should pay for none of that, and someone who
//! never opens a torrent should not have this program talking to the DHT at
//! all. So it is started the first time a torrent is played and shut down when
//! the program exits.

use std::time::{Duration, Instant};

use eon_stream_core::{types::StreamSource, Stream};
use eon_stream_engine::{
    default_download_dir, PlaybackSource, TorrentEngine, TorrentHandle, TorrentOptions,
    TorrentRequest,
};

use crate::text::{self, say, Text};

/// Where the engine puts pieces, beneath the executable's own directory.
const DOWNLOAD_SUBDIRECTORY: &str = "eon-torrents";

/// How long to wait for enough of the file to start playing.
const PREBUFFER_TIMEOUT: Duration = Duration::from_secs(180);

/// A torrent source, read out of an addon's stream object.
pub struct TorrentSource {
    /// What to hand the engine.
    pub request: TorrentRequest,
    /// The file the addon named, if it named one.
    pub file_index: Option<usize>,
}

/// Read a torrent source out of a stream, if that is what it is.
#[must_use]
pub fn source_of(stream: &Stream) -> Option<TorrentSource> {
    match stream.source() {
        Some(StreamSource::Torrent {
            info_hash,
            file_idx,
        }) => Some(TorrentSource {
            request: TorrentRequest::from_addon(info_hash, &stream.sources),
            file_index: file_idx.map(|index| index as usize),
        }),
        _ => None,
    }
}

/// Read a magnet link or a bare info hash out of a command argument.
///
/// Returns `None` for anything else, so `play` falls through to treating the
/// argument as a URL or a file path. Deliberately strict: a 40-character hex
/// string is an info hash and nothing else is, because guessing here would
/// mean handing a file path to the torrent engine.
#[must_use]
pub fn request_from_argument(argument: &str) -> Option<TorrentRequest> {
    let trimmed = argument.trim();
    if trimmed.to_ascii_lowercase().starts_with("magnet:") {
        let request = TorrentRequest::Magnet(trimmed.to_owned());
        // Validated here rather than at use: a malformed magnet should fall
        // through to "not a usable torrent source" with its own message.
        return request.info_hash().ok().map(|_| request);
    }
    if trimmed.len() == 40 && trimmed.chars().all(|c| c.is_ascii_hexdigit()) {
        return Some(TorrentRequest::InfoHash {
            hex: trimmed.to_ascii_lowercase(),
            trackers: Vec::new(),
        });
    }
    None
}

/// Build the engine's options from the user's settings.
///
/// # Errors
///
/// Whatever [`default_download_dir`] reports when the executable's own
/// location is unreadable.
pub fn options_from(
    settings: &eon_stream_core::settings::TorrentSettings,
) -> eon_stream_engine::Result<TorrentOptions> {
    let download_dir = match &settings.download_directory {
        Some(directory) => std::path::PathBuf::from(directory),
        None => default_download_dir(DOWNLOAD_SUBDIRECTORY)?,
    };
    Ok(TorrentOptions {
        download_dir,
        peer_limit: Some(settings.peer_limit as usize),
        // Settings are in kilobytes per second because that is how people
        // think about a connection; the engine wants bytes.
        download_limit: settings
            .download_limit_kbps
            .map(|kbps| kbps.saturating_mul(1024)),
        upload_limit: settings
            .upload_limit_kbps
            .map(|kbps| kbps.saturating_mul(1024)),
        metadata_timeout: Duration::from_secs(60),
        prebuffer_bytes: u64::from(settings.prebuffer_megabytes) * 1024 * 1024,
        keep_files: settings.keep_files,
        use_trackers: true,
    })
}

/// Resolve a torrent source all the way to a URL mpv can open.
///
/// Prints progress while it waits, because the wait is real: metadata comes
/// from strangers and the prebuffer comes after that. A silent thirty seconds
/// looks like a hang.
///
/// Returns the handle alongside the source so the caller can keep reporting on
/// it while playback runs.
pub fn resolve(
    engine: &TorrentEngine,
    source: &TorrentSource,
    explicit_file: Option<usize>,
    text: &Text,
) -> Result<(PlaybackSource, TorrentHandle, usize), String> {
    say!(text, "cli.torrent.resolving");

    let handle = engine.add(&source.request).map_err(|e| e.to_string())?;

    let name = handle.name().unwrap_or("?").to_owned();
    println!(
        "{}",
        text.format(
            "cli.torrent.resolved",
            &[
                &name,
                &handle.files().len().to_string(),
                &text::bytes(handle.total_bytes()),
            ],
        )
    );

    // Three ways to land on a file, most explicit first: what the user typed,
    // what the addon said, then the largest real video. The addon's `fileIdx`
    // is usually right and occasionally refers to a file list that has since
    // changed, which is why an out-of-range value falls through to the
    // heuristic rather than failing.
    let index = match explicit_file.or(source.file_index) {
        Some(index) if handle.file(index).is_ok() => index,
        _ => match handle.best_video() {
            Ok(file) => file.index,
            Err(e) => {
                list_files(&handle, text);
                return Err(e.to_string());
            }
        },
    };

    let file = handle.file(index).map_err(|e| e.to_string())?;
    println!(
        "{}",
        text.format(
            "cli.torrent.picked",
            &[&index.to_string(), file.name(), &text::bytes(file.length),],
        )
    );

    let started = Instant::now();
    let mut last_report = Instant::now();
    engine
        .wait_for_prebuffer(&handle, index, PREBUFFER_TIMEOUT, |have, want| {
            // Once a second, not every quarter second: a progress line that
            // moves faster than it can be read is noise.
            if last_report.elapsed() >= Duration::from_secs(1) {
                last_report = Instant::now();
                print!(
                    "\r  {}   ",
                    text.format(
                        "cli.torrent.buffering",
                        &[&text::bytes(have), &text::bytes(want)],
                    )
                );
                let _ = std::io::Write::flush(&mut std::io::stdout());
            }
            true
        })
        .map_err(|e| {
            println!();
            e.to_string()
        })?;
    println!();
    println!(
        "{}",
        text.format(
            "cli.torrent.ready",
            &[&text::duration(started.elapsed().as_secs())],
        )
    );

    let url = engine
        .stream_url(&handle, index)
        .map_err(|e| e.to_string())?;
    // The URL carries the session token, so it is never printed. The address
    // without it is enough to say where the bytes are coming from.
    say!(
        text,
        "cli.torrent.serving",
        &engine.server_address().to_string()
    );

    Ok((PlaybackSource::Url(url), handle, index))
}

/// Print the file list, numbered the way `play <n> file <i>` expects.
pub fn list_files(handle: &TorrentHandle, text: &Text) {
    say!(text, "cli.torrent.files.header");
    for file in handle.files() {
        let marker = if file.is_video { "video" } else { "     " };
        println!(
            "{:>3}. [{marker}] {:>10}  {}",
            file.index,
            text::bytes(file.length),
            file.path
        );
    }
}

/// Print how the running torrents are getting on.
pub fn status(engine: Option<&TorrentEngine>, text: &Text) {
    let Some(engine) = engine else {
        say!(text, "cli.torrent.nosession");
        return;
    };
    let torrents = engine.list();
    if torrents.is_empty() {
        say!(text, "cli.torrent.nosession");
        return;
    }
    for handle in torrents {
        println!("\n{}", handle.name().unwrap_or(handle.info_hash()));
        match engine.progress(&handle) {
            Ok(progress) => {
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                let percent = (progress.fraction() * 100.0).round() as u64;
                println!(
                    "  {}",
                    text.format(
                        "cli.torrent.progress",
                        &[
                            &text::bytes(progress.downloaded),
                            &text::bytes(progress.total),
                            &format!("{percent}%"),
                            &text::bytes(progress.download_bps),
                            &text::bytes(progress.upload_bps),
                            &progress.peers.to_string(),
                        ],
                    )
                );
                if let Some(eta) = progress.eta_seconds().filter(|_| !progress.finished) {
                    println!(
                        "  {}",
                        text.format("cli.torrent.eta", &[&text::duration(eta)])
                    );
                }
            }
            Err(e) => println!("  {e}"),
        }
    }
    println!(
        "\n  {}",
        text.format(
            "cli.torrent.serving",
            &[&engine.server_address().to_string()]
        )
    );
}

/// Shut the engine down, saying what happened to the pieces.
pub fn shutdown(engine: TorrentEngine, text: &Text) {
    match engine.shutdown() {
        Some(directory) => say!(text, "cli.torrent.kept", &directory.display().to_string()),
        None => say!(text, "cli.torrent.discarded"),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use eon_stream_core::settings::TorrentSettings;

    #[test]
    fn a_torrent_stream_becomes_a_request_with_its_trackers() {
        let stream: Stream = serde_json::from_str(
            r#"{
                "infoHash": "dd8255ecdc7ca55fb0bbf81323d87062db1f6d1c",
                "fileIdx": 2,
                "sources": [
                    "tracker:udp://tracker.example.org:1337/announce",
                    "dht:dd8255ecdc7ca55fb0bbf81323d87062db1f6d1c"
                ]
            }"#,
        )
        .unwrap();
        let source = source_of(&stream).expect("this is a torrent source");
        assert_eq!(source.file_index, Some(2));
        assert_eq!(
            source.request.info_hash().unwrap(),
            "dd8255ecdc7ca55fb0bbf81323d87062db1f6d1c"
        );
    }

    #[test]
    fn magnets_and_info_hashes_are_recognised_and_nothing_else_is() {
        assert!(request_from_argument(
            "magnet:?xt=urn:btih:dd8255ecdc7ca55fb0bbf81323d87062db1f6d1c"
        )
        .is_some());
        assert!(request_from_argument("dd8255ecdc7ca55fb0bbf81323d87062db1f6d1c").is_some());
        assert!(request_from_argument("DD8255ECDC7CA55FB0BBF81323D87062DB1F6D1C").is_some());

        // Guessing here would mean handing a file path to the torrent engine.
        for not_a_torrent in [
            "https://example.org/video.mp4",
            r"D:\Videos\movie.mkv",
            "magnet:?dn=no+hash",
            "dd8255ecdc7ca55fb0bbf81323d87062db1f6d1",
            "zz8255ecdc7ca55fb0bbf81323d87062db1f6d1c",
            "",
        ] {
            assert!(
                request_from_argument(not_a_torrent).is_none(),
                "{not_a_torrent} is not a torrent source"
            );
        }
    }

    #[test]
    fn a_direct_url_stream_is_not_a_torrent_source() {
        let stream: Stream =
            serde_json::from_str(r#"{"url":"https://example.org/video.mp4"}"#).unwrap();
        assert!(source_of(&stream).is_none());
    }

    #[test]
    fn settings_convert_to_engine_options_in_the_right_units() {
        // Settings are kilobytes per second because that is how people think
        // about a connection; the engine wants bytes. Getting this wrong by
        // 1024 is the sort of bug that looks like a slow network.
        let settings = TorrentSettings {
            download_limit_kbps: Some(2048),
            upload_limit_kbps: Some(512),
            prebuffer_megabytes: 32,
            peer_limit: 50,
            keep_files: true,
            ..TorrentSettings::default()
        };
        let options = options_from(&settings).expect("options build");
        assert_eq!(options.download_limit, Some(2048 * 1024));
        assert_eq!(options.upload_limit, Some(512 * 1024));
        assert_eq!(options.prebuffer_bytes, 32 * 1024 * 1024);
        assert_eq!(options.peer_limit, Some(50));
        assert!(options.keep_files);
    }

    #[test]
    fn no_rate_limit_set_means_no_limit_passed() {
        let options = options_from(&TorrentSettings::default()).expect("options build");
        assert_eq!(options.download_limit, None);
        assert_eq!(options.upload_limit, None);
    }

    #[test]
    fn an_absurd_rate_limit_saturates_rather_than_wrapping() {
        let settings = TorrentSettings {
            download_limit_kbps: Some(u32::MAX),
            ..TorrentSettings::default()
        };
        let options = options_from(&settings).expect("options build");
        assert_eq!(options.download_limit, Some(u32::MAX));
    }

    #[test]
    fn status_with_no_engine_says_so_rather_than_printing_nothing() {
        // Printing nothing would look like a hang or a swallowed error.
        let text = Text::new("en");
        status(None, &text);
        assert!(!text.get("cli.torrent.nosession").is_empty());
    }
}
