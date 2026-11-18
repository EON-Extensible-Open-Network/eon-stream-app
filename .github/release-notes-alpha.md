## EON Stream — alpha

**There is a window.** Add a Stremio-compatible addon, browse its catalogues as
posters, search across every addon at once, pick a source, and watch it in mpv —
including a BitTorrent source, which downloads sequentially into a loopback
server the player reads from.

Two files are published. `eon-stream-…exe` is the window. `eon-stream-cli-…exe`
is the headless build every part of it was proven in, and it still does things
the window does not: modules, subtitles by hand, the update check, deep links.
They sit in the same folder and share the same files.

### What works

- **Addons**: add one from its manifest URL, list, remove, reorder, enable,
  refresh, discover more through an addon that advertises them
- **Browsing**: numbered catalogues, paging, search, metadata, seasons and
  episodes for series
- **Sources**: collected and ranked across every installed addon, with failing
  addons reported beside the results rather than swallowed
- **Playback** through mpv: HTTP(S), HLS, DASH, local files — pause, resume,
  seek, tracks, speed, volume, subtitle and audio delay
- **BitTorrent**: magnet links, bare info hashes, and torrent sources from an
  addon. Sequential download ahead of the playhead, a loopback HTTP server bound
  to `127.0.0.1` with a per-session token, and file selection inside multi-file
  torrents
- **Subtitles** from addons, matched by the file's name, hash and size, loaded
  into the running player
- **Resume**: it remembers where you stopped, offers unfinished items, and finds
  the next episode keeping the same release
- **Modules**: install, update, remove, enable, order, with dependency
  resolution and Ed25519 signature verification — see the note below
- **Themes**: declarative, carrying no code, with a WCAG 2.1 contrast report
- **Settings** that persist, and `audit`, which walks them and proves nothing in
  them reports anywhere
- **Turkish and English**: `lang tr`
- Ships with **no addons and no sources**, and sends **nothing anywhere**

### `install` refuses everything, on purpose

This build trusts no signing keys, so the module manager installs nothing. No
key in the hierarchy exists yet: the key custody belongs to a legal entity that
does not exist, and minting a root key for one person to hold on a laptop would
be worse than having none — it would produce signatures that look like the real
thing.

The mechanism is complete rather than pending. A client reads its trust set from
`eon-trust.json` beside the executable, so you can mint your own key, sign a
module and watch the whole chain run. `trust` shows what the build trusts, which
today is nothing. There is no flag that skips verification.

### What does not work yet

- **No installer and no code signing.** See the warnings below.
- **The window does not do everything the prompt does.** Modules, the update
  check, subtitle search by hand, track selection and `eon://` deep links are
  in `eon-stream-cli` only.
- **Playing inside the window is Windows-only so far.** mpv draws into the
  EON Stream window there; it has not been checked anywhere else yet.
- **YouTube sources** are handed to mpv's `ytdl` hook, so they need `yt-dlp`
  installed; without it they will not open.
- **BitTorrent v2** magnet links (`btmh`) are refused by name. This build reads
  v1 info hashes.
- **Private trackers** are not supported: a passkey in a tracker URL is a
  credential and handling it properly is its own piece of work.

### You need mpv

Playback is mpv, running as its own process and drawing inside the EON Stream
window, driven over JSON IPC. Install it:

```powershell
winget install shinchiro.mpv
```

If it is somewhere unusual, set `EON_MPV_PATH` to the full path of `mpv.exe`.

### Try it

Download both files into a folder of their own and run `eon-stream-…exe`. It
opens with nothing in it, because nothing is bundled — go to **Addons**, paste a
manifest URL, and the home screen fills with what that addon offers:

```
https://v3-cinemeta.strem.io/manifest.json
```

Cinemeta serves catalogues and metadata only, so opening a film will find no
sources. That is Cinemeta behaving correctly: it tells you what exists, and a
streaming addon tells you where to watch it. To see playback immediately, paste
a magnet link, a URL or a file path into the **Open** box at the top — or start
the program on one:

```
eon-stream-v0.15-windows-x86_64.exe --play "magnet:?xt=urn:btih:..."
```

The prompt is still there, in `eon-stream-cli-…exe`:

```
play https://download.blender.org/peach/bigbuckbunny_movies/BigBuckBunny_320x180.mp4
play C:\Users\you\Videos\something.mkv
play magnet:?xt=urn:btih:dd8255ecdc7ca55fb0bbf81323d87062db1f6d1c
torrent
pause
seek -10
resume
where
stop
```

(Both Blender sources above are CC-BY — legitimate test material, not a content
recommendation.)

`help` lists every command. `lang tr` switches to Turkish.

### What it writes

Everything beside the executable, and nothing anywhere else — both binaries
share these files: `eon-addons.json`, `eon-history.json`, `eon-library.json`,
`eon-settings.json`, `eon-modules.json`, `eon-revocations.json`,
`eon-modules/` for installed module content, `eon-torrents/` for downloaded
pieces (discarded on exit unless you set `torrent.keepFiles`), `eon-subtitles/`
for subtitles fetched for what is playing (emptied at every start and on exit),
`eon-speeds.json` for how fast each torrent you played came in (info hashes and
numbers only, for sorting sources by speed), `eon-ratings.json` for your own
scores and reviews, `eon-community.json` for a community server you chose and your
session on it,
and `eon-webview/`, the window's browser profile. Builds up to `v0.17` put that
profile in `%LOCALAPPDATA%\org.eonstream.stream` instead; that folder can be
deleted. Delete any of them to start that part over.

Nothing leaves your machine except requests to addons you added yourself, the
torrent swarms you asked for, and — only when you type `update` — a check
against the release manifest.

In `v0.12` that was not quite true: the torrent engine's DHT also wrote a state
file into an OS config directory, with a record of which swarms the machine had
joined. `v0.13` removed it. If you ran `v0.12`, that file is still wherever your
system keeps application data — nothing reads it any more, and deleting it is
safe.

### Honest warnings

**Unsigned.** No code signing certificate yet, so SmartScreen will warn and
**Smart App Control will block it outright if you have that turned on**. We are
not asking anyone to disable a security feature to run an alpha.

**The window needs WebView2**, which Windows 11 has and Windows 10 usually does.
It is Microsoft's, it is not shipped here, and it is what the interface is drawn
with.

**Verify what you downloaded** against `SHA256SUMS.txt`:

```powershell
Get-FileHash .\eon-stream-*.exe -Algorithm SHA256
```

**BitTorrent is not anonymous.** Peers in a swarm see your IP address. That is
how BitTorrent works, and nothing here changes it. This build seeds while it is
running and stops when it exits.

**Alpha means alpha.** Contracts are at `v0` and explicitly unstable.

### Built by CI

Built by GitHub Actions from the tagged commit with a pinned toolchain, not on
anyone's laptop. The mpv IPC layer is covered by tests that run against a real
mpv on the Windows runner.

### Licence

GPL-3.0-or-later with the EON Module ABI Exception.
[eon-stream-app](https://github.com/EON-Extensible-Open-Network/eon-stream-app) ·
[eon-stream-core](https://github.com/EON-Extensible-Open-Network/eon-stream-core) ·
[eon-stream-engine](https://github.com/EON-Extensible-Open-Network/eon-stream-engine)

Not affiliated with, endorsed by, or connected to Stremio. mpv is a separate
program under its own licence; see `NOTICE`.
