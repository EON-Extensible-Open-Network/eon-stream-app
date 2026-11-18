// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 EON contributors
//
// Additional permission under GNU GPL version 3 section 7:
// see LICENSE-EXCEPTION.md (EON Module ABI Exception 1.0).
//
// The player's controls.
//
// The same three rules as app.js: the host is asked for things by name, every
// node is built with createElement and textContent, and no sentence is
// written here. What this page adds is behaviour a person already knows from
// every player they have used:
//
//   * the controls appear when the pointer moves and fade after three seconds
//     of stillness while something plays, and the cursor goes with them;
//   * a click on the picture pauses, a double click goes fullscreen;
//   * space or K pauses; left and right go 10 seconds, with shift 60; up and
//     down change the volume; M mutes; F is fullscreen; Escape leaves
//     fullscreen, closes a panel, or goes back.

const invoke = window.__TAURI__.core.invoke;

let MESSAGES = {};
let STATE = null;
let SLEEP_TIMER = null;
let SCRUBBING = null;
let VOLUME_HELD = 0;
let WAITING_SINCE = 0;

function t(key, ...args) {
  const text = MESSAGES[key];
  if (text === undefined) return key;
  return text.replace(/\{(\d+)\}/g, (whole, index) => {
    const value = args[Number(index)];
    return value === undefined ? whole : String(value);
  });
}

function languageName(code) {
  const key = `ui.lang.${code}`;
  return MESSAGES[key] === undefined ? String(code).toUpperCase() : MESSAGES[key];
}

function $(id) {
  return document.getElementById(id);
}

function el(tag, options = {}, children = []) {
  const node = document.createElement(tag);
  if (options.className) node.className = options.className;
  if (options.text !== undefined && options.text !== null) node.textContent = String(options.text);
  if (options.attrs) {
    for (const [name, value] of Object.entries(options.attrs)) node.setAttribute(name, String(value));
  }
  if (options.onClick) node.addEventListener("click", options.onClick);
  for (const child of children) if (child) node.appendChild(child);
  return node;
}

function clear(node) {
  while (node.firstChild) node.removeChild(node.firstChild);
}

/** Bytes per second, as a person reads a download speed. */
function bytes(n) {
  const value = Number(n) || 0;
  const units = ["B", "KB", "MB", "GB"];
  let at = value;
  let unit = 0;
  while (at >= 1024 && unit < units.length - 1) {
    at /= 1024;
    unit += 1;
  }
  return `${at >= 10 || unit === 0 ? Math.round(at) : at.toFixed(1)} ${units[unit]}`;
}

/** Give up on this: stop the player, stop the torrent, delete what it
 *  fetched. The host takes this page down. */
async function abandon() {
  $("abandon").disabled = true;
  try {
    await invoke("cancel");
  } catch (error) {
    $("abandon").disabled = false;
  }
}

function clock(seconds) {
  if (seconds === null || seconds === undefined || !Number.isFinite(seconds)) return "0:00";
  const whole = Math.max(0, Math.floor(seconds));
  const h = Math.floor(whole / 3600);
  const m = Math.floor((whole % 3600) / 60);
  const s = whole % 60;
  const pad = (n) => String(n).padStart(2, "0");
  return h > 0 ? `${h}:${pad(m)}:${pad(s)}` : `${m}:${pad(s)}`;
}

function label(node, key) {
  node.setAttribute("aria-label", t(key));
  node.title = t(key);
}

function applyStrings() {
  for (const node of document.querySelectorAll("[data-say]")) node.textContent = t(node.dataset.say);
  for (const node of document.querySelectorAll("[data-label]")) label(node, node.dataset.label);
}

/** One thing for the player to do. Errors are not shown over the picture:
 *  the next poll says what the player is actually doing. */
async function act(action, value = null) {
  wake();
  try {
    await invoke("player", { action, value });
  } catch (error) {
    // Nothing to draw it on but the picture; the state poll corrects the
    // controls either way.
  }
  poll();
}

// ---------------------------------------------------------------- awake/asleep

