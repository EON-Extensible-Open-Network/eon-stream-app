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

## [v0.24] — 2026-11-18 — the cache stays inside the space you give it

### Fixed
- **Every torrent took its whole size on disk the moment it started.** librqbit means to
  make its files sparse on Windows and does not: its `FSCTL_SET_SPARSE` call passes a null
  bytes-returned pointer, which Windows refuses for a synchronous handle, and the failure
  was only logged. The file was then grown to full length and NTFS reserved all of it — a
  sixty-gigabyte 4K episode cost sixty gigabytes before a minute had arrived, and three
  episodes could fill a disk. The engine now marks each file sparse itself. Measured: a
  263 MB film at 12% downloaded takes 32 MB, where it took 263 MB.
- **A Windows 7 title bar inside the player while it loaded.** mpv's picture is a child of
  the window, and a child window does not get the themed frame; with mpv's default border
  the frame it drew while sizing itself, before the first frame, came out in the old classic
  style. mpv now runs borderless when it is embedded.

### Added
- **A limit on the disk the cache may use** (Settings → disk space for the cache). Nothing
  is deleted until the cache is 80% full. Past that it is brought down to 60% in one go:
  first whole torrents that are not playing, least recently played first — so the episode
  before is still there if you go back to it — then the part of what is playing that you
  have already watched, never closer than three minutes behind the playhead.
- **The watched part of what is playing is given back to the disk** while it plays: its
  range of the file is deallocated in place. Which bytes are watched is not guessed from the
  clock — a minute of a battle scene can be a hundred times a minute of credits. Every five
  seconds the keeper records where mpv's reader really is in the file; mpv's read-ahead is
  bounded at launch, so anything below a three-minute-old reading, less that bound, has
  been played. Seeking back past it restarts the stream there, from the same source.
- **Near the limit, the download is held to the film's own pace**, judged by what is ready
  without a gap at the playhead rather than by the total, which after a seek counts bytes
  from before it. At the limit only what the picture is about to need is fetched; 5% over
  it, the download stops even if the picture has to wait, and the player says why. Measured
  with a 150 MB limit on a 263 MB film: the disk peaked at 151 MB and playback did not stop.
- **The limit is held under the disk's free space**, less 2 GB, whatever it is set to.
  The setting offers only sizes the disk can hold and shows the free space, what the cache
  uses, and the limit in force. A disk that filled up while the program was closed lowers
  the limit by itself, and the viewer is told. Automatic (the default) is a quarter of the
  free space, between 4 and 50 GB.

### Changed
- Each torrent is downloaded into `eon-torrents/<info hash>/`. Pieces left by an earlier
  version in `eon-torrents/` are not reused; Settings → clear buffer removes them.

## [v0.23] — 2026-11-18 — new episodes, and when the next ones come

### Added
- **Calendar.** A page with the episodes of the library's series from a month back to two
  months ahead: what is coming up, soonest first, and what aired recently, newest first,
  each marked new, coming or watched. A press opens the series.
- **New episodes on the home screen.** After "Continue watching", one card per series in
  the library with an episode that aired in the last two weeks and has not been watched —
  the newest such episode. The calendar link carries the number of such series.
- The calendar asks every series' addon — eight at a time, and at most the forty series
  most recently kept — so the answer is kept for half an hour, or until the library's
  series change, rather than asked on every visit to the home screen.

### Changed
- Discover keeps its own list of catalogues, so a row added to the home screen later (the
  new-episodes row) can no longer shift what its catalogue choices point at.

## [v0.22] — 2026-11-18 — the player adjusts to the viewer

### Added
- **Subtitle size**, in the player (75% to 200%) and in the settings. A size picked while
  watching applies at once and is kept for the next title (`playback.subtitleScale`).
- **Subtitle and sound timing.** Subtitles or sound that run early or late against the
  picture are moved a tenth of a second at a time, with a reset — in the player's new
  Adjust column, and for subtitles with `G` and `H`.
- **Keyboard shortcuts on `?`**: every key the player answers to, in the interface's
  language.
- **Autoplay can be turned off** (`playback.autoplayNext`, on by default). Off, the card
  near the end of an episode offers the next one and waits to be asked, at the end as
  well.
- The settings page shows subtitle size, autoplay and start muted.

