// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 EON contributors
//
// Additional permission under GNU GPL version 3 section 7:
// see LICENSE-EXCEPTION.md (EON Module ABI Exception 1.0).
//
// The catalogue.
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
//
// A fourth, newer one: nothing here assumes where the page was loaded from.
// Every path is relative and the only way out is `invoke`, which is what keeps
// the interface relocatable (docs/architecture.md).

const invoke = window.__TAURI__.core.invoke;

/** The message catalogue for the chosen language. */
let MESSAGES = {};

/** Rows on the home screen, with what each has loaded so far. */
let ROWS = [];

/** Which slice of the home screen is showing: all, series, or movie. */
let FILTER = "home";

/** Where the back button goes. */
let LAST = "home";

/** The detail currently open. */
let DETAIL = null;

/** What the source list holds, and how it is narrowed. */
let SOURCES = { all: [], quality: null, audio: null, subs: null, subject: null };

/** What is being started, for the loading screen. */
let STARTING = null;

// --------------------------------------------------------------------- text

function t(key, ...args) {
  const text = MESSAGES[key];
  if (text === undefined) return key;
  return text.replace(/\{(\d+)\}/g, (whole, index) => {
    const value = args[Number(index)];
    return value === undefined ? whole : String(value);
  });
}

/** A content type as the navigation names it. An addon's own type that the
 *  catalogue has no word for is shown as the addon wrote it. */
function typeName(type) {
  if (type === "movie") return t("ui.nav.movies");
  if (type === "series") return t("ui.nav.series");
  return type || "";
}

/** A language code as a person reads it, in the interface's language. */
function languageName(code) {
  const key = `ui.lang.${code}`;
  return MESSAGES[key] === undefined ? String(code).toUpperCase() : MESSAGES[key];
}

function applyStrings() {
  for (const node of document.querySelectorAll("[data-say]")) {
    // The icon is drawn into the same button, so the text goes in a span of
    // its own rather than replacing everything.
    let span = node.querySelector(":scope > .say");
    if (!span) {
      span = el("span", { className: "say" });
      node.appendChild(span);
    }
    span.textContent = t(node.dataset.say);
  }
  for (const node of document.querySelectorAll("[data-label]")) {
    node.setAttribute("aria-label", t(node.dataset.label));
    node.title = t(node.dataset.label);
  }
  document.getElementById("search-input").placeholder = t("ui.search.placeholder");
  document.getElementById("link-input").placeholder = t("ui.link.placeholder");
  document.getElementById("addon-input").placeholder = t("ui.addons.placeholder");
  document.documentElement.lang = MESSAGES.__locale || "en";
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

/** A button with an icon and, optionally, a visible label. */
function iconButton(name, label, onClick, className = "icon-button", visible = null) {
  const button = el("button", {
    className,
    attrs: { type: "button", "aria-label": label, title: label },
    onClick,
  });
  button.appendChild(icon(name));
  if (visible) button.appendChild(el("span", { text: visible }));
  return button;
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
  }, 7000);
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

/** Fold a string for matching: case and diacritics both, so `turk` finds
 *  `Türkçe` and `ingiliz` finds `İngilizce`. */
function fold(text) {
  return String(text)
    .toLocaleLowerCase("tr")
    .normalize("NFD")
    .replace(/[̀-ͯ]/g, "")
    .replace(/ı/g, "i");
}

/** An image that removes itself when it fails, calling `fallback`. The handler
 *  is attached before the source is set: the only order that catches a URL
 *  that fails immediately. */
function image(src, className, fallback) {
  const img = el("img", { className, attrs: { alt: "", loading: "lazy", draggable: "false" } });
  img.addEventListener("error", () => {
    img.remove();
    if (fallback) fallback();
  });
  img.src = src;
  return img;
}

// -------------------------------------------------------------------- views

function showView(name) {
  for (const section of document.querySelectorAll(".view")) {
    section.hidden = section.id !== `view-${name}`;
  }
  const current = name === "home" ? FILTER : name;
  for (const link of document.querySelectorAll(".link")) {
    link.setAttribute("aria-current", String(link.dataset.go === current));
  }
  if (name !== "detail") LAST = name === "home" ? FILTER : name;
  window.scrollTo({ top: 0 });
  solidNav();
}

function go(target) {
  if (target === "home" || target === "series" || target === "movie") {
    FILTER = target;
    showView("home");
    renderHome();
  } else if (target === "library") openLibrary();
  else if (target === "addons") loadAddons();
  else if (target === "settings") loadSettings();
  else if (target === "search") showView("search");
}

function solidNav() {
  document.getElementById("nav").classList.toggle("solid", window.scrollY > 40);
}

/** A poster, or the item's initials when there is no usable image.
 *
 * An addon's poster host can be down, slow, or simply wrong, and a broken
 * image icon in a row of forty looks like this program is broken. */
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
  poster.appendChild(image(item.poster, "", fallback));
  return poster;
}