/** Where subtitles sit: above the control bar while it is up, near the
 *  bottom edge when it is not. In mpv's units, 720 to the picture's height. */
const SUB_LOW = 34;
const SUB_HIGH = 120;
let SUB_AT = null;

function placeSubtitles(margin) {
  if (SUB_AT === margin) return;
  SUB_AT = margin;
  invoke("player", { action: "sub-margin", value: margin }).catch(() => {
    SUB_AT = null;
  });
}

function wake() {
  if (document.body.classList.contains("sleep")) {
    document.body.classList.remove("sleep");
    drawState();
  }
  placeSubtitles(SUB_HIGH);
  clearTimeout(SLEEP_TIMER);
  SLEEP_TIMER = setTimeout(sleep, 3000);
}

function sleep() {
  // Never while paused, scrubbing, waiting, or with a panel open: those are
  // moments someone is looking at the controls.
  const panelOpen =
    !$("tracks").hidden || !$("speed").hidden || !$("waiting").hidden || !$("upnext").hidden;
  const hovering = document.querySelector(".bottom:hover, .top:hover");
  if (!STATE || STATE.paused || SCRUBBING || panelOpen || hovering) {
    SLEEP_TIMER = setTimeout(sleep, 3000);
    return;
  }
  document.body.classList.add("sleep");
  placeSubtitles(SUB_LOW);
}

function flash(name) {
  const node = $("flash");
  clear(node);
  node.appendChild(icon(name));
  node.classList.remove("go");
  void node.offsetWidth;
  node.classList.add("go");
}

// --------------------------------------------------------------------- state

async function poll() {
  // One question at a time: a slow answer must not let a queue of them build
  // up behind it, each holding the host's lock in turn.
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
  const player = status.player;
  STATE = player;
  if (status.notice) {
    const note = $("note");
    note.textContent = status.notice;
    note.hidden = false;
    clearTimeout(note.timer);
    note.timer = setTimeout(() => {
      note.hidden = true;
    }, 9000);
  }
  if (!player.running) return; // The host is taking this page down.

  put($("title"), player.title || "");
  if (document.title !== (player.title || "EON Stream")) document.title = player.title || "EON Stream";

  const toggle = $("toggle");
  const wanted = player.paused ? "play" : "pause";
  if (toggle.dataset.icon !== wanted) {
    toggle.dataset.icon = wanted;
    drawIcons(toggle.parentElement);
    label(toggle, player.paused ? "ui.player.play" : "ui.player.pause");
  }
  if (player.paused) wake();

  const mute = $("mute");
  const muteIcon = player.muted || player.volume === 0 ? "muted" : "volume";
  if (mute.dataset.icon !== muteIcon) {
    mute.dataset.icon = muteIcon;
    drawIcons(mute.parentElement);
  }
  if (document.activeElement !== $("volume") && player.volume !== null && player.volume !== undefined) {
    $("volume").value = String(Math.round(player.volume));
  }

  // Waiting on the network: the spinner, what it is waiting on, and the way
  // out. The controls stay up meanwhile -- someone watching a spinner is
  // deciding whether to keep waiting.
  // The next episode being found and fetched is a wait too, with the same
  // way out.
  const starting = status.job && status.job.active;
  const waitingNow = player.buffering || starting;
  const waiting = $("waiting");
  if (waiting.hidden !== !waitingNow) waiting.hidden = !waitingNow;
  // Waiting for a while on a slow swarm: offer the next-fastest source.
  if (!waitingNow) WAITING_SINCE = 0;
  else if (!WAITING_SINCE) WAITING_SINCE = Date.now();
  const stalled = waitingNow && Date.now() - WAITING_SINCE > 15000;
  const slowTorrent = !status.torrents || !status.torrents[0] || status.torrents[0].down_bps < 300 * 1024;
  const offer = stalled && slowTorrent;
  if ($("faster").hidden !== !offer) $("faster").hidden = !offer;
  if (waitingNow) {
    wake();
    const torrent = status.torrents && status.torrents[0];
    let line;
    if (starting) {
      line = t(status.job.stage);
      if (status.job.progress !== null && status.job.progress !== undefined) {
        line += ` ${Math.round(status.job.progress * 100)}%`;
      }
    } else if (torrent && !torrent.finished) {
      line = t("ui.player.peers", torrent.peers, bytes(torrent.down_bps));
    } else {
      line = t("ui.player.waiting");
    }
    put($("waiting-line"), line);
  }
  drawNext(player, starting);

  // Nothing of the timeline is on screen while the controls are asleep, and
  // this page is laid over a moving picture: every change to it is a frame
  // the compositor has to blend again. It is drawn when they wake.
  if (!document.body.classList.contains("sleep") || SCRUBBING) drawState();
}