### Fixed
- **Two playback settings were accepted and never applied by the window.** "Start muted"
  and the `playback.playerArguments` list reached the headless build's player but not the
  window's: `launch` built mpv's command line without them. Both apply now; the arguments
  are still validated by core as `--flag`s only, never a bare value mpv would read as a
  file.

## [v0.21] — 2026-11-18 — a title's page leads somewhere

### Added
- **Trailers.** A title whose addon lists a trailer has a Trailer button. A direct file
  plays in the window; a YouTube trailer plays in the window when `yt-dlp` is beside this
  program or on the `PATH`, for mpv's ytdl hook to use, and otherwise opens in the system
  browser — with a note saying how to keep it in the window. A browser is the one thing
  this program hands a link to, and only on that press.
- **Names are searches.** Each cast member and director on a title's page searches every
  addon for them.
- **Genres are a way in.** A genre on a title's page opens Discover on that genre, in the
  first catalogue of the title's type that can be narrowed to it.
- **More of the genre.** A title's page ends with more titles of its genre: an addon
  catalogue narrowed to that genre, without the title itself. It is called "More
  Adventure", not "More like this", because that is what it is — nothing here decides
  what is like what, and no addon was asked to.

## [v0.2] — 2026-11-18 — discover, and a library that shows everything

The twentieth alpha: `v0.19` is followed by `v0.2`, per the version scheme above.

### Added
- **Discover.** One catalogue at a time as a grid of posters, narrowed by content type,
  catalogue and genre, that loads the next page as it is scrolled. Every choice on it is a
  catalogue or a genre an addon declared: a catalogue that must be given a genre (Cinemeta's
  "New" takes a year) offers no "any". Leaving it for a title and coming back returns to
  the same place. An addon that ignores paging is noticed when a page brings nothing new,
  instead of being asked for the same page forever.
- **Search as you type.** Results arrive after a short pause once there are two letters,
  without leaving the page or jumping to the top on every letter, and a slower answer to an
  earlier query is never drawn over a newer one. Enter still searches at once.
- **The library page can be sorted** — most recently added, or by name — and narrowed to
  films or series when it holds both.

### Fixed
- **The library page showed at most 24 titles.** It was drawn from the home screen's
  library row, which is the newest two dozen; it now asks for the whole library.

## [v0.19] — 2026-11-18 — one episode after another

### Added
- **Up next.** Thirty seconds before the end of an episode a card names the next one and
  counts ten seconds down to it, as the button filling up; the count waits while the
  picture is paused. "Watch the credits" stops it, and seeking back out of the credits
  forgets that. At the very end the next episode starts unless the viewer said otherwise.
- **The same release, episode after episode.** The next episode's sources are found and
  the one picked is the source whose `bingeGroup` matches the one the series was last
  watched with, then one from the same addon, then the best match — the rule the
  headless build's `next` already followed. "Cancel and go back" works while it is being
  found.
- **A next-episode button** in the player's controls, with the episode's name on it, and
  `N` on the keyboard.
- **Start over.** A film with somewhere to resume from offers "Start over" beside
  "Resume".
- **Watched, by hand.** Each episode has a mark that sets it watched or not watched — for
  an episode seen somewhere else, or one started by mistake. Not watched also takes it
  out of continue watching.

### Changed
- mpv holds the last frame at the end of a file (`--keep-open`) instead of exiting, so the
  controls decide what happens next. With no next episode, the player goes after a moment
  and the page it was started from is back, as before.
- The next episode is the next by season and episode number, specials last, and only one
  that has aired: an announced episode is not offered.

## [v0.18] — 2026-11-18 — a way out, and subtitles that are found

### Fixed
- **A start that never got going could not be left.** The loading screen's back button
  only hid the screen; the torrent kept resolving, the next status poll saw it still
  running and put the screen straight back, and when the prebuffer finally filled the
  player opened over whatever page the viewer had gone to. Leaving now gives up on the
  start for real: every start carries a ticket, cancelling invalidates it, and a worker
  still waiting on a swarm checks it between stages and four times a second during the
  prebuffer. A cancelled start reports nothing — not even the metadata timeout that
  arrives a minute later.
- **The window wrote outside its folder.** WebView2 kept the window's browser profile in
  `%LOCALAPPDATA%\org.eonstream.stream`, in every build since `v0.14` — the one thing this
  program wrote anywhere but beside the executable, missed because the check deleted the
  files this program names and this one it never named. The profile is now
  `eon-webview/` beside the executable, and the self-test lists it. The old folder can be
  deleted.
