# eon-stream-app

The desktop client (Tauri + Rust). **Both builds are produced here:** EON Stream and EON Edu.

[![Status](https://img.shields.io/badge/status-Faz%200%20·%20active-f59e0b)](https://github.com/EON-Extensible-Open-Network/eon-docs/blob/main/plan/eon-plan.md)
[![License](https://img.shields.io/badge/license-GPL--3.0--or--later%20+%20module%20exception-3b82f6)](LICENSE)

---

## Two builds, one core

```
cargo tauri build                      # EON Stream
cargo tauri build --no-default-features --features edu   # EON Edu
```

The Edu build does not *hide* the open marketplace, arbitrary addon URLs, or open
BitTorrent access. They are **excluded at compile time** — not present in the binary
(madde 22). The Edu module manager loads only modules signed with the Edu key, and the
application id, update channel and signing key are separate.

### The claim is tested, not asserted

Cargo features are additive and easy to leak. CI runs a symbol and string check against the
Edu binary and **fails the release** if a forbidden symbol appears. This matters because the
guarantee given to an institution rests on it (madde 32b): "absent at compile time" has to be
verifiable by someone who does not trust us.

## Status

**Faz 0.** No usable build yet, and **no Tauri scaffold is committed** — deliberately. A
half-generated scaffold is worse than none: it rots, and it hides which decisions are
actually made. The scaffold lands in the first real commit, once the mpv IPC prototype
(madde 2) proves the shape of the player integration.

What exists now: the licence, the notice, the architecture notes, and CI.

## Architecture

```
eon-stream-app (this repo)
  src-tauri/     Rust: window, IPC commands, module host, build profiles
  src/           frontend: interface, themes, declarative module rendering
        |
        +-- eon-stream-core     module manager, addon protocol, signatures
        +-- eon-stream-engine   torrent, local HTTP server, mpv IPC
```

v1 plays through **mpv as a separate process** (madde 2). Controls live in the EON Stream window,
video in the mpv window. Embedding libmpv in a single window is a later slice; if it never
lands, v1 behaviour stands.

See [`docs/architecture.md`](docs/architecture.md).

## What v1 is

Windows and Linux, single-file installer, signed, auto-updating. Stremio-compatible addon
resolution, mpv playback, subtitles, resume. Add addons by URL. Sequential torrent
streaming. Module manager with signature verification. Declarative themes. Compatibility
suite green. Turkish and English, meeting a baseline accessibility bar (madde 35).

**Not in v1:** marketplace, accounts, creator tools, material packages, music, mobile,
code-executing plugins, Edu.

## Non-negotiables in this repo

- **No telemetry.** Not off-by-default — absent. Crash reporting, if it ever exists, is
  opt-in and shows the user what would be sent (madde 36).
- **No bundled content source or addon.** Ever (madde 9).
- **Signing keys are never committed.** `.gitignore` blocks the usual extensions; that is a
  safety net, not a policy.
- **Accessibility is a release gate, not a later polish item**: full keyboard operation,
  screen-reader-usable core flows, WCAG AA contrast, scalable text (madde 35).

## Build (once the scaffold exists)

```bash
rustup show          # toolchain pinned in rust-toolchain.toml
npm install
cargo tauri dev
```

System requirements: a Tauri v2 toolchain (WebView2 on Windows, WebKitGTK on Linux) and mpv
on `PATH` for playback.

## License

`GPL-3.0-or-later` **WITH** the EON Module ABI Exception 1.0 — [`LICENSE`](LICENSE),
[`LICENSE-EXCEPTION.md`](LICENSE-EXCEPTION.md). Read [`NOTICE`](NOTICE) before distributing
anything that bundles mpv or FFmpeg.

## Related

[eon-stream-core](https://github.com/EON-Extensible-Open-Network/eon-stream-core) ·
[eon-stream-engine](https://github.com/EON-Extensible-Open-Network/eon-stream-engine) ·
[eon-stream-spec](https://github.com/EON-Extensible-Open-Network/eon-stream-spec) ·
[eon-edu-server](https://github.com/EON-Extensible-Open-Network/eon-edu-server) ·
[eon-docs](https://github.com/EON-Extensible-Open-Network/eon-docs)
