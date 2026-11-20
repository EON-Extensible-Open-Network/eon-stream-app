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

/** What the source list holds, how it is narrowed, and in what order. */
let SOURCES = sourceState([], null, "");

/** Which source list is the newest asked for: an older one that answers late
 *  is dropped rather than drawn over it. */
let SOURCES_TOKEN = 0;

function sourceState(all, subject, name) {
  return {
    all,
    subject,
    name,
    // Several values may be picked in one filter, and a source shows when it
    // has any of them; between filters, it has to satisfy each.
    pick: { quality: new Set(), audio: new Set(), subs: new Set() },
    sort: "best",
  };
}

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
  const img = el("img", { className, attrs: { alt: "", loading: "lazy", decoding: "async", draggable: "false" } });
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
  } else if (target === "new") openNew();
  else if (target === "discover") openDiscover();
  else if (target === "calendar") openCalendar();
  else if (target === "profile") openProfile();
  else if (target === "downloads") openDownloads();
  else if (target === "community") openCommunity();
  else if (target === "library") openLibrary();
  else if (target === "addons") loadAddons();
  else if (target === "settings") loadSettings();
  else if (target === "search") showView("search");
}

function solidNav() {
  document.getElementById("nav").classList.toggle("solid", window.scrollY > 40);
}

/** The side menu, open or folded to icons. The choice is remembered on this
 *  computer; a narrow window starts folded. */
function setSide(open, remember) {
  document.body.classList.toggle("side-mini", !open);
  document.getElementById("side-toggle").setAttribute("aria-expanded", String(open));
  if (remember) {
    try {
      localStorage.setItem("eon-side", open ? "open" : "mini");
    } catch (error) {
      // A private profile keeps nothing; the menu still works.
    }
  }
}

