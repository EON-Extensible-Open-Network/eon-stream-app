// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 EON contributors
//
// Additional permission under GNU GPL version 3 section 7:
// see LICENSE-EXCEPTION.md (EON Module ABI Exception 1.0).

//! EON Stream alpha — a terminal front end over the addon protocol.
//!
//! This is **not** the product. The product is a Tauri application, and it does
//! not exist yet. This binary exists for one purpose: so a person can add a
//! Stremio-compatible addon and see, with their own eyes, that the protocol
//! layer works.
//!
//! Playback works for anything mpv can open directly — an HTTP(S) stream or a
//! local file — driven over JSON IPC (madde 2). BitTorrent sources are listed
//! but not yet playable: sequential download and the local HTTP server are the
//! next slice.
//!
//! Numbered selection rather than identifiers, because typing
//! `tt0903747:1:1` by hand is not a user interface at any level of shallowness.

#![forbid(unsafe_code)]
#![allow(clippy::print_stdout, clippy::print_stderr)]

use std::{
    fs,
    io::{self, Read, Write},
    path::PathBuf,
    time::Duration,
};

use eon_stream_core::{
    http::{HttpClient, HttpError, HttpResponse, Limits},
    AddonClient, AddonRegistry, MetaPreview, Stream, StreamSource,
};
use eon_stream_engine::{MpvPlayer, PlaybackSource, PlayerOptions, SeekMode};

const VERSION: &str = env!("CARGO_PKG_VERSION");

/// A real HTTP client. Kept here rather than in the library, so the library
/// pulls in no network stack.
struct Http {
    agent: ureq::Agent,
    limits: Limits,
}

impl Http {
    fn new() -> Self {
        let limits = Limits::default();
        let agent = ureq::AgentBuilder::new()
            .timeout(Duration::from_millis(limits.timeout_ms))
            .redirects(u32::from(limits.max_redirects))
            .user_agent(concat!(
                "EON-Stream/",
                env!("CARGO_PKG_VERSION"),
                " (+https://github.com/EON-Extensible-Open-Network)"
            ))
            .build();
        Self { agent, limits }
    }
}

impl HttpClient for Http {
    fn get(&self, url: &str) -> Result<HttpResponse, HttpError> {
        let response = match self.agent.get(url).call() {
            Ok(response) => response,
            // A non-success status is a response, not a transport failure: the
            // caller needs the status the addon actually sent.
            Err(ureq::Error::Status(status, response)) => {
                let mut body = Vec::new();
                let _ = response.into_reader().read_to_end(&mut body);
                return Ok(HttpResponse { status, body });
            }
            // Never put the URL in here: an addon's configuration credential
            // lives in its path.
            Err(ureq::Error::Transport(t)) => {
                return Err(HttpError::new(
                    t.message()
                        .map_or_else(|| t.kind().to_string(), ToOwned::to_owned),
                ));
            }
        };
        let status = response.status();
        let mut body = Vec::new();
        let cap = self.limits.max_body_bytes as u64 + 1;
        response
            .into_reader()
            .take(cap)
            .read_to_end(&mut body)
            .map_err(|e| HttpError::new(e.to_string()))?;
        Ok(HttpResponse { status, body })
    }

    fn limits(&self) -> Limits {
        self.limits
    }
}

/// Where the addon list is kept: beside the executable, so the binary stays
/// self-contained and leaves nothing behind elsewhere. No account, no sync,
/// nothing sent anywhere.
fn store_path() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join("eon-addons.json")))
        .unwrap_or_else(|| PathBuf::from("eon-addons.json"))
}

/// What the last listing showed, so the next command can say "3" instead of an
/// identifier.
#[derive(Default)]
struct Selection {
    /// Catalogues, as (addon id, content type, catalogue id, label).
    catalogs: Vec<(String, String, String, String)>,
    /// Items from the last catalogue page.
    items: Vec<MetaPreview>,
    /// Sources from the last `open`, flattened and numbered.
    streams: Vec<Stream>,
}

