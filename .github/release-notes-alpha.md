## EON Stream — alpha

**Torrents stream now.** Add a Stremio-compatible addon, browse it, and play a
source in mpv — including a BitTorrent source, which downloads sequentially into
a loopback server the player reads from.

Still an alpha with no graphical interface: everything happens at a prompt. That
is deliberate — the system gets proven at the command line first, and the window
comes last.

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

- **No graphical interface.** By design, until `v1`.
- **No installer and no code signing.** See the warnings below.
- **YouTube sources** are handed to mpv's `ytdl` hook, so they need `yt-dlp`
  installed; without it they will not open.
- **BitTorrent v2** magnet links (`btmh`) are refused by name. This build reads
  v1 info hashes.
- **Private trackers** are not supported: a passkey in a tracker URL is a
  credential and handling it properly is its own piece of work.

### You need mpv

Playback is mpv in its own window, driven over JSON IPC. Install it:

```powershell
winget install shinchiro.mpv
```

If it is somewhere unusual, set `EON_MPV_PATH` to the full path of `mpv.exe`.

### Try it

Download the `.exe` into a folder of its own and run it from PowerShell or
Windows Terminal:

```
add https://v3-cinemeta.strem.io/manifest.json
catalogs
browse 1
open 3
```

Cinemeta serves catalogues and metadata only, so `open` will find no sources.
That is Cinemeta behaving correctly. To see playback immediately, hand `play` a
URL, a file, or a magnet link directly:

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

Everything beside the executable, and nothing anywhere else:
`eon-addons.json`, `eon-history.json`, `eon-settings.json`, `eon-modules.json`,
`eon-revocations.json`, `eon-modules/` for installed module content, and
`eon-torrents/` for downloaded pieces — which are discarded on exit unless you
set `torrent.keepFiles`. Delete any of them to start that part over.

Nothing leaves your machine except requests to addons you added yourself, the
torrent swarms you asked for, and — only when you type `update` — a check
against the release manifest.

### Honest warnings

**Unsigned.** No code signing certificate yet, so SmartScreen will warn and
**Smart App Control will block it outright if you have that turned on**. We are
not asking anyone to disable a security feature to run an alpha.

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
