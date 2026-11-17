// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 EON contributors
//
// Additional permission under GNU GPL version 3 section 7:
// see LICENSE-EXCEPTION.md (EON Module ABI Exception 1.0).
//
// The interface.
//
// Three rules it keeps, all of them for the same reason -- this is the layer
// that would host a third-party module's code if the sandbox decision lands on
// an isolated webview (madde 4), so everything it can reach is everything a
// module could reach one day:
//
//   1. It asks the host for things by name or by number. It never sees an
//      addon's address or a stream URL, and could not send one anywhere if it
//      wanted to -- there is no network permission here at all.
//   2. Every node is built with createElement and textContent. An addon writes
//      the titles and descriptions that end up on this screen, and a template
//      string of someone else's HTML is how that becomes script.
//   3. No sentence is written in this file. Labels come from the catalogue
//      (madde 40); what is left are numbers, identifiers and punctuation.

const invoke = window.__TAURI__.core.invoke;

/** The message catalogue for the chosen language. */
let MESSAGES = {};

/** Where the last source list came from, so the back button has somewhere to go. */
let LAST = { view: "home", detail: null };

/** What the source list currently shows, by quality bucket. */
let SOURCES = { all: [], quality: null, subject: null };

/** The detail currently open. */
let DETAIL = null;

/** Rows on the home screen, filled one at a time. */
let ROWS = [];

// --------------------------------------------------------------------- text

function t(key, ...args) {
  const text = MESSAGES[key];
  if (text === undefined) return key;
  return text.replace(/\{(\d+)\}/g, (whole, index) => {
    const value = args[Number(index)];
    return value === undefined ? whole : String(value);
  });
}

function applyStrings() {
  for (const node of document.querySelectorAll("[data-say]")) {
    node.textContent = t(node.dataset.say);
  }
  document.getElementById("search-input").placeholder = t("ui.search.placeholder");
  document.getElementById("link-input").placeholder = t("ui.link.placeholder");
  document.getElementById("addon-input").placeholder = t("ui.addons.placeholder");
}

// ------------------------------------------------------------------ helpers

function el(tag, options = {}, children = []) {
  const node = document.createElement(tag);
  if (options.className) node.className = options.className;
  if (options.text !== undefined && options.text !== null) node.textContent = String(options.text);
  if (options.title) node.title = options.title;
  if (options.attrs) {
    for (const [name, value] of Object.entries(options.attrs)) {
      if (value !== undefined && value !== null) node.setAttribute(name, String(value));
    }
  }
  if (options.onClick) node.addEventListener("click", options.onClick);
  for (const child of children) {
    if (child) node.appendChild(child);
  }
  return node;
}

function clear(node) {
  while (node.firstChild) node.removeChild(node.firstChild);
}

function toast(message) {
  const node = document.getElementById("toast");
  node.textContent = String(message);
  node.hidden = false;
  clearTimeout(toast.timer);
  toast.timer = setTimeout(() => {
    node.hidden = true;
  }, 9000);
}

/** Every call to the host goes through here, so a failure is always shown. */
async function ask(command, args) {
  try {
    return await invoke(command, args);
  } catch (error) {
    toast(error);
    throw error;
  }
}

function clock(seconds) {
  if (seconds === null || seconds === undefined) return "";
  const whole = Math.max(0, Math.floor(seconds));
  const h = Math.floor(whole / 3600);
  const m = Math.floor((whole % 3600) / 60);
  const s = whole % 60;
  const pad = (n) => String(n).padStart(2, "0");
  return h > 0 ? `${h}:${pad(m)}:${pad(s)}` : `${m}:${pad(s)}`;
}

function bytes(count) {
  // Zero is a number a person reads differently from a blank: a torrent
  // sitting at "0 B/s" is stalled, and one showing nothing looks broken.
  if (!count) return "0 B";
  const units = ["B", "KB", "MB", "GB", "TB"];
  let value = count;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  return `${value < 10 && unit > 0 ? value.toFixed(1) : Math.round(value)} ${units[unit]}`;
}

function initials(name) {
  return String(name || "?")
    .split(/\s+/)
    .slice(0, 2)
    .map((word) => word.charAt(0))
    .join("")
    .toUpperCase();
}

