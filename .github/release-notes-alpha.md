## EON Stream — alpha

**It plays things now.** Add a Stremio-compatible addon, browse it, and open a
source in mpv.

Still an alpha with no graphical interface: everything happens at a prompt. That
is deliberate — the system gets proven at the command line first, and the window
comes last.

### What works

- **Addons**: add one from its manifest URL, list, remove, and keep the list
  between runs
- **Browsing**: numbered catalogues, paging, search
- **Metadata**: details, and seasons with episodes for series
- **Sources**: collected from every installed addon, with failing addons reported
  beside the results rather than swallowed
- **Playback**: HTTP(S) streams and local files, through mpv — pause, resume,
  seek, position, stop
- Ships with **no addons and no sources**, and sends **no telemetry**

### What does not work yet

- **BitTorrent sources are listed but not playable.** Sequential download and the
  local HTTP server are the next piece of work; until then mpv has nothing to be
  pointed at.
- **YouTube sources** need yt-dlp wiring that is not done.
- No graphical interface.

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
URL or a file directly:

```
play https://download.blender.org/peach/bigbuckbunny_movies/BigBuckBunny_320x180.mp4
play C:\Users\you\Videos\something.mkv
pause
seek -10
resume
where
stop
```

(The Blender film above is CC-BY — a legitimate test source, not a content
recommendation.)

`help` lists every command. The addon list lives in `eon-addons.json` next to the
executable; delete it to start over. Nothing else is written, and nothing leaves
your machine except requests to addons you added yourself.

### Honest warnings

**Unsigned.** No code signing certificate yet, so SmartScreen will warn and
**Smart App Control will block it outright if you have that turned on**. We are
not asking anyone to disable a security feature to run an alpha.

**Verify what you downloaded** against `SHA256SUMS.txt`:

```powershell
Get-FileHash .\eon-stream-*.exe -Algorithm SHA256
```

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