function initSide() {
  let kept = null;
  try {
    kept = localStorage.getItem("eon-side");
  } catch (error) {
    kept = null;
  }
  setSide(kept ? kept === "open" : window.innerWidth >= 1200, false);
  document.getElementById("side-toggle").addEventListener("click", () => {
    setSide(document.body.classList.contains("side-mini"), true);
  });
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

/** One poster card, with a menu of `actions(item, holder)` when given. */
function card(item, actions = null) {
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

  const button = el(
    "button",
    {
      className: "card",
      attrs: { type: "button" },
      title: item.name,
      onClick: () => openDetail(item.content_type, item.id),
    },
    [poster, el("div", { className: "name", text: item.name }), el("div", { className: "sub", text: sub })]
  );
  if (!actions) return button;

  // A card with a menu: the menu button sits beside the card's own button
  // rather than inside it, because a button inside a button is not one a
  // keyboard or a screen reader can reach.
  const holder = el("div", { className: "card-holder" }, [button]);
  const more = iconButton("more", t("ui.card.more", item.name), () => openMenu(more, actions(item, holder)), "card-more");
  more.setAttribute("aria-haspopup", "menu");
  holder.appendChild(more);
  return holder;
}

// --------------------------------------------------------------------- menu

/** A small menu under the button that opened it. Closes on a choice, on
 *  Escape, on a click anywhere else, and when the page moves under it. */
function openMenu(anchor, items) {
  const menu = document.getElementById("menu");
  clear(menu);
  for (const item of items) {
    const entry = el("button", {
      className: "menu-item",
      attrs: { type: "button", role: "menuitem" },
      onClick: () => {
        closeMenu();
        item.run();
      },
    });
    entry.appendChild(icon(item.icon));
    entry.appendChild(el("span", { text: item.label }));
    menu.appendChild(entry);
  }
  menu.hidden = false;
  const box = anchor.getBoundingClientRect();
  const width = menu.offsetWidth;
  menu.style.left = `${Math.max(8, Math.min(window.innerWidth - width - 8, box.right - width))}px`;
  menu.style.top = `${Math.min(window.innerHeight - menu.offsetHeight - 8, box.bottom + 6)}px`;
  closeMenu.anchor = anchor;
  anchor.setAttribute("aria-expanded", "true");
  const first = menu.querySelector("button");
  if (first) first.focus();
}

function closeMenu(returnFocus = false) {
  const menu = document.getElementById("menu");
  if (menu.hidden) return;
  menu.hidden = true;
  const anchor = closeMenu.anchor;
  closeMenu.anchor = null;
  if (anchor) {
    anchor.setAttribute("aria-expanded", "false");
    if (returnFocus && anchor.isConnected) anchor.focus();
  }
}

/** What a continue-watching card offers. */
function continueActions(row) {
  return (item, holder) => [
    { icon: "info", label: t("ui.action.details"), run: () => openDetail(item.content_type, item.id) },
    {
      icon: "close",
      label: t("ui.continue.remove"),
      run: async () => {
        try {
          await ask("continue_remove", { contentType: item.content_type, id: item.id });
        } catch (error) {
          return;
        }
        row.items = row.items.filter((other) => other !== item);
        const section = holder.closest(".row");
        holder.remove();
        if (row.items.length === 0) {
          ROWS = ROWS.filter((other) => other !== row);
          if (section) section.remove();
        }
        toast(t("ui.continue.removed"));
      },
    },
  ];
}

// --------------------------------------------------------------------- home

/** Whether the profile in use is a child's, and what a child's sees. */
let KIDS = false;
const KIDS_GENRES = ["animation", "family", "kids", "children"];

async function loadRows() {
  let rows = await ask("rows");
  // Discover indexes into this list: a new list is a new Discover.
  DISCOVER.built = false;
  // Two addons can offer the same catalogue under the same name -- two
  // "Popular" film rows that look like one repeated. The first, in the
  // viewer's addon order, is kept.
  const seen = new Set();
  rows = rows.filter((row) => {
    if (row.kind !== "catalog") return true;
    const key = `${row.content_type}|${row.title.trim().toLocaleLowerCase()}|${row.extra ? row.extra.join("=") : ""}`;
    if (seen.has(key)) return false;
    seen.add(key);
    return true;
  });
  // "Popular" for films and "Popular" for series read as one row twice:
  // a name two catalogue rows share carries its type.
  const names = new Map();
  for (const row of rows) {
    if (row.kind === "catalog") names.set(row.title, (names.get(row.title) || 0) + 1);
  }
  rows = rows.map((row) =>
    row.kind === "catalog" && names.get(row.title) > 1
      ? { ...row, title: t("ui.row.typed", row.title, typeName(row.content_type)) }
      : row
  );
  // A child's profile: each catalogue row asks for its family or animation
  // titles, and a catalogue that cannot be asked that way is left out.
  const me = PROFILES.profiles.find((p) => p.id === PROFILES.current);
  KIDS = Boolean(me && me.kids);
  if (KIDS) {
    rows = rows
      .map((row) => {
        if (row.kind !== "catalog") return row;
        const genre = row.genres.find((g) => KIDS_GENRES.includes(g.toLowerCase()));
        return genre ? { ...row, extra: ["genre", genre], title: `${row.title} · ${genre}` } : null;
      })
      .filter(Boolean);
  }
  ROWS = rows.map((row) => ({
    ...row,
    // A catalogue row fetches itself; the others arrive filled.
    loaded: row.kind !== "catalog",
    exhausted: row.kind !== "catalog",
    loading: null,
  }));
  renderHome();
  addNewEpisodes();
  addYourGenres();
  addForYou();
  // Continue-watching cards from before their artwork was remembered get it
  // now, once; the row is drawn again if anything came back.
  invoke("fill_art")
    .then(async (filled) => {
      if (!filled) return;
      const fresh = await invoke("rows");
      const updated = fresh.find((row) => row.kind === "continue");
      const row = ROWS.find((other) => other.kind === "continue");
      if (updated && row) {
        row.items = updated.items;
        if (!document.getElementById("view-home").hidden) renderHome();
      }
    })
    .catch(() => {});
}

// ------------------------------------------------------------------ profiles

/** The six avatar colours, as CSS classes. */
const AVATARS = 6;
let PROFILES = { profiles: [], current: "main", onboarded: true };

function profileName(profile) {
  return profile.name || t("ui.profiles.main");
}

function avatar(profile, size = "") {
  return el("span", {
    className: `avatar c${profile.color % AVATARS} ${size}`.trim(),
    text: (profileName(profile).trim()[0] || "?").toLocaleUpperCase(),
    attrs: { "aria-hidden": "true" },
  });
}

async function loadProfiles() {
  PROFILES = await invoke("profiles");
  const me = PROFILES.profiles.find((p) => p.id === PROFILES.current) || PROFILES.profiles[0];
  const button = document.getElementById("who-open");
  clear(button);
  button.appendChild(avatar(me, "small"));
  button.setAttribute("aria-label", t("ui.profiles.switch", profileName(me)));
  button.title = t("ui.profiles.switch", profileName(me));
  return PROFILES;
}

/** "Who is watching?" -- every profile, a way to add one, and a way to
 *  change them. */
function openWho(editing = false) {
  const layer = document.getElementById("who");
  const grid = document.getElementById("who-grid");
  clear(grid);
  document.getElementById("who-title").textContent = editing ? t("ui.profiles.manage") : t("ui.profiles.who");
  for (const profile of PROFILES.profiles) {
    const tile = el(
      "button",
      {
        className: `who-tile${profile.id === PROFILES.current ? " current" : ""}`,
        attrs: { type: "button" },
        onClick: () => (editing ? editProfile(profile) : switchTo(profile.id)),
      },
      [
        avatar(profile, "big"),
        el("span", { className: "who-name", text: profileName(profile) }),
        profile.kids || profile.locked
          ? el("span", { className: "who-badges", text: [profile.kids ? t("ui.profiles.kids") : null, profile.locked ? t("ui.profiles.lockedbadge") : null].filter(Boolean).join(" · ") })
          : null,
      ]
    );
    if (editing) tile.appendChild(el("span", { className: "who-edit", text: t("ui.profiles.edit") }));
    grid.appendChild(tile);
  }
  if (!editing && PROFILES.profiles.length < 8) {
    const add = el("button", { className: "who-tile add", attrs: { type: "button" }, onClick: () => editProfile(null) }, [
      el("span", { className: "avatar big plus" }, [icon("plus")]),
      el("span", { className: "who-name", text: t("ui.profiles.add") }),
    ]);
    grid.appendChild(add);
  }
  const manage = document.getElementById("who-manage");
  manage.textContent = editing ? t("ui.profiles.done") : t("ui.profiles.manage");
  manage.onclick = () => openWho(!editing);
  layer.hidden = false;
  const first = grid.querySelector(".who-tile.current") || grid.querySelector(".who-tile");
  if (first) first.focus();
}

/** At start, a locked profile that was in use last is not opened without
 *  its PIN: "Who is watching?" stays until someone picks a profile. */
let PIN_GATE = false;

function closeWho() {
  if (PIN_GATE) return;
  document.getElementById("who").hidden = true;
}

async function switchTo(id) {
  if (id === PROFILES.current && PIN_GATE) {
    const me = PROFILES.profiles.find((p) => p.id === id);
    const pin = await promptText(t("ui.profiles.pinask", profileName(me)), null, { min: 4, max: 4, multiline: false, secret: true });
    if (pin === null) return;
    try {
      await ask("profile_switch", { id, pin });
    } catch (error) {
      return;
    }
    PIN_GATE = false;
  }
  if (id !== PROFILES.current) {
    const target = PROFILES.profiles.find((p) => p.id === id);
    let pin = null;
    if (target && target.locked) {
      pin = await promptText(t("ui.profiles.pinask", profileName(target)), null, { min: 4, max: 4, multiline: false, secret: true });
      if (pin === null) return;
    }
    try {
      await ask("profile_switch", { id, pin });
    } catch (error) {
      return;
    }
    PIN_GATE = false;
    await loadProfiles();
    go("home");
    await loadRows();
  }
  closeWho();
}

/** Add a profile (null) or change one: a name and a colour; a profile that
 *  is not the main one or the current one can be removed. */
function editProfile(profile) {
  const grid = document.getElementById("who-grid");
  clear(grid);
  let color = profile ? profile.color : PROFILES.profiles.length % AVATARS;
  const preview = el("div", { className: "who-preview" });
  const name = el("input", {
    attrs: { maxlength: "24", "aria-label": t("ui.profiles.name"), placeholder: t("ui.profiles.name"), value: profile ? profile.name : "" },
  });
  const draw = () => {
    clear(preview);
    preview.appendChild(avatar({ name: name.value || "?", color }, "big"));
  };
  name.addEventListener("input", draw);
  const colors = el("div", { className: "who-colors", attrs: { role: "radiogroup", "aria-label": t("ui.profiles.color") } });
  for (let c = 0; c < AVATARS; c += 1) {
    colors.appendChild(
      el("button", {
        className: `swatch c${c}`,
        attrs: { type: "button", role: "radio", "aria-label": t("ui.profiles.colorn", c + 1) },
        onClick: () => {
          color = c;
          draw();
        },
      })
    );
  }
  const save = el("button", {
    className: "button play",
    attrs: { type: "button" },
    text: profile ? t("ui.profiles.save") : t("ui.profiles.create"),
    onClick: async () => {
      try {
        PROFILES = profile
          ? await ask("profile_edit", { id: profile.id, name: name.value, color })
          : await ask("profile_add", { name: name.value, color });
      } catch (error) {
        return;
      }
      await loadProfiles();
      openWho(Boolean(profile));
    },
  });
  const actions = el("div", { className: "loading-actions" }, [
    save,
    el("button", { className: "button ghost", attrs: { type: "button" }, text: t("ui.profiles.cancel"), onClick: () => openWho(Boolean(profile)) }),
  ]);
  if (profile && profile.id !== "main" && profile.id !== PROFILES.current) {
    actions.appendChild(
      el("button", {
        className: "button danger",
        attrs: { type: "button" },
        text: t("ui.profiles.remove"),
        onClick: async (event) => {
          const button = event.currentTarget;
          if (!button.dataset.armed) {
            button.dataset.armed = "true";
            button.textContent = t("ui.profiles.removesure");
            return;
          }
          PROFILES = await ask("profile_remove", { id: profile.id });
          await loadProfiles();
          openWho(true);
        },
      })
    );
  }
  // A child's profile, and a PIN -- for a profile that already exists.
  const extras = el("div", { className: "who-extras" });
  if (profile) {
    const kids = el("input", { attrs: { type: "checkbox" } });
    kids.checked = Boolean(profile.kids);
    kids.addEventListener("change", async () => {
      try {
        PROFILES = await ask("profile_kids", { id: profile.id, kids: kids.checked });
        profile.kids = kids.checked;
      } catch (error) {
        kids.checked = !kids.checked;
      }
    });
    extras.appendChild(el("label", { className: "check" }, [kids, el("span", { text: t("ui.profiles.kidslabel") })]));
    extras.appendChild(el("p", { className: "hint", text: t("ui.profiles.kidsnote") }));
    extras.appendChild(
      el("button", {
        className: "button ghost",
        attrs: { type: "button" },
        text: profile.locked ? t("ui.profiles.pinchange") : t("ui.profiles.pinset"),
        onClick: async () => {
          let old = null;
          if (profile.locked) {
            old = await promptText(t("ui.profiles.pinold"), null, { min: 4, max: 4, multiline: false, secret: true });
            if (old === null) return;
          }
          const pin = await promptText(t("ui.profiles.pinnew"), t("ui.profiles.pinhint"), { min: 0, max: 4, multiline: false, secret: true });
          if (pin === null) return;
          try {
            PROFILES = await ask("profile_pin", { id: profile.id, pin: pin || null, old });
            toast(pin ? t("ui.profiles.pinsaved") : t("ui.profiles.pinremoved"));
            openWho(true);
          } catch (error) {
            // `ask` said it.
          }
        },
      })
    );
  }
  grid.appendChild(el("div", { className: "who-form" }, [preview, name, colors, extras, actions]));
  draw();
  name.focus();
}

// --------------------------------------------------------------- first run

/** How the window opens: a row of choices; in Settings each press applies
 *  at once, in the first-run setup on "Next". */
function windowPicker(current, live) {
  const box = el("div", { className: "chips", attrs: { role: "radiogroup", "aria-label": t("ui.window.title") } });
  box.dataset.value = current;
  const draw = () => {
    clear(box);
    for (const mode of ["maximized", "fullscreen", "large", "medium", "small"]) {
      box.appendChild(
        el("button", {
          className: "chip",
          text: t(`ui.window.${mode}`),
          attrs: { type: "button", role: "radio", "aria-checked": String(box.dataset.value === mode), "aria-pressed": String(box.dataset.value === mode) },
          onClick: async () => {
            box.dataset.value = mode;
            draw();
            if (live) await ask("window_mode_set", { mode });
          },
        })
      );
    }
  };
  draw();
  return box;
}

/** The first time: who is watching, in which language, how much disk the
 *  cache may use, and what an addon is -- since this program comes with
 *  none (madde 9) and does nothing until one is added. */
async function openWelcome() {
  const layer = document.getElementById("welcome");
  const body = document.getElementById("welcome-body");
  const settings = await ask("settings");
  let step = 0;
  const me = PROFILES.profiles.find((p) => p.id === PROFILES.current) || PROFILES.profiles[0];

  const finish = async () => {
    await invoke("onboarding_done");
    layer.hidden = true;
    PROFILES.onboarded = true;
    await loadProfiles();
    await loadRows();
  };

  const steps = [
    () => {
      const name = el("input", {
        attrs: { maxlength: "24", "aria-label": t("ui.profiles.name"), placeholder: t("ui.profiles.name"), value: me.name || "" },
      });
      return {
        title: t("ui.welcome.hello"),
        text: t("ui.welcome.hellotext"),
        content: [name],
        next: async () => {
          if (name.value.trim()) await ask("profile_edit", { id: me.id, name: name.value, color: me.color });
        },
      };
    },
    () => {
      const language = el("select", { className: "sort-select", attrs: { "aria-label": t("ui.settings.language") } });
      for (const locale of STRINGS.locales) language.appendChild(el("option", { text: t(`ui.language.${locale}`), attrs: { value: locale } }));
      language.value = STRINGS.locale;
      const subs = el("select", { className: "sort-select", attrs: { "aria-label": t("ui.welcome.subs") } });
      subs.appendChild(el("option", { text: t("ui.welcome.nosubs"), attrs: { value: "" } }));
      for (const code of settings.languages) subs.appendChild(el("option", { text: languageName(code), attrs: { value: code } }));
      subs.value = (settings.settings.playback.subtitleLanguages || [])[0] || (STRINGS.locale === "en" ? "" : STRINGS.locale);
      return {
        title: t("ui.welcome.language"),
        text: t("ui.welcome.languagetext"),
        content: [
          el("label", { className: "pick" }, [el("span", { text: t("ui.settings.language") }), language]),
          el("label", { className: "pick" }, [el("span", { text: t("ui.welcome.subs") }), subs]),
        ],
        next: async () => {
          if (language.value !== STRINGS.locale) adoptStrings(await ask("language_set", { code: language.value }));
          await ask("settings_set", { path: "playback.subtitleLanguages", value: subs.value });
        },
      };
    },
    () => {
      const mode = windowPicker(PROFILES.window || "maximized", false);
      return {
        title: t("ui.welcome.window"),
        text: t("ui.welcome.windowtext"),
        content: [mode],
        next: async () => {
          await ask("window_mode_set", { mode: mode.dataset.value });
        },
      };
    },
    () => ({
      title: t("ui.welcome.disk"),
      text: t("ui.welcome.disktext"),
      content: [cacheBox(settings)],
      next: async () => {},
    }),
    () => {
      const address = el("input", { attrs: { type: "url", placeholder: "https://…/manifest.json", spellcheck: "false", "aria-label": t("ui.welcome.addon") } });
      return {
        title: t("ui.welcome.addons"),
        text: t("ui.welcome.addonstext"),
        content: [address],
        next: async () => {
          if (address.value.trim()) await ask("addon_add", { address: address.value.trim() });
        },
      };
    },
  ];

  const draw = () => {
    clear(body);
    const current = steps[step]();
    const dots = el("div", { className: "welcome-dots", attrs: { "aria-hidden": "true" } });
    steps.forEach((_, at) => dots.appendChild(el("i", { className: at === step ? "on" : "" })));
    const forward = el("button", {
      className: "button play",
      attrs: { type: "button" },
      text: step === steps.length - 1 ? t("ui.welcome.start") : t("ui.welcome.next"),
      onClick: async () => {
        try {
          await current.next();
        } catch (error) {
          return;
        }
        if (step === steps.length - 1) {
          finish();
          return;
        }
        step += 1;
        draw();
      },
    });
    body.appendChild(dots);
    body.appendChild(el("h2", { className: "welcome-title", text: current.title }));
    body.appendChild(el("p", { className: "lede", text: current.text }));
    body.appendChild(el("div", { className: "welcome-content" }, current.content));
    body.appendChild(
      el("div", { className: "loading-actions" }, [
        forward,
        step > 0
          ? el("button", { className: "button ghost", attrs: { type: "button" }, text: t("ui.welcome.back"), onClick: () => { step -= 1; draw(); } })
          : null,
        el("button", { className: "text-button quiet", attrs: { type: "button" }, text: t("ui.welcome.skip"), onClick: finish }),
      ])
    );
    forward.focus();
  };
  layer.hidden = false;
  draw();
}

// --------------------------------------------------------------- EON Account

/** One account for all of EON (madde 46). The sheet uses the welcome layer:
 *  sign in, create an account, or continue as a guest -- a full answer that
 *  is never asked again by itself (madde 7). The password goes to the host
 *  and from there nowhere: sign-in is OPAQUE. */
let ACCOUNT = { server: null, username: null, prompted: true, last_sync: null, syncing: false };

async function loadAccount() {
  try {
    ACCOUNT = await invoke("account_state");
  } catch (error) {
    // No account is a fine state to be in.
  }
  return ACCOUNT;
}

/** A labelled field with a hint and an error line, wired for screen readers. */
function accountField(id, label, attrs, hint) {
  const input = el("input", { attrs: { id, ...attrs, "aria-describedby": `${id}-hint ${id}-error` } });
  const parts = [el("label", { className: "field-label", text: label, attrs: { for: id } })];
  if (attrs.type === "password") {
    const toggle = el("button", {
      className: "text-button field-toggle",
      attrs: { type: "button", "aria-pressed": "false", "aria-controls": id },
      text: t("ui.account.show"),
    });
    toggle.addEventListener("click", () => {
      const shown = input.type === "text";
      input.type = shown ? "password" : "text";
      toggle.textContent = shown ? t("ui.account.show") : t("ui.account.hide");
      toggle.setAttribute("aria-pressed", String(!shown));
    });
    parts.push(el("div", { className: "field-row" }, [input, toggle]));
  } else {
    parts.push(input);
  }
  parts.push(el("p", { className: "field-hint", text: hint || "", attrs: { id: `${id}-hint` } }));
  const error = el("p", { className: "field-error", attrs: { id: `${id}-error`, role: "alert" } });
  parts.push(error);
  // A complaint goes as soon as the person starts fixing what it was about.
  input.addEventListener("input", () => {
    error.textContent = "";
    input.removeAttribute("aria-invalid");
  });
  return { node: el("div", { className: "field" }, parts), input };
}

/** The account sheet. `done` runs once the person has signed in, created an
 *  account and kept its key, or chosen to stay a guest. */
async function openAccount(done, mode = "signin") {
  const layer = document.getElementById("welcome");
  const body = document.getElementById("welcome-body");
  await loadAccount();
  const close = async () => {
    try {
      await invoke("account_prompted");
    } catch (error) {
      // Asking again next time is the worst that can happen.
    }
    layer.hidden = true;
    await loadAccount();
    if (done) await done();
  };

  const draw = () => {
    clear(body);
    if (mode === "key") return;
    const form = el("form", { className: "account", attrs: { novalidate: "true" } });
    const tabs = el("div", { className: "account-tabs", attrs: { role: "group", "aria-label": t("ui.account.title") } });
    for (const [which, label] of [["signin", t("ui.account.signin")], ["create", t("ui.account.create")]]) {
      tabs.appendChild(
        el("button", {
          className: "account-tab",
          attrs: { type: "button", "aria-pressed": String(mode === which) },
          text: label,
          onClick: () => { mode = which; draw(); },
        })
      );
    }
    body.appendChild(el("h2", { className: "welcome-title", text: mode === "recover" ? t("ui.account.recover") : t("ui.account.title") }));
    body.appendChild(el("p", { className: "lede", text: mode === "recover" ? t("ui.account.recovertext") : t("ui.account.lede") }));
    if (mode !== "recover") form.appendChild(tabs);

    const fields = {};
    const add = (name, field) => { fields[name] = field; form.appendChild(field.node); };
    if (mode === "signin") {
      add("login", accountField("acc-login", t("ui.account.login"), { autocomplete: "username", spellcheck: "false", autocapitalize: "none" }));
      add("password", accountField("acc-password", t("ui.account.password"), { type: "password", autocomplete: "current-password" }));
    } else if (mode === "create") {
      add("login", accountField("acc-login", t("ui.account.username"), { autocomplete: "username", spellcheck: "false", autocapitalize: "none", maxlength: "32" }, t("ui.account.usernamehint")));
      add("password", accountField("acc-password", t("ui.account.password"), { type: "password", autocomplete: "new-password" }, t("ui.account.passwordhint")));
    } else {
      add("login", accountField("acc-login", t("ui.account.username"), { autocomplete: "username", spellcheck: "false", autocapitalize: "none" }));
      add("key", accountField("acc-key", t("ui.account.recoverykey"), { autocomplete: "off", spellcheck: "false", autocapitalize: "characters" }));
      add("password", accountField("acc-password", t("ui.account.newpassword"), { type: "password", autocomplete: "new-password" }, t("ui.account.passwordhint")));
    }

    // The server: empty until someone enters one, because EON runs none yet.
    const server = accountField("acc-server", t("ui.account.server"), { type: "url", placeholder: "https://…", spellcheck: "false" }, ACCOUNT.server ? "" : t("ui.account.servernone"));
    server.input.value = ACCOUNT.server || "";
    // With no server yet, the field is simply there; once one is set, it
    // folds away behind the address it points at.
    const serverBox = ACCOUNT.server
      ? el("details", { className: "account-server" }, [el("summary", { text: t("ui.account.serverat", ACCOUNT.server) }), server.node])
      : el("div", { className: "account-server" }, [server.node]);
    form.appendChild(serverBox);

    const general = el("p", { className: "field-error", attrs: { role: "alert" } });
    form.appendChild(general);
    const primary = el("button", {
      className: "button play",
      attrs: { type: "submit" },
      text: mode === "signin" ? t("ui.account.signin") : mode === "create" ? t("ui.account.create") : t("ui.account.recoverbutton"),
    });
    const actions = el("div", { className: "loading-actions" }, [primary]);
    if (mode === "recover") {
      actions.appendChild(el("button", { className: "button ghost", attrs: { type: "button" }, text: t("ui.account.backtosignin"), onClick: () => { mode = "signin"; draw(); } }));
    } else {
      actions.appendChild(el("button", { className: "button ghost", attrs: { type: "button" }, text: t("ui.account.guest"), onClick: close }));
    }
    form.appendChild(actions);
    if (mode === "signin") {
      form.appendChild(el("button", { className: "text-button quiet", attrs: { type: "button" }, text: t("ui.account.forgot"), onClick: () => { mode = "recover"; draw(); } }));
    }
    if (mode !== "recover") {
      form.appendChild(el("p", { className: "account-seal" }, [icon("lock"), el("span", { text: t("ui.account.guestnote") })]));
    }

    const showError = (field, message) => {
      for (const f of Object.values(fields)) {
        f.node.querySelector(".field-error").textContent = "";
        f.input.removeAttribute("aria-invalid");
      }
      server.node.querySelector(".field-error").textContent = "";
      general.textContent = "";
      if (!field) {
        general.textContent = message;
        return;
      }
      field.node.querySelector(".field-error").textContent = message;
      field.input.setAttribute("aria-invalid", "true");
      field.input.focus();
    };

    form.addEventListener("submit", async (event) => {
      event.preventDefault();
      const login = fields.login.input.value.trim();
      const password = fields.password.input.value;
      if (!login) return showError(fields.login, t("ui.account.error.invalid"));
      if ([...password].length < 10) return showError(fields.password, t("ui.account.error.weak_password"));
      primary.disabled = true;
      const label = primary.textContent;
      primary.textContent = t("ui.account.working");
      try {
        const address = server.input.value.trim();
        if (address !== (ACCOUNT.server || "")) {
          try {
            ACCOUNT = await invoke("account_server", { address });
          } catch (error) {
            if ("open" in serverBox) serverBox.open = true;
            showError(server, String(error));
            return;
          }
        }
        if (mode === "signin") {
          await invoke("account_login", { login, password });
          await close();
        } else if (mode === "create") {
          const key = await invoke("account_register", { username: login, password });
          mode = "key";
          showKey(key);
        } else {
          await invoke("account_recover", { username: login, key: fields.key.input.value, password });
          await close();
        }
      } catch (error) {
        const message = String(error);
        if (message === t("ui.account.error.taken") || message === t("ui.account.error.unauthorised")) showError(mode === "create" ? fields.login : fields.password, message);
        else if (message === t("ui.account.error.bad_recovery_key")) showError(fields.key, message);
        else showError(null, message);
      } finally {
        primary.disabled = false;
        primary.textContent = label;
      }
    });
    body.appendChild(form);
    fields.login.input.focus();
  };

  /** The recovery key, once. Continuing needs the person to say they kept it. */
  const showKey = (key) => {
    clear(body);
    body.appendChild(el("h2", { className: "welcome-title", text: t("ui.account.keytitle") }));
    body.appendChild(el("p", { className: "lede", text: t("ui.account.keytext") }));
    const groups = el("div", { className: "recovery-key", attrs: { role: "group", "aria-label": t("ui.account.recoverykey") } });
    for (const part of key.split("-")) groups.appendChild(el("span", { text: part }));
    body.appendChild(groups);
    const copy = el("button", { className: "button ghost", attrs: { type: "button" }, text: t("ui.account.copy") });
    copy.addEventListener("click", async () => {
      try {
        await navigator.clipboard.writeText(key);
        copy.textContent = t("ui.account.copied");
      } catch (error) {
        const range = document.createRange();
        range.selectNodeContents(groups);
        const selection = window.getSelection();
        selection.removeAllRanges();
        selection.addRange(range);
      }
    });
    const box = el("input", { attrs: { type: "checkbox", id: "acc-kept" } });
    const kept = el("label", { className: "account-kept", attrs: { for: "acc-kept" } }, [box, el("span", { text: t("ui.account.kept") })]);
    const go = el("button", { className: "button play", attrs: { type: "button", disabled: "true" }, text: t("ui.account.continue"), onClick: close });
    box.addEventListener("change", () => { go.disabled = !box.checked; });
    body.appendChild(el("div", { className: "loading-actions" }, [copy]));
    body.appendChild(el("div", { className: "account-confirm" }, [kept, el("div", { className: "loading-actions" }, [go])]));
    copy.focus();
  };

  layer.hidden = false;
  draw();
}

/** When something was last synced, in the viewer's own clock. */
function syncedText(view) {
  if (view.syncing) return t("ui.account.syncing");
  if (!view.last_sync) return t("ui.account.never");
  const when = new Date(view.last_sync * 1000);
  return t("ui.account.synced", when.toLocaleString(STRINGS.locale, { dateStyle: "medium", timeStyle: "short" }));
}

/** The server's own list of what it keeps, as it gives it. */
async function openTransparency(panel) {
  let doc;
  try {
    doc = await ask("account_transparency");
  } catch (error) {
    return;
  }
  const lang = STRINGS.locale === "tr" ? "tr" : "en";
  const say = (text) => (text && (text[lang] || text.en)) || "";
  const list = el("div", { className: "keeps" }, [el("h4", { text: t("ui.account.keepstitle", doc.server || ACCOUNT.server) })]);
  for (const item of doc.kept || []) {
    list.appendChild(
      el("div", { className: "keeps-item" }, [
        el("p", { className: "keeps-what", text: say(item.what) }),
        el("p", { className: "note", text: say(item.why) }),
        el("p", { className: "note", text: t("ui.account.keepsfor", say(item.kept_for)) }),
        el("p", { className: "note", text: t("ui.account.keepsreader", say(item.readable_by)) }),
      ])
    );
  }
  const plain = (title, items) => {
    if (!items || items.length === 0) return;
    list.appendChild(el("h4", { text: title }));
    const ul = el("ul", { className: "keeps-list" });
    for (const item of items) ul.appendChild(el("li", { text: say(item) }));
    list.appendChild(ul);
  };
  plain(t("ui.account.keepsmemory"), doc.memory_only);
  plain(t("ui.account.keepsnever"), doc.never);
  const old = panel.querySelector(".keeps");
  if (old) old.remove();
  panel.appendChild(list);
}

/** The account's place in Settings. */
async function accountPanel() {
  await loadAccount();
  const panel = el("div", { className: "card-panel account-panel", attrs: { id: "account-panel" } }, [el("h3", { text: t("ui.account.section") })]);
  if (!ACCOUNT.username) {
    panel.appendChild(el("p", { className: "note", text: t("ui.account.prompt") }));
    panel.appendChild(
      el("div", { className: "loading-actions" }, [
        el("button", { className: "button play", attrs: { type: "button" }, text: t("ui.account.open"), onClick: () => openAccount(() => loadSettings()) }),
      ])
    );
    return panel;
  }
  const status = el("p", { className: "note", text: syncedText(ACCOUNT) });
  panel.appendChild(el("p", { className: "account-who", text: t("ui.account.signedin", ACCOUNT.username) }));
  panel.appendChild(status);
  const syncNow = el("button", { className: "button ghost", attrs: { type: "button" }, text: t("ui.account.syncnow") });
  syncNow.addEventListener("click", async () => {
    syncNow.disabled = true;
    status.textContent = t("ui.account.syncing");
    try {
      ACCOUNT = await ask("account_sync");
      await loadRows();
    } finally {
      status.textContent = syncedText(ACCOUNT);
      syncNow.disabled = false;
    }
  });
  const keeps = el("button", { className: "button ghost", attrs: { type: "button" }, text: t("ui.account.keeps"), onClick: () => openTransparency(panel) });
  const signOut = el("button", {
    className: "button ghost",
    attrs: { type: "button" },
    text: t("ui.account.signout"),
    onClick: async () => {
      await ask("account_logout");
      loadSettings();
    },
  });
  panel.appendChild(el("div", { className: "loading-actions" }, [syncNow, keeps, signOut]));

  // E-mail, optional, for signing in with it and for a reset.
  const emailBox = el("div", { className: "account-email" }, [el("h4", { text: t("ui.account.email") }), el("p", { className: "note", text: t("ui.account.emailtext") })]);
  const emailState = el("p", { className: "note", attrs: { role: "status" } });
  emailBox.appendChild(emailState);
  try {
    const me = await invoke("account_me");
    if (me.email) emailState.textContent = me.email.verified ? t("ui.account.emailverified", me.email.address) : t("ui.account.emailpending", me.email.address);
  } catch (error) {
    // Offline: the rest of the panel still works.
  }
  const address = accountField("acc-email", t("ui.account.emailaddress"), { type: "email", autocomplete: "email", spellcheck: "false" });
  const code = accountField("acc-code", t("ui.account.code"), { inputmode: "numeric", autocomplete: "one-time-code", maxlength: "6" });
  code.node.hidden = true;
  const verify = el("button", { className: "button ghost", attrs: { type: "button" }, text: t("ui.account.verify") });
  verify.hidden = true;
  const send = el("button", { className: "button ghost", attrs: { type: "button" }, text: t("ui.account.emailsend") });
  send.addEventListener("click", async () => {
    const value = address.input.value.trim();
    if (!value) return;
    try {
      await ask("account_email", { address: value });
      emailState.textContent = t("ui.account.emailsent", value);
      code.node.hidden = false;
      verify.hidden = false;
      code.input.focus();
    } catch (error) {
      // ask() has said why.
    }
  });
  verify.addEventListener("click", async () => {
    try {
      await ask("account_email_verify", { code: code.input.value.trim() });
      loadSettings();
    } catch (error) {
      code.node.querySelector(".field-error").textContent = String(error);
    }
  });
  emailBox.appendChild(address.node);
  emailBox.appendChild(el("div", { className: "loading-actions" }, [send]));
  emailBox.appendChild(code.node);
  emailBox.appendChild(el("div", { className: "loading-actions" }, [verify]));
  panel.appendChild(emailBox);

  // Deleting asks once more, on the page, and says what stays.
  const sure = el("div", { className: "account-delete", attrs: { hidden: "true" } }, [
    el("p", { className: "note", text: t("ui.account.deleteconfirm") }),
    el("div", { className: "loading-actions" }, [
      el("button", {
        className: "button danger",
        attrs: { type: "button" },
        text: t("ui.account.delete"),
        onClick: async () => {
          await ask("account_delete");
          loadSettings();
        },
      }),
    ]),
  ]);
  const remove = el("button", {
    className: "text-button quiet",
    attrs: { type: "button", "aria-expanded": "false" },
    text: t("ui.account.delete"),
    onClick: () => {
      sure.hidden = !sure.hidden;
      remove.setAttribute("aria-expanded", String(!sure.hidden));
    },
  });
  panel.appendChild(el("div", { className: "loading-actions" }, [remove]));
  panel.appendChild(sure);
  return panel;
}

// --------------------------------------------------------------- your genres

/** Rows of the genres this profile scores highest: the first catalogue that
 *  can be narrowed to each, narrowed to it. The addon's catalogue, labelled
 *  for what it is -- this program judges nothing "for you". */
async function addYourGenres() {
  let view;
  try {
    view = await invoke("profile");
  } catch (error) {
    return;
  }
  ROWS = ROWS.filter((row) => !row.personal);
  if (KIDS) return;
  const genres = view.liked_genres.slice(0, 2).map(([genre]) => genre);
  if (!genres.length) return;
  const catalogs = ROWS.filter((row) => row.kind === "catalog");
  const added = [];
  for (const genre of genres) {
    const base =
      catalogs.find((row) => row.content_type === "movie" && row.genres.includes(genre) && !row.genre_required) ||
      catalogs.find((row) => row.genres.includes(genre) && !row.genre_required);
    if (!base) continue;
    added.push({
      ...base,
      title: t("ui.row.yourgenre", genre),
      extra: ["genre", genre],
      items: [],
      loaded: false,
      exhausted: false,
      loading: null,
      personal: true,
      genreRow: false,
    });
  }
  if (!added.length) return;
  let at = ROWS.findIndex((row) => row.kind === "catalog");
  if (at === -1) at = ROWS.length;
  ROWS.splice(at, 0, ...added);
  if (!document.getElementById("view-home").hidden) renderHome();
}

/** The avatar's menu: who else can watch, and the pages that are this
 *  profile's -- the way a streaming service keeps them out of the top bar. */
function openAvatarMenu() {
  const items = [
    { icon: "user", label: t("ui.profiles.switchmenu"), run: () => openWho(false) },
    { icon: "star", label: t("ui.nav.profile"), run: () => go("profile") },
    { icon: "settings", label: t("ui.nav.settings"), run: () => go("settings") },
    { icon: "lock", label: t("ui.account.menu"), run: () => (ACCOUNT.username ? go("settings") : openAccount(null)) },
  ];
  openMenu(document.getElementById("who-open"), items);
}

// ------------------------------------------------------------ new episodes

let CALENDAR = null;

/** New episodes of the library's series, as a row after "Continue
 *  watching". Asked after the rest of the home screen, because it asks every
 *  series' addon; the host keeps the answer for half an hour. */
async function addNewEpisodes() {
  let found;
  try {
    found = await invoke("calendar", { fresh: false });
  } catch (error) {
    return;
  }
  CALENDAR = found;
  // The number on the calendar link is how many series have something new.
  const dot = document.getElementById("calendar-dot");
  dot.hidden = found.fresh.length === 0;
  dot.textContent = found.fresh.length ? String(found.fresh.length) : "";
  dot.setAttribute("aria-label", t("ui.calendar.newcount", found.fresh.length));
  ROWS = ROWS.filter((row) => row.kind !== "fresh");
  if (found.fresh.length === 0) return;
  const row = {
    kind: "fresh",
    title: t("ui.row.fresh"),
    items: found.fresh,
    loaded: true,
    exhausted: true,
    loading: null,
  };
  ROWS.splice(ROWS.findIndex((other) => other.kind === "continue") + 1, 0, row);
  // Drawn in place: the rest of the screen stays as it is.
  if (document.getElementById("view-home").hidden) return;
  const container = document.getElementById("rows");
  const at = rowsForFilter().indexOf(row);
  if (at === -1) return;
  container.insertBefore(slider(row), container.children[at] || null);
}

/** Today's date, as the addons write dates. */
function isoToday() {
  const now = new Date();
  return new Date(now.getTime() - now.getTimezoneOffset() * 60000).toISOString().slice(0, 10);
}

async function openCalendar() {
  showView("calendar");
  const list = document.getElementById("calendar-list");
  const note = document.getElementById("calendar-note");
  clear(list);
  note.hidden = true;
  list.appendChild(el("div", { className: "spinner", text: t("ui.loading") }));
  let found;
  try {
    found = await ask("calendar", { fresh: false });
  } catch (error) {
    clear(list);
    return;
  }
  CALENDAR = found;
  clear(list);
  if (found.failures.length) toast(found.failures.join("\n"));
  if (found.episodes.length === 0) {
    const rows = await ask("library", { order: "recent" });
    note.textContent = t(rows.some((item) => item.content_type === "series") ? "ui.calendar.empty" : "ui.calendar.nolibrary");
    note.hidden = false;
    return;
  }
  const today = isoToday();
  const dayName = new Intl.DateTimeFormat(MESSAGES.__locale || "en", { weekday: "long", day: "numeric", month: "long" });
  const section = (titleKey, episodes) => {
    if (episodes.length === 0) return;
    list.appendChild(el("h3", { className: "section-title", text: t(titleKey) }));
    let day = null;
    for (const episode of episodes) {
      if (episode.date !== day) {
        day = episode.date;
        const date = new Date(`${day}T12:00:00`);
        const label = day === today ? `${t("ui.calendar.today")} · ${dayName.format(date)}` : dayName.format(date);
        list.appendChild(el("h4", { className: "cal-day", text: label }));
      }
      const thumb = el("div", { className: "cal-thumb" });
      if (episode.poster) thumb.appendChild(image(episode.poster, ""));
      const state = episode.watched ? "ui.episode.watched" : episode.aired ? "ui.calendar.aired" : "ui.calendar.upcoming";
      list.appendChild(
        el(
          "button",
          {
            className: "cal-entry",
            attrs: { type: "button" },
            onClick: () => openDetail("series", episode.series_id),
          },
          [
            thumb,
            el("span", { className: "grow" }, [
              el("div", { className: "title", text: episode.series_name }),
              el("div", { className: "when", text: episode.title ? `${episode.label} · ${episode.title}` : episode.label }),
            ]),
            el("span", { className: `pill${episode.aired && !episode.watched ? " on" : ""}`, text: t(state) }),
          ]
        )
      );
    }
  };
  // What is coming, soonest first; then what aired, newest first.
  section("ui.calendar.coming", found.episodes.filter((episode) => !episode.aired));
  section(
    "ui.calendar.recent",
    found.episodes.filter((episode) => episode.aired).reverse()
  );
}

function rowsForFilter() {
  // A genre already shown as "Because you score ... high" is not shown again.
  const personal = new Set(ROWS.filter((row) => row.personal).map((row) => `${row.content_type}|${row.extra[1]}`));
  const genreType = FILTER === "home" ? "movie" : FILTER;
  return ROWS.filter((row) => {
    if (row.genreRow) return row.content_type === genreType && !personal.has(`${row.content_type}|${row.extra[1]}`);
    if (FILTER === "home") return true;
    return row.kind === "catalog"
      ? row.content_type === FILTER
      : row.items.some((item) => item.content_type === FILTER);
  });
}

// --------------------------------------------------------- recommended for you

/** "Recommended for you": ranked on this computer from what this profile
 *  watched, scored and saved (recommend.rs). Its first page is a row near
 *  the top; the rest is the feed at the end of the home screen, which goes
 *  on as long as the addons' catalogues do. */
const FEED = { started: false, page: 0, items: [], more: false, busy: false, token: 0 };

async function addForYou() {
  const token = (FEED.token += 1);
  let first;
  try {
    first = await invoke("recommend", { page: 0, fresh: true });
  } catch (error) {
    return;
  }
  if (token !== FEED.token) return;
  ROWS = ROWS.filter((row) => row.kind !== "foryou" && row.kind !== "language" && !row.genreRow);
  const inRow = first.items.slice(0, 20);
  if (inRow.length) {
    let at = ROWS.findIndex((row) => row.kind === "catalog");
    if (at === -1) at = ROWS.length;
    ROWS.splice(at, 0, { kind: "foryou", title: t("ui.row.foryou"), items: inRow, loaded: true, exhausted: true, loading: null });
  }
  if (first.language && first.in_language.length >= 4) {
    ROWS.push({
      kind: "language",
      title: t("ui.row.inlanguage", languageName(first.language)),
      items: first.in_language,
      loaded: true,
      exhausted: true,
      loading: null,
    });
  }
  ROWS.push(...genreRows(first.genres));
  FEED.started = true;
  FEED.page = 0;
  FEED.items = first.items.slice(inRow.length);
  FEED.more = first.more;
  if (!document.getElementById("view-home").hidden) renderHome();
}

/** A row per genre the catalogues offer, the genres this profile likes
 *  first. One row per genre and type; the home screen shows the films',
 *  the Series and Movies pages their own. */
function genreRows(liked) {
  if (KIDS) return [];
  const catalogs = ROWS.filter((row) => row.kind === "catalog" && !row.personal && !row.genreRow);
  const rows = [];
  for (const type of ["movie", "series"]) {
    const offered = [];
    for (const row of catalogs) {
      if (row.content_type !== type) continue;
      for (const genre of row.genres) if (!offered.includes(genre)) offered.push(genre);
    }
    const order = [
      ...liked.map((g) => offered.find((o) => o.toLowerCase() === g)).filter(Boolean),
      ...offered,
    ].filter((genre, at, all) => all.indexOf(genre) === at);
    for (const genre of order) {
      const base = catalogs.find((row) => row.content_type === type && row.genres.includes(genre));
      if (!base) continue;
      rows.push({
        ...base,
        title: genre,
        extra: ["genre", genre],
        items: [],
        loaded: false,
        exhausted: false,
        loading: null,
        genreRow: true,
      });
    }
  }
  return rows;
}

function languageName(code) {
  try {
    return new Intl.DisplayNames([MESSAGES.__locale || "en"], { type: "language" }).of(code) || code;
  } catch (error) {
    return code;
  }
}

/** The feed after the last row: a grid that asks for its next page when its
 *  end comes near, for as long as there is one. */
function feedSection() {
  const shows = (item) => FILTER === "home" || item.content_type === FILTER;
  const grid = el("div", { className: "grid feed-grid" });
  for (const item of FEED.items) if (shows(item)) grid.appendChild(card(item));
  const end = el("div", { className: "spinner feed-end", text: t("ui.loading") });
  end.hidden = !FEED.more;
  const section = el("section", { className: "feed", attrs: { "aria-label": t("ui.row.morefor") } }, [
    el("div", { className: "row-head" }, [el("h3", { text: t("ui.row.morefor") })]),
    grid,
    end,
  ]);
  const pump = () =>
    whenNear(end, async () => {
      if (!FEED.more || FEED.busy || !grid.isConnected) return;
      FEED.busy = true;
      const token = FEED.token;
      try {
        const next = await invoke("recommend", { page: FEED.page + 1, fresh: false });
        if (token !== FEED.token) return;
        FEED.page = next.page;
        FEED.more = next.more;
        const known = new Set(FEED.items.map((item) => `${item.content_type}:${item.id}`));
        const fresh = next.items.filter((item) => !known.has(`${item.content_type}:${item.id}`));
        FEED.items.push(...fresh);
        const cards = document.createDocumentFragment();
        for (const item of fresh) if (shows(item)) cards.appendChild(card(item));
        grid.appendChild(cards);
      } catch (error) {
        FEED.more = false;
      } finally {
        FEED.busy = false;
      }
      end.hidden = !FEED.more;
      if (FEED.more && grid.isConnected) pump();
    });
  if (FEED.more) pump();
  return section;
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
  if (FEED.started) container.appendChild(feedSection());
  // The billboard is chosen once the first catalogue has answered, from what
  // it offered: the same for every profile, so it is a place to find what is
  // new rather than more of the same.
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
    extra: row.extra || null,
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

  const kind = [typeName(row.content_type), row.extra ? row.extra[1] : null, row.addon_name]
    .filter(Boolean)
    .join(", ");
  const section = el("section", { className: "row", attrs: { "aria-label": row.title } }, [
    el("div", { className: "row-head" }, [
      el("h3", { text: row.title }),
      kind ? el("span", { className: "row-kind", text: kind }) : null,
      pages,
    ]),
    el("div", { className: "slider" }, [prev, track, next]),
  ]);

  const actions = row.kind === "continue" ? continueActions(row) : null;

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
    const cards = document.createDocumentFragment();
    for (const item of row.items) cards.appendChild(card(item, actions));
    track.appendChild(cards);
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
          for (const item of row.items.slice(before)) track.appendChild(card(item, actions));
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

  fill();
  // A row asks its addon for its first page when it comes near the screen,
  // not when the page is drawn: twenty rows answering at once is twenty
  // requests and several hundred posters decoding for a screen that shows
  // three of them.
  if (!row.loaded) whenNear(section, () => ensureRow(row).then(fill));
  return section;
}

/** Run `then` once, when `node` comes within a screen of the viewport. */
function whenNear(node, then) {
  if (!("IntersectionObserver" in window)) {
    then();
    return;
  }
  const watcher = new IntersectionObserver(
    (entries) => {
      if (entries.some((entry) => entry.isIntersecting)) {
        watcher.disconnect();
        then();
      }
    },
    { rootMargin: "100% 0px" }
  );
  watcher.observe(node);
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

/** The billboard turns through the first few titles of the first
 *  catalogue, one every twelve seconds -- never while it is pointed at or
 *  focused, never on another page or a hidden window, and not at all for
 *  someone who asked for reduced motion. Each title's backdrop is asked for
 *  once and remembered. */
let HERO_TIMER = 0;
const HERO_FEATURES = new Map();
const HERO_EVERY = 12000;

function scheduleHero(row, at, count) {
  clearTimeout(HERO_TIMER);
  if (count < 2 || window.matchMedia("(prefers-reduced-motion: reduce)").matches) return;
  HERO_TIMER = setTimeout(() => {
    const hero = document.getElementById("hero");
    const resting =
      document.getElementById("view-home").hidden ||
      document.hidden ||
      hero.matches(":hover") ||
      hero.contains(document.activeElement);
    if (resting) scheduleHero(row, at, count);
    else renderHero(row, (at + 1) % count);
  }, HERO_EVERY);
}

function drawHeroDots(row, at, count) {
  let dots = document.getElementById("hero-dots");
  if (!dots) {
    dots = el("div", { className: "hero-dots", attrs: { id: "hero-dots", role: "group", "aria-label": t("ui.hero.more") } });
    document.querySelector(".hero-body").appendChild(dots);
  }
  clear(dots);
  dots.hidden = count < 2;
  for (let n = 0; n < count; n += 1) {
    dots.appendChild(
      el("button", {
        className: n === at ? "on" : "",
        attrs: { type: "button", "aria-label": t("ui.hero.nth", n + 1, count), "aria-current": String(n === at) },
        onClick: () => renderHero(row, n),
      })
    );
  }
}

async function renderHero(row, at = 0) {
  const hero = document.getElementById("hero");
  const candidates = row.items.filter((candidate) => candidate.poster).slice(0, 5);
  const item = candidates[at] || row.items[0];
  if (!item) {
    hero.hidden = true;
    return;
  }
  drawHeroDots(row, at, candidates.length);
  scheduleHero(row, at, candidates.length);
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
  let detail = HERO_FEATURES.get(item.id) || null;
  if (!detail) {
    try {
      detail = await invoke("feature", { contentType: item.content_type, id: item.id });
      HERO_FEATURES.set(item.id, detail);
    } catch (error) {
      detail = null;
    }
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

let SEARCH_TOKEN = 0;

async function runSearch(query, typing = false) {
  if (!query.trim()) return;
  const token = (SEARCH_TOKEN += 1);
  // While typing, the page stays where it is: no jump to the top on every
  // letter.
  if (!typing || document.getElementById("view-search").hidden) showView("search");
  const grid = document.getElementById("search-grid");
  clear(grid);
  document.getElementById("search-heading").textContent = t("ui.search.heading", query);
  document.getElementById("search-correction").hidden = true;
  grid.appendChild(el("div", { className: "spinner", text: t("ui.loading") }));

  SEARCH_QUERY = query;
  let result;
  try {
    result = typing ? await invoke("search_page", { query, page: 0 }) : await ask("search_page", { query, page: 0 });
  } catch (error) {
    if (token === SEARCH_TOKEN) clear(grid);
    return;
  }
  // A slower answer to an earlier query is not drawn over a newer one.
  if (token !== SEARCH_TOKEN) return;
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
  drawPager(result);
}

let SEARCH_QUERY = "";

/** Page numbers under the results, as a search engine shows them. A page is
 *  read from the addons when it is opened, not before. */
function drawPager(result) {
  const pager = document.getElementById("search-pages");
  clear(pager);
  pager.hidden = result.known_pages < 2 && !result.more;
  if (pager.hidden) return;
  const go = (page) => openSearchPage(page);
  if (result.page > 0) {
    pager.appendChild(el("button", { className: "page-step", text: t("ui.search.previous"), attrs: { type: "button" }, onClick: () => go(result.page - 1) }));
  }
  const last = result.known_pages - 1;
  const from = Math.max(0, result.page - 4);
  const to = Math.min(last, from + 9);
  for (let n = from; n <= to; n += 1) {
    pager.appendChild(
      el("button", {
        className: `page-number${n === result.page ? " on" : ""}`,
        text: String(n + 1),
        attrs: { type: "button", "aria-current": n === result.page ? "page" : "false", "aria-label": t("ui.search.page", n + 1) },
        onClick: () => go(n),
      })
    );
  }
  if (result.more) {
    pager.appendChild(el("button", { className: "page-step", text: t("ui.search.next"), attrs: { type: "button" }, onClick: () => go(result.page + 1) }));
  }
}

async function openSearchPage(page) {
  const grid = document.getElementById("search-grid");
  const token = (SEARCH_TOKEN += 1);
  clear(grid);
  grid.appendChild(el("div", { className: "spinner", text: t("ui.loading") }));
  window.scrollTo({ top: 0 });
  let result;
  try {
    result = await ask("search_page", { query: SEARCH_QUERY, page });
  } catch (error) {
    clear(grid);
    return;
  }
  if (token !== SEARCH_TOKEN) return;
  clear(grid);
  if (result.items.length === 0) grid.appendChild(el("div", { className: "spinner", text: t("ui.search.none") }));
  const cards = document.createDocumentFragment();
  for (const item of result.items) cards.appendChild(card(item));
  grid.appendChild(cards);
  drawPager(result);
}

// ------------------------------------------------------------------ discover

/** Where Discover is: the choices, how far the grid has loaded, and whether
 *  more is coming. Kept when the page is left, so going back to it from a
 *  title is going back to where one was. */
const DISCOVER = { type: null, row: null, genre: "", skip: 0, done: false, busy: false, token: 0, built: false, rows: [] };

function discoverRows() {
  return ROWS.filter((row) => row.kind === "catalog" && !row.personal && !row.genreRow);
}

async function openDiscover() {
  showView("discover");
  if (!ROWS.length) await loadRows();
  if (DISCOVER.built) return;
  DISCOVER.built = true;
  const rows = discoverRows();
  DISCOVER.rows = rows;
  const empty = document.getElementById("discover-empty");
  if (rows.length === 0) {
    empty.textContent = t("ui.discover.empty");
    empty.hidden = false;
    return;
  }
  empty.hidden = true;
  const typeSelect = document.getElementById("discover-type");
  const catalogSelect = document.getElementById("discover-catalog");
  const genreSelect = document.getElementById("discover-genre");

  const types = [...new Set(rows.map((row) => row.content_type))];
  // Films, then series, then whatever else the addons offer.
  const rank = (type) => {
    const at = ["movie", "series"].indexOf(type);
    return at === -1 ? 9 : at;
  };
  types.sort((a, b) => rank(a) - rank(b));
  clear(typeSelect);
  for (const type of types) typeSelect.appendChild(el("option", { text: typeName(type), attrs: { value: type } }));

  const fillCatalogs = () => {
    clear(catalogSelect);
    rows
      .filter((row) => row.content_type === typeSelect.value)
      .forEach((row) => {
        catalogSelect.appendChild(el("option", { text: row.title, attrs: { value: String(rows.indexOf(row)) } }));
      });
  };
  const fillGenres = () => {
    const row = DISCOVER.rows[Number(catalogSelect.value)];
    clear(genreSelect);
    if (!row.genre_required) genreSelect.appendChild(el("option", { text: t("ui.sources.any"), attrs: { value: "" } }));
    for (const genre of row.genres) genreSelect.appendChild(el("option", { text: genre, attrs: { value: genre } }));
    // A catalogue that cannot be narrowed has nothing to choose here.
    genreSelect.closest(".pick").hidden = row.genres.length === 0;
  };

  typeSelect.addEventListener("change", () => {
    fillCatalogs();
    fillGenres();
    restartDiscover();
  });
  catalogSelect.addEventListener("change", () => {
    fillGenres();
    restartDiscover();
  });
  genreSelect.addEventListener("change", restartDiscover);

  fillCatalogs();
  fillGenres();
  DISCOVER.fillCatalogs = fillCatalogs;
  DISCOVER.fillGenres = fillGenres;

  // The next page when the end of the grid comes near.
  const observer = new IntersectionObserver(
    (entries) => {
      if (entries.some((entry) => entry.isIntersecting)) moreDiscover();
    },
    { rootMargin: "600px 0px" }
  );
  observer.observe(document.getElementById("discover-more"));
  restartDiscover();
}

/** Discover, opened on one genre of one type: the first catalogue of that
 *  type that can be narrowed to it. */
async function openDiscoverAt(type, genre) {
  await openDiscover();
  if (!DISCOVER.fillCatalogs) return;
  const typeSelect = document.getElementById("discover-type");
  const catalogSelect = document.getElementById("discover-catalog");
  const genreSelect = document.getElementById("discover-genre");
  if (![...typeSelect.options].some((option) => option.value === type)) return;
  typeSelect.value = type;
  DISCOVER.fillCatalogs();
  const fits = [...catalogSelect.options].find((option) => DISCOVER.rows[Number(option.value)].genres.includes(genre));
  if (fits) catalogSelect.value = fits.value;
  DISCOVER.fillGenres();
  if ([...genreSelect.options].some((option) => option.value === genre)) genreSelect.value = genre;
  restartDiscover();
}

function restartDiscover() {
  const row = DISCOVER.rows[Number(document.getElementById("discover-catalog").value)];
  DISCOVER.row = row;
  DISCOVER.genre = document.getElementById("discover-genre").value;
  DISCOVER.skip = 0;
  DISCOVER.done = false;
  DISCOVER.busy = false;
  DISCOVER.token += 1;
  DISCOVER.seen = new Set();
  clear(document.getElementById("discover-grid"));
  moreDiscover();
}

async function moreDiscover() {
  const row = DISCOVER.row;
  if (!row || DISCOVER.busy || DISCOVER.done) return;
  DISCOVER.busy = true;
  const token = DISCOVER.token;
  const grid = document.getElementById("discover-grid");
  const waiting = el("div", { className: "spinner", text: t("ui.loading") });
  grid.appendChild(waiting);
  let items = [];
  try {
    items = await invoke("row_items", {
      addonId: row.addon_id,
      contentType: row.content_type,
      catalogId: row.catalog_id,
      skip: DISCOVER.skip,
      query: null,
      extra: DISCOVER.genre ? ["genre", DISCOVER.genre] : row.extra || null,
    });
  } catch (error) {
    if (token === DISCOVER.token) toast(error);
    items = [];
  }
  waiting.remove();
  if (token !== DISCOVER.token) return;
  DISCOVER.busy = false;
  // An addon that ignores `skip` sends the first page again: nothing new
  // means the end.
  const fresh = items.filter((item) => !DISCOVER.seen.has(item.id));
  for (const item of fresh) DISCOVER.seen.add(item.id);
  DISCOVER.skip += items.length;
  if (fresh.length === 0) {
    DISCOVER.done = true;
    if (grid.children.length === 0) grid.appendChild(el("div", { className: "spinner", text: t("ui.discover.nothing") }));
    return;
  }
  const cards = document.createDocumentFragment();
  for (const item of fresh) cards.appendChild(card(item));
  grid.appendChild(cards);
  // A short page may not reach the strip that asks for more: ask now.
  const strip = document.getElementById("discover-more").getBoundingClientRect();
  if (strip.top < window.innerHeight + 600) moreDiscover();
}

/** What the library page shows: which kind, in which order. */
const LIBRARY = { kind: "", order: "recent" };

async function openLibrary() {
  showView("library");
  const grid = document.getElementById("library-grid");
  const kinds = document.getElementById("library-kinds");
  const order = document.getElementById("library-order");
  clear(grid);
  const items = await ask("library", { order: LIBRARY.order });

  clear(order);
  for (const [value, key] of [
    ["recent", "ui.library.recent"],
    ["name", "ui.library.byname"],
  ]) {
    order.appendChild(el("option", { text: t(key), attrs: { value } }));
  }
  order.value = LIBRARY.order;
  order.onchange = () => {
    LIBRARY.order = order.value;
    openLibrary();
  };

  // One chip per kind actually kept, and "any".
  clear(kinds);
  const present = [...new Set(items.map((item) => item.content_type))];
  if (!present.includes(LIBRARY.kind)) LIBRARY.kind = "";
  const chip = (value, text) => {
    const button = el("button", {
      className: "chip",
      text,
      attrs: { type: "button", "aria-pressed": String(LIBRARY.kind === value) },
      onClick: () => {
        LIBRARY.kind = value;
        openLibrary();
      },
    });
    kinds.appendChild(button);
  };
  chip("", t("ui.sources.any"));
  if (present.length > 1) for (const kind of present) chip(kind, typeName(kind));
  kinds.hidden = present.length < 2;

  const shown = items.filter((item) => !LIBRARY.kind || item.content_type === LIBRARY.kind);
  if (shown.length === 0) {
    grid.appendChild(el("div", { className: "spinner", text: t("ui.library.empty") }));
    return;
  }
  const cards = document.createDocumentFragment();
  for (const item of shown) cards.appendChild(card(item));
  grid.appendChild(cards);
}

/** New & popular: the catalogues that call themselves trending, then the
 *  ones that call themselves new. Which those are is the addons' word, read
 *  from their catalogue ids and names on the host -- this program does not
 *  pick titles itself. */
async function openNew() {
  showView("new");
  const container = document.getElementById("new-rows");
  clear(container);
  if (ROWS.length === 0) await loadRows();
  const sections = [
    ["trending", "ui.new.trending"],
    ["new", "ui.new.recent"],
  ];
  let any = false;
  for (const [role, title] of sections) {
    const rows = ROWS.filter((row) => row.role === role);
    if (rows.length === 0) continue;
    any = true;
    container.appendChild(el("h2", { className: "section-title new-heading", text: t(title) }));
    for (const row of rows) container.appendChild(slider(row));
  }
  document.getElementById("new-empty").hidden = any;
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
  if (detail.imdb_rating) facts.appendChild(el("span", { className: "imdb", text: `IMDb ${detail.imdb_rating}` }));
  for (const fact of [detail.year, detail.runtime]) {
    if (fact) facts.appendChild(el("span", { text: fact }));
  }
  // A genre is a way in: Discover, narrowed to it.
  for (const genre of detail.genres.slice(0, 4)) {
    facts.appendChild(
      el("button", {
        className: "pill",
        text: genre,
        attrs: { type: "button", title: t("ui.detail.browsegenre", genre) },
        onClick: () => openDiscoverAt(detail.content_type, genre),
      })
    );
  }

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
  // Somewhere to resume from is also somewhere not to: the beginning.
  if (detail.resume_at) {
    const over = el("button", {
      className: "button ghost",
      attrs: { type: "button" },
      onClick: () => playFromDetail(true),
    });
    over.appendChild(icon("refresh"));
    over.appendChild(el("span", { text: t("ui.action.startover") }));
    actions.appendChild(over);
  }

  if (detail.has_trailer) {
    const trailer = el("button", {
      className: "button ghost",
      attrs: { type: "button" },
      onClick: async () => {
        const where = await ask("trailer");
        if (where === "browser") toast(t("ui.detail.trailerbrowser"));
      },
    });
    trailer.appendChild(icon("play"));
    trailer.appendChild(el("span", { text: t("ui.detail.trailer") }));
    actions.appendChild(trailer);
  }

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
    ratingBox(detail),
  ]);
  if (detail.cast.length || detail.director.length) {
    // Each name searches every addon for it.
    const people = (key, names) => {
      const line = el("div", {}, [el("span", { className: "credit-label", text: t(key) })]);
      names.forEach((name, at) => {
        if (at > 0) line.appendChild(document.createTextNode(", "));
        line.appendChild(
          el("button", {
            className: "name-link",
            text: name,
            attrs: { type: "button", title: t("ui.detail.searchname", name) },
            // With a community server, a name is a page of its own;
            // without one, it searches every addon for it.
            onClick: () => (COMMUNITY.server ? openPerson(name) : runSearch(name)),
          })
        );
      });
      return line;
    };
    const credits = el("div", { className: "credits" });
    if (detail.cast.length) credits.appendChild(people("ui.detail.castlabel", detail.cast.slice(0, 5)));
    if (detail.director.length) credits.appendChild(people("ui.detail.directorlabel", detail.director.slice(0, 3)));
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

  // More of the same genre, from an addon catalogue narrowed to it -- named
  // for what it is, not "like this": nothing here judges likeness.
  communityBox(detail, sections);
  const more = el("section", { className: "more-like" });
  sections.appendChild(more);
  invoke("more_like", { contentType: detail.content_type, id: detail.id })
    .then((found) => {
      if (!found || found.items.length === 0 || DETAIL !== detail) return;
      more.appendChild(el("h2", { className: "section-title", text: t("ui.detail.more", found.genre) }));
      const grid = el("div", { className: "grid" });
      for (const item of found.items.slice(0, 18)) grid.appendChild(card(item));
      more.appendChild(grid);
    })
    .catch(() => more.remove());
}

/** The viewer's own score of a title, out of ten, and an optional review.
 *  Kept on this machine; nothing here is sent anywhere. */
function ratingBox(detail) {
  const box = el("div", { className: "my-rating" });
  const stars = el("div", { className: "stars", attrs: { role: "radiogroup", "aria-label": t("ui.rating.yours") } });
  const said = el("span", { className: "rating-said" });
  const write = el("button", { className: "text-button", attrs: { type: "button" } });
  const remove = el("button", { className: "text-button quiet", attrs: { type: "button" }, text: t("ui.rating.remove") });
  const editor = el("div", { className: "review-editor" });
  editor.hidden = true;
  const area = el("textarea", {
    attrs: { rows: "4", maxlength: "5000", "aria-label": t("ui.rating.review"), placeholder: t("ui.rating.placeholder") },
  });
  const save = el("button", { className: "button play", attrs: { type: "button" }, text: t("ui.rating.save") });
  editor.appendChild(area);
  editor.appendChild(save);
  let current = null;

  const store = async (score, review) => {
    try {
      current = await invoke("rating_set", { contentType: detail.content_type, id: detail.id, score, review });
    } catch (error) {
      toast(error);
      return;
    }
    draw();
    shareRating(current, detail);
  };
  const draw = () => {
    const score = current ? current.score : 0;
    clear(stars);
    for (let n = 1; n <= 10; n += 1) {
      const star = el("button", {
        className: `star${n <= score ? " on" : ""}`,
        attrs: { type: "button", role: "radio", "aria-checked": String(n === score), "aria-label": t("ui.rating.give", n), title: String(n) },
        onClick: () => store(n, current ? current.review : null),
      });
      star.appendChild(icon("star"));
      stars.appendChild(star);
    }
    said.textContent = score ? t("ui.rating.yourscore", score) : t("ui.rating.prompt");
    write.textContent = current && current.review ? t("ui.rating.edit") : t("ui.rating.write");
    write.hidden = !current;
    remove.hidden = !current;
    area.value = current && current.review ? current.review : "";
  };
  write.addEventListener("click", () => {
    editor.hidden = !editor.hidden;
    if (!editor.hidden) area.focus();
  });
  save.addEventListener("click", async () => {
    await store(current ? current.score : 7, area.value);
    editor.hidden = true;
    toast(t("ui.rating.saved"));
  });
  remove.addEventListener("click", () => store(null, null));

  box.appendChild(el("div", { className: "rating-line" }, [stars, said, write, remove]));
  box.appendChild(editor);
  invoke("rating_get", { contentType: detail.content_type, id: detail.id })
    .then((found) => {
      current = found;
      draw();
    })
    .catch(() => draw());
  return box;
}

// ----------------------------------------------------------------- downloads

let DOWNLOADS_TIMER = 0;

async function openDownloads() {
  showView("downloads");
  clearTimeout(DOWNLOADS_TIMER);
  const list = document.getElementById("download-list");
  let items;
  try {
    items = await invoke("downloads");
  } catch (error) {
    toast(error);
    return;
  }
  if (document.getElementById("view-downloads").hidden) return;
  clear(list);
  if (!items.length) {
    list.appendChild(el("div", { className: "spinner", text: t("ui.download.empty") }));
    return;
  }
  for (const item of items) {
    const thumb = el("div", { className: "cal-thumb" });
    if (item.poster) thumb.appendChild(image(item.poster, ""));
    const meter = el("div", { className: "meter" }, [el("i")]);
    meter.firstChild.style.width = `${item.percent}%`;
    const state = item.done
      ? t("ui.download.ready", item.length ? bytes(item.length) : "")
      : t("ui.download.progress", item.percent, bytes(item.down_bps));
    const playButton = el("button", {
      className: "button play",
      attrs: { type: "button" },
      onClick: async () => {
        showLoading(item.name, item.poster, t("ui.job.starting"));
        try {
          await ask("download_play", { hex: item.hex });
        } catch (error) {
          hideLoading();
        }
      },
    });
    playButton.appendChild(icon("play"));
    playButton.appendChild(el("span", { text: t("ui.action.play") }));
    playButton.disabled = !item.done;
    const remove = el("button", {
      className: "text-button quiet",
      attrs: { type: "button" },
      text: t("ui.download.remove"),
      onClick: async () => {
        await ask("download_remove", { hex: item.hex });
        openDownloads();
      },
    });
    list.appendChild(
      el("div", { className: "cal-entry download" }, [
        thumb,
        el("span", { className: "grow" }, [
          el("div", { className: "title", text: item.name }),
          el("div", { className: "when", text: state }),
          item.done ? null : meter,
        ]),
        playButton,
        remove,
      ])
    );
  }
  // While anything is still coming in, the page keeps itself current.
  if (items.some((item) => !item.done)) DOWNLOADS_TIMER = setTimeout(openDownloads, 2000);
}

// ------------------------------------------------------------------- profile

const PROFILE = { order: "recent" };

async function openProfile() {
  showView("profile");
  const stats = document.getElementById("profile-stats");
  const grid = document.getElementById("profile-grid");
  const order = document.getElementById("profile-order");
  clear(stats);
  clear(grid);
  const view = await ask("profile");

  if (view.count === 0) {
    grid.appendChild(el("div", { className: "spinner", text: t("ui.profile.empty") }));
    order.closest(".discover-bar").hidden = true;
    return;
  }
  order.closest(".discover-bar").hidden = false;

  // The numbers, and the shape of the scores.
  const number = (value, key) => el("div", { className: "stat" }, [el("strong", { text: value }), el("span", { text: t(key) })]);
  stats.appendChild(
    el("div", { className: "stat-row" }, [
      number(String(view.count), "ui.profile.count"),
      number(view.average === null ? "—" : view.average.toFixed(1), "ui.profile.average"),
      number(String(view.reviews), "ui.profile.reviews"),
    ])
  );
  const most = Math.max(1, ...view.histogram);
  const bars = el("div", { className: "histogram", attrs: { role: "img", "aria-label": t("ui.profile.histogram") } });
  view.histogram.forEach((n, at) => {
    const fill = el("i");
    fill.style.height = `${Math.round((n / most) * 100)}%`;
    const bar = el("div", { className: "hist-bar" }, [fill, el("span", { text: String(at + 1) })]);
    bar.title = t("ui.profile.bar", at + 1, n);
    bars.appendChild(bar);
  });
  stats.appendChild(bars);
  if (view.liked_genres.length) {
    const chips = el("div", { className: "chips" });
    for (const [genre, n] of view.liked_genres) {
      chips.appendChild(
        el("button", {
          className: "chip",
          attrs: { type: "button" },
          text: `${genre} · ${n}`,
          onClick: () => openDiscoverAt("movie", genre),
        })
      );
    }
    stats.appendChild(el("div", { className: "liked" }, [el("span", { className: "filter-name", text: t("ui.profile.liked") }), chips]));
  }

  clear(order);
  for (const [value, key] of [
    ["recent", "ui.profile.recent"],
    ["high", "ui.profile.high"],
    ["low", "ui.profile.low"],
  ]) {
    order.appendChild(el("option", { text: t(key), attrs: { value } }));
  }
  order.value = PROFILE.order;
  order.onchange = () => {
    PROFILE.order = order.value;
    openProfile();
  };
  const items = [...view.items];
  if (PROFILE.order === "high") items.sort((a, b) => b.score - a.score || b.at - a.at);
  if (PROFILE.order === "low") items.sort((a, b) => a.score - b.score || b.at - a.at);
  const cards = document.createDocumentFragment();
  for (const item of items) {
    const shown = card({
      id: item.id,
      content_type: item.content_type,
      name: item.name,
      poster: item.poster,
      year: item.year,
      rating: String(item.score),
    });
    if (item.review) shown.title = item.review;
    cards.appendChild(shown);
  }
  grid.appendChild(cards);
}

function backButton() {
  const button = iconButton("arrowLeft", t("ui.action.back"), () => go(LAST), "icon-button back-button");
  return button;
}

/** The Play button on a detail page: the first source the list shows, which
 *  is the best one the viewer's preferences and filters allow. */
async function playFromDetail(fromStart = false) {
  if (!DETAIL) return;
  if (DETAIL.episodes.length > 0) {
    const next =
      DETAIL.episodes.find((episode) => episode.progress && !episode.finished) ||
      DETAIL.episodes.find((episode) => !episode.finished && (episode.season || 0) > 0) ||
      DETAIL.episodes[0];
    await loadSources("series", next.id, episodeName(DETAIL, next));
  } else if (SOURCES.subject !== DETAIL.id) {
    await loadSources(DETAIL.content_type, DETAIL.id, DETAIL.name);
  }
  const shown = sortedSources(filteredSources());
  if (shown.length === 0) {
    toast(t("ui.detail.nosources"));
    return;
  }
  play(shown[0].index, SOURCES.name, DETAIL.background || DETAIL.poster, fromStart);
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
  SOURCES = sourceState(result.sources, id, name);
  play(result.sources[0].index, name, art);
}

function episodeLabel(episode) {
  return episode.season !== null && episode.episode !== null
    ? `S${String(episode.season).padStart(2, "0")}E${String(episode.episode).padStart(2, "0")}`
    : episode.title;
}

/** What the player and the loading screen call an episode: the series, the
 *  number, and the episode's own name when there is one. */
function episodeName(detail, episode) {
  const label = episodeLabel(episode);
  return episode.named && episode.title !== label
    ? `${detail.name} · ${label} · ${episode.title}`
    : `${detail.name} · ${label}`;
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
            for (const other of list.querySelectorAll(".episode")) other.removeAttribute("aria-current");
            row.setAttribute("aria-current", "true");
            loadSources("series", episode.id, episodeName(detail, episode));
          },
        },
        [
          el("span", { className: "no", text: episode.episode !== null ? episode.episode : "" }),
          thumb,
          // The episode's own name when the addon gives one; otherwise the
          // season and episode number, once rather than twice.
          el("span", { className: "grow" }, [
            el("div", { className: "title", text: episode.named ? episode.title : label, title: episode.title }),
            episode.named ? el("div", { className: "when", text: label }) : null,
            episode.overview ? el("p", { className: "synopsis", text: episode.overview }) : null,
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
      // Watched or not, by hand: for an episode seen somewhere else, or one
      // started by mistake.
      const mark = el("button", {
        className: "round mark",
        attrs: {
          type: "button",
          "aria-pressed": String(Boolean(episode.finished)),
          "aria-label": t(episode.finished ? "ui.episode.unmark" : "ui.episode.mark"),
          title: t(episode.finished ? "ui.episode.unmark" : "ui.episode.mark"),
        },
        onClick: async () => {
          const watched = await ask("watched_set", {
            contentType: "series",
            id: episode.id,
            watched: !episode.finished,
          });
          episode.finished = watched;
          episode.progress = watched ? 1 : null;
          draw(season);
        },
      });
      mark.appendChild(icon("check"));
      list.appendChild(el("div", { className: "episode-line" }, [row, mark]));
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
  const token = (SOURCES_TOKEN += 1);
  clear(container);
  container.appendChild(el("h2", { className: "section-title", text: t("ui.detail.sources") }));
  const waiting = el("div", { className: "spinner", text: t("ui.loading") });
  container.appendChild(waiting);
  if (contentType === "series") container.scrollIntoView({ behavior: "smooth", block: "start" });

  // Not through `ask`: a list that was overtaken by a newer one is not an
  // error worth a toast, it is simply not drawn.
  let result;
  try {
    result = await invoke("sources", { contentType, id, name });
  } catch (error) {
    if (token === SOURCES_TOKEN) {
      waiting.remove();
      toast(error);
    }
    return;
  }
  if (token !== SOURCES_TOKEN) return;
  SOURCES = sourceState(result.sources, id, name);
  waiting.remove();

  if (result.failures.length) toast(result.failures.join("\n"));
  if (result.sources.length === 0) {
    container.appendChild(el("div", { className: "spinner", text: t("ui.detail.nosources") }));
    return;
  }

  // Best first, as the host ranked the buckets.
  QUALITY_ORDER = result.qualities.map(([label]) => label);

  const filters = el("div", { className: "filters" });
  const bar = el("div", { className: "count-line" });
  const count = el("span", { className: "grow" });
  const reset = el("button", {
    className: "text-button",
    attrs: { type: "button" },
    text: t("ui.sources.clear"),
    onClick: () => {
      for (const set of Object.values(SOURCES.pick)) set.clear();
      update();
    },
  });
  const sort = el("select", { className: "sort-select", attrs: { "aria-label": t("ui.sources.sort") } });
  for (const [value, key] of SORTS) sort.appendChild(el("option", { text: t(key), attrs: { value } }));
  sort.addEventListener("change", () => {
    SOURCES.sort = sort.value;
    drawList();
  });
  bar.appendChild(count);
  bar.appendChild(reset);
  bar.appendChild(el("label", { className: "sort" }, [el("span", { text: t("ui.sources.sort") }), sort]));
  const list = el("div");
  // What the viewer's subtitle addons have for this, asked beside the list
  // rather than inside it: a subtitle addon matches the film, not the
  // release, so this holds for whichever source is picked.
  const subsLine = el("p", { className: "subs-line" });
  container.appendChild(subsLine);
  container.appendChild(filters);
  container.appendChild(bar);
  container.appendChild(list);
  invoke("subtitle_languages", { contentType, id })
    .then((found) => {
      if (token !== SOURCES_TOKEN) return;
      if (found.addons === 0) subsLine.textContent = t("ui.sources.subsnoaddon");
      else if (found.languages.length === 0) subsLine.textContent = t("ui.sources.subsnone");
      else
        subsLine.textContent = t(
          "ui.sources.subsfound",
          found.languages
            .slice(0, 8)
            .map(([code, n]) => `${languageName(code)} (${n})`)
            .join(", ")
        );
    })
    .catch(() => subsLine.remove());

  // Three filters, always all three, in this order: quality, the language
  // it is dubbed in, the language of its subtitles. Each offers only values
  // the list holds. The number on a value is how many sources would show if
  // it were picked, given what the *other* filters already say -- so it
  // changes as they do, and a value that would empty the list reads 0 and
  // cannot be pressed.
  const chipsOf = {};
  const group = (facet, title, options) => {
    const chips = el("div", { className: "chips", attrs: { role: "group", "aria-label": title } });
    const any = filterChip(t("ui.sources.any"), () => {
      SOURCES.pick[facet].clear();
      update();
    });
    any.dataset.any = "true";
    chips.appendChild(any);
    for (const [value, label] of options) {
      const chip = filterChip(label, () => {
        const picked = SOURCES.pick[facet];
        if (picked.has(value)) picked.delete(value);
        else picked.add(value);
        update();
      });
      chip.dataset.value = value;
      chips.appendChild(chip);
    }
    chipsOf[facet] = chips;
    filters.appendChild(el("div", { className: "filter" }, [el("span", { className: "filter-name", text: title }), chips]));
  };

  const quality = QUALITY_ORDER.map((label) => [label, label === "—" ? t("ui.sources.unknown") : label]);
  const audio = valuesOf("audio").map((code) => [code, code === "" ? t("ui.sources.unknown") : languageName(code)]);
  const subs = valuesOf("subs").map((code) => [
    code,
    code === "?" ? t("ui.sources.subs") : code === "" ? t("ui.sources.unknown") : languageName(code),
  ]);
  group("quality", t("ui.sources.quality"), quality);
  group("audio", t("ui.sources.audio"), audio);
  group("subs", t("ui.sources.subtitles"), subs);

  const drawList = () => {
    clear(list);
    const shown = sortedSources(filteredSources());
    count.textContent = t("ui.sources.shown", shown.length, SOURCES.all.length);
    if (shown.length === 0) {
      list.appendChild(el("div", { className: "spinner", text: t("ui.sources.nomatch") }));
    }
    // Built off-document and attached once: one layout for the whole list
    // rather than one per row.
    const rows = document.createDocumentFragment();
    for (const source of shown.slice(0, 200)) rows.appendChild(sourceRow(source));
    list.appendChild(rows);
  };

  function update() {
    for (const facet of Object.keys(FACETS)) {
      const picked = SOURCES.pick[facet];
      // Every source the other filters let through: what this filter's
      // numbers are counted over.
      const base = SOURCES.all.filter((source) => passes(source, facet));
      const counts = new Map();
      for (const source of base) {
        for (const value of FACETS[facet](source)) counts.set(value, (counts.get(value) || 0) + 1);
      }
      for (const chip of chipsOf[facet].children) {
        if (chip.dataset.any) {
          setChip(chip, picked.size === 0, base.length, false);
        } else {
          const n = counts.get(chip.dataset.value) || 0;
          const on = picked.has(chip.dataset.value);
          setChip(chip, on, n, n === 0 && !on);
        }
      }
    }
    reset.hidden = Object.values(SOURCES.pick).every((set) => set.size === 0);
    drawList();
  }

  update();
}

/** The orders a source list can be put in. "Best match" is the host's
 *  ranking against the viewer's preferences, and the default. */
const SORTS = [
  ["best", "ui.sort.best"],
  ["quality", "ui.sort.quality"],
  ["fastest", "ui.sort.fastest"],
  ["seeders", "ui.sort.seeders"],
  ["largest", "ui.sort.largest"],
  ["smallest", "ui.sort.smallest"],
];

/** Quality buckets, best first, as the host ranked them. */
let QUALITY_ORDER = [];

/** What each filter reads off a source. A source that states nothing for a
 *  filter is filed under "" -- "not stated" is a value someone can pick --
 *  and one that says it has subtitles without saying in what under "?". */
const FACETS = {
  quality: (source) => [source.quality],
  audio: (source) => (source.languages.length ? source.languages : [""]),
  subs: (source) =>
    source.subtitle_languages.length ? source.subtitle_languages : source.has_subtitles ? ["?"] : [""],
};

function filterChip(label, onClick) {
  const chip = el("button", { className: "chip", attrs: { type: "button", "aria-pressed": "false" }, onClick });
  chip.appendChild(document.createTextNode(label));
  chip.appendChild(el("small"));
  return chip;
}

function setChip(chip, on, n, off) {
  chip.setAttribute("aria-pressed", String(on));
  chip.disabled = off;
  chip.lastChild.textContent = String(n);
}

/** Every value a filter can take over the whole list, most common first and
 *  the vague ones last. */
function valuesOf(facet) {
  const counts = new Map();
  for (const source of SOURCES.all) {
    for (const value of FACETS[facet](source)) counts.set(value, (counts.get(value) || 0) + 1);
  }
  const rank = (value) => (value === "" ? 2 : value === "?" ? 1 : 0);
  return [...counts.entries()]
    .sort((a, b) => rank(a[0]) - rank(b[0]) || b[1] - a[1])
    .map(([value]) => value);
}

/** Whether a source gets through every filter -- except `skip`, when one is
 *  being counted. Within one filter, any picked value will do. */
function passes(source, skip = null) {
  for (const facet of Object.keys(FACETS)) {
    if (facet === skip) continue;
    const picked = SOURCES.pick[facet];
    if (picked.size === 0) continue;
    if (!FACETS[facet](source).some((value) => picked.has(value))) return false;
  }
  return true;
}

function filteredSources() {
  return SOURCES.all.filter((source) => passes(source));
}

/** In the chosen order. Ties, and sources that do not state what is sorted
 *  on, keep the host's ranking -- so the best match is always next. */
function sortedSources(sources) {
  const missingLast = (value, direction) =>
    value === null || value === undefined ? Infinity : direction * value;
  const key = {
    best: () => 0,
    quality: (source) => {
      const at = QUALITY_ORDER.indexOf(source.quality);
      return at === -1 ? Infinity : at;
    },
    seeders: (source) => missingLast(source.seeders, -1),
    // Measured speed when the source played before, otherwise an estimate
    // from its seeders; a direct link counts as fast.
    fastest: (source) => -(source.speed_bps || 0),
    largest: (source) => missingLast(source.size_bytes, -1),
    smallest: (source) => missingLast(source.size_bytes, 1),
  }[SOURCES.sort || "best"];
  return [...sources].sort((a, b) => {
    const ka = key(a);
    const kb = key(b);
    if (ka !== kb) return ka < kb ? -1 : 1;
    return a.index - b.index;
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
  if (source.measured_bps) tag(t("ui.sources.measured", bytes(source.measured_bps)), "speed");
  if (source.seeders !== null && source.seeders !== undefined) tag(t("ui.sources.seeders", source.seeders));
  if (source.size) tag(source.size);
  if (source.is_torrent) tag(t("ui.sources.torrent"));
  if (source.needs_headers) tag(t("ui.sources.headers"));

  const row = el(
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
  if (!source.is_torrent) return row;
  // Download to watch later: a torrent only, since it is fetched whole.
  const keep = iconButton("download", t("ui.download.this"), async () => {
    keep.disabled = true;
    toast(t("ui.download.started"));
    try {
      await invoke("download", { index: source.index });
    } catch (error) {
      toast(error);
    }
    keep.disabled = false;
  }, "icon-button source-download");
  return el("div", { className: "source-line" }, [row, keep]);
}

// --------------------------------------------------------------- playing it

async function play(index, name, art, fromStart = false) {
  showLoading(name, art, t("ui.job.starting"));
  try {
    await ask("play", { index, file: null, fromStart });
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
      { path: "playback.subtitleScale", kind: "number", options: [0.75, 1, 1.25, 1.5, 2], percent: true },
      { path: "playback.autoplayNext", kind: "boolean" },
      { path: "playback.startMuted", kind: "boolean" },
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
  await loadProfiles();
  languagePanel.appendChild(row(t("ui.window.title"), windowPicker(PROFILES.window || "maximized", true)));
  form.appendChild(languagePanel);
  form.appendChild(await accountPanel());

  for (const section of SETTINGS_SECTIONS) {
    const panel = el("div", { className: "card-panel" }, [el("h3", { text: t(section.title) })]);
    for (const field of section.fields) {
      const current = stored(view.settings, field);
      panel.appendChild(row(t(`ui.settings.${field.path}`), control(field, current, languages)));
    }
    if (section.title === "ui.settings.section.torrent") {
      panel.appendChild(cacheBox(view));
      panel.appendChild(bufferBox(view.buffer_bytes));
    }
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
    const named = (value) =>
      field.percent ? t("ui.unit.percent", Math.round(value * 100)) : field.unit ? t(field.unit, value) : String(value);
    for (const value of values) option(String(value), named(value));
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
/** The cache limit: how much disk the torrent cache may use. Only sizes the
 *  disk can hold are offered, and what is free, what is used and the limit
 *  in force are shown beside it. */
function cacheBox(view) {
  const GB = 1024 * 1024 * 1024;
  const current = view.settings.torrent.cacheLimitMegabytes || 0;
  const free = view.disk_free_bytes;
  const room = free === null || free === undefined ? null : free + view.buffer_bytes - 2 * GB;
  const select = el("select", { attrs: { "aria-label": t("ui.settings.torrent.cacheLimitMegabytes") } });
  select.appendChild(el("option", { text: t("ui.cache.auto"), attrs: { value: "0" } }));
  const sizes = [4, 8, 16, 32, 50, 75, 100, 150, 200, 300, 500, 1000];
  for (const gb of sizes) {
    if (room !== null && gb * GB > room) continue;
    select.appendChild(el("option", { text: t("ui.cache.gb", gb), attrs: { value: String(gb * 1024) } }));
  }
  // A limit set before the disk filled up is still shown, marked.
  if (current && ![...select.options].some((option) => option.value === String(current))) {
    select.appendChild(
      el("option", { text: t("ui.cache.toolarge", Math.round(current / 1024)), attrs: { value: String(current) } })
    );
  }
  select.value = String(current);
  select.addEventListener("change", () => save("torrent.cacheLimitMegabytes", select.value));
  const facts = [
    free === null || free === undefined ? null : t("ui.cache.free", bytes(free)),
    t("ui.cache.used", bytes(view.buffer_bytes)),
    t("ui.cache.limit", bytes(view.cache_limit_bytes)),
  ].filter(Boolean);
  return el("div", { className: "setting" }, [
    el("span", { className: "setting-name" }, [
      document.createTextNode(t("ui.settings.torrent.cacheLimitMegabytes")),
      el("span", { className: "hint", text: `${t("ui.cache.note")} ${facts.join(" · ")}` }),
    ]),
    select,
  ]);
}

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
  // One question at a time. A status that takes longer than the interval --
  // the host busy with mpv, say -- must not queue up a second and a third
  // behind it.
  if (poll.busy) return;
  poll.busy = true;
  let status;
  try {
    status = await invoke("status");
  } catch (error) {
    return;
  } finally {
    poll.busy = false;
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
    // A start that has been slow for a while gets a way to a faster source:
    // still fetching metadata after twenty seconds, or filling the
    // prebuffer at under 300 KB/s after fifteen.
    const waited = STARTING ? (Date.now() - STARTING.since) / 1000 : 0;
    const slow =
      (status.job.stage === "ui.job.metadata" && waited > 20) ||
      (status.job.stage === "ui.job.buffering" && waited > 15 && (!torrent || torrent.down_bps < 300 * 1024));
    document.getElementById("loading-faster").hidden = !slow;
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
  if (status.notice) toast(status.notice);
  if (!status.job.active && status.job.error) {
    hideLoading();
    toast(status.job.error);
  }
}

// ---------------------------------------------------------------- arrow keys

/** Whether an element takes typing, where the arrow keys belong to it. */
function isTyping(node) {
  if (!node) return false;
  const tag = node.tagName;
  return tag === "INPUT" || tag === "TEXTAREA" || tag === "SELECT" || node.isContentEditable;
}

/** Moving around with the arrow keys alone, the way a television remote
 *  does: from whatever has focus to the nearest control in that direction,
 *  by where things are on the screen rather than by their order in the page
 *  -- so down from a poster goes to the poster below it, not to the next
 *  one in the row. Enter presses, Backspace goes back. Nothing changes for a
 *  mouse, and a slider, a text box or a list box keeps its own arrows. */
function spatial(event) {
  const dirs = { ArrowLeft: [-1, 0], ArrowRight: [1, 0], ArrowUp: [0, -1], ArrowDown: [0, 1] };
  const dir = dirs[event.key];
  if (!dir || event.altKey || event.ctrlKey || event.metaKey) return;
  const active = document.activeElement;
  if (isTyping(active) || (active && active.closest('[role="slider"], [role="radiogroup"], #menu'))) return;
  const view = [...document.querySelectorAll(".view")].find((section) => !section.hidden);
  const scope = [document.getElementById("nav"), document.getElementById("side"), view, document.getElementById("loading")].filter(
    (node) => node && !node.hidden
  );
  const visible = (node) => {
    // A card's menu button shows on hover only: Tab still reaches it, the
    // arrows go to the card.
    if (node.disabled || node.closest("[hidden]") || node.classList.contains("card-more")) return false;
    const box = node.getBoundingClientRect();
    return box.width > 0 && box.height > 0;
  };
  const all = scope
    .flatMap((root) => [...root.querySelectorAll('button, a[href], [tabindex="0"]')])
    .filter(visible);
  if (!all.length) return;
  event.preventDefault();
  if (!active || active === document.body || !all.includes(active)) {
    // Nothing focused yet: the first poster on the page, or the first
    // control.
    const first = all.find((node) => node.classList.contains("card")) || all[0];
    first.focus();
    return;
  }
  const from = active.getBoundingClientRect();
  const cx = from.left + from.width / 2;
  const cy = from.top + from.height / 2;
  let best = null;
  let bestScore = Infinity;
  for (const node of all) {
    if (node === active) continue;
    const box = node.getBoundingClientRect();
    const x = box.left + box.width / 2 - cx;
    const y = box.top + box.height / 2 - cy;
    // Along the direction it must go forward; across it, it should stay
    // close. Being off to the side costs twice what distance does.
    const along = x * dir[0] + y * dir[1];
    if (along <= 1) continue;
    // Left and right stay on the same row: at the end of a row the focus
    // stops, as it does on a television, rather than jumping to whatever
    // happens to be further right higher up the page.
    if (dir[0] !== 0) {
      const middle = box.top + box.height / 2;
      if (middle < from.top || middle > from.bottom) continue;
    }
    const across = Math.abs(x * dir[1] + y * dir[0]);
    const score = along + across * 2;
    if (score < bestScore) {
      bestScore = score;
      best = node;
    }
  }
  if (!best) return;
  best.focus({ preventScroll: true });
  best.scrollIntoView({ block: "nearest", inline: "nearest", behavior: "smooth" });
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
  initSide();
  // Unread community notifications, when signed in to a server.
  refreshCommunity().then(refreshBadge);

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
  // Results as the words are typed, once there are enough of them to mean
  // something, and after a pause: every search asks every addon.
  let typing = 0;
  searchInput.addEventListener("input", () => {
    clearTimeout(typing);
    const query = searchInput.value.trim();
    if (query.length < 2) return;
    typing = setTimeout(() => runSearch(query, true), 450);
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
  // Both ways out of the loading screen give up on the start for real. The
  // back button used to only hide the screen, and the next poll -- seeing the
  // start still running -- put it straight back.
  const giveUp = async () => {
    hideLoading();
    try {
      await invoke("cancel");
    } catch (error) {
      toast(error);
    }
  };
  document.getElementById("loading-back").addEventListener("click", giveUp);
  document.getElementById("loading-cancel").addEventListener("click", giveUp);
  document.getElementById("loading-faster").addEventListener("click", async () => {
    document.getElementById("loading-faster").hidden = true;
    if (STARTING) STARTING.since = Date.now();
    try {
      await invoke("play_faster");
    } catch (error) {
      toast(error);
    }
  });

  document.addEventListener("keydown", spatial);
  document.addEventListener("keydown", (event) => {
    // Backspace goes back too, as a remote's Back does -- not while typing.
    if (event.key === "Backspace" && !isTyping(document.activeElement)) {
      if (!document.getElementById("view-detail").hidden) {
        event.preventDefault();
        go(LAST);
        return;
      }
    }
    if (event.key === "Escape") {
      if (!document.getElementById("menu").hidden) closeMenu(true);
      else if (!document.getElementById("who").hidden) closeWho();
      else if (!dialog.hidden) closeDialog();
      else if (!document.getElementById("loading").hidden) document.getElementById("loading-cancel").click();
      else if (!document.getElementById("view-detail").hidden) go(LAST);
    } else if (event.key === "/" && document.activeElement === document.body) {
      event.preventDefault();
      searchForm.classList.add("open");
      searchInput.focus();
    }
  });

  // A click anywhere else, Escape, or the page moving closes a menu.
  document.addEventListener("pointerdown", (event) => {
    if (!event.target.closest("#menu") && !event.target.closest(".card-more")) closeMenu();
  });
  window.addEventListener("resize", () => closeMenu());
  document.addEventListener("scroll", () => closeMenu(), { capture: true, passive: true });

  document.getElementById("who-open").addEventListener("click", openAvatarMenu);
  await loadProfiles();
  const opening = PROFILES.profiles.find((p) => p.id === PROFILES.current);
  PIN_GATE = Boolean(opening && opening.locked);
  if (PIN_GATE) openWho(false);
  showView("home");
  await loadRows();
  // The first time, the account question and then the setup; with several
  // profiles, who is watching. Someone who already used EON Stream is asked
  // about the account once, unless a profile lock or a choice of profile
  // comes first.
  await loadAccount();
  if (!PROFILES.onboarded) {
    if (!ACCOUNT.prompted) openAccount(() => openWelcome());
    else openWelcome();
  } else if (PROFILES.profiles.length > 1) openWho(false);
  else if (!ACCOUNT.prompted && !PIN_GATE) openAccount(null);
  await poll();
  // Every second while something is starting, so the loading screen moves;
  // every few seconds otherwise, which is only there to notice a playback
  // started from somewhere else. While the player covers this page it is
  // hidden, and the player's own webview asks.
  const tick = () => {
    const busy = STARTING !== null || !document.getElementById("loading").hidden;
    const wait = busy ? 1000 : document.hidden ? 5000 : 3000;
    setTimeout(async () => {
      await poll();
      tick();
    }, wait);
  };
  tick();
}

boot();