// -------------------------------------------------------------------- views

function showView(name) {
  for (const section of document.querySelectorAll(".view")) {
    section.hidden = section.id !== `view-${name}`;
  }
  for (const tab of document.querySelectorAll(".tab")) {
    tab.setAttribute("aria-current", String(tab.dataset.view === name));
  }
  if (name !== "detail") LAST.view = name;
}

/** A poster, or the item's initials when there is no usable image.
 *
 * An addon's poster host can be down, slow, or simply wrong, and a broken
 * image icon in a grid of forty looks like this program is broken. The
 * fallback is attached before the image loads, which is the only order that
 * catches a URL that fails immediately. */
function posterFor(item) {
  const poster = el("div", { className: "poster" });
  const fallback = () => {
    if (!poster.querySelector(".initials")) {
      poster.appendChild(el("span", { className: "initials", text: initials(item.name) }));
    }
  };
  if (!item.poster) {
    fallback();
    return poster;
  }
  const image = el("img", { attrs: { src: item.poster, alt: "", loading: "lazy" } });
  image.addEventListener("error", () => {
    image.remove();
    fallback();
  });
  poster.appendChild(image);
  return poster;
}

/** One poster card. */
function card(item) {
  const poster = posterFor(item);
  if (item.episode_label) {
    poster.appendChild(el("span", { className: "badge", text: item.episode_label }));
  } else if (item.rating) {
    poster.appendChild(el("span", { className: "badge", text: item.rating }));
  }
  if (item.progress) {
    const bar = el("span", { className: "bar" });
    const fill = el("i");
    fill.style.width = `${Math.round(item.progress * 100)}%`;
    bar.appendChild(fill);
    poster.appendChild(bar);
  }

  const sub = item.resume_at
    ? t("ui.card.resume", item.resume_at)
    : item.year || item.content_type;

  return el(
    "button",
    {
      className: "card",
      attrs: { type: "button" },
      title: item.name,
      onClick: () => openDetail(item.content_type, item.id),
    },
    [poster, el("div", { className: "name", text: item.name }), el("div", { className: "sub", text: sub })]
  );
}

async function loadRows() {
  ROWS = await ask("rows");
  const container = document.getElementById("rows");
  clear(container);
  document.getElementById("home-empty").hidden = ROWS.length > 0;

  ROWS.forEach((row, index) => {
    const strip = el("div", { className: "strip" });
    const section = el("section", { className: "row" }, [
      el("div", { className: "row-head" }, [el("h3", { text: row.title })]),
      strip,
    ]);
    container.appendChild(section);

    if (row.kind === "catalog") {
      strip.appendChild(el("div", { className: "spinner", text: t("ui.loading") }));
      // One request per row, in order, so a slow addon delays its own row and
      // not the screen.
      fillRow(row, strip, index);
    } else {
      for (const item of row.items) strip.appendChild(card(item));
    }
  });
}

async function fillRow(row, strip, index) {
  try {
    const items = await invoke("row_items", {
      addonId: row.addon_id,
      contentType: row.content_type,
      catalogId: row.catalog_id,
      skip: 0,
      query: null,
    });
    clear(strip);
    if (items.length === 0) {
      strip.parentElement.remove();
      return;
    }
    for (const item of items) strip.appendChild(card(item));
    ROWS[index].items = items;
  } catch (error) {
    // A catalogue that will not answer is reported where it belongs -- in its
    // own row -- rather than as a message over the whole window.
    clear(strip);
    strip.appendChild(el("div", { className: "spinner", text: String(error) }));
  }
}

async function runSearch(query) {
  if (!query.trim()) return;
  showView("search");
  const grid = document.getElementById("search-grid");
  clear(grid);
  document.getElementById("search-heading").textContent = t("ui.search.heading", query);
  document.getElementById("search-correction").hidden = true;
  grid.appendChild(el("div", { className: "spinner", text: t("ui.loading") }));

  const result = await ask("search", { query });
  clear(grid);

  const correction = document.getElementById("search-correction");
  if (result.corrected_from) {
    correction.textContent = t("ui.search.corrected", result.corrected_from, result.query);
    correction.hidden = false;
    document.getElementById("search-heading").textContent = t("ui.search.heading", result.query);
  }
  if (result.items.length === 0) {
    grid.appendChild(el("div", { className: "spinner", text: t("ui.search.none") }));
  }
  for (const item of result.items) grid.appendChild(card(item));
  for (const failure of result.failures) toast(failure);
}

