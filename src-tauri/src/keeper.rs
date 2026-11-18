// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 EON contributors
//
// Additional permission under GNU GPL version 3 section 7:
// see LICENSE-EXCEPTION.md (EON Module ABI Exception 1.0).

//! The cache limit: the torrent cache never grows past what the viewer set,
//! nor past what the disk has free.
//!
//! The rule is a high and a low water mark. Until the cache reaches 80% of
//! the limit nothing is deleted -- the episode before stays, so going back to
//! it costs nothing. Past 80%, space is made down to 60% in one go, rather
//! than a little at a time on every tick:
//!
//! 1. whole torrents that are not playing, least recently played first;
//! 2. then the watched part of what is playing, never closer than three
//!    minutes behind the playhead.
//!
//! When that is still not enough -- one 4K episode larger than the whole
//! limit -- the download is held to the film's own pace so the cache stays
//! under the limit while the watched part keeps being given back.
//!
//! Which bytes are "watched" is not guessed from the clock. A film's bitrate
//! moves: a minute of credits is a few megabytes and a minute of a battle
//! scene can be hundreds, so the byte a timestamp falls on cannot be computed
//! from the average. Every five seconds this records where mpv's reader
//! actually is in the file next to the playback time. mpv reads ahead into a
//! cache of a known size (`--demuxer-max-bytes`, set at launch), so the byte
//! being played at that moment was at least the reader's position minus that
//! cache. A range is given back only below such a recorded point from three
//! minutes ago -- which cannot be ahead of anything still to be watched. A
//! seek to before that point restarts the stream there instead of reading
//! zeros (see `commands::player`).

use std::collections::VecDeque;
use std::sync::Arc;
use std::time::Duration;

use eon_stream_engine::{TorrentEngine, TorrentHandle};

use crate::commands::Shared;

const MIB: u64 = 1 << 20;
const GIB: u64 = 1 << 30;

/// What is kept of the start of a file no matter what: its header, which a
/// player may read again.
pub const KEEP_HEAD: u64 = 64 * MIB;
/// How far behind the playhead nothing is given back, in seconds.
const KEEP_BEHIND_SECS: f64 = 180.0;
/// The same, once the cache is at its limit.
const TIGHT_BEHIND_SECS: f64 = 30.0;
/// Past the limit by this much, the download stops even if the picture has
/// to wait: the limit is the viewer's, and their disk is not ours to fill.
const HARD: f64 = 1.05;
/// "Stopped": librqbit has no zero, and a trickle keeps the peers.
const STOPPED: u64 = 16 * 1024;
/// mpv's forward cache, set at launch so the bound above holds.
pub const PLAYER_CACHE: &str = "--demuxer-max-bytes=150MiB";
pub const PLAYER_BACK_CACHE: &str = "--demuxer-max-back-bytes=50MiB";
/// The forward cache in bytes, with room to spare.
const PLAYER_CACHE_BYTES: u64 = 200 * MIB;
/// Left free on the disk whatever the limit says.
const RESERVE: u64 = 2 * GIB;
const HIGH: f64 = 0.8;
const LOW: f64 = 0.6;

/// A debug build's override of the limit, the three minutes and mpv's cache,
/// as `EON_KEEPER_TEST=<limit bytes>,<behind seconds>,<cache bytes>`, so the
/// whole mechanism can be exercised on a short film. A release build has no
/// such switch.
#[cfg(debug_assertions)]
fn test_tuning() -> Option<(u64, f64, u64)> {
    let raw = std::env::var("EON_KEEPER_TEST").ok()?;
    let mut parts = raw.split(',');
    Some((
        parts.next()?.trim().parse().ok()?,
        parts.next()?.trim().parse().ok()?,
        parts.next()?.trim().parse().ok()?,
    ))
}

#[cfg(not(debug_assertions))]
fn test_tuning() -> Option<(u64, f64, u64)> {
    None
}

fn behind_secs() -> f64 {
    test_tuning().map_or(KEEP_BEHIND_SECS, |(_, behind, _)| behind)
}

fn cache_bytes() -> u64 {
    test_tuning().map_or(PLAYER_CACHE_BYTES, |(_, _, cache)| cache)
}

