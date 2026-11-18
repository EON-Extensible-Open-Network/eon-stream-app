// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 EON contributors
//
// Additional permission under GNU GPL version 3 section 7:
// see LICENSE-EXCEPTION.md (EON Module ABI Exception 1.0).

//! Where the video goes: inside this window, not beside it.
//!
//! mpv is still a separate process driven over JSON IPC (madde 2) -- nothing
//! about the player changed. What changed is where it draws: it is handed this
//! window's native handle with `--wid`, so its picture is a child of the EON
//! Stream window instead of a window of its own.
//!
//! The controls are a second webview, `player`, laid over the picture, and the
//! order of the three surfaces is the whole trick -- it is why this file
//! exists:
//!
//! 1. mpv is started with the window's handle and makes its surface a child of
//!    the window. Windows puts it *beneath* the catalogue's webview, which was
//!    there first -- measured, not assumed: the child-window list during
//!    playback reads catalogue, then mpv, bottom last.
//! 2. The control webview is added after that, which puts it on top of both.
//!    It is transparent, so whatever is beneath it shows through everywhere a
//!    control is not drawn.
//! 3. The catalogue's webview is hidden while the controls are up, so what
//!    shows through is mpv's picture.
//!
//! That is the z-order problem madde 2 names, solved by sequencing and by
//! hiding rather than by reaching into the Win32 API, which keeps `unsafe` out
//! of this crate. When the player goes, the control webview is closed, mpv's
//! surface goes with its process, and the catalogue is shown again exactly as
//! it was left -- it was hidden, never unloaded.
//!
//! On a platform where the handle cannot be had, nothing here applies and mpv
//! opens its own window as it did before `v0.16`.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use tauri::{AppHandle, Manager, WebviewBuilder, WebviewUrl};

/// The running application, once it exists.
///
/// A `OnceLock` rather than a field on the session, because `--play` starts
/// resolving a link before the window has been built, and the self-test builds
/// a session with no window at all.
static APP: OnceLock<AppHandle> = OnceLock::new();

/// Whether the controls are up. Both webviews poll, and either may be the
/// one that notices the player has gone; this makes sure only one of them
/// takes the controls down, rather than the second closing a webview the
/// first already closed.
static OPEN: AtomicBool = AtomicBool::new(false);

/// The label of the control webview. Capabilities name it.
pub const CONTROLS: &str = "player";

/// The label of the window everything lives in.
const MAIN: &str = "main";

/// Where the webview keeps its profile: beside the executable, like
/// everything else this program writes.
///
/// WebView2's default is `%LOCALAPPDATA%\<identifier>\EBWebView`, which is
/// where every build from `v0.14` to `v0.17` put it -- the one thing this
/// program wrote outside its own folder, unnoticed because the check for it
/// deleted the files this program names, and this one it never named. Both
/// webviews must use the same folder: WebView2 runs one browser process per
/// profile, and two in one window is two of everything.
fn profile() -> std::path::PathBuf {
    crate::state::beside_exe("eon-webview")
}

/// Build the window. In code rather than `tauri.conf.json`, because the
/// profile folder is only known at run time.
///
/// # Errors
///
/// Whatever Tauri reports when the window cannot be built.
pub fn main_window(app: &AppHandle) -> tauri::Result<()> {
    #[allow(unused_mut)]
    let mut builder =
        tauri::WebviewWindowBuilder::new(app, MAIN, WebviewUrl::App("index.html".into()))
            .title("EON Stream")
            .inner_size(1280.0, 820.0)
            .min_inner_size(880.0, 560.0)
            .resizable(true)
            .center()
            .theme(Some(tauri::Theme::Dark))
            .data_directory(profile());
    // A debug build can be driven over the DevTools protocol, which is how
    // the window is tested without anyone clicking it -- and without SendKeys.
    // The release build ignores the variable: it is not a switch a viewer's
    // environment should be able to flip.
    #[cfg(debug_assertions)]
    if let Ok(extra) = std::env::var("EON_WEBVIEW_ARGS") {
        builder = builder.additional_browser_args(&format!(
            "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection {extra}"
        ));
    }
    builder.build().map(|_| ())
}

/// Remember the application. Called once, from `setup`.
pub fn install(app: &AppHandle) {
    let _ = APP.set(app.clone());
}