async function openLibrary() {
  showView("library");
  const grid = document.getElementById("library-grid");
  clear(grid);
  const rows = await ask("rows");
  const library = rows.find((row) => row.kind === "library");
  if (!library) {
    grid.appendChild(el("div", { className: "spinner", text: t("ui.library.empty") }));
    return;
  }
  for (const item of library.items) grid.appendChild(card(item));
}

// ------------------------------------------------------------------- detail

async function openDetail(contentType, id) {
  showView("detail");
  const container = document.getElementById("detail");
  clear(container);
  container.appendChild(el("div", { className: "spinner", text: t("ui.loading") }));

  const detail = await ask("detail", { contentType, id });
  DETAIL = detail;
  LAST.detail = { contentType, id };
  clear(container);

  const poster = posterFor(detail);

  const facts = el("div", { className: "facts" });
  for (const fact of [detail.year, detail.runtime, ...detail.genres.slice(0, 4)]) {
    if (fact) facts.appendChild(el("span", { text: fact }));
  }

  const actions = el("div", { className: "detail-actions" });
  const sourcesButton = el("button", {
    className: "primary",
    text: detail.resume_at ? t("ui.action.resume", detail.resume_at) : t("ui.action.sources"),
    onClick: () => loadSources(detail.content_type, detail.id, detail.name),
  });
  if (detail.episodes.length === 0) actions.appendChild(sourcesButton);

  const keep = el("button", {
    text: detail.in_library ? t("ui.action.unsave") : t("ui.action.save"),
    onClick: async () => {
      if (detail.in_library) {
        await ask("library_remove", { contentType: detail.content_type, id: detail.id });
        detail.in_library = false;
      } else {
        await ask("library_save", { contentType: detail.content_type, id: detail.id });
        detail.in_library = true;
      }
      keep.textContent = detail.in_library ? t("ui.action.unsave") : t("ui.action.save");
    },
  });
  actions.appendChild(keep);

  const head = el("div", { className: "detail-head" }, [
    poster,
    el("div", {}, [
      el("h2", { text: detail.name }),
      facts,
      el("p", { className: "overview", text: detail.description || "" }),
      actions,
    ]),
  ]);
  container.appendChild(head);

  if (detail.cast.length > 0) {
    container.appendChild(
      el("p", { className: "muted", text: t("ui.detail.cast", detail.cast.slice(0, 6).join(", ")) })
    );
  }
  for (const failure of detail.failures) toast(failure);

  if (detail.episodes.length > 0) {
    renderEpisodes(container, detail);
  }
  container.appendChild(el("div", { attrs: { id: "sources" } }));
}

function renderEpisodes(container, detail) {
  container.appendChild(el("h3", { text: t("ui.detail.episodes") }));
  const seasons = el("div", { className: "seasons" });
  const list = el("div");
  const chosen = detail.seasons.length > 0 ? detail.seasons[0] : null;

  const draw = (season) => {
    clear(list);
    for (const button of seasons.children) {
      button.setAttribute("aria-pressed", String(Number(button.dataset.season) === season));
    }
    const episodes = detail.episodes.filter((episode) =>
      season === null ? true : episode.season === season
    );
    for (const episode of episodes) {
      const number =
        episode.season !== null && episode.episode !== null
          ? `S${String(episode.season).padStart(2, "0")}E${String(episode.episode).padStart(2, "0")}`
          : "";
      const name = episode.title || number;
      const row = el(
        "button",
        {
          className: "episode",
          attrs: { type: "button" },
          onClick: () =>
            loadSources("series", episode.id, `${detail.name} · ${number || name}`),
        },
        [
          el("span", { className: "no", text: number }),
          el("span", { className: "grow", text: name }),
          el("span", {
            className: "muted",
            text: episode.finished
              ? t("ui.episode.watched")
              : episode.progress
                ? `${Math.round(episode.progress * 100)}%`
                : episode.released
                  ? episode.released.slice(0, 10)
                  : "",
          }),
        ]
      );
      list.appendChild(row);
    }
  };

  for (const season of detail.seasons) {
    seasons.appendChild(
      el("button", {
        text: t("ui.detail.season", season),
        attrs: { "data-season": season, "aria-pressed": "false", type: "button" },
        onClick: () => draw(season),
      })
    );
  }
  container.appendChild(seasons);
  container.appendChild(list);
  draw(chosen);
}

