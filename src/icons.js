// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 EON contributors
//
// Additional permission under GNU GPL version 3 section 7:
// see LICENSE-EXCEPTION.md (EON Module ABI Exception 1.0).
//
// The icons, shared by the catalogue and the player.
//
// Drawn here as SVG paths rather than shipped as files or a font: they stay
// in the binary with the rest of the interface, they take the colour of the
// text around them (`currentColor`), and they are built with createElementNS
// like every other node -- never parsed out of a string (rule 2 in app.js).
//
// Every icon is on a 24-unit grid with a 2-unit stroke, so they sit together
// at any size. An icon is never the only label of a control: the button that
// carries one also carries an aria-label from the catalogue.

const SVG_NS = "http://www.w3.org/2000/svg";

/** Each icon is a list of [element, attributes]. */
const ICONS = {
  play: [["path", { d: "M7 4.8v14.4a.8.8 0 0 0 1.2.7l11.6-7.2a.8.8 0 0 0 0-1.4L8.2 4.1A.8.8 0 0 0 7 4.8z", fill: "currentColor", stroke: "none" }]],
  pause: [
    ["rect", { x: 6, y: 4.5, width: 4, height: 15, rx: 1, fill: "currentColor", stroke: "none" }],
    ["rect", { x: 14, y: 4.5, width: 4, height: 15, rx: 1, fill: "currentColor", stroke: "none" }],
  ],
  stop: [["rect", { x: 6, y: 6, width: 12, height: 12, rx: 1.5, fill: "currentColor", stroke: "none" }]],
  // A circular arrow with the number of seconds inside it. The number is part
  // of the drawing, the way it is on every player people already know.
  back: [
    ["path", { d: "M4.5 12a7.5 7.5 0 1 0 2.2-5.3" }],
    ["path", { d: "M4 3.5v4.2h4.2" }],
  ],
  forward: [
    ["path", { d: "M19.5 12a7.5 7.5 0 1 1-2.2-5.3" }],
    ["path", { d: "M20 3.5v4.2h-4.2" }],
  ],
  volume: [
    ["path", { d: "M4 9.5v5h3.5l4.5 4V5.5l-4.5 4z", fill: "currentColor" }],
    ["path", { d: "M15.5 9a4 4 0 0 1 0 6" }],
    ["path", { d: "M18 6.5a7.5 7.5 0 0 1 0 11" }],
  ],
  muted: [
    ["path", { d: "M4 9.5v5h3.5l4.5 4V5.5l-4.5 4z", fill: "currentColor" }],
    ["path", { d: "M16 9.5l5 5M21 9.5l-5 5" }],
  ],
  fullscreen: [["path", { d: "M4 9V4h5M15 4h5v5M20 15v5h-5M9 20H4v-5" }]],
  windowed: [["path", { d: "M9 4v5H4M20 9h-5V4M15 20v-5h5M4 15h5v5" }]],
  subtitles: [
    ["rect", { x: 3, y: 5, width: 18, height: 14, rx: 2.5 }],
    ["path", { d: "M7 12.5h3M13 12.5h4M7 15.5h6M15.5 15.5H17" }],
  ],
  speed: [
    ["path", { d: "M4.5 17a8 8 0 1 1 15 0" }],
    ["path", { d: "M12 13l4-4.5" }],
    ["circle", { cx: 12, cy: 13, r: 1.2, fill: "currentColor" }],
  ],
  arrowLeft: [["path", { d: "M19 12H5M11 6l-6 6 6 6" }]],
  chevronLeft: [["path", { d: "M14.5 5.5L8 12l6.5 6.5" }]],
  chevronRight: [["path", { d: "M9.5 5.5L16 12l-6.5 6.5" }]],
  chevronDown: [["path", { d: "M6 9.5l6 6 6-6" }]],
  search: [
    ["circle", { cx: 10.5, cy: 10.5, r: 6.5 }],
    ["path", { d: "M15.5 15.5L20.5 20.5" }],
  ],
  link: [
    ["path", { d: "M10 14a4.5 4.5 0 0 0 6.4 0l3.2-3.2a4.5 4.5 0 0 0-6.4-6.4L12 5.6" }],
    ["path", { d: "M14 10a4.5 4.5 0 0 0-6.4 0l-3.2 3.2a4.5 4.5 0 0 0 6.4 6.4l1.2-1.2" }],
  ],
  settings: [
    ["path", { d: "M10.3 3.2h3.4l.5 2.4 1.6.9 2.3-.8 1.7 2.9-1.8 1.6v1.8l1.8 1.6-1.7 2.9-2.3-.8-1.6.9-.5 2.4h-3.4l-.5-2.4-1.6-.9-2.3.8-1.7-2.9 1.8-1.6v-1.8L4.2 8.6l1.7-2.9 2.3.8 1.6-.9z" }],
    ["circle", { cx: 12, cy: 12, r: 2.8 }],
  ],
  plus: [["path", { d: "M12 5v14M5 12h14" }]],
  download: [["path", { d: "M12 4v11M7 10.5l5 5 5-5M5 19.5h14" }]],
  star: [["path", { d: "M12 3.6l2.6 5.3 5.8.8-4.2 4.1 1 5.8L12 16.9l-5.2 2.7 1-5.8-4.2-4.1 5.8-.8z" }]],
  next: [
    ["path", { d: "M6 5.5l9 6.5-9 6.5z", fill: "currentColor" }],
    ["path", { d: "M18 5.5v13" }],
  ],
  check: [["path", { d: "M5 12.5l4.5 4.5L19 7.5" }]],
  info: [
    ["circle", { cx: 12, cy: 12, r: 9 }],
    ["path", { d: "M12 11v6" }],
    ["circle", { cx: 12, cy: 7.8, r: 1.1, fill: "currentColor", stroke: "none" }],
  ],
  close: [["path", { d: "M6 6l12 12M18 6L6 18" }]],
  trash: [["path", { d: "M4.5 7h15M9.5 7V4.5h5V7M6.5 7l1 12.5h9l1-12.5M10 11v5M14 11v5" }]],
  up: [["path", { d: "M12 19V5M6 11l6-6 6 6" }]],
  down: [["path", { d: "M12 5v14M6 13l6 6 6-6" }]],
  refresh: [
    ["path", { d: "M19.5 12a7.5 7.5 0 1 1-2.2-5.3" }],
    ["path", { d: "M20 3.5v4.2h-4.2" }],
  ],
  power: [
    ["path", { d: "M12 3.5v8" }],
    ["path", { d: "M7 6.5a7 7 0 1 0 10 0" }],
  ],
  more: [
    ["circle", { cx: 12, cy: 5.5, r: 1.6, fill: "currentColor", stroke: "none" }],
    ["circle", { cx: 12, cy: 12, r: 1.6, fill: "currentColor", stroke: "none" }],
    ["circle", { cx: 12, cy: 18.5, r: 1.6, fill: "currentColor", stroke: "none" }],
  ],
  addon: [["path", { d: "M9.5 3.5h5v3a1.8 1.8 0 1 0 3.5 0v-3H20.5v6h-3a1.8 1.8 0 1 0 0 3.5h3v7.5h-17v-17z" }]],
};