function drawState() {
  if (!STATE) return;
  const duration = STATE.duration || 0;
  const position = SCRUBBING ? SCRUBBING.at : STATE.position || 0;
  drawTimeline(position, duration, STATE.buffered_seconds || 0);
}

/** Write text only when it changed: rewriting the same words still costs a
 *  layout, twice a second, over the video. */
function put(node, text) {
  if (node.textContent !== text) node.textContent = text;
}

function drawTimeline(position, duration, buffered) {
  const fraction = duration ? Math.min(1, position / duration) : 0;
  const ahead = duration ? Math.min(1, (position + buffered) / duration) : 0;
  $("played").style.width = `${fraction * 100}%`;
  $("buffered").style.width = `${ahead * 100}%`;
  $("thumb").style.left = `${fraction * 100}%`;
  put($("clock"), duration ? `${clock(position)} / ${clock(duration)}` : clock(position));
  put($("remaining"), duration ? t("ui.player.remaining", clock(duration - position)) : "");
  const scrub = $("scrub");
  scrub.setAttribute("aria-valuemax", String(Math.round(duration)));
  scrub.setAttribute("aria-valuenow", String(Math.round(position)));
  scrub.setAttribute("aria-valuetext", clock(position));
}

// ------------------------------------------------------------------ up next

/** How long the count down to the next episode runs, in milliseconds. */
const UPNEXT_MS = 10000;
/** How near the end the card appears, in seconds. */
const UPNEXT_AT = 30;
const UPNEXT = { shown: false, dismissed: false, going: false, deadline: 0, leaving: false };

function nextName(next) {
  return next.title ? `${next.label} · ${next.title}` : next.label;
}

/** The next-episode button, and near the end the card that counts down to
 *  it. At the very end -- mpv holds the last frame -- the next episode starts
 *  unless the viewer said they would rather watch the credits; with nothing
 *  next, the player goes and the page it came from is back. */
function drawNext(player, starting) {
  const next = player.next;
  const button = $("next");
  if (button.hidden !== !next) button.hidden = !next;
  if (next) {
    const text = t("ui.player.nextis", nextName(next));
    if (button.title !== text) {
      button.title = text;
      button.setAttribute("aria-label", text);
    }
  }

  const remaining =
    player.duration && player.position !== null && player.position !== undefined
      ? player.duration - player.position
      : null;
  const near = remaining !== null && remaining <= UPNEXT_AT && player.duration > UPNEXT_AT * 2;
  const card = $("upnext");
  const want = Boolean(next) && !UPNEXT.dismissed && !UPNEXT.going && !starting && (near || player.ended);

  if (want && !UPNEXT.shown) {
    UPNEXT.shown = true;
    UPNEXT.deadline = Date.now() + UPNEXT_MS;
    put($("upnext-title"), nextName(next));
    card.hidden = false;
    wake();
  } else if (!want && UPNEXT.shown) {
    UPNEXT.shown = false;
    card.hidden = true;
  }
  if (!near && !player.ended) {
    // Seeking back out of the credits forgets a "watch the credits".
    UPNEXT.dismissed = false;
  }

  if (UPNEXT.shown && !player.autoplay) {
    // Autoplay is off: the card offers the next episode and waits to be
    // asked, at the end as well.
    put($("upnext-text"), t("ui.player.playnext"));
    $("upnext-fill").style.width = "0%";
  } else if (UPNEXT.shown) {
    // Paused is paused: the count down waits too.
    if (player.paused && !player.ended) UPNEXT.deadline += 500;
    const left = Math.max(0, UPNEXT.deadline - Date.now());
    put($("upnext-text"), t("ui.player.nextin", Math.ceil(left / 1000)));
    $("upnext-fill").style.width = `${(1 - left / UPNEXT_MS) * 100}%`;
    if (left === 0 || player.ended) playNext();
  } else if (player.ended && !starting && !UPNEXT.going && !UPNEXT.leaving) {
    // The end, and nothing (wanted) after it.
    UPNEXT.leaving = true;
    setTimeout(() => act("stop"), 1500);
  }
}

