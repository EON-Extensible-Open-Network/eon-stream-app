# Architecture notes

Working notes for the client. The authoritative decisions live in the project plan
(`eon-docs/plan/eon-plan.md`); this file explains how they land in code.

## Layers

| Layer | Where | Rule |
|---|---|---|
| Interface | `src/` | Knows nothing about torrents or mpv. Talks to the Rust side through named IPC commands only. |
| Host | `src-tauri/` | Owns windows, IPC commands, the module host, build profiles. |
| Decisions | `eon-stream-core` | Module manager, addon protocol, signatures. No GUI dependency, ever. |
| Bytes | `eon-stream-engine` | Torrent, local HTTP server, mpv IPC. |

A GUI dependency appearing in `eon-stream-core`, or an addon URL reaching `src/`, means a layer
has leaked.

## Player: separate process in v1

The risky part of mpv is not compiling it. It is managing a native video surface — a child
HWND on Windows, an NSView on macOS, X11/Wayland on Linux — beneath or above the system
webview, and compositing the interface onto it. Z-order and transparency there are a known
ordeal, and getting it wrong on one platform blocks the release on all of them.

So: **v1 runs mpv as a separate process** over JSON IPC. Controls in the EON Stream window, video
in the mpv window (madde 2).

Embedding is a later, independent slice behind `eon-stream-engine`'s `embedded-mpv` feature. If it
never works, v1 behaviour stands rather than the release stalling. On mobile the backend is
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
