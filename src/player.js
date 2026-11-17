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

function wake() {
  if (document.body.classList.contains("sleep")) {
    document.body.classList.remove("sleep");
    drawState();
  }
  clearTimeout(SLEEP_TIMER);
  SLEEP_TIMER = setTimeout(sleep, 3000);
}

function sleep() {
  // Never while paused, scrubbing, or with a panel open: those are moments
  // someone is looking at the controls.
  const panelOpen = !$("tracks").hidden || !$("speed").hidden;
  const hovering = document.querySelector(".bottom:hover, .top:hover");
  if (!STATE || STATE.paused || SCRUBBING || panelOpen || hovering) {
    SLEEP_TIMER = setTimeout(sleep, 3000);
    return;
  }
  document.body.classList.add("sleep");
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

  if ($("spinner").hidden !== !player.buffering) $("spinner").hidden = !player.buffering;

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
  } else if (key === "m") {
    act("mute");
  } else if (key === "f") {
    act("fullscreen");
  } else if (key === "escape") {
    if (!$("tracks").hidden || !$("speed").hidden) closePanels();
    else if (window.innerHeight >= screen.height - 1) act("fullscreen");
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
