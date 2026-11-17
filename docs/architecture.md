# Architecture notes

Working notes for the client. The authoritative decisions live in the project plan
(`eon-docs/plan/eon-plan.md`); this file explains how they land in code.

## Layers

| Layer | Where | Rule |
|---|---|---|
| Interface | `src/` | Static HTML, CSS and JavaScript. Knows nothing about torrents or mpv. Talks to the Rust side through named IPC commands only. |
| Host | `src-tauri/` | Owns the window, the IPC commands, the session state, build profiles. |
| Prompt | `cli/` | The headless build. Shares the libraries below, not the code above. |
| Decisions | `eon-stream-core` | Module manager, addon protocol, signatures. No GUI dependency, ever. |
| Bytes | `eon-stream-engine` | Torrent, local HTTP server, mpv IPC. |

A GUI dependency appearing in `eon-stream-core`, or an addon URL reaching `src/`, means a layer
has leaked.

**What the interface is handed, and what it is not.** Views in `src-tauri/src/view.rs` carry
what a person reads, not what a request needs: an addon has an id, a name and a version and
no address; a source has an index, a label and its facts and no URL; a running torrent has an
address with no session token. Three reasons, in order of how badly each would bite:

1. A configured addon address can carry a credential in its path, and a stream URL can carry
   a debrid token (madde 22). `addons.read` in the Module ABI makes the same promise.
2. `src/` is the layer that would host a third-party module's code if the sandbox decision
   (madde 4) lands on an isolated webview. What it can reach is what a module could reach.
3. `play 3` meaning "the third source the host resolved" is also simply the clearest
   contract: the number cannot go stale in a way that plays the wrong thing.

The interface also has **no network permission at all** — no plugin is enabled, the CSP
forbids it, and CI fails the build if a `fetch` appears in `src/`. Every request it causes is
a command in `src-tauri/src/commands.rs`, where the whole surface is enumerated in one file.

## Player: a separate process, drawing inside the window

The risky part of mpv is not compiling it. It is managing a native video surface — a child
HWND on Windows, an NSView on macOS, X11/Wayland on Linux — beneath or above the system
webview, and compositing the interface onto it. Z-order and transparency there are a known
ordeal, and getting it wrong on one platform blocks the release on all of them.

**mpv is still a separate process over JSON IPC** (madde 2). Since `v0.16` it draws inside the
EON Stream window rather than in a window of its own (plan v0.7), and it got there without
libmpv and without `unsafe`, by sequencing three surfaces (`src-tauri/src/surface.rs`):

1. mpv is launched with `--wid=<the window's handle>` and makes its picture a child of the
   window. Windows places it *beneath* the catalogue webview — measured during playback,
   not assumed.
2. A second, transparent webview (`src/player.html`) is added after that, so it is on top of
   both. It carries the controls and nothing else, and only exists while something plays.
3. The catalogue webview is hidden for as long as the controls are up, so what shows through
   the transparent page is mpv's picture. When playback ends either way — Stop, the end of
   the file, mpv dying — the control webview is closed and the catalogue shown again, exactly
   as it was left.

Measured on Windows only. Where the handle cannot be had, `surface::mpv_arguments` returns
nothing and mpv opens its own window, which is the pre-`v0.16` behaviour and still works.
Two webviews in one window need Tauri's `unstable` feature; that is a dependency on an API
that can change under a Tauri upgrade, so an upgrade is not done without playing something.

Embedding libmpv stays a later, independent slice behind `eon-stream-engine`'s `embedded-mpv`
feature, for the platforms where `--wid` will not do (Android first).

## The interface is relocatable

The owner asked that the whole interface be fetchable from the web without shipping a new
binary, or that the project be able to get there at once. Today it is **compiled in** — the
files in `src/` are embedded at build time — and it is kept in a shape where moving it is a
configuration change, not a rewrite:

- `src/` is plain static files with **relative paths only**: no build step, no bundler, no
  absolute URL, no inline script (the CSP forbids it). Copy the folder to any HTTPS origin and
  it is the same interface.
- Its **only** way to the host is `window.__TAURI__.core.invoke` with the commands in
  `commands.rs`. It has no network permission of its own (CI fails on a `fetch`), so where it
  is served from changes nothing about what it can do.
- No sentence lives in `src/`: every label comes from the catalogue the host serves
  (madde 40), so a remote copy cannot drift from the binary's language files.

Switching is three edits in one commit: the window's URL in `tauri.conf.json` becomes the remote
origin; `capabilities/default.json` gains a `remote.urls` entry for exactly that origin; and
the CSP's `default-src`/`script-src`/`style-src` name it.

It is **not switched on**, and doing it is a plan decision rather than a convenience, because
a page served from elsewhere is code nobody here built, holding every command in
`commands.rs`. The precondition is the same one modules have: a **signed** interface bundle
verified against the trust set before it is loaded (madde 3, 30, 37). With an empty trust set
the honest answer is that it cannot be done safely yet. It must also never apply to the Edu
build (madde 22): an institution's window does not load code from our origin. On mobile the backend is
expected to be media3/ExoPlayer entirely — which is why `PlayerBackend` is an interface from
the start, not a refactor waiting to happen.

## Build profiles

Cargo features, resolved at compile time:

- default → **EON Stream**: marketplace client, addon-by-URL, open torrent access.
- `edu` → **EON Edu**: none of the above compiled in; institution catalogue instead; Edu
  signing key; Edu update channel; separate application id.

Features are additive and leak easily, so the exclusion is verified rather than trusted: CI
inspects the Edu binary for forbidden symbols and strings and fails the release on a hit.
Treat that job as part of the feature, not as an optional lint.

## IPC surface

Every command is named, typed, and enumerated in one place. Reasons, in order of how badly
each has burned other projects:

1. It is the security boundary between untrusted frontend code and the host.
2. Modules reach the same surface through the Module ABI; two ways in would mean two
   permission models.
3. An enumerated surface can be diffed in review; an ad-hoc one cannot.

## Update flow

Signature verified before anything is written. Version never goes backwards. Staged rollout
with an emergency stop. In Edu the institution approves a version before it reaches school
machines — a school must not be surprised mid-term (madde 39).

## Accessibility

A release gate, not later polish (madde 35): full keyboard operation with a visible focus
ring, screen-reader-usable core flows (add addon, find content, play, choose subtitles),
WCAG AA contrast, text scaling without layout breakage, and respect for reduced-motion.

## Open questions

- Frontend framework. Not decided; the constraint is that it must not fight screen readers
  or make the bundle enormous.
- Whether declarative themes are interpreted in the frontend or compiled to CSS in Rust.
- How the mpv window is positioned relative to the EON Stream window on each platform, and what
  happens on multi-monitor and fullscreen transitions.