async function playNext() {
  if (UPNEXT.going) return;
  UPNEXT.going = true;
  UPNEXT.shown = false;
  $("upnext").hidden = true;
  try {
    await invoke("play_next");
  } catch (error) {
    UPNEXT.going = false;
    UPNEXT.dismissed = true;
    put($("waiting-line"), String(error));
  }
}

// ------------------------------------------------------------------ timeline

function timelineAt(event) {
  const rect = $("scrub").getBoundingClientRect();
  const fraction = Math.min(1, Math.max(0, (event.clientX - rect.left) / rect.width));
  return { fraction, seconds: fraction * (STATE && STATE.duration ? STATE.duration : 0), x: event.clientX - rect.left };
}

function setupTimeline() {
  const scrub = $("scrub");
  const hover = $("hover-time");

  scrub.addEventListener("pointermove", (event) => {
    if (!STATE || !STATE.duration) return;
    const at = timelineAt(event);
    hover.hidden = false;
    hover.textContent = clock(at.seconds);
    hover.style.left = `${at.x}px`;
    if (SCRUBBING) {
      SCRUBBING.at = at.seconds;
      drawTimeline(at.seconds, STATE.duration, 0);
    }
  });
  scrub.addEventListener("pointerleave", () => {
    if (!SCRUBBING) hover.hidden = true;
  });
  scrub.addEventListener("pointerdown", (event) => {
    if (!STATE || !STATE.duration) return;
    scrub.setPointerCapture(event.pointerId);
    scrub.classList.add("dragging");
    SCRUBBING = { at: timelineAt(event).seconds };
    drawTimeline(SCRUBBING.at, STATE.duration, 0);
  });
  const release = () => {
    if (!SCRUBBING) return;
    const target = SCRUBBING.at;
    SCRUBBING = null;
    scrub.classList.remove("dragging");
    hover.hidden = true;
    act("seek-to", target);
  };
  scrub.addEventListener("pointerup", release);
  scrub.addEventListener("pointercancel", release);
  scrub.addEventListener("keydown", (event) => {
    if (event.key === "ArrowLeft" || event.key === "ArrowRight") {
      event.preventDefault();
      event.stopPropagation();
      act("seek", (event.key === "ArrowLeft" ? -1 : 1) * (event.shiftKey ? 60 : 10));
    }
  });
}

// -------------------------------------------------------------------- panels

function closePanels() {
  $("tracks").hidden = true;
  $("speed").hidden = true;
  $("keys").hidden = true;
  clearTimeout(FOUND_TIMER);
  FOUND_ALL = false;
}

async function openTracks() {
  const panel = $("tracks");
  if (!panel.hidden) {
    panel.hidden = true;
    return;
  }
  closePanels();
  let tracks = [];
  try {
    tracks = await invoke("tracks");
  } catch (error) {
    tracks = [];
  }
  const fill = (list, kind, withOff) => {
    clear(list);
    const ofKind = tracks.filter((track) => track.kind === kind);
    const option = (text, checked, value) => {
      const button = el("button", { attrs: { type: "button", role: "menuitemradio", "aria-checked": String(checked) } });
      button.appendChild(icon("check"));
      button.appendChild(el("span", { text }));
      button.addEventListener("click", async () => {
        await act(kind, value);
        panel.hidden = true;
      });
      list.appendChild(el("li", {}, [button]));
    };
    if (withOff) option(t("ui.player.off"), !ofKind.some((track) => track.selected), -1);
    ofKind.forEach((track, index) => {
      const name = [track.language ? languageName(track.language) : null, track.title]
        .filter(Boolean)
        .join(" · ");
      option(name || t("ui.player.track", index + 1), track.selected, track.id);
    });
  };
  fill($("audio-list"), "audio", false);
  fill($("sub-list"), "sub", true);
  panel.hidden = false;
  fillTuning();
  await fillFound();
}