/** An icon as an SVG element, optionally with a number drawn inside it. */
function icon(name, number) {
  const svg = document.createElementNS(SVG_NS, "svg");
  svg.setAttribute("viewBox", "0 0 24 24");
  svg.setAttribute("aria-hidden", "true");
  svg.setAttribute("focusable", "false");
  svg.setAttribute("class", `icon icon-${name}`);
  svg.setAttribute("fill", "none");
  svg.setAttribute("stroke", "currentColor");
  svg.setAttribute("stroke-width", "2");
  svg.setAttribute("stroke-linecap", "round");
  svg.setAttribute("stroke-linejoin", "round");
  for (const [tag, attributes] of ICONS[name] || []) {
    const node = document.createElementNS(SVG_NS, tag);
    for (const [key, value] of Object.entries(attributes)) node.setAttribute(key, String(value));
    svg.appendChild(node);
  }
  if (number !== undefined) {
    const text = document.createElementNS(SVG_NS, "text");
    text.setAttribute("x", "12");
    text.setAttribute("y", "15.2");
    text.setAttribute("text-anchor", "middle");
    text.setAttribute("font-size", "7.5");
    text.setAttribute("font-weight", "700");
    text.setAttribute("fill", "currentColor");
    text.setAttribute("stroke", "none");
    text.textContent = String(number);
    svg.appendChild(text);
  }
  return svg;
}

/** Put an icon into every element that names one: `data-icon="play"`,
 *  optionally `data-number="10"`. */
function drawIcons(root = document) {
  for (const node of root.querySelectorAll("[data-icon]")) {
    const old = node.querySelector(":scope > svg.icon");
    if (old) old.remove();
    node.prepend(icon(node.dataset.icon, node.dataset.number));
  }
}