fn main() {
    println!("EON Stream {VERSION}  ·  protocol alpha");
    println!("An open ecosystem for everyone.  https://github.com/EON-Extensible-Open-Network");
    println!();
    println!("Plays HTTP streams and local files through mpv. BitTorrent sources are");
    println!("listed but not playable yet -- the streaming engine is the next piece of work.");
    println!();

    let http = Http::new();
    let client = AddonClient::new(&http);
    let mut registry = load();
    let mut selection = Selection::default();
    let mut player: Option<MpvPlayer> = None;

    if registry.is_empty() {
        println!("No addons installed. EON Stream ships with none, ever.");
        println!("Try a first one:");
        println!("  add https://v3-cinemeta.strem.io/manifest.json");
    } else {
        println!(
            "{} addon(s) installed. Type `list` or `help`.",
            registry.len()
        );
    }

    loop {
        print!("\neon> ");
        if io::stdout().flush().is_err() {
            return;
        }
        let mut line = String::new();
        match io::stdin().read_line(&mut line) {
            Ok(0) => return, // end of input
            Ok(_) => {}
            Err(e) => {
                eprintln!("could not read input: {e}");
                return;
            }
        }

        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let mut parts = line.split_whitespace();
        let command = parts.next().unwrap_or("");
        let rest: Vec<&str> = parts.collect();

        match command {
            "quit" | "exit" | "q" => return,
            "help" | "?" => help(),
            "add" => add(&client, &mut registry, &rest),
            "list" => list(&registry),
            "remove" | "rm" => remove(&mut registry, &rest),
            "catalogs" | "cat" => catalogs(&registry, &mut selection),
            "browse" | "b" => browse(&client, &registry, &mut selection, &rest),
            "open" | "o" => open(&client, &registry, &mut selection, &rest),
            "play" | "p" => play(&mut player, &selection, &rest),
            "pause" | "resume" | "seek" | "where" => control(&mut player, command, &rest),
            "stop" => match player.take() {
                Some(active) => {
                    let _ = active.quit();
                    println!("stopped");
                }
                None => println!("nothing is playing"),
            },
            other => {
                println!("unknown command: {other}   (try `help`)");
            }
        }
    }
}

fn help() {
    println!("  add <url>        install an addon from its manifest URL");
    println!("  list             installed addons");
    println!("  remove <n>       uninstall addon number n");
    println!("  catalogs         numbered list of every catalogue on offer");
    println!("  browse <n> [q]   open catalogue n, optionally searching for q");
    println!("  open <n>         details and sources for item n of the last listing");
    println!("  play <n>         open source n in mpv  (or: play <url or file path>)");
    println!("  pause / resume   hold and release playback");
    println!("  seek <n>         jump n seconds, negative to go back");
    println!("  where            current position");
    println!("  stop             close the player");
    println!("  quit");
}

fn load() -> AddonRegistry {
    fs::read_to_string(store_path())
        .ok()
        .and_then(|json| AddonRegistry::from_json(&json).ok())
        .unwrap_or_default()
}

fn save(registry: &AddonRegistry) {
    let Ok(json) = registry.to_json() else {
        eprintln!("could not serialise the addon list");
        return;
    };
    if let Err(e) = fs::write(store_path(), json) {
        eprintln!("could not save the addon list: {e}");
    }
}

fn add(client: &AddonClient<&Http>, registry: &mut AddonRegistry, rest: &[&str]) {
    let Some(url) = rest.first() else {
        println!("usage: add <manifest url>");
        return;
    };
    print!("fetching... ");
    let _ = io::stdout().flush();
    match client.install(registry, url) {
        Ok(addon) => {
            save(registry);
            let s = addon.summary();
            println!("\radded {} {}", s.name, s.version);
            println!("  id         {}", s.id);
            println!("  types      {}", s.types.join(", "));
            println!("  resources  {}", s.resources.join(", "));
            println!("  catalogues {}", s.catalog_count);
            if s.configuration_required {
                println!("  note: this addon says it needs configuring before it is useful");
            }
        }
        Err(e) => println!("\rcould not add the addon: {e}"),
    }
}

fn list(registry: &AddonRegistry) {
    if registry.is_empty() {
        println!("no addons installed");
        return;
    }
    for (i, s) in registry.summaries().iter().enumerate() {
        let state = if s.enabled { "" } else { "  [disabled]" };
        println!("{:>3}. {} {}{}", i + 1, s.name, s.version, state);
        println!("     {}", s.id);
        if let Some(d) = &s.description {
            println!("     {d}");
        }
    }
}

fn remove(registry: &mut AddonRegistry, rest: &[&str]) {
    let Some(id) = nth_addon_id(registry, rest.first().copied()) else {
        println!("usage: remove <number from `list`>");
        return;
    };
    match registry.remove(&id) {
        Ok(addon) => {
            save(registry);
            println!("removed {}", addon.manifest.name);
        }
        Err(e) => println!("{e}"),
    }
}

/// Resolve "3" against the installed list.
fn nth_addon_id(registry: &AddonRegistry, arg: Option<&str>) -> Option<String> {
    let n: usize = arg?.parse().ok()?;
    registry
        .addons()
        .get(n.checked_sub(1)?)
        .map(|a| a.id().to_owned())
}