/** Subtitle sizes offered, as multiples of the player's own. */
const SIZES = [0.75, 1, 1.25, 1.5, 2];
let TUNING = { sub_scale: 1, sub_delay: 0, audio_delay: 0 };

function seconds(value) {
  const rounded = Math.round(value * 10) / 10;
  return `${rounded > 0 ? "+" : ""}${rounded.toFixed(1)} s`;
}

/** The adjustments column: subtitle size, and subtitles and sound moved
 *  against the picture. */
async function fillTuning() {
  try {
    TUNING = await invoke("tuning");
  } catch (error) {
    return;
  }
  const sizes = $("sizes");
  clear(sizes);
  for (const size of SIZES) {
    sizes.appendChild(
      el("button", {
        text: t("ui.unit.percent", Math.round(size * 100)),
        attrs: { type: "button", "aria-pressed": String(Math.abs(TUNING.sub_scale - size) < 0.01) },
        onClick: async () => {
          await act("sub-scale", size);
          // Kept for the next film, too.
          invoke("settings_set", { path: "playback.subtitleScale", value: String(size) }).catch(() => {});
          fillTuning();
        },
      })
    );
  }
  put($("sub-delay-value"), seconds(TUNING.sub_delay));
  put($("audio-delay-value"), seconds(TUNING.audio_delay));
}

async function nudge(kind, by) {
  const key = kind === "sub-delay" ? "sub_delay" : "audio_delay";
  const value = by === 0 ? 0 : Math.round((TUNING[key] + by) * 10) / 10;
  TUNING[key] = value;
  put($(kind === "sub-delay" ? "sub-delay-value" : "audio-delay-value"), seconds(value));
  await act(kind, value);
}

/** Every key, as the "?" panel lists it. */
const KEYS = [
  ["Space / K", "ui.player.key.toggle"],
  ["← / →", "ui.player.key.seek10"],
  ["Shift ← / →, J / L", "ui.player.key.seek60"],
  ["↑ / ↓", "ui.player.key.volume"],
  ["M", "ui.player.key.mute"],
  ["F", "ui.player.key.fullscreen"],
  ["N", "ui.player.key.next"],
  ["G / H", "ui.player.key.subsync"],
  ["Esc", "ui.player.key.back"],
  ["?", "ui.player.key.help"],
];

function toggleKeys() {
  const panel = $("keys");
  if (!panel.hidden) {
    panel.hidden = true;
    return;
  }
  closePanels();
  const list = $("keys-list");
  clear(list);
  for (const [keys, what] of KEYS) {
    list.appendChild(el("dt", { text: keys }));
    list.appendChild(el("dd", { text: t(what) }));
  }
  panel.hidden = false;
}

/** How many found subtitles are listed before "show all". */
const FOUND_SHOWN = 8;
let FOUND_ALL = false;
let FOUND_TIMER = 0;

/** The subtitles the host found -- inside the torrent, from subtitle
 *  addons, or chosen from a file -- that are not loaded yet. One press
 *  loads one and switches it on. */