/** One poster card. */
function card(item) {
  const poster = posterFor(item);
  if (item.episode_label) {
    poster.appendChild(el("span", { className: "badge", text: item.episode_label }));
  } else if (item.rating) {
    poster.appendChild(el("span", { className: "badge", text: `★ ${item.rating}` }));
  }
  if (item.progress) {
    const bar = el("span", { className: "bar" });
    const fill = el("i");
    fill.style.width = `${Math.round(item.progress * 100)}%`;
    bar.appendChild(fill);
    poster.appendChild(bar);
  }

  const sub = item.resume_at ? t("ui.card.resume", item.resume_at) : item.year || "";

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

// --------------------------------------------------------------------- home

async function loadRows() {
  const rows = await ask("rows");
  ROWS = rows.map((row) => ({
    ...row,
    // A catalogue row fetches itself; the others arrive filled.
    loaded: row.kind !== "catalog",
    exhausted: row.kind !== "catalog",
    loading: null,
  }));
  renderHome();
}

function rowsForFilter() {
  if (FILTER === "home") return ROWS;
  return ROWS.filter((row) =>
    row.kind === "catalog"
      ? row.content_type === FILTER
      : row.items.some((item) => item.content_type === FILTER)
  );
}

async function renderHome() {
  const container = document.getElementById("rows");
  clear(container);
  const rows = rowsForFilter();
  document.getElementById("home-empty").hidden = ROWS.length > 0;
  if (ROWS.length === 0) {
    document.getElementById("hero").hidden = true;
    return;
  }
  for (const row of rows) container.appendChild(slider(row));
  // The billboard is chosen once the first catalogue has answered, from what
  // it offered -- never from a list of our own, which would be a
  // recommendation, and this program does not make those.
  const first = rows.find((row) => row.kind === "catalog") || rows[0];
  if (first) {
    await ensureRow(first);
    renderHero(first);
  } else {
    document.getElementById("hero").hidden = true;
  }
}

/** Fetch a row's next page, once. */
function ensureRow(row) {
  if (row.loaded || row.kind !== "catalog") return Promise.resolve();
  return loadMore(row);
}

function loadMore(row) {
  if (row.exhausted) return Promise.resolve();
  if (row.loading) return row.loading;
  const skip = row.items.length;
  row.loading = invoke("row_items", {
    addonId: row.addon_id,
    contentType: row.content_type,
    catalogId: row.catalog_id,
    skip,
    query: null,
  })
    .then((items) => {
      // Catalogues page by a fixed size the addon picks; an empty or repeated
      // page is the end.
      const known = new Set(row.items.map((item) => item.id));
      const fresh = items.filter((item) => !known.has(item.id));
      row.items.push(...fresh);
      row.exhausted = fresh.length === 0;
      row.loaded = true;
      row.error = null;
    })
    .catch((error) => {
      row.loaded = true;
      row.exhausted = true;
      row.error = String(error);
    })
    .finally(() => {
      row.loading = null;
    });
  return row.loading;
}

// ------------------------------------------------------------------- slider

/** A row of cards that pages sideways.
 *
 * Three ways to move it, because people arrive with different hands: the
 * arrow bubbles at either end turn one page; a mouse can grab the row and
 * drag it; a trackpad or wheel scrolls it natively. Pages are whole screens
 * of cards, and the dashes above the row say which one is showing. When the
 * last page comes into view the next page of the catalogue is asked for, so a
 * row is as long as the addon's catalogue rather than as long as its first
 * answer. */
function slider(row) {
  const track = el("div", { className: "track", attrs: { role: "list" } });
  const pages = el("div", { className: "pages", attrs: { "aria-hidden": "true" } });
  const prev = el("button", {
    className: "arrow prev",
    attrs: { type: "button", "aria-label": t("ui.row.prev"), title: t("ui.row.prev") },
  });
  const next = el("button", {
    className: "arrow next",
    attrs: { type: "button", "aria-label": t("ui.row.next"), title: t("ui.row.next") },
  });
  prev.appendChild(el("span", {}, [icon("chevronLeft")]));
  next.appendChild(el("span", {}, [icon("chevronRight")]));

  const kind = [typeName(row.content_type), row.addon_name].filter(Boolean).join(", ");
  const section = el("section", { className: "row", attrs: { "aria-label": row.title } }, [
    el("div", { className: "row-head" }, [
      el("h3", { text: row.title }),
      kind ? el("span", { className: "row-kind", text: kind }) : null,
      pages,
    ]),
    el("div", { className: "slider" }, [prev, track, next]),
  ]);

  const fill = () => {
    clear(track);
    if (row.error) {
      track.appendChild(el("div", { className: "spinner", text: row.error }));
    } else if (!row.loaded) {
      // Poster-shaped placeholders, so the row has its final height before
      // the catalogue answers and nothing below it jumps when it does.
      for (let i = 0; i < 8; i += 1) {
        track.appendChild(el("div", { className: "card placeholder", attrs: { "aria-hidden": "true" } }, [el("div", { className: "poster" })]));
      }
      track.setAttribute("aria-busy", "true");
    } else if (row.items.length === 0) {
      section.remove();
      return;
    }
    if (row.loaded) track.removeAttribute("aria-busy");
    for (const item of row.items) track.appendChild(card(item));
    update();
  };

  const page = () => Math.max(1, track.clientWidth - 40);

  const update = () => {
    const max = track.scrollWidth - track.clientWidth;
    prev.disabled = track.scrollLeft <= 4;
    next.disabled = track.scrollLeft >= max - 4 && row.exhausted;
    const count = Math.max(1, Math.ceil(track.scrollWidth / page() - 0.05));
    const at = Math.min(count - 1, Math.round(track.scrollLeft / page()));
    if (pages.childElementCount !== count) {
      clear(pages);
      for (let i = 0; i < count; i += 1) pages.appendChild(el("i"));
    }
    [...pages.children].forEach((dash, index) => dash.classList.toggle("on", index === at));
    track.setAttribute("aria-label", t("ui.row.page", at + 1, count));
    // Near the end: ask for more before the viewer gets there.
    if (!row.exhausted && track.scrollLeft + track.clientWidth >= track.scrollWidth - page()) {
      const before = row.items.length;
      loadMore(row).then(() => {
        if (row.items.length !== before) {
          for (const item of row.items.slice(before)) track.appendChild(card(item));
        }
        update();
      });
    }
  };

  prev.addEventListener("click", () => track.scrollBy({ left: -page() }));
  next.addEventListener("click", () => track.scrollBy({ left: page() }));
  track.addEventListener("scroll", () => {
    cancelAnimationFrame(update.frame);
    update.frame = requestAnimationFrame(update);
  });
  // A vertical wheel over a row scrolls the page, as it should; a sideways one
  // (trackpad, tilt wheel) scrolls the row, natively.
  dragToScroll(track, () => update());
  new ResizeObserver(() => update()).observe(track);

  if (row.loaded) {
    fill();
  } else {
    fill();
    ensureRow(row).then(fill);
  }
  return section;
}

/** Let a mouse grab a row and throw it. Touch and pens already scroll it
 *  natively; this is for the hand on a mouse, which otherwise has only the
 *  arrows. A drag that moved more than a few pixels does not count as a click
 *  on the card it started on. */
function dragToScroll(track, onEnd) {
  let start = null;
  track.addEventListener("pointerdown", (event) => {
    if (event.pointerType !== "mouse" || event.button !== 0) return;
    start = { x: event.clientX, left: track.scrollLeft, moved: false, id: event.pointerId };
  });
  track.addEventListener("pointermove", (event) => {
    if (!start || event.pointerId !== start.id) return;
    const dx = event.clientX - start.x;
    if (!start.moved && Math.abs(dx) > 6) {
      start.moved = true;
      track.classList.add("dragging");
      track.setPointerCapture(event.pointerId);
    }
    if (start.moved) track.scrollLeft = start.left - dx;
  });
  const end = (event) => {
    if (!start || event.pointerId !== start.id) return;
    const moved = start.moved;
    start = null;
    if (!moved) return;
    track.classList.remove("dragging");
    // Settle on a card edge, the way the arrows leave it.
    track.style.scrollSnapType = "";
    const swallow = (click) => {
      click.stopPropagation();
      click.preventDefault();
    };
    track.addEventListener("click", swallow, { capture: true, once: true });
    setTimeout(() => track.removeEventListener("click", swallow, { capture: true }), 0);
    onEnd();
  };
  track.addEventListener("pointerup", end);
  track.addEventListener("pointercancel", end);
}

// --------------------------------------------------------------------- hero

let HERO_TOKEN = 0;

async function renderHero(row) {
  const hero = document.getElementById("hero");
  const item = row.items.find((candidate) => candidate.poster) || row.items[0];
  if (!item) {
    hero.hidden = true;
    return;
  }
  const token = (HERO_TOKEN += 1);
  hero.hidden = false;
  document.getElementById("hero-title").textContent = item.name;
  document.getElementById("hero-text").textContent = item.description || "";
  const facts = document.getElementById("hero-facts");
  clear(facts);
  if (item.rating) facts.appendChild(el("span", { className: "score", text: `★ ${item.rating}` }));
  if (item.year) facts.appendChild(el("span", { text: item.year }));

  const art = document.getElementById("hero-art");
  clear(art);
  art.className = "hero-art";
  document.getElementById("hero-play").onclick = () => playBest(item.content_type, item.id, item.name, item.poster);
  document.getElementById("hero-info").onclick = () => openDetail(item.content_type, item.id);

  // The backdrop is not in a catalogue answer, so it is asked for -- through
  // `feature`, which looks the item up without making it the one the viewer
  // is looking at.
  let detail = null;
  try {
    detail = await invoke("feature", { contentType: item.content_type, id: item.id });
  } catch (error) {
    detail = null;
  }
  if (token !== HERO_TOKEN) return;
  if (detail) {
    if (detail.description) document.getElementById("hero-text").textContent = detail.description;
    clear(facts);
    if (item.rating) facts.appendChild(el("span", { className: "score", text: `★ ${item.rating}` }));
    for (const fact of [detail.year, detail.runtime, ...detail.genres.slice(0, 3)]) {
      if (fact) facts.appendChild(el("span", { text: fact }));
    }
  }
  drawArt(art, detail ? detail.background : null, item.poster);
}

/** A backdrop when there is one; otherwise the poster, washed out wide and
 *  kept sharp at the side. */
function drawArt(art, background, poster) {
  clear(art);
  art.className = "hero-art";
  const posterOnly = () => {
    if (!poster) return;
    art.classList.add("poster-only");
    art.appendChild(image(poster, "wash"));
    art.appendChild(image(poster, "sharp"));
  };
  if (background) art.appendChild(image(background, "", posterOnly));
  else posterOnly();
}

// ------------------------------------------------------------------- search

async function runSearch(query) {
  if (!query.trim()) return;
  showView("search");
  const grid = document.getElementById("search-grid");
  clear(grid);
  document.getElementById("search-heading").textContent = t("ui.search.heading", query);
  document.getElementById("search-correction").hidden = true;
  grid.appendChild(el("div", { className: "spinner", text: t("ui.loading") }));

  let result;
  try {
    result = await ask("search", { query });
  } catch (error) {
    clear(grid);
    return;
  }
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
  if (result.failures.length) toast(result.failures.join("\n"));
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
  container.appendChild(el("div", { className: "page spinner", text: t("ui.loading") }));

  let detail;
  try {
    detail = await ask("detail", { contentType, id });
  } catch (error) {
    clear(container);
    container.appendChild(backButton());
    return;
  }
  DETAIL = detail;
  clear(container);
  if (detail.failures.length) toast(detail.failures.join("\n"));

  // The billboard of the page.
  const art = el("div", { className: "hero-art" });
  drawArt(art, detail.background, detail.poster);

  const facts = el("div", { className: "facts" });
  for (const fact of [detail.year, detail.runtime]) {
    if (fact) facts.appendChild(el("span", { text: fact }));
  }
  for (const genre of detail.genres.slice(0, 4)) facts.appendChild(el("span", { className: "pill", text: genre }));

  const actions = el("div", { className: "detail-actions" });
  const playLabel = detail.resume_at ? t("ui.action.resume", detail.resume_at) : t("ui.action.play");
  const play = el("button", {
    className: "button play",
    attrs: { type: "button" },
    onClick: () => playFromDetail(),
  });
  play.appendChild(icon("play"));
  play.appendChild(el("span", { text: playLabel }));
  actions.appendChild(play);

  const keep = el("button", { className: "round", attrs: { type: "button" } });
  const drawKeep = () => {
    clear(keep);
    keep.appendChild(icon(detail.in_library ? "check" : "plus"));
    const label = detail.in_library ? t("ui.action.unsave") : t("ui.action.save");
    keep.setAttribute("aria-label", label);
    keep.title = label;
  };
  keep.addEventListener("click", async () => {
    if (detail.in_library) {
      await ask("library_remove", { contentType: detail.content_type, id: detail.id });
      detail.in_library = false;
    } else {
      await ask("library_save", { contentType: detail.content_type, id: detail.id });
      detail.in_library = true;
    }
    drawKeep();
  });
  drawKeep();
  actions.appendChild(keep);

  const copy = el("div", { className: "detail-copy" }, [
    el("h1", { text: detail.name, title: detail.name }),
    facts,
    detail.description ? el("p", { className: "overview", text: detail.description }) : null,
    actions,
  ]);
  if (detail.cast.length || detail.director.length) {
    const credits = el("div", { className: "credits" });
    if (detail.cast.length) {
      credits.appendChild(el("div", { text: t("ui.detail.cast", detail.cast.slice(0, 5).join(", ")) }));
    }
    if (detail.director.length) credits.appendChild(el("div", { text: detail.director.slice(0, 3).join(", ") }));
    copy.appendChild(credits);
  }

  container.appendChild(
    el("section", { className: "detail-hero" }, [
      art,
      el("div", { className: "hero-shade" }),
      backButton(),
      el("div", { className: "detail-body" }, [posterFor(detail), copy]),
    ])
  );

  const sections = el("div", { className: "detail-sections" });
  container.appendChild(sections);
  if (detail.episodes.length > 0) renderEpisodes(sections, detail);
  sections.appendChild(el("section", { attrs: { id: "sources" } }));

  // A film's sources are what its page is for, so they are listed straight
  // away. A series waits for an episode to be chosen.
  if (detail.episodes.length === 0) loadSources(detail.content_type, detail.id, detail.name);
}

function backButton() {
  const button = iconButton("arrowLeft", t("ui.action.back"), () => go(LAST), "icon-button back-button");
  return button;
}

/** The Play button on a detail page: the first source the list shows, which
 *  is the best one the viewer's preferences and filters allow. */
async function playFromDetail() {
  if (!DETAIL) return;
  if (DETAIL.episodes.length > 0) {
    const next =
      DETAIL.episodes.find((episode) => episode.progress && !episode.finished) ||
      DETAIL.episodes.find((episode) => !episode.finished && (episode.season || 0) > 0) ||
      DETAIL.episodes[0];
    const label = episodeLabel(next);
    await loadSources("series", next.id, `${DETAIL.name} · ${label}`);
  } else if (SOURCES.subject !== DETAIL.id) {
    await loadSources(DETAIL.content_type, DETAIL.id, DETAIL.name);
  }
  const shown = filteredSources();
  if (shown.length === 0) {
    toast(t("ui.detail.nosources"));
    return;
  }
  play(shown[0].index, SOURCES.name, DETAIL.background || DETAIL.poster);
}

/** Play from the billboard: a film starts on its best source; a series opens,
 *  because which episode is a question only the viewer can answer. */
async function playBest(contentType, id, name, art) {
  if (contentType !== "movie") {
    openDetail(contentType, id);
    return;
  }
  showLoading(name, art, t("ui.detail.finding"));
  let result;
  try {
    result = await ask("sources", { contentType, id, name });
  } catch (error) {
    hideLoading();
    return;
  }
  if (result.sources.length === 0) {
    hideLoading();
    toast(t("ui.detail.nosources"));
    return;
  }
  SOURCES = { all: result.sources, quality: null, audio: null, subs: null, subject: id, name };
  play(result.sources[0].index, name, art);
}

function episodeLabel(episode) {
  return episode.season !== null && episode.episode !== null
    ? `S${String(episode.season).padStart(2, "0")}E${String(episode.episode).padStart(2, "0")}`
    : episode.title;
}

function renderEpisodes(container, detail) {
  const list = el("div");
  const select = el("select", { className: "season-select", attrs: { "aria-label": t("ui.detail.season", "") } });
  for (const season of detail.seasons) {
    select.appendChild(el("option", { text: t("ui.detail.season", season), attrs: { value: season } }));
  }
  // Season 0 is where addons put specials; it is not where anyone starts.
  const firstReal = detail.seasons.find((season) => season > 0);
  const chosen = firstReal !== undefined ? firstReal : detail.seasons[0];

  const draw = (season) => {
    clear(list);
    const episodes = detail.episodes.filter((episode) =>
      season === undefined || season === null ? true : episode.season === season
    );
    for (const episode of episodes) {
      const label = episodeLabel(episode);
      const thumb = el("div", { className: "thumb" });
      if (episode.thumbnail) thumb.appendChild(image(episode.thumbnail, ""));
      const row = el(
        "button",
        {
          className: "episode",
          attrs: { type: "button" },
          onClick: () => {
            for (const other of list.children) other.removeAttribute("aria-current");
            row.setAttribute("aria-current", "true");
            loadSources("series", episode.id, `${detail.name} · ${label}`);
          },
        },
        [
          el("span", { className: "no", text: episode.episode !== null ? episode.episode : "" }),
          thumb,
          el("span", { className: "grow" }, [
            el("div", { className: "title", text: episode.title || label, title: episode.title }),
            el("div", { className: "when", text: label }),
          ]),
          el("span", {
            className: "when",
            text: episode.finished
              ? t("ui.episode.watched")
              : episode.progress
                ? t("ui.unit.percent", Math.round(episode.progress * 100))
                : episode.released
                  ? episode.released.slice(0, 10)
                  : "",
          }),
        ]
      );
      list.appendChild(row);
    }
  };

  select.addEventListener("change", () => draw(Number(select.value)));
  if (chosen !== undefined) select.value = String(chosen);

  container.appendChild(
    el("section", {}, [
      el("div", { className: "section-head" }, [
        el("h2", { className: "section-title", text: t("ui.detail.episodes") }),
        detail.seasons.length > 0 ? select : null,
      ]),
      list,
    ])
  );
  draw(chosen);
}

// ------------------------------------------------------------------ sources

async function loadSources(contentType, id, name) {
  const container = document.getElementById("sources");
  if (!container) return;
  clear(container);
  container.appendChild(el("h2", { className: "section-title", text: t("ui.detail.sources") }));
  const waiting = el("div", { className: "spinner", text: t("ui.loading") });
  container.appendChild(waiting);
  if (contentType === "series") container.scrollIntoView({ behavior: "smooth", block: "start" });

  let result;
  try {
    result = await ask("sources", { contentType, id, name });
  } catch (error) {
    waiting.remove();
    return;
  }
  SOURCES = { all: result.sources, quality: null, audio: null, subs: null, subject: id, name };
  waiting.remove();

  if (result.failures.length) toast(result.failures.join("\n"));
  if (result.sources.length === 0) {
    container.appendChild(el("div", { className: "spinner", text: t("ui.detail.nosources") }));
    return;
  }

  const filters = el("div", { className: "filters" });
  const count = el("div", { className: "count-line" });
  const list = el("div");
  container.appendChild(filters);
  container.appendChild(count);
  container.appendChild(list);

  // Three ways to narrow the list: quality, the language it is dubbed in, and
  // the language of the subtitles that come with it. Each offers only what
  // the list actually holds, with how many of each, so no filter can be set
  // to something that empties the list on its own.
  // The host files sources that state no quality under "—"; a person reads
  // that as nothing, so the chip says what it means.
  const qualities = result.qualities.map(([label, n]) => [label, label === "—" ? t("ui.sources.unknown") : label, n]);
  const audio = tally(SOURCES.all, (source) => source.languages);
  const subs = tally(SOURCES.all, (source) =>
    source.subtitle_languages.length ? source.subtitle_languages : source.has_subtitles ? ["?"] : []
  );

  const draw = () => {
    clear(list);
    const shown = filteredSources();
    count.textContent = t("ui.sources.shown", shown.length, SOURCES.all.length);
    if (shown.length === 0) {
      list.appendChild(el("div", { className: "spinner", text: t("ui.sources.nomatch") }));
    }
    for (const source of shown.slice(0, 200)) list.appendChild(sourceRow(source));
  };

  const group = (name, key, options) => {
    if (options.length === 0) return;
    const chips = el("div", { className: "chips" });
    const all = [[null, t("ui.sources.any"), SOURCES.all.length], ...options];
    for (const [value, label, n] of all) {
      const chip = el("button", {
        className: "chip",
        attrs: { type: "button", "aria-pressed": String(SOURCES[key] === value) },
        onClick: () => {
          SOURCES[key] = value;
          for (const other of chips.children) other.setAttribute("aria-pressed", "false");
          chip.setAttribute("aria-pressed", "true");
          draw();
        },
      });
      chip.appendChild(document.createTextNode(label));
      chip.appendChild(el("small", { text: n }));
      chips.appendChild(chip);
    }
    filters.appendChild(el("div", { className: "filter" }, [el("span", { className: "filter-name", text: name }), chips]));
  };

  group(t("ui.sources.quality"), "quality", qualities);
  group(
    t("ui.sources.audio"),
    "audio",
    audio.map(([code, n]) => [code, code === "" ? t("ui.sources.unknown") : languageName(code), n])
  );
  group(
    t("ui.sources.subtitles"),
    "subs",
    subs.map(([code, n]) => [
      code,
      code === "?" ? t("ui.sources.unknown") : code === "" ? t("ui.sources.nosubs") : languageName(code),
      n,
    ])
  );
  draw();
}

/** How many sources carry each value, most common first. Sources that state
 *  nothing are counted under "", so "not stated" is a choice too. */
function tally(sources, valuesOf) {
  const counts = new Map();
  for (const source of sources) {
    const values = valuesOf(source);
    for (const value of values.length ? values : [""]) counts.set(value, (counts.get(value) || 0) + 1);
  }
  return [...counts.entries()]
    .filter(([value]) => value !== "" || counts.size > 1)
    .sort((a, b) => (a[0] === "" ? 1 : b[0] === "" ? -1 : b[1] - a[1]));
}

function filteredSources() {
  return SOURCES.all.filter((source) => {
    if (SOURCES.quality !== null && source.quality !== SOURCES.quality) return false;
    if (SOURCES.audio !== null) {
      if (SOURCES.audio === "" ? source.languages.length > 0 : !source.languages.includes(SOURCES.audio)) {
        return false;
      }
    }
    if (SOURCES.subs !== null) {
      if (SOURCES.subs === "?") {
        if (!(source.has_subtitles && source.subtitle_languages.length === 0)) return false;
      } else if (SOURCES.subs === "") {
        if (source.has_subtitles) return false;
      } else if (!source.subtitle_languages.includes(SOURCES.subs)) {
        return false;
      }
    }
    return true;
  });
}

function sourceRow(source) {
  const tags = el("span", { className: "tags" });
  const tag = (text, kind = "") => tags.appendChild(el("span", { className: `tag ${kind}`, text }));
  if (source.languages.length) {
    tag(t("ui.sources.dub", source.languages.map(languageName).join(", ")), "lang");
  }
  if (source.subtitle_languages.length) {
    tag(t("ui.sources.subs.langs", source.subtitle_languages.map(languageName).join(", ")), "subs");
  } else if (source.has_subtitles) {
    tag(t("ui.sources.subs"), "subs");
  }
  if (source.seeders !== null && source.seeders !== undefined) tag(t("ui.sources.seeders", source.seeders));
  if (source.size) tag(source.size);
  if (source.is_torrent) tag(t("ui.sources.torrent"));
  if (source.needs_headers) tag(t("ui.sources.headers"));

  return el(
    "button",
    {
      className: "source",
      attrs: { type: "button" },
      onClick: () => play(source.index, SOURCES.name, DETAIL ? DETAIL.background || DETAIL.poster : null),
    },
    [
      el("span", { className: "q", text: source.quality }),
      el("span", {}, [
        el("div", { className: "label", text: source.label, title: source.label }),
        el("div", { className: "from", text: t("ui.sources.from", source.addon_name) }),
      ]),
      tags,
    ]
  );
}

// --------------------------------------------------------------- playing it

async function play(index, name, art) {
  showLoading(name, art, t("ui.job.starting"));
  try {
    await ask("play", { index, file: null });
  } catch (error) {
    hideLoading();
    return;
  }
  poll();
}

/** The black screen between pressing play and the first frame. The player
 *  itself is another webview laid over mpv's picture; this page never draws
 *  a control for it. */
function showLoading(name, art, stage) {
  STARTING = { name: name || "", since: Date.now() };
  document.getElementById("loading-title").textContent = name || "";
  document.getElementById("loading-stage").textContent = stage || "";
  document.getElementById("loading-detail").textContent = "";
  document.getElementById("loading-torrent").textContent = "";
  document.getElementById("loading-meter").style.width = "0%";
  const holder = document.getElementById("loading-art");
  clear(holder);
  if (art) holder.appendChild(image(art, ""));
  document.getElementById("loading").hidden = false;
}

function hideLoading() {
  STARTING = null;
  document.getElementById("loading").hidden = true;
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
  const act = (command, args) => async () => {
    await ask(command, args);
    loadAddons();
  };
  for (const addon of addons) {
    const detail = [
      addon.version ? `v${addon.version}` : "",
      t("ui.addons.catalogs", addon.catalogs),
      addon.types.join(", "),
    ]
      .filter(Boolean)
      .join(" · ");

    const toggle = el("label", { className: "switch", title: addon.enabled ? t("ui.action.disable") : t("ui.action.enable") });
    const box = el("input", {
      attrs: { type: "checkbox", "aria-label": addon.enabled ? t("ui.action.disable") : t("ui.action.enable") },
    });
    box.checked = addon.enabled;
    box.addEventListener("change", act("addon_enabled", { id: addon.id, enabled: !addon.enabled }));
    toggle.appendChild(box);
    toggle.appendChild(el("i"));

    list.appendChild(
      el("div", { className: "addon" }, [
        el("div", { className: "grow" }, [
          el("div", { className: "title", text: addon.name }),
          el("div", { className: "sub", text: detail }),
          addon.needs_configuration ? el("div", { className: "sub", text: t("ui.addons.needsconfig") }) : null,
        ]),
        el("div", { className: "tools" }, [
          iconButton("up", t("ui.action.up"), act("addon_move", { id: addon.id, by: -1 })),
          iconButton("down", t("ui.action.down"), act("addon_move", { id: addon.id, by: 1 })),
          iconButton("refresh", t("ui.action.refresh"), act("addon_refresh", { id: addon.id })),
          iconButton("trash", t("ui.action.remove"), act("addon_remove", { id: addon.id })),
          toggle,
        ]),
      ])
    );
  }
}

// ----------------------------------------------------------------- settings

/** Every setting the window offers, and how it is chosen.
 *
 * Nothing here is a free-text box. A free-text box let someone type a
 * language no addon will ever match, or a number the host then refused; a
 * list offers only what works. Where the list is long -- languages -- typing
 * narrows it, but only a click chooses. */
const MEGABYTES = [8, 16, 32, 64, 128, 256, 512];
const RATES = [256, 512, 1024, 2048, 5120, 10240, 25600];
const PEERS = [20, 50, 100, 200, 400];

const SETTINGS_SECTIONS = [
  {
    title: "ui.settings.section.sources",
    fields: [
      { path: "ranking.minimumResolution", kind: "choice", options: ["none", "720", "1080", "1440", "2160"] },
      { path: "ranking.audioLanguages", kind: "languages", from: "ranking.preferredAudioLanguages" },
      { path: "ranking.preferHigherResolution", kind: "boolean" },
      { path: "ranking.preferSourcesWithSubtitles", kind: "boolean" },
    ],
  },
  {
    title: "ui.settings.section.playback",
    fields: [
      { path: "playback.subtitleLanguages", kind: "languages" },
      { path: "playback.volume", kind: "range", min: 0, max: 130, step: 5 },
    ],
  },
  {
    title: "ui.settings.section.torrent",
    fields: [
      { path: "torrent.prebufferMegabytes", kind: "number", options: MEGABYTES, unit: "ui.unit.megabytes" },
      { path: "torrent.downloadLimitKbps", kind: "number", options: RATES, unit: "ui.unit.kbps", optional: true },
      { path: "torrent.uploadLimitKbps", kind: "number", options: RATES, unit: "ui.unit.kbps", optional: true },
      { path: "torrent.peerLimit", kind: "number", options: PEERS },
      { path: "torrent.keepFiles", kind: "boolean" },
      { path: "torrent.downloadDirectory", kind: "folder" },
    ],
  },
];

/** Read a stored value. The path a setting is *set* by is the headless
 *  build's, and for one setting it differs from the key it is stored under
 *  (`ranking.audioLanguages` is stored as `preferredAudioLanguages`), which a
 *  field names in `from`. */
function at(document_, path) {
  return path.split(".").reduce((value, key) => (value == null ? undefined : value[key]), document_);
}

function stored(settings, field) {
  const value = at(settings, field.path);
  return value !== undefined ? value : field.from ? at(settings, field.from) : undefined;
}

async function save(path, value) {
  await ask("settings_set", { path, value: String(value) });
  toast(t("ui.settings.saved"));
}

async function loadSettings() {
  showView("settings");
  const form = document.getElementById("settings-form");
  clear(form);
  const view = await ask("settings");
  const languages = view.languages;

  // The interface's own language comes first: it is the setting someone who
  // cannot read the rest of the page is looking for.
  const languagePanel = el("div", { className: "card-panel" }, [el("h3", { text: t("ui.settings.section.interface") })]);
  const interfaceLanguage = el("select", { attrs: { "aria-label": t("ui.settings.language") } });
  for (const locale of STRINGS.locales) {
    interfaceLanguage.appendChild(el("option", { text: t(`ui.language.${locale}`), attrs: { value: locale } }));
  }
  interfaceLanguage.value = STRINGS.locale;
  interfaceLanguage.addEventListener("change", async () => {
    const next = await ask("language_set", { code: interfaceLanguage.value });
    adoptStrings(next);
    loadSettings();
    loadRows();
  });
  languagePanel.appendChild(row(t("ui.settings.language"), interfaceLanguage));
  form.appendChild(languagePanel);

  for (const section of SETTINGS_SECTIONS) {
    const panel = el("div", { className: "card-panel" }, [el("h3", { text: t(section.title) })]);
    for (const field of section.fields) {
      const current = stored(view.settings, field);
      panel.appendChild(row(t(`ui.settings.${field.path}`), control(field, current, languages)));
    }
    if (section.title === "ui.settings.section.torrent") panel.appendChild(bufferBox(view.buffer_bytes));
    form.appendChild(panel);
  }

  const about = await ask("about");
  const aboutPanel = el("div", { className: "card-panel" }, [el("h3", { text: t("ui.about.title") })]);
  aboutPanel.appendChild(el("div", { className: "note", text: t("ui.about.version", about.version) }));
  aboutPanel.appendChild(
    el("div", {
      className: "note",
      text: view.audit_clean ? t("ui.settings.audit.clean") : t("ui.settings.audit.dirty"),
    })
  );
  aboutPanel.appendChild(
    el("div", {
      className: "note",
      text: about.trusted_keys === 0 ? t("ui.about.trust.empty") : t("ui.about.trust.count", about.trusted_keys),
    })
  );
  aboutPanel.appendChild(el("div", { className: "note", text: t("ui.about.nothing") }));
  aboutPanel.appendChild(el("div", { className: "note", text: t("ui.about.mpv") }));
  aboutPanel.appendChild(el("div", { className: "note", text: t("ui.about.files", about.files.join("  ")) }));
  form.appendChild(aboutPanel);
}

function row(label, controlNode) {
  return el("div", { className: "setting" }, [el("span", { className: "setting-name", text: label }), controlNode]);
}

function control(field, current, languages) {
  if (field.kind === "boolean") {
    const toggle = el("label", { className: "switch" });
    const box = el("input", { attrs: { type: "checkbox", "aria-label": t(`ui.settings.${field.path}`) } });
    box.checked = Boolean(current);
    box.addEventListener("change", () => save(field.path, box.checked ? "true" : "false"));
    toggle.appendChild(box);
    toggle.appendChild(el("i"));
    return toggle;
  }
  if (field.kind === "languages") {
    return languagePicker(Array.isArray(current) ? current : [], languages, (codes) => save(field.path, codes.join(", ")));
  }
  if (field.kind === "range") {
    const wrap = el("div", { className: "range" });
    const input = el("input", {
      attrs: { type: "range", min: field.min, max: field.max, step: field.step, "aria-label": t(`ui.settings.${field.path}`) },
    });
    input.value = String(current ?? 100);
    const out = el("output", { text: input.value });
    input.addEventListener("input", () => {
      out.textContent = input.value;
    });
    input.addEventListener("change", () => save(field.path, input.value));
    wrap.appendChild(input);
    wrap.appendChild(out);
    return wrap;
  }

  const select = el("select", { attrs: { "aria-label": t(`ui.settings.${field.path}`) } });
  const option = (value, label) => select.appendChild(el("option", { text: label, attrs: { value } }));

  if (field.kind === "choice") {
    for (const value of field.options) option(value, value === "none" ? t("ui.sources.any") : `${value}p`);
    select.value = current === null || current === undefined ? "none" : resolutionName(current);
  } else if (field.kind === "number") {
    if (field.optional) option("", t("ui.value.unlimited"));
    const values = [...field.options];
    // A value set elsewhere -- by hand, or by the headless build -- is kept
    // and shown, rather than silently replaced by the nearest preset.
    if (current !== null && current !== undefined && !values.includes(Number(current))) {
      values.push(Number(current));
      values.sort((a, b) => a - b);
    }
    for (const value of values) option(String(value), field.unit ? t(field.unit, value) : String(value));
    select.value = current === null || current === undefined ? "" : String(current);
  } else if (field.kind === "folder") {
    // The folder is the one setting that is a place rather than a value, and
    // choosing a new one needs a folder picker this build does not have yet.
    // So it offers the default, and keeps showing a folder chosen elsewhere.
    option("", t("ui.settings.dir.default"));
    if (current) option(String(current), String(current));
    select.value = current ? String(current) : "";
  }
  select.addEventListener("change", () => save(field.path, select.value));
  return select;
}

/** The stored form is the enum's lowercase name (`fullhd`), the offered form
 *  a number of lines. */
function resolutionName(value) {
  return { hd: "720", fullhd: "1080", quadhd: "1440", ultrahd: "2160", low: "none" }[String(value).toLowerCase()] || "none";
}

/** Pick languages from a list, in order of preference.
 *
 * Typing narrows the list -- by name in the interface's language or by code
 * -- and only a click, or Enter on the highlighted row, adds one. The text in
 * the box is never what gets saved, so nothing that is not a language can be.
 * The order of the chips is the order of preference, which is what the
 * ranking reads. */
function languagePicker(chosen, known, onChange) {
  const picked = chosen.map((code) => String(code).toLowerCase());
  const wrap = el("div", { className: "combo" });
  const box = el("div", { className: "combo-box" });
  const input = el("input", {
    className: "combo-input",
    attrs: {
      type: "text",
      role: "combobox",
      "aria-expanded": "false",
      "aria-autocomplete": "list",
      "aria-label": t("ui.combo.add"),
      placeholder: t("ui.combo.placeholder"),
      spellcheck: "false",
      autocomplete: "off",
    },
  });
  const list = el("ul", { className: "combo-list", attrs: { role: "listbox" } });
  list.hidden = true;
  let highlighted = 0;
  let matches = [];

  const chips = () => {
    for (const chip of [...box.querySelectorAll(".combo-chip")]) chip.remove();
    picked.forEach((code, index) => {
      const remove = iconButton("close", t("ui.combo.remove", languageName(code)), (event) => {
        event.stopPropagation();
        picked.splice(index, 1);
        chips();
        onChange(picked);
      }, "");
      box.insertBefore(
        el("span", { className: "combo-chip" }, [
          el("span", { className: "order", text: index + 1 }),
          el("span", { text: languageName(code) }),
          remove,
        ]),
        input
      );
    });
  };

  const draw = () => {
    clear(list);
    const query = fold(input.value.trim());
    matches = known.filter(
      (code) => !picked.includes(code) && (!query || fold(languageName(code)).includes(query) || code.startsWith(query))
    );
    if (highlighted >= matches.length) highlighted = 0;
    if (matches.length === 0) {
      list.appendChild(el("li", { className: "none", text: t("ui.combo.nomatch") }));
    }
    matches.forEach((code, index) => {
      const item = el(
        "li",
        { attrs: { role: "option", "aria-selected": String(index === highlighted) } },
        [el("span", { text: languageName(code) }), el("small", { text: code })]
      );
      // mousedown, not click: a click would blur the input first and close
      // the list under the pointer.
      item.addEventListener("mousedown", (event) => {
        event.preventDefault();
        choose(code);
      });
      list.appendChild(item);
    });
  };

  const open = () => {
    list.hidden = false;
    input.setAttribute("aria-expanded", "true");
    draw();
  };
  const close = () => {
    list.hidden = true;
    input.setAttribute("aria-expanded", "false");
    input.value = "";
  };
  const choose = (code) => {
    picked.push(code);
    input.value = "";
    chips();
    draw();
    onChange(picked);
  };

  input.addEventListener("focus", open);
  input.addEventListener("input", () => {
    highlighted = 0;
    open();
  });
  input.addEventListener("blur", close);
  input.addEventListener("keydown", (event) => {
    if (event.key === "ArrowDown") {
      event.preventDefault();
      highlighted = Math.min(matches.length - 1, highlighted + 1);
      draw();
    } else if (event.key === "ArrowUp") {
      event.preventDefault();
      highlighted = Math.max(0, highlighted - 1);
      draw();
    } else if (event.key === "Enter") {
      event.preventDefault();
      if (matches[highlighted]) choose(matches[highlighted]);
    } else if (event.key === "Escape") {
      input.blur();
    } else if (event.key === "Backspace" && input.value === "" && picked.length) {
      picked.pop();
      chips();
      draw();
      onChange(picked);
    }
  });
  box.addEventListener("click", () => input.focus());

  box.appendChild(input);
  wrap.appendChild(box);
  wrap.appendChild(list);
  chips();
  return wrap;
}

/** How much the buffer holds, and the button that empties it. */
function bufferBox(size) {
  const amount = el("strong", { text: bytes(size) });
  const clearButton = el("button", { className: "button danger", attrs: { type: "button" } });
  clearButton.appendChild(icon("trash"));
  clearButton.appendChild(el("span", { text: t("ui.buffer.clear") }));
  clearButton.disabled = !size;
  clearButton.addEventListener("click", async () => {
    let freed;
    try {
      freed = await ask("buffer_clear");
    } catch (error) {
      return;
    }
    toast(t("ui.buffer.cleared", bytes(freed)));
    loadSettings();
  });
  return el("div", { className: "setting buffer-row" }, [
    el("span", { className: "setting-name" }, [
      document.createTextNode(t("ui.buffer.title")),
      el("span", { className: "hint", text: t("ui.buffer.note") }),
    ]),
    el("div", { className: "buffer" }, [
      el("div", { className: "grow", attrs: { "aria-label": t("ui.buffer.size", bytes(size)) } }, [amount]),
      clearButton,
    ]),
  ]);
}

// ------------------------------------------------------------------ polling

async function poll() {
  let status;
  try {
    status = await invoke("status");
  } catch (error) {
    return;
  }

  // The loading screen follows the job. Once mpv is up, the player's own
  // webview covers this page, and the screen is taken down underneath it so
  // the catalogue is what is there when the player goes.
  if (status.job.active) {
    if (document.getElementById("loading").hidden && STARTING === null) {
      showLoading(status.player.title || "", null, t(status.job.stage));
    }
    document.getElementById("loading-stage").textContent = t(status.job.stage);
    document.getElementById("loading-detail").textContent = status.job.detail;
    document.getElementById("loading-meter").style.width =
      status.job.progress !== null && status.job.progress !== undefined
        ? `${Math.round(status.job.progress * 100)}%`
        : "0%";
    const torrent = status.torrents[0];
    document.getElementById("loading-torrent").textContent = torrent
      ? t(
          "ui.torrent.line",
          torrent.name,
          t("ui.unit.percent", torrent.percent),
          bytes(torrent.down_bps),
          torrent.peers,
          torrent.eta_seconds ? clock(torrent.eta_seconds) : "—"
        )
      : "";
  } else if (STARTING !== null && (status.player.running || Date.now() - STARTING.since > 1500)) {
    hideLoading();
  }
  if (!status.job.active && status.job.error) {
    hideLoading();
    toast(status.job.error);
  }
}

// --------------------------------------------------------------------- boot

let STRINGS = { locale: "en", locales: [] };

function adoptStrings(strings) {
  STRINGS = strings;
  MESSAGES = { ...strings.messages, __locale: strings.locale };
  applyStrings();
}

async function boot() {
  drawIcons();
  adoptStrings(await invoke("strings"));

  for (const node of document.querySelectorAll("[data-go]")) {
    node.addEventListener("click", () => go(node.dataset.go));
  }

  window.addEventListener("scroll", solidNav, { passive: true });

  const searchForm = document.getElementById("search-form");
  const searchInput = document.getElementById("search-input");
  searchForm.addEventListener("submit", (event) => {
    event.preventDefault();
    if (!searchInput.value.trim()) {
      searchForm.classList.add("open");
      searchInput.focus();
      return;
    }
    runSearch(searchInput.value);
  });
  searchInput.addEventListener("blur", () => {
    if (!searchInput.value) searchForm.classList.remove("open");
  });

  const dialog = document.getElementById("link-dialog");
  const linkInput = document.getElementById("link-input");
  const opener = document.getElementById("link-open");
  // Focus goes into the dialog when it opens and back to the button that
  // opened it when it closes, so a keyboard is never left somewhere it did
  // not put itself.
  const closeDialog = () => {
    dialog.hidden = true;
    opener.focus();
  };
  opener.addEventListener("click", () => {
    dialog.hidden = false;
    linkInput.focus();
  });
  document.getElementById("link-cancel").addEventListener("click", closeDialog);
  dialog.addEventListener("click", (event) => {
    if (event.target === dialog) closeDialog();
  });
  document.getElementById("link-form").addEventListener("submit", async (event) => {
    event.preventDefault();
    const link = linkInput.value;
    if (!link.trim()) return;
    linkInput.value = "";
    dialog.hidden = true;
    showLoading("", null, t("ui.job.starting"));
    try {
      await ask("play_link", { link });
    } catch (error) {
      hideLoading();
      return;
    }
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
  document.getElementById("loading-back").addEventListener("click", hideLoading);

  document.addEventListener("keydown", (event) => {
    if (event.key === "Escape") {
      if (!dialog.hidden) closeDialog();
      else if (!document.getElementById("view-detail").hidden) go(LAST);
    } else if (event.key === "/" && document.activeElement === document.body) {
      event.preventDefault();
      searchForm.classList.add("open");
      searchInput.focus();
    }
  });

  showView("home");
  await loadRows();
  await poll();
  setInterval(poll, 1000);
}

boot();
