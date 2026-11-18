// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 EON contributors
//
// Additional permission under GNU GPL version 3 section 7:
// see LICENSE-EXCEPTION.md (EON Module ABI Exception 1.0).

//! The buffer: torrent pieces on disk, and the button that clears them.
//!
//! What gets deleted depends on whose folder it is, and the difference is the
//! one rule in this file:
//!
//! * The default folder, `eon-torrents/` beside the executable, is ours. This
//!   program created it, nothing else writes into it, and clearing it removes
//!   it whole -- including pieces a crashed earlier run left behind, which no
//!   running session knows about.
//! * A folder the viewer chose in the settings is theirs. Only torrents the
//!   running session holds are removed, through the session, which knows which
//!   files belong to which torrent. A recursive delete of a folder someone
//!   picked is not something this program ever does: it could be their
//!   Downloads folder.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::state::Session;

/// The folder this program created for itself, when that is the one in use.
fn owned_folder(session: &Session) -> Option<PathBuf> {
    if session.settings.torrent.download_directory.is_some() {
        return None;
    }
    crate::torrents::options_from(&session.settings.torrent)
        .ok()
        .map(|options| options.download_dir)
}

/// Free space on the disk the cache is on.
#[must_use]
pub fn free(session: &Session) -> Option<u64> {
    let folder = crate::torrents::options_from(&session.settings.torrent)
        .ok()
        .map(|options| options.download_dir)?;
    // The folder may not exist yet; its disk does.
    let probe = if folder.exists() {
        folder
    } else {
        crate::state::beside_exe("")
    };
    eon_stream_engine::free_space(&probe)
}

/// Bytes of pieces on disk.
///
/// What the session's torrents actually occupy -- downloaded, less what was
/// given back -- when a session is running: a torrent's files are sparse,
/// and their length says nothing about their size on disk. Without one, the
/// folder's total, which is what a crashed earlier run left.
#[must_use]
pub fn size(session: &Session) -> u64 {
    if let Some(engine) = session.torrent.as_deref() {
        return engine
            .list()
            .iter()
            .filter_map(|handle| engine.bytes_on_disk(handle).ok())
            .sum();
    }
    if let Some(folder) = owned_folder(session) {
        return folder_size(&folder);
    }
    // Someone else's folder: count only what the session holds, rather than
    // reporting their whole Downloads folder as our buffer.
    crate::torrents::snapshot(session.torrent.as_deref())
        .iter()
        .map(|torrent| torrent.downloaded)
        .sum()
}

fn folder_size(folder: &Path) -> u64 {
    let Ok(entries) = std::fs::read_dir(folder) else {
        return 0;
    };
    entries
        .flatten()
        .map(|entry| match entry.file_type() {
            Ok(kind) if kind.is_dir() => folder_size(&entry.path()),
            // A symlink is counted as itself, never followed: the folder is
            // ours, what a link points at may not be.
            Ok(_) => entry.metadata().map(|m| m.len()).unwrap_or_default(),
            Err(_) => 0,
        })
        .sum()
}

/// Clear the buffer. Returns how many bytes it freed.
///
/// # Errors
///
/// As text: something is playing or still being resolved -- clearing the
/// pieces under a player that is reading them is how a stream dies halfway --
/// or the folder could not be removed.
pub fn clear(session: &mut Session) -> Result<u64, String> {
    let busy = session.player.is_some() || session.job.lock().map(|job| job.active).unwrap_or(true);
    if busy {
        return Err(session.text("ui.error.bufferbusy"));
    }
    let before = size(session);

    if let Some(engine) = session.torrent.take() {
        match Arc::try_unwrap(engine) {
            Ok(engine) => {
                // Through the session first, which is right for both kinds of
                // folder: it deletes exactly the files of the torrents it
                // holds, and stops serving them.
                for handle in engine.list() {
                    let _ = engine.remove(&handle, true);
                }
                let _ = engine.shutdown();
            }
            Err(shared) => {
                // A worker still holds it, so a torrent is being resolved
                // after all. Put it back and refuse.
                session.torrent = Some(shared);
                return Err(session.text("ui.error.bufferbusy"));
            }
        }
    }

    if let Some(folder) = owned_folder(session) {
        if folder.exists() {
            std::fs::remove_dir_all(&folder).map_err(|e| e.to_string())?;
        }
    }
    Ok(before.saturating_sub(size(session)))
}