- **The spinner only appeared once playback had started and run dry.** A player that
  opened and never got its first frame — the usual shape of a dead swarm — showed a still
  black picture with nothing to say it was waiting. mpv idling with nobody having paused
  it now counts as waiting too.

### Added
- **"Cancel and go back", under the spinner.** While the picture waits on the network the
  player shows the peers and speed it is waiting on and one button: it closes mpv, takes
  the torrent out of the engine and deletes its pieces, and returns to the page it was
  started from. The loading screen has the same button, and Escape does the same on both.
  Where playback got to is still written down.
- **Subtitles are found, not hoped for.** Almost no source says which subtitles it has,
  so the filter over what sources say rarely helped. Every start now looks in three
  places, on its own thread, after the picture is up:
  - inside the video — mpv is told the viewer's languages (`--slang`, and `--alang` from
    the preferred audio languages) so an embedded track in one of them is picked;
  - beside the video in the same torrent — `.srt`, `.ass`, `.ssa` and `.vtt` files,
    served over loopback like the film, with the language read from the file name;
  - from the viewer's subtitle addons, told the file's name, size and hash where known.
    None is built in (madde 9); with none installed, the menu says how to get one.

  The best two per wanted language are loaded and the first is switched on — unless a
  subtitle is already on, or the film is already being heard in that language. The
  wanted languages are `playback.subtitleLanguages`, or the interface's language when it
  is not English.
- **A subtitle menu that lists what was found** but not loaded, with where it came from;
  one press loads and switches on one. **Load from a file…** takes a subtitle from this
  computer.
- **The sources page says which subtitle languages your addons have** for the title, and
  that they work with whichever source is picked.

### Changed
- Subtitles move up above the control bar while the controls are showing and back down
  when they fade, and have a heavier outline so they read over a bright picture.
- Fetched subtitles are written to `eon-subtitles/` beside the executable and handed to
  mpv as files: mpv loads a subtitle URL on its playback thread, so a slow server would
  have frozen the picture. The folder is emptied at every start and on exit.

## [v0.17] — 2026-11-17 — less waiting, better filters

### Fixed
- **Source filter counts now take the other filters into account.** The number on a
  value is how many sources would show if it were picked, given what the other filters
  already say. Before, picking English on a title with 44 4K sources and 3 English ones
  still said "4K 44", and picking 4K then listed nothing. A value that would empty the
  list now reads 0 and cannot be pressed.
- **Episode names.** Cinemeta and most addons write an episode's title as `name`, which
  was not read, so every episode showed only SxxEyy. Names and synopses show now; an
  episode without one keeps its season and episode number.
- **Catalogues that need a parameter** — Cinemeta's "New" needs a year — are asked with
  the first value the addon lists, instead of being drawn as a row that only showed an
  error. One that needs a value nobody lists is left off the screen.

### Added
- **Several values per filter.** Within one filter a source shows when it has any of the
  picked values (English *or* Turkish); between filters it has to satisfy each. The
  subtitle filter is always shown, below quality and audio. "Clear filters" resets them.
- **Sorting sources**: best match (the ranking, and the default), highest quality, most
  seeders, largest first, smallest first.
- **New & popular**: the catalogues that call themselves trending, then the ones that call
  themselves new. Chosen from the addons' own catalogue ids and names; this program still
  picks no titles itself.
- **Taking something off Continue watching**, from a menu on the card. For a series it
  removes every unfinished episode, so the previous one does not take its place; what was
  finished stays watched.

### Changed — performance
- Requests to addons no longer hold the session lock while they wait. One slow addon used
  to hold up every other row, the billboard, and the status poll behind the loading
  screen and the player's controls.
- mpv decodes on the graphics card where that is known to work (`--hwdec=auto-safe`). It
  runs with `--no-config`, so its default applied: decoding on the processor, which a 4K
  HEVC release saturates on a laptop while the torrent engine hashes on the same cores.
- Rows ask for their first page when they come near the screen, and rows off screen are
  not laid out or painted. Posters decode off the main thread. The blurred backdrop
  behind every card's badge and the resting shadow under every poster are gone; both were
  repainted as a row scrolled.
- The player's controls are not redrawn while they are hidden, and once faded they are
  taken out of the picture rather than left transparent over it. Neither webview queues a
  second status question behind a slow one, and the catalogue asks less often when
  nothing is starting.
- A source list asked for while an older one is still coming is the one that counts: an
  index read off one list can no longer be played from another.