/// mpv's cache arguments, sized so the bound above holds.
#[must_use]
pub fn player_cache_args() -> Vec<String> {
    match test_tuning() {
        Some((_, _, cache)) => vec![
            format!("--demuxer-max-bytes={}", cache / 2),
            format!("--demuxer-max-back-bytes={}", cache / 4),
        ],
        None => vec![PLAYER_CACHE.to_owned(), PLAYER_BACK_CACHE.to_owned()],
    }
}

/// The limit in force, and whether it had to be lowered from the setting.
#[must_use]
pub fn budget(setting_mb: u32, free: Option<u64>, usage: u64) -> (u64, bool) {
    if let Some((limit, _, _)) = test_tuning() {
        return (limit, false);
    }
    let room = free.map(|free| (free + usage).saturating_sub(RESERVE));
    let wanted = if setting_mb == 0 {
        room.map_or(10 * GIB, |room| (room / 4).clamp(4 * GIB, 50 * GIB))
    } else {
        u64::from(setting_mb) * MIB
    };
    match room {
        Some(room) if wanted > room => (room.max(GIB), setting_mb != 0),
        _ => (wanted, false),
    }
}

/// The playback the keeper is looking after.
#[derive(Default)]
pub struct Keeper {
    /// The start's ticket. A keeper whose ticket is no longer this one stops.
    pub ticket: u64,
    /// The torrent and the file in it that is playing.
    pub torrent: Option<(Arc<TorrentEngine>, TorrentHandle, usize)>,
    /// Which source of the current list is playing, and which file: what a
    /// restart plays again.
    pub source: Option<(usize, Option<usize>)>,
    /// (playback time, reader position) pairs, oldest first.
    samples: VecDeque<(f64, u64)>,
    /// Seeking to before this time needs a restart: the bytes are gone.
    pub released_secs: f64,
    /// Something to tell the viewer: shown by whichever page asks first.
    pub notice: Option<String>,
    /// Whether the lowered limit was already told, this run.
    warned: bool,
    /// Whether "the cache is full" was already told, this playback.
    full_told: bool,
    /// When each torrent was last played, by info hash, for "least recently
    /// played first".
    pub played: std::collections::HashMap<String, u64>,
}

impl Keeper {
    /// Start looking after a new playback.
    pub fn begin(
        &mut self,
        ticket: u64,
        torrent: Option<(Arc<TorrentEngine>, TorrentHandle, usize)>,
        source: Option<(usize, Option<usize>)>,
    ) {
        self.ticket = ticket;
        if let Some((_, handle, _)) = &torrent {
            self.played.insert(
                handle.info_hash().to_owned(),
                eon_stream_core::history::now_unix(),
            );
        }
        self.torrent = torrent;
        self.source = source;
        self.samples.clear();
        self.released_secs = 0.0;
        self.full_told = false;
    }

    /// The furthest point that may be given back now, and the playback time
    /// from which everything later is still on disk.
    fn safe_point(&self, now: f64, behind: f64) -> Option<(u64, f64)> {
        self.samples
            .iter()
            .filter(|(at, _)| *at <= now - behind)
            .max_by_key(|(_, reader)| *reader)
            .map(|(at, reader)| (reader.saturating_sub(cache_bytes()), *at))
    }
}

/// Run every few seconds while one playback lasts.
pub fn watch(shared: &Shared, ticket: u64) {
    loop {
        std::thread::sleep(Duration::from_secs(5));
        if !tick(shared, ticket) {
            break;
        }
    }
    // Whatever pace the download was held to, it is the viewer's own again;
    // and what was measured is kept.
    if let Ok(session) = shared.lock() {
        session.speeds.save();
        if let Some(engine) = &session.torrent {
            engine.set_download_limit(user_limit(&session));
        }
    }
}

fn user_limit(session: &crate::state::Session) -> Option<u64> {
    session
        .settings
        .torrent
        .download_limit_kbps
        .map(|kbps| u64::from(kbps) * 1024)
}