async function fillFound(note = null) {
  clearTimeout(FOUND_TIMER);
  const list = $("sub-found");
  let found;
  try {
    found = await invoke("subtitles");
  } catch (error) {
    found = { searching: false, addons: 0, offers: [] };
  }
  if ($("tracks").hidden) return;
  clear(list);
  const waiting = found.offers.filter((offer) => !offer.loaded);
  const shown = FOUND_ALL ? waiting : waiting.slice(0, FOUND_SHOWN);
  for (const offer of shown) {
    const name = offer.language ? languageName(offer.language) : t("ui.sources.unknown");
    const origin = offer.kind === "torrent" ? t("ui.subs.intorrent", offer.origin) : offer.origin;
    const button = el("button", { attrs: { type: "button", title: `${name} — ${origin}` } }, [
      icon("plus"),
      el("span", {}, [el("span", { text: name }), el("span", { className: "origin", text: origin })]),
    ]);
    button.addEventListener("click", async () => {
      put($("sub-note"), t("ui.subs.loading"));
      try {
        await invoke("subtitle_load", { index: offer.index });
        await refreshTracks();
      } catch (error) {
        await fillFound(t("ui.subs.failed", String(error)));
      }
    });
    list.appendChild(el("li", {}, [button]));
  }
  if (!FOUND_ALL && waiting.length > FOUND_SHOWN) {
    list.appendChild(
      el("li", {}, [
        el("button", {
          className: "more",
          text: t("ui.subs.more", waiting.length),
          attrs: { type: "button" },
          onClick: () => {
            FOUND_ALL = true;
            fillFound();
          },
        }),
      ])
    );
  }
  let text = note;
  if (!text) {
    if (found.searching) text = t("ui.subs.searching");
    else if (found.addons === 0) text = t("ui.subs.noaddon");
    else if (waiting.length === 0) text = t("ui.subs.none");
    else text = "";
  }
  put($("sub-note"), text);
  // Still looking: look again shortly, while the menu is open.
  if (found.searching) FOUND_TIMER = setTimeout(() => refreshTracks(), 1500);
}

/** Redraw an open track menu in place. */
async function refreshTracks() {
  const panel = $("tracks");
  if (panel.hidden) return;
  panel.hidden = true;
  await openTracks();
}

/** A subtitle file from this computer. Read here and sent as bytes: this
 *  page has no file paths to give, and should not. */
async function loadFile(file) {
  if (!file) return;
  if (file.size > 8 * 1024 * 1024) {
    put($("sub-note"), t("ui.subs.failed", t("ui.subs.toobig")));
    return;
  }
  put($("sub-note"), t("ui.subs.loading"));
  try {
    const buffer = await file.arrayBuffer();
    await invoke("subtitle_file", { name: file.name, bytes: Array.from(new Uint8Array(buffer)) });
    await refreshTracks();
  } catch (error) {
    put($("sub-note"), t("ui.subs.failed", String(error)));
  }
}

function openSpeed() {
  const panel = $("speed");
  if (!panel.hidden) {
    panel.hidden = true;
    return;
  }
  closePanels();
  const list = $("speed-list");
  clear(list);
  const current = STATE && STATE.speed ? STATE.speed : 1;
  for (const speed of [0.5, 0.75, 1, 1.25, 1.5, 2]) {
    list.appendChild(
      el("button", {
        text: `${speed}×`,
        attrs: { type: "button", role: "menuitemradio", "aria-checked": String(Math.abs(current - speed) < 0.01) },
        onClick: async () => {
          await act("speed", speed);
          panel.hidden = true;
        },
      })
    );
  }
  panel.hidden = false;
}

// ---------------------------------------------------------------------- keys

function toggle() {
  flash(STATE && STATE.paused ? "play" : "pause");
  act("toggle");
}

function setVolume(value) {
  const volume = Math.max(0, Math.min(130, value));
  $("volume").value = String(volume);
  act("volume", volume);
}