## [v0.16] — 2026-11-17 — the picture is in the window

The window now looks like a place to watch things, and the video plays in it.

### Changed
- **Video plays inside the EON Stream window** (madde 2, plan v0.7). mpv is still a
  separate process over JSON IPC; it is handed the window's handle with `--wid`, and the
  controls are a transparent second webview laid over its picture. Windows only so far;
  elsewhere mpv keeps its own window. Needs Tauri's `unstable` feature, for two webviews in
  one window.
- **A new layout, on the pattern people already know from streaming services**: a bar
  across the top (Home, Series, Movies, Library, Addons, search, play a link, settings), a
  billboard with Play and More info, and rows of posters. EON's own colours throughout —
  the cyan-to-violet of the icon on ink-blue black.
- **Rows page sideways.** Arrow bubbles at either end turn one page, a mouse can grab and
  drag a row, and the dashes above it say which page is showing. Reaching the end asks the
  addon for its next page, so a row is as long as the catalogue.
- **The player** has the controls a viewer expects, drawn as icons: play/pause, back and
  forward 10 and 60 seconds, volume, stop, audio and subtitle tracks, speed, fullscreen, a
  timeline you can scrub, and keyboard shortcuts. They fade after three seconds of
  stillness and do not exist at all when nothing is playing.
- **Settings offer choices instead of text boxes.** Languages are picked from a list —
  typing narrows it, only a click chooses, and nothing that is not a language can be
  saved. Sizes, limits and peer counts are lists of values that work; booleans are
  switches. The interface language moved here from the corner of the window.
- Row titles are the catalogue's own name; the type and the addon are named beside it in
  the interface's language, instead of `movie` joined into the title.

### Added
- **A film's page lists its sources, filtered by dub and by subtitles** as well as by
  quality. Each filter offers only what the list holds, with how many of each, and a
  source names its audio and subtitle languages.
- **Clear buffer**, in Settings: deletes downloaded torrent pieces and says how much it
  freed. The default folder beside the program is removed whole; a folder the viewer
  chose only loses the torrents this session holds — it could be their Downloads folder.
  Refused while something plays.
- A loading screen between Play and the first frame, with the buffering progress.
- The volume setting is applied when mpv starts.

### Fixed
- **Long titles no longer push cards out of line**: a name is two lines tall at most and
  ends with an ellipsis, on every card.
- The preferred-audio and lowest-quality settings showed as empty in the window: they were
  read under the wrong keys.

## [v0.15] — 2026-11-17

Two things `v0.14` shipped wrong, found where this project keeps finding them: by
running the published binary and by reading what CI said about it.

### Fixed
- **The headless build announced "no interface yet, by design"** — in the release whose
  headline is the interface. It now says what it is: the headless build.
- **The smoke test asserted the shape of `help`, not the commands.** `help` became a list
  of sections at `v0.14`, so the step looking for `play <n>` in its output failed, and the
  release went out with a red CI run beside it. It now opens the sections by name and
  checks the commands are in them — the same lesson this step's own comment records from
  the last time it broke.

### Added
- **`eon-stream --play <magnet|url|path>`** opens the window and starts on it. It exists
  because verifying `v0.14` meant driving the window with synthetic keystrokes, which is
  not a check anyone should have to repeat; it is also the shape a file association and an
  `eon://` handler need. Same code path as pasting a link into the window.

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

[Unreleased]: https://github.com/EON-Extensible-Open-Network/eon-stream-app/compare/v0.15...HEAD
[v0.15]: https://github.com/EON-Extensible-Open-Network/eon-stream-app/compare/v0.14...v0.15
[v0.14]: https://github.com/EON-Extensible-Open-Network/eon-stream-app/compare/v0.13...v0.14
[v0.13]: https://github.com/EON-Extensible-Open-Network/eon-stream-app/compare/v0.12...v0.13
[v0.12]: https://github.com/EON-Extensible-Open-Network/eon-stream-app/compare/v0.11...v0.12
[v0.11]: https://github.com/EON-Extensible-Open-Network/eon-stream-app/compare/v0.1...v0.11
[v0.1]: https://github.com/EON-Extensible-Open-Network/eon-stream-app/compare/v0.0.0-alpha...v0.1
[v0.0.0-alpha]: https://github.com/EON-Extensible-Open-Network/eon-stream-app/releases/tag/v0.0.0-alpha
