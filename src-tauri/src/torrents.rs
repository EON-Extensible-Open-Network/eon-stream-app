// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 EON contributors
//
// Additional permission under GNU GPL version 3 section 7:
// see LICENSE-EXCEPTION.md (EON Module ABI Exception 1.0).

//! Turning a torrent source into something mpv can open, with a window
//! watching.
//!
//! The chain is the headless build's, unchanged:
//!
//! ```text
//! addon stream { infoHash, fileIdx, sources }
//!   -> TorrentRequest
//!   -> session.add, wait for metadata        (eon-stream-engine::torrent)
//!   -> pick the file
//!   -> prebuffer ahead of the playhead       (eon-stream-engine::torrent)
//!   -> http://127.0.0.1:<port>/<token>/...   (eon-stream-engine::server)
//!   -> mpv, which never learns it is a torrent
//! ```
//!
//! What differs is the reporting. The prompt prints a line a second and the
//! person reading it accepts that the program is busy. A window may not stop
//! answering, so this runs on a worker thread and writes its progress into the
//! shared [`Job`](crate::state::Job) — numbers only, with the sentence chosen
//! by the interface in its own language.

use std::time::{Duration, Instant};

use eon_stream_core::{ranking::format_size, types::StreamSource, Stream};
use eon_stream_engine::{
    default_download_dir, PlaybackSource, TorrentEngine, TorrentHandle, TorrentOptions,
    TorrentRequest,
};
use serde::Serialize;

use crate::state::JobHandle;

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

/// Read a magnet link or a bare info hash out of what someone pasted.
///
/// Deliberately strict: a 40-character hex string is an info hash and nothing
/// else is, because guessing here would mean handing a file path to the torrent
/// engine.
pub fn request_from_argument(argument: &str) -> Option<TorrentRequest> {
    let trimmed = argument.trim();
    if trimmed.to_ascii_lowercase().starts_with("magnet:") {
        let request = TorrentRequest::Magnet(trimmed.to_owned());
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

/// What resolving a torrent produced.
pub struct Resolved {
    pub source: PlaybackSource,
    /// The torrent, with its file list: subtitles shipped beside the video
    /// are found in it.
    pub handle: TorrentHandle,
    /// Which file inside it is playing.
    pub index: usize,
}

/// The answer a worker gives when the viewer gave up while it waited.
///
/// Not an error and not shown: the person pressed the button that asked for
/// exactly this.
pub const CANCELLED: &str = "ui.job.cancelled";

/// Resolve a torrent source all the way to a URL mpv can open.
///
/// Reports into `job` as it goes, and checks between every stage -- and four
/// times a second during the prebuffer -- that `ticket` is still the start
/// someone is waiting for. When it is not, the torrent is taken out of the
/// engine with its pieces and [`CANCELLED`] comes back.
///
/// # Errors
///
/// As text: a torrent whose metadata never arrived, a torrent with no video in
/// it, a prebuffer that timed out, or [`CANCELLED`].
pub fn resolve(
    engine: &TorrentEngine,
    source: &TorrentSource,
    explicit_file: Option<usize>,
    job: &JobHandle,
    ticket: u64,
) -> Result<Resolved, String> {
    let wanted = || job.lock().map(|job| job.wanted(ticket)).unwrap_or(true);
    if let Ok(mut job) = job.lock() {
        if !job.wanted(ticket) {
            return Err(CANCELLED.to_owned());
        }
        job.at("ui.job.metadata", String::new());
        // Written down before the wait, so a cancel during it knows what to
        // take out of the engine.
        job.torrent = source.request.info_hash().ok();
    }

    let handle = engine.add(&source.request).map_err(|e| e.to_string())?;
    let give_up = |handle: &TorrentHandle| {
        let _ = engine.remove(handle, true);
        Err(CANCELLED.to_owned())
    };
    if !wanted() {
        return give_up(&handle);
    }
    let name = handle.name().unwrap_or("").to_owned();
    report(job, ticket, "ui.job.metadata", name);

    // Which file, and why, is the engine's rule: it owns the file list and it
    // is where that decision can be tested (eon-stream-engine::torrent).
    let file = handle
        .choose_file(explicit_file, source.file_index)
        .map_err(|e| e.to_string())?;
    let index = file.index;
    report(job, ticket, "ui.job.picked", file.name().to_owned());

    let mut last = Instant::now();
    engine
        .wait_for_prebuffer(&handle, index, PREBUFFER_TIMEOUT, |have, want| {
            // Twice a second: faster would redraw a progress bar quicker than
            // it can be read, and lock the job mutex for nothing.
            if last.elapsed() >= Duration::from_millis(500) {
                last = Instant::now();
                #[allow(clippy::cast_precision_loss)]
                let fraction = if want == 0 {
                    0.0
                } else {
                    have as f64 / want as f64
                };
                if let Ok(mut job) = job.lock() {
                    if !job.wanted(ticket) {
                        return false;
                    }
                    job.at_fraction(
                        "ui.job.buffering",
                        format!("{} / {}", format_size(have), format_size(want)),
                        fraction,
                    );
                }
            }
            wanted()
        })
        .map_err(|e| e.to_string())?;
    if !wanted() {
        return give_up(&handle);
    }

    let url = engine
        .stream_url(&handle, index)
        .map_err(|e| e.to_string())?;
    Ok(Resolved {
        source: PlaybackSource::Url(url),
        handle,
        index,
    })
}

/// Write a stage into the job without caring whether anyone is reading it.
///
/// A poisoned mutex here would mean a worker panicked while holding it. The
/// honest response is to carry on resolving the torrent rather than to take
/// the window down over a progress label, so the result is discarded.
fn report(job: &JobHandle, ticket: u64, stage: &str, detail: String) {
    if let Ok(mut job) = job.lock() {
        // A cancelled start says nothing more: the screen it would write to
        // has gone.
        if job.wanted(ticket) {
            job.at(stage, detail);
        }
    }
}

/// How one running torrent is getting on.
#[derive(Serialize)]
pub struct TorrentView {
    pub name: String,
    pub downloaded: u64,
    pub total: u64,
    pub percent: u8,
    pub down_bps: u64,
    pub up_bps: u64,
    pub peers: u32,
    pub eta_seconds: Option<u64>,
    pub finished: bool,
    /// Where the bytes are being served from. The address without the session
    /// token, which is never shown and never leaves the host.
    pub serving: String,
}

/// A snapshot of every running torrent.
pub fn snapshot(engine: Option<&TorrentEngine>) -> Vec<TorrentView> {
    let Some(engine) = engine else {
        return Vec::new();
    };
    let serving = engine.server_address().to_string();
    engine
        .list()
        .into_iter()
        .filter_map(|handle| {
            let progress = engine.progress(&handle).ok()?;
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let percent = (progress.fraction() * 100.0).round().clamp(0.0, 100.0) as u8;
            Some(TorrentView {
                name: handle
                    .name()
                    .unwrap_or_else(|| handle.info_hash())
                    .to_owned(),
                downloaded: progress.downloaded,
                total: progress.total,
                percent,
                down_bps: progress.download_bps,
                up_bps: progress.upload_bps,
                peers: progress.peers,
                eta_seconds: progress.eta_seconds().filter(|_| !progress.finished),
                finished: progress.finished,
                serving: serving.clone(),
            })
        })
        .collect()
}