// ------------------------------------------------------------------ sources

async function loadSources(contentType, id, name) {
  const container = document.getElementById("sources");
  if (!container) return;
  clear(container);
  container.appendChild(el("h3", { text: t("ui.detail.sources") }));
  const waiting = el("div", { className: "spinner", text: t("ui.loading") });
  container.appendChild(waiting);
  container.scrollIntoView({ behavior: "smooth", block: "nearest" });

  const result = await ask("sources", { contentType, id, name });
  SOURCES = { all: result.sources, quality: null, subject: name };
  waiting.remove();

  for (const failure of result.failures) toast(failure);
  if (result.sources.length === 0) {
    container.appendChild(el("div", { className: "spinner", text: t("ui.detail.nosources") }));
    return;
  }

  const filter = el("div", { className: "quality-filter" });
  const list = el("div");
  const draw = () => {
    clear(list);
    const shown = SOURCES.all.filter(
      (source) => SOURCES.quality === null || source.quality === SOURCES.quality
    );
    for (const button of filter.children) {
      button.setAttribute(
        "aria-pressed",
        String((button.dataset.quality || null) === SOURCES.quality)
      );
    }
    for (const source of shown.slice(0, 200)) list.appendChild(sourceRow(source));
  };

  filter.appendChild(
    el("button", {
      text: t("ui.sources.all", result.sources.length),
      attrs: { type: "button", "aria-pressed": "true" },
      onClick: () => {
        SOURCES.quality = null;
        draw();
      },
    })
  );
  for (const [label, count] of result.qualities) {
    filter.appendChild(
      el("button", {
        text: `${label} · ${count}`,
        attrs: { type: "button", "data-quality": label, "aria-pressed": "false" },
        onClick: () => {
          SOURCES.quality = label;
          draw();
        },
      })
    );
  }

  container.appendChild(filter);
  container.appendChild(list);
  draw();
}

function sourceRow(source) {
  const tags = [];
  if (source.is_torrent) tags.push(t("ui.sources.torrent"));
  if (source.seeders !== null && source.seeders !== undefined) {
    tags.push(t("ui.sources.seeders", source.seeders));
  }
  if (source.size) tags.push(source.size);
  if (source.has_subtitles) tags.push(t("ui.sources.subs"));
  if (source.needs_headers) tags.push(t("ui.sources.headers"));

  return el(
    "button",
    {
      className: "source",
      attrs: { type: "button" },
      onClick: () => play(source.index),
    },
    [
      el("span", { className: "q", text: source.quality }),
      el("span", {}, [
        el("div", { className: "label", text: source.label }),
        el("div", { className: "sub muted", text: t("ui.sources.from", source.addon_name) }),
      ]),
      el("span", { className: "tags", text: tags.join(" · ") }),
    ]
  );
}

async function play(index) {
  await ask("play", { index, file: null });
  await poll();
}

// ------------------------------------------------------------------- addons

