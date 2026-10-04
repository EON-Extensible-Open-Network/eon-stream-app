# Changelog

Format: [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

Versions here are the **tags**, not the crate version. Tags count the digits after the dot
as an alpha counter with the trailing zero dropped — `v0.19` is the twentieth alpha and is
followed by `v0.2` — and the crate version maps mechanically: tag `v0.X` is `0.X.0-alpha`,
because Cargo will not accept anything but three-component semver. One consequence worth
knowing: semver ordering is not build order during the alpha, so read the dates.

**No interface until `v1`.** The whole alpha line is a terminal program, by design. Every
piece is proven at a prompt first, so that when there is a window it sits on work that
already works.

## [Unreleased]

## [v0.12] — 2026-10-04

### Added — torrents play, and modules install

Four items of the v1 definition-of-done land here: torrent streaming (4), the module
manager with signature verification (5), declarative themes (6), and the non-interface half
of Turkish and English (8). The compatibility suite (7) becomes a named CI check.

- **BitTorrent sources play.** `play <n>` on a torrent source resolves it through the
  engine — metadata from the swarm, the file picked, a prebuffer ahead of the playhead —
  and hands mpv a loopback URL. `play <magnet>` and `play <info hash>` work too, which is
  how a torrent someone sent you gets played without an addon.
  - `play <n> file <i>` picks a file inside the torrent, for when the addon's `fileIdx` is
    wrong or absent and the guess is not what you wanted.
  - `torrent` shows progress, rates, peers and where it is being served. `files` lists
    every file in the running torrents.
  - The download directory, rate limits, peer limit, prebuffer size and whether to keep the
    pieces are all settings.
- **A module manager.** `modules`, `install <manifest>`, `uninstall`, `enable-module`,
  `disable-module`, `order-module`. Installing shows what the module is asking for and waits
  for a yes, before anything is installed (madde 3) — and in v1 that list is always empty,
  because only declarative modules install.
- **Declarative themes.** `theme <id>` applies one and prints its WCAG 2.1 contrast for
  every pair that actually gets rendered; `theme tokens` shows the resolved values. Being
  able to check a palette from a command line is the only reason it can be checked at all
  before there is a window — which is what makes basic accessibility (madde 35) something
  this release can claim rather than defer.
- **Turkish and English.** `lang tr` switches, and every sentence this program prints comes
  from a catalogue rather than from the code (madde 40). Command names stay as typed:
  translating `play` would mean the documentation stops matching the program.
- **Settings that persist.** `settings`, `set <path> <value>`. Ranking preferences used to
  be session-only and now survive a restart. `audit` walks the serialised settings and
  proves there is no reporting key in them — a sentence saying there is no telemetry is
  worth exactly as much as the check behind it (madde 36).
- **Updates.** `update` reads a signed release manifest and refuses a downgrade (madde 39).
  `update verify <file>` checks a download against the hash *inside that signed manifest*,
  which is the check the release process asks for and is not the same thing as the checksum
  published next to the file.
- **Revocation.** `revocations` shows what is known and when a newer list is due;
  `revocations refresh` fetches one. A revoked module is disabled and reported with its
  reason and advisory, never quietly removed.
- `trust` shows which signing keys this build trusts — which today is none, see below.

### Changed
- The banner no longer says BitTorrent sources are unplayable, because they are not.
- `prefer` writes through to the settings file. A preference that forgets itself on restart
  is a preference nobody sets twice.
- The CLI is five files instead of one: `main.rs` plus `torrents`, `modules`, `prefs` and
  `text`.

### Known limitation, by design
**This build trusts no signing keys, so `install` refuses everything.** No key in the
hierarchy exists: custody belongs to a legal entity that does not exist yet (madde 30), and
minting a root key for one person to hold on a laptop in the meantime would be worse than
having none — it would produce signatures that look like the real thing. The client says so
rather than failing obscurely.

The mechanism is complete and exercised, not theoretical. A client reads its trust set from
`eon-trust.json` beside the executable, so anyone can mint a key, sign a module and watch
the whole chain work: signature verified, content hash checked, permissions shown, tampered
content refused, downgrade refused. That file is a statement of what one machine trusts,
which is the same mechanism an institution will use for its own key. There is no flag that
skips verification.

### Files this writes, all beside the executable
`eon-addons.json`, `eon-history.json`, `eon-settings.json`, `eon-modules.json`,
`eon-revocations.json`, `eon-trust.json` (read only), and `eon-modules/` for installed
module content. Nothing elsewhere, no account, nothing sent anywhere.

## [v0.11] — 2026-09-26 — the protocol, properly

Stream behaviour hints, so sources that need headers actually play. Ranked and deduplicated
sources across addons. Subtitles fetched with the file's name, hash and size, and loaded
into the running player. Track selection, speed, delays, volume, buffer and network state
over IPC. Series navigation with resume, next episode and `bingeGroup`. Addon health with
bounded backoff, catalogue pagination, addon discovery through `addon_catalog`, and `eon://`
deep links.

## [v0.1] — 2026-09-26 — it plays things

Playback through mpv over JSON IPC: HTTP streams and local files, with pause, resume, seek,
position and stop.

## [v0.0.0-alpha] — 2026-09-25 — protocol alpha

Adds a Stremio-compatible addon and shows what it returns: catalogues, search, metadata and
the list of sources. No playback.

### Note on this file
It was not maintained for `v0.0.0-alpha`, `v0.1` or `v0.11` — those three shipped with tag
messages and no changelog entry. The three summaries above were reconstructed from those
tag messages. Recorded rather than quietly backfilled, because a changelog that silently
acquires history is less trustworthy than one that says where it lapsed.

### Scaffolding, from the initial commit
- Licence, module exception, and `NOTICE` covering the mpv/FFmpeg distribution
  requirements.
- `docs/architecture.md`: layer boundaries, the separate-process player decision, build
  profiles, IPC surface rules, update flow, accessibility gate.
- `.env.example` with no network defaults.

### Still deliberately absent
- The Tauri scaffold. The whole alpha line is headless on purpose (see the top of this
  file); the interface is the last thing added, at `v1`, on top of work already proven from
  a prompt. A half-generated scaffold rots and hides which decisions are actually made
  (madde 2).

[Unreleased]: https://github.com/EON-Extensible-Open-Network/eon-stream-app/compare/v0.12...HEAD
[v0.12]: https://github.com/EON-Extensible-Open-Network/eon-stream-app/compare/v0.11...v0.12
[v0.11]: https://github.com/EON-Extensible-Open-Network/eon-stream-app/compare/v0.1...v0.11
[v0.1]: https://github.com/EON-Extensible-Open-Network/eon-stream-app/compare/v0.0.0-alpha...v0.1
[v0.0.0-alpha]: https://github.com/EON-Extensible-Open-Network/eon-stream-app/releases/tag/v0.0.0-alpha
