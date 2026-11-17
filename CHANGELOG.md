# Changelog

Format: [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

Versions here are the **tags**, not the crate version. Tags count the digits after the dot
as an alpha counter with the trailing zero dropped — `v0.19` is the twentieth alpha and is
followed by `v0.2` — and the crate version maps mechanically: tag `v0.X` is `0.X.0-alpha`,
because Cargo will not accept anything but three-component semver. One consequence worth
knowing: semver ordering is not build order during the alpha, so read the dates.

**The interface arrived at `v0.14`, not at `v1`.** The plan (madde 2) had the window as the
last thing added; it was brought forward at the owner's decision. What did not change is
the half that mattered: every part of what the window does was working at a prompt first,
and the headless build still ships beside it.

## [Unreleased]

## [v0.14] — 2026-11-17 — there is a window

`eon-stream.exe` is now a Tauri application. `eon-stream-cli.exe` is the headless build,
published alongside it under the same naming scheme, and it is still the only place some
things live.

### Added — the interface
- **A window** (madde 2): the system webview through Tauri, with the interface as static
  HTML, CSS and JavaScript compiled into the binary. No bundler, no Node, no second
  dependency tree — `cargo build` is the whole build.
- **Home**: continue watching, your library, and every catalogue of every enabled addon as
  a row of posters. Rows fill one at a time, so a slow addon delays its own row rather
  than the screen.
- **Search** across every searchable catalogue of every addon at once, with the same
  correction towards titles that exist the prompt does.
- **An item page**: poster, description, year, runtime, genres, cast; seasons and episodes
  for a series, with what has been watched marked; keep it in the library or stop keeping
  it.
- **Sources**, ranked by the viewer's preferences, narrowed by quality bucket, each row
  carrying what it is: torrent, seeders, size, subtitles, whether it needs headers.
- **Playback** with a player bar: position, duration, pause, seek, stop, and — for a
  torrent — what the swarm is doing, a line a second, while it buffers and while it plays.
- **Addons**: add by URL, enable, disable, reorder, refresh, remove.
- **Settings**: the preferences that change what gets picked and what gets downloaded, plus
  the settings audit (madde 36) and what this build trusts (madde 30), said in the window
  rather than discovered later.
- **Turkish and English in the interface**, switchable without restarting. The interface
  holds no text of its own: every label comes from the catalogue in `eon-stream-core`
  (madde 40), which is what makes item 8 of the v1 definition-of-done complete.
- `--selftest`, which checks everything behind the window that a machine can check and
  gates the release: both catalogues, the interface's strings, the empty trust set, the
  settings audit, and that **no addon address appears in anything the interface is
  handed** (madde 22).

### Added — at the prompt
- One search box over every addon, quality buckets, a library (`save`, `unsave`,
  `library`), paging everywhere, and `help` as numbered sections rather than sixty lines.
- `trailer`, and `files` paging a season pack by the torrent's own indices.

### Fixed
- **An addon's `fileIdx` pointing at a subtitle made playback fail with "lost contact with
  mpv".** Found by playing a torrent from the new window: the addon named file 0, file 0
  was a 140-byte `.srt`, mpv had nothing to play and quit before answering. The engine now
  decides which file to play — what the viewer picked, else what the addon named *if it is
  a video*, else the largest real video — and says what mpv actually did when it will not
  answer.
- **Where playback got to is now remembered when mpv closes itself.** It was only recorded
  while mpv was still running, so watching something to the end — the ordinary case — left
  nothing in continue-watching.

### Changed
- The binaries are `eon-stream` (the window) and `eon-stream-cli` (the prompt); the
  headless build was `eon-stream` before `v0.14`. Both read and write the same files in the
  same folder.
- The version lives in `[workspace.package]` so the two binaries cannot disagree about what
  they are.

### Known limits of the window
Modules, the update check, subtitle search by hand, track selection and `eon://` deep links
are in the headless build only. Video plays in mpv's own window (madde 2): embedding a
native surface under the webview is a later, independent slice.

## [v0.13] — 2026-10-04

Found by playing a torrent through the published `v0.12` binary with mpv, rather than by
reading the code. Everything in `v0.12` passed its tests; both of these were in the part a
person actually sees and does.

### Fixed
- **A second run could not play anything.** Starting the engine twice failed with
  `error binding UDP socket ... only one usage of each socket address is permitted`, and the
  failure was total — no torrent would start. librqbit's default DHT persists its port and
  then prefers it on the next start, so anything still holding that port (a second instance,
  or the previous one closing) stopped the whole session. The DHT is now configured
  explicitly with no persistence, so the OS picks a free port every run.
- **This program was writing outside its own folder, and `v0.12`'s notes said it was not.**
  The same DHT default wrote a JSON file into an OS config directory, carrying a `peer_store`
  — a record of which swarms this machine had joined, outliving the session. It no longer
  writes anything.
- **The player's window title showed a percent-encoded name.** Someone watching Big Buck
  Bunny read `Big%20Buck%20Bunny.mp4` in their title bar.
- **The metadata timeout blamed the swarm for a cold DHT.** It said the torrent has no
  seeders, which is misleading when the real cause is a bare info hash with no trackers on a
  freshly started routing table. It now names both possibilities.

### Known cost of the DHT fix
A cold routing table at every start. It lands on exactly one path — a bare info hash with no
trackers, which has to find peers through the DHT — and nowhere else; a magnet link carrying
trackers never waits for it. Keeping a peer store on disk to avoid this was the trade that
was already rejected above.

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

> **This was not true in `v0.12`.** The torrent engine's DHT also wrote a state file into an
> OS config directory, carrying a record of which swarms the machine had joined. Fixed in
> `v0.13`; left stated here rather than quietly corrected, because a claim about what a
> program writes to your disk is the kind a reader is entitled to see withdrawn.

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
- The Tauri scaffold. The whole alpha line is headless on purpose; the interface is the
  last thing added, on top of work already proven from a prompt. A half-generated scaffold
  rots and hides which decisions are actually made (madde 2).

  *Superseded at `v0.14`*, which added the interface — written against a system that
  already worked, which is what this entry was protecting.

[Unreleased]: https://github.com/EON-Extensible-Open-Network/eon-stream-app/compare/v0.14...HEAD
[v0.14]: https://github.com/EON-Extensible-Open-Network/eon-stream-app/compare/v0.13...v0.14
[v0.13]: https://github.com/EON-Extensible-Open-Network/eon-stream-app/compare/v0.12...v0.13
[v0.12]: https://github.com/EON-Extensible-Open-Network/eon-stream-app/compare/v0.11...v0.12
[v0.11]: https://github.com/EON-Extensible-Open-Network/eon-stream-app/compare/v0.1...v0.11
[v0.1]: https://github.com/EON-Extensible-Open-Network/eon-stream-app/compare/v0.0.0-alpha...v0.1
[v0.0.0-alpha]: https://github.com/EON-Extensible-Open-Network/eon-stream-app/releases/tag/v0.0.0-alpha