async function loadAddons() {
  showView("addons");
  const list = document.getElementById("addon-list");
  clear(list);
  const addons = await ask("addons");
  if (addons.length === 0) {
    list.appendChild(el("div", { className: "spinner", text: t("ui.addons.none") }));
    return;
  }
  for (const addon of addons) {
    const detail = [
      addon.version ? `v${addon.version}` : "",
      t("ui.addons.catalogs", addon.catalogs),
      addon.types.join(", "),
    ]
      .filter(Boolean)
      .join(" · ");

    list.appendChild(
      el("div", { className: "addon" }, [
        el("div", { className: "grow" }, [
          el("div", { text: addon.name }),
          el("div", { className: "sub muted", text: detail }),
          addon.needs_configuration
            ? el("div", { className: "sub muted", text: t("ui.addons.needsconfig") })
            : null,
        ]),
        el("button", {
          text: addon.enabled ? t("ui.action.disable") : t("ui.action.enable"),
          onClick: async () => {
            await ask("addon_enabled", { id: addon.id, enabled: !addon.enabled });
            loadAddons();
          },
        }),
        el("button", {
          text: "↑",
          attrs: { "aria-label": t("ui.action.up") },
          onClick: async () => {
            await ask("addon_move", { id: addon.id, by: -1 });
            loadAddons();
          },
        }),
        el("button", {
          text: "↓",
          attrs: { "aria-label": t("ui.action.down") },
          onClick: async () => {
            await ask("addon_move", { id: addon.id, by: 1 });
            loadAddons();
          },
        }),
        el("button", {
          text: t("ui.action.refresh"),
          onClick: async () => {
            await ask("addon_refresh", { id: addon.id });
            loadAddons();
          },
        }),
        el("button", {
          text: t("ui.action.remove"),
          onClick: async () => {
            await ask("addon_remove", { id: addon.id });
            loadAddons();
          },
        }),
      ])
    );
  }
}

// ----------------------------------------------------------------- settings

const SETTINGS_FIELDS = [
  { path: "ranking.minimumResolution", kind: "choice", options: ["none", "720", "1080", "1440", "2160"] },
  { path: "ranking.audioLanguages", kind: "text" },
  { path: "ranking.preferHigherResolution", kind: "boolean" },
  { path: "ranking.preferSourcesWithSubtitles", kind: "boolean" },
  { path: "playback.subtitleLanguages", kind: "text" },
  { path: "playback.volume", kind: "number" },
  { path: "torrent.downloadDirectory", kind: "text" },
  { path: "torrent.prebufferMegabytes", kind: "number" },
  { path: "torrent.downloadLimitKbps", kind: "number" },
  { path: "torrent.uploadLimitKbps", kind: "number" },
  { path: "torrent.peerLimit", kind: "number" },
  { path: "torrent.keepFiles", kind: "boolean" },
];

function at(document_, path) {
  return path.split(".").reduce((value, key) => (value == null ? undefined : value[key]), document_);
}

async function loadSettings() {
  showView("settings");
  const form = document.getElementById("settings-form");
  clear(form);
  const view = await ask("settings");

  for (const field of SETTINGS_FIELDS) {
    const current = at(view.settings, field.path);
    let input;
    if (field.kind === "boolean") {
      input = el("select");
      for (const value of ["true", "false"]) {
        input.appendChild(el("option", { text: t(`ui.value.${value}`), attrs: { value } }));
      }
      input.value = String(Boolean(current));
    } else if (field.kind === "choice") {
      input = el("select");
      for (const value of field.options) {
        input.appendChild(el("option", { text: value, attrs: { value } }));
      }
      input.value = current === null || current === undefined ? "none" : resolutionName(current);
    } else {
      input = el("input", {
        attrs: { type: field.kind === "number" ? "number" : "text" },
      });
      input.value = Array.isArray(current) ? current.join(", ") : current === null || current === undefined ? "" : String(current);
    }
    input.addEventListener("change", async () => {
      await ask("settings_set", { path: field.path, value: String(input.value) });
      toast(t("ui.settings.saved"));
    });

    form.appendChild(
      el("div", { className: "setting" }, [
        el("label", { text: t(`ui.settings.${field.path}`) }),
        input,
      ])
    );
  }

  const audit = el("div", { className: "note" }, [
    el("div", {
      text: view.audit_clean ? t("ui.settings.audit.clean") : t("ui.settings.audit.dirty"),
    }),
  ]);
  form.appendChild(audit);

  const about = await ask("about");
  const node = document.getElementById("about");
  clear(node);
  node.appendChild(el("div", { className: "muted", text: t("ui.about.version", about.version) }));
  node.appendChild(
    el("div", {
      className: "note",
      text:
        about.trusted_keys === 0
          ? t("ui.about.trust.empty")
          : t("ui.about.trust.count", about.trusted_keys),
    })
  );
  node.appendChild(el("div", { className: "note", text: t("ui.about.nothing") }));
  node.appendChild(el("div", { className: "note", text: t("ui.about.mpv") }));
  node.appendChild(
    el("div", { className: "muted", text: t("ui.about.files", about.files.join("  ")) })
  );
}