fn catalogs(registry: &AddonRegistry, selection: &mut Selection) {
    selection.catalogs.clear();
    for addon in registry.addons() {
        for catalog in &addon.manifest.catalogs {
            selection.catalogs.push((
                addon.id().to_owned(),
                catalog.content_type.clone(),
                catalog.id.clone(),
                format!(
                    "{} · {} / {}{}",
                    addon.manifest.name,
                    catalog.content_type,
                    catalog.display_name(),
                    if catalog.is_searchable() {
                        "  (searchable)"
                    } else {
                        ""
                    }
                ),
            ));
        }
    }

    if selection.catalogs.is_empty() {
        println!("none of the installed addons offers a browsable catalogue");
        return;
    }
    for (i, (_, _, _, label)) in selection.catalogs.iter().enumerate() {
        println!("{:>3}. {label}", i + 1);
    }
    println!("\n`browse <n>` to open one, or `browse <n> <search terms>`");
}

fn browse(
    client: &AddonClient<&Http>,
    registry: &AddonRegistry,
    selection: &mut Selection,
    rest: &[&str],
) {
    if selection.catalogs.is_empty() {
        println!("run `catalogs` first");
        return;
    }
    let Some(index) = rest.first().and_then(|n| n.parse::<usize>().ok()) else {
        println!("usage: browse <number from `catalogs`> [search terms]");
        return;
    };
    let Some((addon_id, content_type, catalog_id, label)) =
        index.checked_sub(1).and_then(|i| selection.catalogs.get(i))
    else {
        println!("no catalogue number {index}");
        return;
    };
    let Some(addon) = registry.get(addon_id) else {
        println!("that addon is no longer installed");
        return;
    };

    let query = rest[1..].join(" ");
    let extra: Vec<(&str, &str)> = if query.is_empty() {
        Vec::new()
    } else {
        vec![("search", query.as_str())]
    };

    println!("{label}");
    match client.catalog(addon, content_type, catalog_id, &extra) {
        Ok(page) => {
            selection.items = page;
            if selection.items.is_empty() {
                println!("  (empty)");
                return;
            }
            for (i, item) in selection.items.iter().enumerate() {
                let year = item.release_info.as_deref().unwrap_or("    ");
                let rating = item
                    .imdb_rating
                    .as_deref()
                    .map_or_else(String::new, |r| format!("  {r}"));
                println!("{:>3}. {}  {}{}", i + 1, year, item.display_name(), rating);
            }
            println!("\n`open <n>` for details and sources");
        }
        Err(e) => println!("  catalogue request failed: {e}"),
    }
}

fn open(
    client: &AddonClient<&Http>,
    registry: &AddonRegistry,
    selection: &mut Selection,
    rest: &[&str],
) {
    let Some(index) = rest.first().and_then(|n| n.parse::<usize>().ok()) else {
        println!("usage: open <number from the last listing>");
        return;
    };
    let Some(item) = index.checked_sub(1).and_then(|i| selection.items.get(i)) else {
        println!("no item number {index} in the last listing");
        return;
    };

    // Metadata comes from whichever installed addon offers `meta` for this
    // type and id -- not necessarily the one the catalogue came from.
    let meta_source = registry
        .candidates_for("meta", &item.content_type, &item.id)
        .next();

    match meta_source {
        Some(addon) => match client.meta(addon, &item.content_type, &item.id) {
            Ok(meta) => print_meta(&meta),
            Err(e) => println!("metadata request failed: {e}"),
        },
        None => {
            println!("{}", item.display_name());
            println!("  (no installed addon provides metadata for this item)");
        }
    }

    // Sources from every addon that offers `stream`, with failures reported
    // beside the results rather than swallowed.
    let found = client.streams_from_all(registry, &item.content_type, &item.id);
    println!();
    if found.items.is_empty() && found.failures.is_empty() {
        println!("No addon installed here provides sources for this item.");
        println!("Cinemeta is catalogue and metadata only -- it serves no streams.");
    }

    // Flattened and numbered, so `play 2` means something.
    selection.streams.clear();
    for (addon_id, streams) in &found.items {
        let name = registry
            .get(addon_id)
            .map_or(addon_id.as_str(), |a| a.manifest.name.as_str());
        println!("sources from {name}:");
        for stream in streams {
            let kind = match stream.source() {
                Some(StreamSource::Direct(_)) => "http",
                Some(StreamSource::Torrent { .. }) => "torrent",
                Some(StreamSource::YouTube(_)) => "youtube",
                Some(StreamSource::External(_)) => "external",
                None => "unplayable",
            };
            selection.streams.push(stream.clone());
            println!(
                "{:>3}. [{kind:>10}] {}",
                selection.streams.len(),
                stream.display_label()
            );
        }
    }
    for failure in &found.failures {
        println!("  addon failed: {failure}");
    }
    if found.total() > 0 {
        println!(
            "\n{} source(s). `play <n>` to open one in mpv.",
            found.total()
        );
    }
}