/// One look. Answers whether to keep looking.
fn tick(shared: &Shared, ticket: u64) -> bool {
    // What the player says, and what the session holds -- under the lock,
    // briefly.
    let (engine, handle, index, setting, user, position, duration, reader, folder) = {
        let Ok(mut session) = shared.lock() else {
            return false;
        };
        if session.keeper.ticket != ticket {
            return false;
        }
        let Some((engine, handle, index)) = session.keeper.torrent.clone() else {
            return false;
        };
        let setting = session.settings.torrent.cache_limit_megabytes;
        let user = user_limit(&session);
        let folder = engine.options().download_dir.clone();
        let Some(player) = session.player.as_mut() else {
            // Between a stop and the next start: nothing to look after.
            return false;
        };
        let position = player.position().ok().flatten();
        let duration = player.duration().ok().flatten();
        let reader = player
            .get_property("stream-pos")
            .ok()
            .and_then(|value| value.parse::<f64>().ok())
            .filter(|value| value.is_finite() && *value >= 0.0);
        if let (Some(position), Some(reader)) = (position, reader) {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            session.keeper.samples.push_back((position, reader as u64));
            // Half an hour of samples is more than any choice needs.
            while session.keeper.samples.len() > 360 {
                session.keeper.samples.pop_front();
            }
        }
        session.keeper.played.insert(
            handle.info_hash().to_owned(),
            eon_stream_core::history::now_unix(),
        );
        (
            engine, handle, index, setting, user, position, duration, reader, folder,
        )
    };

    // The engine is asked with nothing locked: removing a torrent waits on
    // its runtime.
    let usage = |engine: &TorrentEngine| -> u64 {
        engine
            .list()
            .iter()
            .filter_map(|other| engine.bytes_on_disk(other).ok())
            .sum()
    };
    let mut used = usage(&engine);
    let free = eon_stream_engine::free_space(&folder);
    let (limit, lowered) = budget(setting, free, used);
    if lowered {
        if let Ok(mut session) = shared.lock() {
            if !session.keeper.warned {
                session.keeper.warned = true;
                #[allow(clippy::cast_precision_loss)]
                let gigabytes = format!("{:.1}", limit as f64 / GIB as f64);
                let text = session.text("ui.cache.lowered").replace("{0}", &gigabytes);
                session.keeper.notice = Some(text);
            }
        }
    }

    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss
    )]
    let high = (limit as f64 * HIGH) as u64;
    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss
    )]
    let low = (limit as f64 * LOW) as u64;

    if used > high {
        // 1. Other torrents, least recently played first.
        let played = shared
            .lock()
            .map(|session| session.keeper.played.clone())
            .unwrap_or_default();
        let mut others: Vec<TorrentHandle> = engine
            .list()
            .into_iter()
            .filter(|other| other.info_hash() != handle.info_hash())
            .collect();
        others.sort_by_key(|other| played.get(other.info_hash()).copied().unwrap_or_default());
        for other in others {
            if used <= low {
                break;
            }
            let size = engine.bytes_on_disk(&other).unwrap_or_default();
            if engine.remove(&other, true).is_ok() {
                used = used.saturating_sub(size);
            }
        }
        // 2. The watched part of what is playing.
        if used > low {
            // At the limit, three minutes behind shrinks to thirty seconds:
            // space now matters more than a short seek back being instant.
            let behind = if used >= limit {
                behind_secs().min(TIGHT_BEHIND_SECS)
            } else {
                behind_secs()
            };
            let point = shared.lock().ok().and_then(|session| {
                position.and_then(|now| session.keeper.safe_point(now, behind))
            });
            if let Some((before, from_secs)) = point {
                // As much as brings the cache down to the low mark, from where
                // the last give-back ended, and never past the safe point.
                let from = engine.released_until(&handle, index).unwrap_or(KEEP_HEAD);
                let target = from.saturating_add(used - low).min(before);
                // Whether the space came back or not, the range is marked
                // unreadable from here on, so a seek before it restarts.
                let result = engine.release_before(&handle, index, KEEP_HEAD, target);
                if let Ok(mut session) = shared.lock() {
                    session.keeper.released_secs = session.keeper.released_secs.max(from_secs);
                }
                match result {
                    Ok(freed) => used = used.saturating_sub(freed),
                    Err(e) => eprintln!("could not give disk space back: {e}"),
                }
            }
        }
    }

    // The pace. Free run while there is room; the film's own pace once the
    // cache is near the limit; a trickle that keeps the peers once it is at
    // the limit. Never faster than the viewer's own limit.
    let pace = match (handle.file(index).ok().map(|f| f.length), duration) {
        (Some(length), Some(duration)) if duration > 0.0 => {
            #[allow(
                clippy::cast_precision_loss,
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss
            )]
            let rate = (length as f64 / duration) as u64;
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let read = reader.map_or(0, |r| r as u64);
            // What is ready without a gap from where mpv is reading -- not
            // the file's total, which after a seek forward counts bytes from
            // before the seek.
            let ahead = engine
                .available_from(&handle, index, read)
                .unwrap_or_default();
            #[allow(
                clippy::cast_precision_loss,
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss
            )]
            let at_pace = ((rate as f64) * 1.15) as u64;
            // librqbit fetches beyond the playhead with gaps -- the next
            // stretch first, then pieces from all over the file -- so how
            // much is downloaded says little about what the picture needs.
            // The pace is therefore set by how full the cache is, and only
            // what is ready right at the playhead can lift it.
            let short = ahead < 30 * rate;
            #[allow(
                clippy::cast_precision_loss,
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss
            )]
            let catch_up = ((rate as f64) * 2.0) as u64;
            #[allow(
                clippy::cast_precision_loss,
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss
            )]
            let hard = (limit as f64 * HARD) as u64;
            if used >= hard {
                // Over: nothing more, whatever the picture needs.
                Some(STOPPED)
            } else if used >= limit {
                // At the limit: only what the picture is about to need.
                Some(if short {
                    catch_up.max(256 * 1024)
                } else {
                    64 * 1024
                })
            } else if used > high && !short {
                // Near it: the film's own pace.
                Some(at_pace.max(256 * 1024))
            } else {
                None
            }
        }
        _ => None,
    };
    if pace == Some(STOPPED) {
        if let Ok(mut session) = shared.lock() {
            if !session.keeper.full_told {
                session.keeper.full_told = true;
                session.keeper.notice = Some(session.text("ui.cache.full"));
            }
        }
    }
    let effective = match (pace, user) {
        (Some(pace), Some(user)) => Some(pace.min(user)),
        (pace, user) => pace.or(user),
    };
    engine.set_download_limit(effective);
    // A reading of the swarm's speed, when nothing was holding it back.
    if effective.is_none() {
        if let Ok(progress) = engine.progress(&handle) {
            if !progress.finished {
                if let Ok(mut session) = shared.lock() {
                    session
                        .speeds
                        .record(handle.info_hash(), progress.download_bps);
                }
            }
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_limit_is_the_setting_unless_the_disk_has_less() {
        // 100 GB asked, 500 GB free: 100 GB.
        assert_eq!(budget(100 * 1024, Some(500 * GIB), 0), (100 * GIB, false));
        // 100 GB asked, 30 GB free and 10 used: what fits, less the reserve.
        assert_eq!(
            budget(100 * 1024, Some(30 * GIB), 10 * GIB),
            (38 * GIB, true)
        );
        // Automatic: a quarter of the room, between 4 and 50 GB, never
        // reported as lowered.
        assert_eq!(budget(0, Some(102 * GIB), 0), (25 * GIB, false));
        assert_eq!(budget(0, Some(1000 * GIB), 0), (50 * GIB, false));
        assert_eq!(budget(0, None, 0), (10 * GIB, false));
    }

    #[test]
    fn nothing_is_given_back_closer_than_three_minutes() {
        let mut keeper = Keeper::default();
        for (at, reader) in [
            (0.0, 300 * MIB),
            (100.0, 900 * MIB),
            (200.0, 1500 * MIB),
            (400.0, 2600 * MIB),
        ] {
            keeper.samples.push_back((at, reader));
        }
        // At 410 s the newest usable sample is from 200 s: its reader, less
        // the player's cache.
        assert_eq!(
            keeper.safe_point(410.0, KEEP_BEHIND_SECS),
            Some((1500 * MIB - PLAYER_CACHE_BYTES, 200.0))
        );
        // At 150 s nothing is three minutes old yet.
        assert_eq!(keeper.safe_point(150.0, KEEP_BEHIND_SECS), None);
    }
}
