# eon-stream-app

The desktop client (Tauri + Rust). **Both builds are produced here:** EON Stream and EON Edu.

[![Status](https://img.shields.io/badge/status-Faz%200%20·%20active-f59e0b)](https://github.com/EON-Extensible-Open-Network/eon-docs/blob/main/plan/eon-plan.md)
[![License](https://img.shields.io/badge/license-GPL--3.0--or--later%20+%20module%20exception-3b82f6)](LICENSE)

---

## Two builds, one core

```
cargo build --release                                    # EON Stream
cargo build --release --no-default-features --features edu   # EON Edu
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

## Status: there is a window now

**`v0.14` is the first build with an interface.** It arrived during the alpha rather than at
`v1`, which is a change from what madde 2 planned — the plan said the window came last, on
top of work already proven at a prompt. The first half of that still held: everything the
window does was working at a prompt first, and the headless build ships beside it.

Two binaries come out of this repository, both from the same libraries:

| | |
|---|---|
| **`eon-stream.exe`** | the product. A Tauri window: catalogues as posters, search, a library, sources ranked by quality, playback with progress, addons and settings. |
| **`eon-stream-cli.exe`** | the headless build all of it was proven in, and still the only place some things live — modules, subtitles by hand, the update check, deep links. |

What the current build does:

| | |
|---|---|
| **Addons** | add by URL, list, remove, order, enable, refresh, configure, discover, health |
| **Browsing** | catalogues, search, pagination, metadata, episodes, `eon://` deep links |
| **Playback** | mpv over JSON IPC — HTTP, HLS, DASH, local files, and **BitTorrent** |
| **Torrents** | sequential download, loopback HTTP server, prebuffer, file selection, progress |
| **Subtitles** | from addons, matched by name, hash and size, loaded into the running player |
| **History** | resume where you left off, continue-watching, next episode, same release |
| **Modules** | install, update, remove, enable, order, dependencies, **signature verification** |
| **Themes** | declarative, no code, with a WCAG contrast report |
| **Settings** | persisted, typed, and auditably free of anything that reports anywhere |
| **Updates** | signed release manifests, downgrades refused, revocation applied |
| **Language** | Turkish and English, in the window and at the prompt |

Against the eight items of the v1 definition-of-done: 2, 3, 4, 5, 6, 7 and 8 are done; 1 is
done apart from packaging and signing. What is left between here and `v1` is **the
installer and the signing keys** — plus the parts of the window that still send a person to
the prompt.

**One thing refuses to work, by design.** `install` installs nothing, because this build
trusts no signing keys — no key in the hierarchy exists yet (madde 30). The mechanism is
complete and exercised; see the note in [`CHANGELOG.md`](CHANGELOG.md#known-limitation-by-design)
for how to mint one and watch the chain run.

## Architecture

```
eon-stream-app (this repo)
  src-tauri/     Rust: window, the IPC surface, state, build profiles
  src/           the interface: static HTML, CSS and JavaScript. No bundler, no Node.
  cli/           the headless build, which shares the libraries and not the code above
        |
        +-- eon-stream-core     module manager, addon protocol, signatures
        +-- eon-stream-engine   torrent, local HTTP server, mpv IPC
```

The interface asks the host for things **by name or by number**. It never sees an addon's
address or a stream URL — both can carry a credential (madde 22) — and it has no network
permission of its own: every request it makes is one of the commands enumerated in
`src-tauri/src/commands.rs`. CI fails the build if `src/` grows a `fetch`.

v1 plays through **mpv as a separate process** (madde 2). Controls live in the EON Stream window,
video in the mpv window. Embedding libmpv in a single window is a later slice; if it never
lands, v1 behaviour stands.

See [`docs/architecture.md`](docs/architecture.md).

## What v1 is

Windows and Linux, single-file installer, signed, auto-updating. Stremio-compatible addon
resolution, mpv playback, subtitles, resume. Add addons by URL. Sequential torrent
streaming. Module manager with signature verification. Declarative themes. Compatibility
suite green. Turkish and English, meeting a baseline accessibility bar (madde 35).

What is left between here and `v1`: the installer and the signing keys.

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

## Build

```bash
rustup show                      # toolchain pinned in rust-toolchain.toml
cargo build --release            # both binaries
cargo run -p eon-stream-app      # the window
cargo run -p eon-stream-cli      # the prompt
```

**There is no `npm install` step and there never was one.** The interface is static HTML,
CSS and JavaScript compiled into the binary by `tauri-build`; no bundler means no second
dependency tree to audit (madde 37) and no build that only works on a machine with Node.

`cargo run -p eon-stream-app -- --selftest` is what CI runs before a release: it checks
everything behind the window that a machine can check — both message catalogues, the
interface's strings, the empty trust set, the settings audit, and that no addon address
appears in anything the interface is handed.

System requirements: WebView2 (present on Windows 11) and mpv for playback.

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
