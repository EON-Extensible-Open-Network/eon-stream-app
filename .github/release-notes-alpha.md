## EON Stream — protocol alpha

**This is not a media player yet. It cannot play anything.**

It exists so you can add a Stremio-compatible addon and see, with your own eyes,
that the protocol layer works: manifest, catalogues, search, metadata, and the
list of sources an addon returns. Playback needs mpv, which is the next piece of
work.

### What it does

- Adds an addon from its manifest URL, and keeps the list between runs
- Lists catalogues, browses them, searches them
- Shows metadata, including seasons and episodes for series
- Collects sources from every installed addon and reports which addons failed
- Ships with **no addons and no sources**, and sends **no telemetry**

### Try it

Download the `.exe`, put it in a folder of its own, and run it from a terminal
(PowerShell or Windows Terminal — it is a console program, double-clicking works
but the window closes when you quit):

```
eon-stream-v0.0.0-alpha-windows-x86_64.exe
```

Then:

```
add https://v3-cinemeta.strem.io/manifest.json
catalogs
browse 1
open 3
```

`catalogs` numbers everything on offer; `browse 1` opens the first one;
`open 3` shows the third item of that listing. `help` lists the commands.

Cinemeta serves catalogues and metadata only — it provides no streams, so `open`
will say no sources were found. That is Cinemeta behaving correctly, not a bug.

The addon list is stored in `eon-addons.json` next to the executable. Delete it
to start over. Nothing else is written, and nothing leaves your machine except
the requests to the addons you added yourself.

### Honest warnings

**Unsigned.** There is no code signing certificate yet, so Windows SmartScreen
will warn, and **Smart App Control will block it outright if you have that
turned on**. We are not asking anyone to disable a security feature to run an
alpha. If it is blocked, that is the expected outcome for an unsigned binary from
an unknown publisher, and the right response is to wait for signed builds.

**Verify what you downloaded** against `SHA256SUMS.txt`:

```powershell
Get-FileHash .\eon-stream-v0.0.0-alpha-windows-x86_64.exe -Algorithm SHA256
```

**Alpha means alpha.** Contracts are at `v0` and explicitly unstable. Anything
here can change without a migration path.

### Built by CI

This binary was built by GitHub Actions from the tagged commit with a pinned
toolchain, not on anyone's laptop. The workflow is
`.github/workflows/release.yml`, and the run that produced it is linked from this
release.

### Licence

GPL-3.0-or-later, with the EON Module ABI Exception. Source:
[eon-stream-app](https://github.com/EON-Extensible-Open-Network/eon-stream-app),
protocol layer in
[eon-stream-core](https://github.com/EON-Extensible-Open-Network/eon-stream-core).

Not affiliated with, endorsed by, or connected to Stremio.