/// Wait briefly for the window to exist.
///
/// A link passed with `--play` can be ready before the event loop has built
/// the window. Waiting a few seconds for it is better than opening mpv in a
/// window of its own, which is what a `None` here would mean.
fn app() -> Option<&'static AppHandle> {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(app) = APP.get() {
            if app.get_window(MAIN).is_some() {
                return Some(app);
            }
        }
        if Instant::now() >= deadline {
            return None;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// The arguments that put mpv's picture inside the window.
///
/// Empty when there is no window to put it in, which leaves mpv on its own
/// window -- the behaviour every build before `v0.16` had, and still a working
/// one.
#[must_use]
pub fn mpv_arguments() -> Vec<String> {
    let Some(handle) = app()
        .and_then(|app| app.get_window(MAIN))
        .and_then(|w| native(&w))
    else {
        return Vec::new();
    };
    vec![
        format!("--wid={handle}"),
        // The controls are ours, drawn in the webview over the picture. mpv's
        // own on-screen controller would be a second set under the first.
        "--osc=no".to_owned(),
        "--osd-bar=no".to_owned(),
        // Every key and click lands on the control webview, which is on top.
        // mpv acting on one as well would mean a space bar that pauses twice.
        "--input-default-bindings=no".to_owned(),
        "--input-vo-keyboard=no".to_owned(),
        "--background-color=#000000".to_owned(),
        // No frame of mpv's own. Its picture is a child of this window, and
        // Windows does not give a child window the themed frame a top-level
        // one gets: with mpv's default `--border=yes` the frame it draws
        // while it sizes itself -- before the first frame, which is exactly
        // the loading screen -- came out in the old classic style, a
        // Windows 7 title bar inside the player.
        // (`--title-bar` would say it more precisely, but it only exists in
        // recent mpv, and an mpv that meets an option it does not know
        // refuses to start at all.)
        "--border=no".to_owned(),
        "--window-dragging=no".to_owned(),
    ]
}

#[cfg(windows)]
fn native(window: &tauri::Window) -> Option<isize> {
    // The handle is a pointer-sized integer to mpv, which is all `--wid`
    // takes. Reading it is not `unsafe`; only dereferencing it would be.
    window.hwnd().ok().map(|hwnd| hwnd.0 as isize)
}

#[cfg(not(windows))]
fn native(_window: &tauri::Window) -> Option<isize> {
    None
}

/// Lay the controls over the picture. Call after mpv has started.
///
/// Replaces an existing control webview rather than keeping it: one that was
/// created before this mpv would be *under* this mpv's surface.
pub fn open_controls() {
    let Some(app) = app() else { return };
    let Some(window) = app.get_window(MAIN) else {
        return;
    };
    if let Some(existing) = app.get_webview(CONTROLS) {
        let _ = existing.close();
    }
    let size = window
        .inner_size()
        .ok()
        .and_then(|size| {
            window
                .scale_factor()
                .ok()
                .map(|scale| size.to_logical::<f64>(scale))
        })
        .unwrap_or(tauri::LogicalSize::new(1280.0, 820.0));
    #[allow(unused_mut)]
    let mut builder = WebviewBuilder::new(CONTROLS, WebviewUrl::App("player.html".into()))
        .data_directory(profile())
        .transparent(true)
        .auto_resize()
        .focused(true);
    #[cfg(debug_assertions)]
    if let Ok(extra) = std::env::var("EON_WEBVIEW_ARGS") {
        builder = builder.additional_browser_args(&format!(
            "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection {extra}"
        ));
    }
    match window.add_child(builder, tauri::LogicalPosition::new(0.0, 0.0), size) {
        Ok(webview) => {
            OPEN.store(true, Ordering::SeqCst);
            // The catalogue steps aside. mpv's surface sits beneath it, so
            // while it is drawn the picture is not seen at all.
            if let Some(main) = app.get_webview(MAIN) {
                let _ = main.hide();
            }
            let _ = webview.set_focus();
        }
        Err(e) => eprintln!("could not lay the controls over the picture: {e}"),
    }
}

/// Take the controls away. The picture goes with mpv's process.
pub fn close_controls() {
    if !OPEN.swap(false, Ordering::SeqCst) {
        return;
    }
    let Some(app) = APP.get() else { return };
    if let Some(webview) = app.get_webview(CONTROLS) {
        let _ = webview.close();
    }
    if let Some(main) = app.get_webview(MAIN) {
        let _ = main.show();
        let _ = main.set_focus();
    }
    if let Some(window) = app.get_window(MAIN) {
        // Leaving fullscreen with the player is what a viewer expects: the
        // catalogue was never meant to fill the screen.
        if window.is_fullscreen().unwrap_or(false) {
            let _ = window.set_fullscreen(false);
        }
    }
}

/// Fullscreen on or off, for the whole window. The picture and the controls
/// both follow the window's size.
pub fn toggle_fullscreen() -> Result<bool, String> {
    let app = APP.get().ok_or_else(|| "there is no window".to_owned())?;
    let window = app
        .get_window(MAIN)
        .ok_or_else(|| "there is no window".to_owned())?;
    let next = !window.is_fullscreen().map_err(|e| e.to_string())?;
    window.set_fullscreen(next).map_err(|e| e.to_string())?;
    Ok(next)
}