function resolutionName(value) {
  return { Hd: "720", FullHd: "1080", QuadHd: "1440", UltraHd: "2160", Low: "none" }[value] || "none";
}

// ------------------------------------------------------------------ polling

async function poll() {
  let status;
  try {
    status = await invoke("status");
  } catch (error) {
    return;
  }

  document.getElementById("version").textContent = status.version;

  const job = document.getElementById("job");
  if (status.job.active) {
    job.hidden = false;
    document.getElementById("job-text").textContent = t(status.job.stage);
    document.getElementById("job-detail").textContent = status.job.detail;
  } else {
    job.hidden = true;
    if (status.job.error) {
      toast(status.job.error);
      status.job.error = null;
    }
  }

  const bar = document.getElementById("playbar");
  bar.hidden = !status.player.running && status.torrents.length === 0;
  if (status.player.running) {
    document.getElementById("playing-name").textContent = status.player.title;
    document.getElementById("playing-clock").textContent = status.player.duration
      ? `${clock(status.player.position)} / ${clock(status.player.duration)}`
      : clock(status.player.position);
    const fraction =
      status.player.duration && status.player.position
        ? Math.min(1, status.player.position / status.player.duration)
        : 0;
    document.getElementById("playing-bar").style.width = `${Math.round(fraction * 100)}%`;
  } else {
    document.getElementById("playing-name").textContent = "";
    document.getElementById("playing-clock").textContent = "";
    document.getElementById("playing-bar").style.width = "0%";
  }

  const torrents = document.getElementById("torrents");
  clear(torrents);
  for (const torrent of status.torrents) {
    torrents.appendChild(
      el("div", {
        text: t(
          "ui.torrent.line",
          torrent.name,
          `${torrent.percent}%`,
          bytes(torrent.down_bps),
          torrent.peers,
          torrent.eta_seconds ? clock(torrent.eta_seconds) : "—"
        ),
      })
    );
  }
}

// --------------------------------------------------------------------- boot

async function boot() {
  const strings = await invoke("strings");
  MESSAGES = strings.messages;
  applyStrings();

  const language = document.getElementById("language");
  clear(language);
  for (const locale of strings.locales) {
    language.appendChild(el("option", { text: t(`ui.language.${locale}`), attrs: { value: locale } }));
  }
  language.value = strings.locale;
  language.addEventListener("change", async () => {
    const next = await ask("language_set", { code: language.value });
    MESSAGES = next.messages;
    applyStrings();
    loadRows();
  });

  for (const tab of document.querySelectorAll(".tab")) {
    tab.addEventListener("click", () => {
      const view = tab.dataset.view;
      if (view === "addons") loadAddons();
      else if (view === "settings") loadSettings();
      else if (view === "library") openLibrary();
      else if (view === "search") showView("search");
      else showView("home");
    });
  }

  document.querySelector(".back").addEventListener("click", () => showView(LAST.view));

  document.getElementById("search-form").addEventListener("submit", (event) => {
    event.preventDefault();
    runSearch(document.getElementById("search-input").value);
  });

  document.getElementById("link-form").addEventListener("submit", async (event) => {
    event.preventDefault();
    const input = document.getElementById("link-input");
    const link = input.value;
    input.value = "";
    await ask("play_link", { link });
    poll();
  });

  document.getElementById("addon-form").addEventListener("submit", async (event) => {
    event.preventDefault();
    const input = document.getElementById("addon-input");
    const address = input.value;
    await ask("addon_add", { address });
    input.value = "";
    await loadAddons();
    await loadRows();
  });

  document.getElementById("empty-add").addEventListener("click", () => loadAddons());

  for (const button of document.querySelectorAll("[data-player]")) {
    button.addEventListener("click", async () => {
      await ask("player", {
        action: button.dataset.player,
        value: button.dataset.value ? Number(button.dataset.value) : null,
      });
      poll();
    });
  }

  showView("home");
  await loadRows();
  await poll();
  setInterval(poll, 1000);
}

boot();