function onKey(event) {
  if (event.target instanceof HTMLInputElement && event.target.type === "range" && event.key.startsWith("Arrow")) return;
  const key = event.key.toLowerCase();
  wake();
  if (key === " " || key === "k") {
    event.preventDefault();
    toggle();
  } else if (key === "arrowleft" || key === "j") {
    event.preventDefault();
    act("seek", event.shiftKey || key === "j" ? -60 : -10);
  } else if (key === "arrowright" || key === "l") {
    event.preventDefault();
    act("seek", event.shiftKey || key === "l" ? 60 : 10);
  } else if (key === "arrowup") {
    event.preventDefault();
    setVolume(Number($("volume").value) + 5);
  } else if (key === "arrowdown") {
    event.preventDefault();
    setVolume(Number($("volume").value) - 5);
  } else if (key === "n" && !$("next").hidden) {
    playNext();
  } else if (key === "g" || key === "h") {
    // Subtitles earlier or later, a tenth of a second at a time.
    fillTuning().then(() => nudge("sub-delay", key === "g" ? -0.1 : 0.1));
  } else if (event.key === "?") {
    toggleKeys();
  } else if (key === "m") {
    act("mute");
  } else if (key === "f") {
    act("fullscreen");
  } else if (key === "escape") {
    if (!$("tracks").hidden || !$("speed").hidden || !$("keys").hidden) closePanels();
    else if (window.innerHeight >= screen.height - 1) act("fullscreen");
    // Leaving something that never started is giving up on it.
    else if (!$("waiting").hidden) abandon();
    else act("stop");
  }
}

// ---------------------------------------------------------------------- boot

async function boot() {
  drawIcons();
  try {
    const strings = await invoke("strings");
    MESSAGES = strings.messages;
    document.documentElement.lang = strings.locale;
  } catch (error) {
    MESSAGES = {};
  }
  applyStrings();

  $("back").addEventListener("click", () => act("stop"));
  $("stop").addEventListener("click", () => act("stop"));
  $("abandon").addEventListener("click", abandon);
  $("faster").addEventListener("click", async () => {
    $("faster").hidden = true;
    WAITING_SINCE = Date.now();
    try {
      await invoke("play_faster");
    } catch (error) {
      put($("waiting-line"), String(error));
    }
  });
  $("next").addEventListener("click", playNext);
  for (const button of document.querySelectorAll("[data-tune]")) {
    button.addEventListener("click", () => nudge(button.dataset.tune, Number(button.dataset.by)));
  }
  $("upnext-play").addEventListener("click", playNext);
  $("upnext-dismiss").addEventListener("click", () => {
    UPNEXT.dismissed = true;
    UPNEXT.shown = false;
    $("upnext").hidden = true;
  });
  $("sub-file-open").addEventListener("click", () => $("sub-file").click());
  $("sub-file").addEventListener("change", () => {
    const input = $("sub-file");
    loadFile(input.files && input.files[0]);
    input.value = "";
  });
  $("toggle").addEventListener("click", toggle);
  $("fullscreen").addEventListener("click", () => act("fullscreen"));
  $("mute").addEventListener("click", () => act("mute"));
  $("tracks-open").addEventListener("click", openTracks);
  $("speed-open").addEventListener("click", openSpeed);
  for (const button of document.querySelectorAll("[data-seek]")) {
    button.addEventListener("click", () => act("seek", Number(button.dataset.seek)));
  }
  $("volume").addEventListener("input", () => {
    clearTimeout(VOLUME_HELD);
    VOLUME_HELD = setTimeout(() => act("volume", Number($("volume").value)), 60);
  });

  // The picture: one click pauses, two go fullscreen. The single click waits
  // a moment to be sure it is not the first half of a double.
  let pending = null;
  $("stage").addEventListener("click", () => {
    if (!$("tracks").hidden || !$("speed").hidden) {
      closePanels();
      return;
    }
    clearTimeout(pending);
    pending = setTimeout(toggle, 220);
  });
  $("stage").addEventListener("dblclick", () => {
    clearTimeout(pending);
    act("fullscreen");
  });

  setupTimeline();
  document.addEventListener("pointermove", wake);
  // A control that is focused from the keyboard has to be seen: tabbing into
  // a faded-out bar wakes it, rather than moving focus somewhere invisible.
  document.addEventListener("focusin", wake);
  document.addEventListener("keydown", onKey);
  window.addEventListener("resize", () => {
    const full = window.innerHeight >= screen.height - 1;
    const button = $("fullscreen");
    const wanted = full ? "windowed" : "fullscreen";
    if (button.dataset.icon !== wanted) {
      button.dataset.icon = wanted;
      drawIcons(button.parentElement);
    }
  });

  wake();
  await poll();
  setInterval(poll, 500);
  document.body.focus();
}

boot();