/// Hand a source to mpv, or explain why it cannot be handed over yet.
fn play(player: &mut Option<MpvPlayer>, selection: &Selection, rest: &[&str]) {
    let Some(first) = rest.first() else {
        println!("usage: play <number from the last `open`>   |   play <url or file path>");
        return;
    };

    // A number selects from the last listing; anything else is taken literally,
    // which is how a local file or a direct URL gets tested without an addon.
    let source = if let Ok(index) = first.parse::<usize>() {
        let Some(stream) = index.checked_sub(1).and_then(|i| selection.streams.get(i)) else {
            println!("no source number {index} in the last listing");
            return;
        };
        match stream.source() {
            Some(StreamSource::Direct(url)) => PlaybackSource::Url(url.to_owned()),
            Some(StreamSource::Torrent { .. }) => {
                println!("This is a BitTorrent source. The streaming engine is the next piece");
                println!("of work, so it cannot be played yet -- sequential download and the");
                println!("local HTTP server come before mpv can be pointed at it.");
                return;
            }
            Some(StreamSource::YouTube(id)) => {
                println!("This is a YouTube id ({id}). mpv can play those with yt-dlp");
                println!("installed; wiring that up is not part of this alpha.");
                return;
            }
            Some(StreamSource::External(url)) => {
                println!("This source is a link to open in a browser, not a stream:");
                println!("  {url}");
                return;
            }
            None => {
                println!("That source object has nothing playable in it.");
                return;
            }
        }
    } else {
        let joined = rest.join(" ");
        let path = std::path::Path::new(&joined);
        if path.is_file() {
            PlaybackSource::File(path.to_path_buf())
        } else {
            PlaybackSource::Url(joined)
        }
    };

    // Replace any player already running, rather than leaving it orphaned.
    if let Some(existing) = player.take() {
        let _ = existing.quit();
    }

    println!("starting mpv on {}...", source.display_hint());
    let options = PlayerOptions {
        title: Some(format!("EON Stream — {}", source.display_hint())),
        ..PlayerOptions::default()
    };
    match MpvPlayer::launch(&source, &options) {
        Ok(mut started) => {
            match started.duration() {
                Ok(Some(seconds)) => println!("  duration {seconds:.0}s"),
                _ => println!("  (mpv is loading; duration not known yet)"),
            }
            println!("  mpv has its own window. `pause`, `resume`, `seek <n>`, `stop`.");
            *player = Some(started);
        }
        Err(e) => println!("could not start playback: {e}"),
    }
}

/// Commands that only make sense while something is playing.
fn control(player: &mut Option<MpvPlayer>, command: &str, rest: &[&str]) {
    let Some(active) = player.as_mut() else {
        println!("nothing is playing. `play <n>` first.");
        return;
    };
    if !active.is_running() {
        println!("mpv has exited.");
        *player = None;
        return;
    }

    let outcome = match command {
        "pause" => active.set_paused(true),
        "resume" => active.set_paused(false),
        "seek" => match rest.first().and_then(|s| s.parse::<f64>().ok()) {
            Some(seconds) => active.seek(seconds, SeekMode::Relative),
            None => {
                println!("usage: seek <seconds, negative to go back>");
                return;
            }
        },
        "where" => {
            match active.position() {
                Ok(Some(position)) => println!("  at {position:.1}s"),
                Ok(None) => println!("  position not known yet"),
                Err(e) => println!("  {e}"),
            }
            return;
        }
        _ => return,
    };

    match outcome {
        Ok(()) => println!("  ok"),
        Err(e) => println!("  {e}"),
    }
}

fn print_meta(meta: &eon_stream_core::Meta) {
    println!("\n{}", meta.display_name());
    if let Some(year) = &meta.release_info {
        println!("  year     {year}");
    }
    if let Some(runtime) = &meta.runtime {
        println!("  runtime  {runtime}");
    }
    if !meta.genres.is_empty() {
        println!("  genres   {}", meta.genres.join(", "));
    }
    if !meta.cast.is_empty() {
        println!("  cast     {}", meta.cast.join(", "));
    }
    if let Some(description) = &meta.description {
        println!("\n  {description}");
    }
    let seasons = meta.seasons();
    if !seasons.is_empty() {
        println!();
        for season in seasons {
            let episodes = meta.season(season);
            println!("  Season {season}  ({} episode(s))", episodes.len());
            for episode in episodes.iter().take(5) {
                println!("    {}  {}", episode.id, episode.display_title());
            }
            if episodes.len() > 5 {
                println!("    ... {} more", episodes.len() - 5);
            }
        }
    }
}
