// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 EON contributors
//
// Additional permission under GNU GPL version 3 section 7:
// see LICENSE-EXCEPTION.md (EON Module ABI Exception 1.0).
//
// The community: a film and series database run by the people who use it,
// against eon-stream-spec/docs/community-api.md.
//
// Dormant until the viewer enters a server address. Every request goes
// through the host (`community_request`), which keeps the address and the
// session token and only lets the contract's paths through: this page never
// sees either, and has no network of its own. What it sends is what the
// viewer chose to publish -- scores, words, votes, lists. Never what they
// watched.
//
// Built like the rest of the interface: createElement only (app.js rule 2),
// strings from the catalogue, styles through CSSOM.

"use strict";

// ------------------------------------------------------------------ requests

/** One request to the community server. Answers the body; throws the
 *  server's sentence for anything but 2xx. */
async function cq(method, path, body = null) {
  const answer = await invoke("community_request", { method, path, body });
  if (answer.status >= 200 && answer.status < 300) return answer.body;
  const message = answer.body && answer.body.message ? answer.body.message : t("ui.community.failed", answer.status);
  throw message;
}

let COMMUNITY = { server: null, username: null, role: null };

async function refreshCommunity() {
  try {
    COMMUNITY = await invoke("community_state");
  } catch (error) {
    COMMUNITY = { server: null, username: null, role: null };
  }
  return COMMUNITY;
}

const enc = encodeURIComponent;
const isModerator = () => COMMUNITY.role === "moderator" || COMMUNITY.role === "admin";

/** What a title is, as the server keeps it: what this page already had. */
function titlePayload(detail) {
  return {
    name: detail.name,
    year: detail.year || null,
    poster: detail.poster || null,
    genres: detail.genres || [],
    cast: (detail.cast || []).slice(0, 30),
    director: (detail.director || []).slice(0, 10),
  };
}

/** A person's id: their name folded, as the contract says. */
function personId(name) {
  const fold = { ç: "c", ğ: "g", ı: "i", ö: "o", ş: "s", ü: "u", é: "e", è: "e", á: "a", à: "a", ä: "a", ñ: "n", ë: "e", ï: "i", ô: "o", û: "u" };
  let out = "";
  for (const raw of name.trim().toLowerCase()) {
    const c = fold[raw] || raw;
    if (/[a-z0-9]/.test(c)) out += c;
    else if (out && !out.endsWith("-")) out += "-";
  }
  return out.replace(/-+$/, "").slice(0, 80);
}

// ------------------------------------------------------------------- helpers

/** A labelled input. */
function field(label, attrs, tag = "input") {
  const input = el(tag, { attrs });
  return [el("label", { className: "field" }, [el("span", { text: label }), input]), input];
}

function button(text, onClick, className = "button ghost", attrs = {}) {
  return el("button", { className, text, attrs: { type: "button", ...attrs }, onClick });
}

function chipRow(options, current, onPick, label) {
  const row = el("div", { className: "chips", attrs: { role: "radiogroup", "aria-label": label || "" } });
  for (const [value, text] of options) {
    row.appendChild(
      el("button", {
        className: "chip",
        text,
        attrs: { type: "button", role: "radio", "aria-checked": String(value === current), "aria-pressed": String(value === current) },
        onClick: () => onPick(value),
      })
    );
  }
  return row;
}

function empty(text) {
  return el("div", { className: "spinner", text });
}

function when(seconds) {
  try {
    return new Date(seconds * 1000).toLocaleDateString(MESSAGES.__locale || "en", { year: "numeric", month: "short", day: "numeric" });
  } catch (error) {
    return "";
  }
}

/** Ask for a few words, in a dialog of our own: a reason for a report, a
 *  note on a decision, an appeal. Resolves with the text, or null. */
function promptText(title, hint, { min = 0, max = 2000, multiline = true, value = "", secret = false } = {}) {
  return new Promise((resolve) => {
    const input = el(multiline ? "textarea" : "input", {
      attrs: {
        maxlength: String(max),
        rows: multiline ? "5" : null,
        type: secret ? "password" : null,
        inputmode: secret ? "numeric" : null,
        autocomplete: secret ? "off" : null,
      },
    });
    input.value = value;
    const counter = el("span", { className: "hint" });
    const ok = button(t("ui.action.ok"), () => finish(input.value.trim()), "button play");
    const update = () => {
      const n = input.value.trim().length;
      counter.textContent = secret ? "" : t("ui.cm.chars", n, max);
      ok.disabled = n < min || n > max;
    };
    input.addEventListener("input", update);
    input.addEventListener("keydown", (event) => {
      if (event.key === "Enter" && !multiline && !ok.disabled) finish(input.value.trim());
    });
    const box = el("div", { className: "dialog", attrs: { role: "dialog", "aria-modal": "true", "aria-label": title } }, [
      el("div", { className: "dialog-card" }, [
        el("h3", { text: title }),
        hint ? el("p", { className: "hint", text: hint }) : null,
        input,
        counter,
        el("div", { className: "dialog-actions" }, [button(t("ui.action.cancel"), () => finish(null)), ok]),
      ]),
    ]);
    const before = document.activeElement;
    function finish(result) {
      box.remove();
      if (before && before.focus) before.focus();
      resolve(result);
    }
    box.addEventListener("keydown", (event) => {
      if (event.key === "Escape") finish(null);
    });
    document.body.appendChild(box);
    update();
    input.focus();
  });
}

/** Report something; a person will read the reason. */
async function report(what) {
  const reason = await promptText(t("ui.cm.reporttitle"), t("ui.cm.reporthint"), { min: 3 });
  if (!reason) return;
  try {
    await cq("POST", "/v0/reports", { ...what, reason });
    toast(t("ui.community.reported"));
  } catch (error) {
    toast(error);
  }
}

/** A row of title posters from a list of `{type, id, title}`. */
function titleRow(items, note) {
  const row = el("div", { className: "title-grid" });
  for (const item of items) {
    const title = item.title || {};
    const tile = el("button", { className: "title-tile", attrs: { type: "button", title: title.name || item.id }, onClick: () => openDetail(item.type, item.id) }, [
      el("div", { className: "cal-thumb tall" }, [title.poster ? image(title.poster, "") : el("span", { text: (title.name || "?").slice(0, 2) })]),
      el("span", { className: "tile-name", text: title.name || item.id }),
      note ? el("span", { className: "hint", text: note(item) }) : null,
    ]);
    row.appendChild(tile);
  }
  return row;
}

/** "Load more", for any paged list. */
function moreButton(load) {
  const holder = el("div", { className: "loading-actions" });
  const press = button(t("ui.cm.more"), async () => {
    press.disabled = true;
    try {
      const done = await load();
      if (done) holder.remove();
      else press.disabled = false;
    } catch (error) {
      toast(error);
      press.disabled = false;
    }
  });
  holder.appendChild(press);
  return holder;
}

/** A ten-bar histogram of scores 1 to 10. */
function histogramBars(histogram) {
  const top = Math.max(1, ...histogram);
  const bars = el("div", { className: "histogram", attrs: { role: "img", "aria-label": histogram.map((n, at) => `${at + 1}: ${n}`).join(", ") } });
  histogram.forEach((n, at) => {
    const bar = el("i");
    bar.style.height = `${Math.round((n / top) * 100)}%`;
    bars.appendChild(el("div", { className: "hist-bar", attrs: { title: `${at + 1}: ${n}` } }, [bar, el("span", { text: String(at + 1) })]));
  });
  return bars;
}

// --------------------------------------------------------------- the page

const CM_TABS = [
  ["feed", "ui.community.feed"],
  ["charts", "ui.cm.charts"],
  ["lists", "ui.cm.lists"],
  ["awards", "ui.cm.awards"],
  ["polls", "ui.cm.polls"],
  ["people", "ui.cm.people"],
  ["friends", "ui.community.friends"],
  ["notifications", "ui.cm.notifications"],
  ["account", "ui.community.account"],
];

async function openCommunity(tab = "feed") {
  showView("community");
  const page = document.getElementById("community-body");
  clear(page);
  await refreshCommunity();

  // No server: what this is, why there is none, and where one would go.
  if (!COMMUNITY.server) {
    page.appendChild(el("p", { className: "lede", text: t("ui.community.none") }));
    page.appendChild(serverForm());
    return;
  }

  // Charts, lists, awards, polls and people are public: they need no account.
  const signedIn = Boolean(COMMUNITY.username);
  const tabs = CM_TABS.filter(([name]) => signedIn || ["charts", "lists", "awards", "polls", "people"].includes(name));
  if (signedIn && isModerator()) tabs.push(["moderation", "ui.cm.moderation"]);
  if (!signedIn) tabs.unshift(["signin", "ui.community.signin"]);
  if (!tabs.some(([name]) => name === tab)) tab = tabs[0][0];

  page.appendChild(
    el("p", {
      className: "lede",
      text: signedIn ? t("ui.community.signedin", COMMUNITY.username, COMMUNITY.server) : t("ui.community.at", COMMUNITY.server),
    })
  );
  const strip = el("div", { className: "chips tabs", attrs: { role: "tablist" } });
  for (const [name, key] of tabs) {
    strip.appendChild(
      el("button", {
        className: "chip",
        text: t(key),
        attrs: { type: "button", role: "tab", "aria-selected": String(name === tab), "aria-pressed": String(name === tab) },
        onClick: () => openCommunity(name),
      })
    );
  }
  page.appendChild(strip);
  const body = el("div", { className: "community-tab" });
  page.appendChild(body);
  try {
    const draw = {
      signin: drawSignIn,
      feed: drawFeed,
      charts: drawCharts,
      lists: drawLists,
      awards: drawAwards,
      polls: drawPolls,
      people: drawPeople,
      friends: drawFriends,
      notifications: drawNotifications,
      account: drawAccount,
      moderation: drawModeration,
    }[tab];
    await draw(body);
  } catch (error) {
    body.appendChild(empty(String(error)));
  }
}

function drawSignIn(body) {
  const forms = el("div", { className: "auth" });
  const [loginField, login] = field(t("ui.community.login"), { autocomplete: "username" });
  const [passField, pass] = field(t("ui.community.password"), { type: "password", autocomplete: "current-password" });
  const signIn = el("form", { className: "card-panel auth-form" }, [
    el("h3", { text: t("ui.community.signin") }),
    loginField,
    passField,
    el("button", { className: "button play", attrs: { type: "submit" }, text: t("ui.community.signin") }),
  ]);
  signIn.addEventListener("submit", async (event) => {
    event.preventDefault();
    try {
      await cq("POST", "/v0/sessions", { login: login.value.trim(), password: pass.value });
      pass.value = "";
      await openCommunity();
      refreshBadge();
    } catch (error) {
      toast(error);
    }
  });
  const [userField, user] = field(t("ui.community.username"), { autocomplete: "username", maxlength: "24" });
  const [mailField, mail] = field(t("ui.community.email"), { type: "email", autocomplete: "email" });
  const [newPassField, newPass] = field(t("ui.community.password"), { type: "password", autocomplete: "new-password", minlength: "10" });
  const register = el("form", { className: "card-panel auth-form" }, [
    el("h3", { text: t("ui.community.register") }),
    el("p", { className: "hint", text: t("ui.community.minimal") }),
    userField,
    mailField,
    newPassField,
    el("button", { className: "button ghost", attrs: { type: "submit" }, text: t("ui.community.register") }),
  ]);
  register.addEventListener("submit", async (event) => {
    event.preventDefault();
    try {
      await cq("POST", "/v0/accounts", { username: user.value.trim().toLowerCase(), email: mail.value.trim(), password: newPass.value });
      await cq("POST", "/v0/sessions", { login: user.value.trim().toLowerCase(), password: newPass.value });
      newPass.value = "";
      toast(t("ui.community.welcome", user.value.trim()));
      openCommunity();
    } catch (error) {
      toast(error);
    }
  });
  forms.appendChild(signIn);
  forms.appendChild(register);
  body.appendChild(forms);
  body.appendChild(serverForm());
}

function serverForm() {
  const [serverField, address] = field(t("ui.community.server"), { type: "url", placeholder: "https://", spellcheck: "false" });
  address.value = COMMUNITY.server || "";
  const form = el("form", { className: "card-panel auth-form" }, [
    el("h3", { text: t("ui.community.server") }),
    el("p", { className: "hint", text: t("ui.community.servernote") }),
    serverField,
    el("button", { className: "button ghost", attrs: { type: "submit" }, text: t("ui.community.useserver") }),
  ]);
  form.addEventListener("submit", async (event) => {
    event.preventDefault();
    try {
      await ask("community_server", { address: address.value });
      openCommunity();
    } catch (error) {
      // `ask` said it.
    }
  });
  return form;
}

// ---------------------------------------------------------------- the feed

/** A score as a line: who, what, how much, and what they wrote. */
function scoreLine(username, rating, extra = [], bare = false) {
  const title = rating.title || {};
  const line = el("div", { className: "score-line" });
  // On the title's own page a review needs neither its poster nor its name.
  if (!bare) {
    const thumb = el("div", { className: "cal-thumb" });
    if (title.poster) thumb.appendChild(image(title.poster, ""));
    line.appendChild(thumb);
  }
  const review = rating.review ? spoilerText(rating.review, rating.spoiler) : null;
  line.appendChild(
    el("div", { className: "grow" }, [
      el("div", { className: "title" }, [
        username ? el("button", { className: "name-link", text: username, attrs: { type: "button" }, onClick: () => openUser(username) }) : null,
        bare ? null : document.createTextNode(username ? ` · ${title.name || rating.id}` : title.name || rating.id),
      ]),
      el("div", { className: "when", text: `★ ${rating.score}/10${title.year ? ` · ${title.year}` : ""}${rating.at ? ` · ${when(rating.at)}` : ""}` }),
      review,
      ...extra,
    ])
  );
  if (!bare) line.appendChild(button(t("ui.action.details"), () => openDetail(rating.type, rating.id), "text-button"));
  return line;
}

/** Text that gives the story away stays covered until it is asked for. */
function spoilerText(text, spoiler) {
  const p = el("p", { className: "synopsis", text });
  if (!spoiler) return p;
  p.classList.add("spoiler");
  p.setAttribute("aria-hidden", "true");
  const reveal = button(t("ui.cm.spoilershow"), () => {
    p.classList.remove("spoiler");
    p.removeAttribute("aria-hidden");
    reveal.remove();
  }, "text-button");
  return el("div", {}, [reveal, p]);
}

async function drawFeed(body) {
  const feed = await cq("GET", "/v0/feed");
  if (!feed.length) {
    body.appendChild(empty(t("ui.community.feedempty")));
    return;
  }
  for (const entry of feed) {
    if (entry.kind === "list") {
      body.appendChild(
        el("div", { className: "score-line" }, [
          el("div", { className: "grow" }, [
            el("div", { className: "title" }, [
              el("button", { className: "name-link", text: entry.username, attrs: { type: "button" }, onClick: () => openUser(entry.username) }),
              document.createTextNode(` · ${t("ui.cm.updatedlist", entry.list.name)}`),
            ]),
            el("div", { className: "when", text: when(entry.at) }),
          ]),
          button(t("ui.cm.open"), () => openList(entry.list.id), "text-button"),
        ])
      );
    } else body.appendChild(scoreLine(entry.username, entry.rating));
  }
}

// ------------------------------------------------------------------ charts

const CHART = { chart: "top", type: "movie", genre: "" };

async function drawCharts(body) {
  clear(body);
  body.appendChild(
    chipRow(
      [["top", t("ui.cm.top250")], ["trending", t("ui.cm.trending")], ["bottom", t("ui.cm.bottom")]],
      CHART.chart,
      (value) => {
        CHART.chart = value;
        drawCharts(body);
      },
      t("ui.cm.charts")
    )
  );
  body.appendChild(chipRow([["movie", typeName("movie")], ["series", typeName("series")]], CHART.type, (value) => {
    CHART.type = value;
    drawCharts(body);
  }));
  if (CHART.chart === "top") {
    const genres = ["", "Action", "Adventure", "Animation", "Comedy", "Crime", "Documentary", "Drama", "Family", "Fantasy", "Horror", "Mystery", "Romance", "Sci-Fi", "Thriller", "War", "Western"];
    body.appendChild(chipRow(genres.map((g) => [g, g || t("ui.cm.allgenres")]), CHART.genre, (value) => {
      CHART.genre = value;
      drawCharts(body);
    }));
  }
  body.appendChild(el("p", { className: "hint", text: t(`ui.cm.${CHART.chart}note`) }));
  const list = el("div", { className: "stack" });
  body.appendChild(list);
  let page = 0;
  const load = async () => {
    const genre = CHART.chart === "top" && CHART.genre ? `&genre=${enc(CHART.genre)}` : "";
    const answer = await cq("GET", `/v0/${CHART.chart}/${CHART.type}?page=${page}${genre}`);
    if (page === 0 && !answer.items.length) list.appendChild(empty(t("ui.community.topempty")));
    for (const entry of answer.items) {
      const line = scoreLine(null, { type: entry.type, id: entry.id, title: entry.title, score: entry.average === undefined ? "–" : Number(entry.average).toFixed(1) });
      line.insertBefore(el("span", { className: "rank", text: String(entry.rank) }), line.firstChild);
      const note = line.querySelector(".when");
      if (CHART.chart === "trending") {
        const change = entry.change === null || entry.change === undefined ? t("ui.cm.new") : entry.change > 0 ? `▲ ${entry.change}` : entry.change < 0 ? `▼ ${-entry.change}` : "=";
        note.textContent = `${change} · ${t("ui.cm.activity", entry.count)}`;
      } else note.textContent = t("ui.community.average", Number(entry.average).toFixed(1), entry.count);
      list.appendChild(line);
    }
    page += 1;
    // The Top 250 ends at 250.
    return !answer.more || (CHART.chart === "top" && page * 20 >= 250);
  };
  const done = await load();
  if (!done) body.appendChild(moreButton(load));
}

// ------------------------------------------------------------------- lists

async function drawLists(body) {
  if (COMMUNITY.username) {
    body.appendChild(
      el("div", { className: "loading-actions" }, [
        button(t("ui.cm.newlist"), () => editList(null), "button play"),
        button(t("ui.cm.watchlist"), async () => {
          try {
            openList((await cq("GET", "/v0/me/watchlist")).id);
          } catch (error) {
            toast(error);
          }
        }),
      ])
    );
    body.appendChild(el("h3", { className: "section-title", text: t("ui.cm.mylists") }));
    const mine = await cq("GET", `/v0/lists?owner=${enc(COMMUNITY.username)}`);
    for (const list of mine.lists) body.appendChild(listLine(list));
  }
  body.appendChild(el("h3", { className: "section-title", text: t("ui.cm.popularlists") }));
  const holder = el("div", { className: "stack" });
  body.appendChild(holder);
  let page = 0;
  const load = async () => {
    const answer = await cq("GET", `/v0/lists?sort=popular&page=${page}`);
    if (page === 0 && !answer.lists.length) holder.appendChild(empty(t("ui.cm.nolists")));
    for (const list of answer.lists) holder.appendChild(listLine(list));
    page += 1;
    return !answer.more;
  };
  if (!(await load())) body.appendChild(moreButton(load));
}

function listLine(list) {
  const what = list.kind === "watchlist" ? t("ui.cm.watchlist") : list.name;
  return el("div", { className: "score-line" }, [
    el("div", { className: "grow" }, [
      el("div", { className: "title", text: what }),
      el("div", {
        className: "when",
        text: [t("ui.cm.bywho", list.owner), t("ui.cm.titles", list.count), t("ui.cm.likes", list.likes), list.public ? null : t("ui.cm.private")]
          .filter(Boolean)
          .join(" · "),
      }),
      list.description ? el("p", { className: "synopsis", text: list.description }) : null,
    ]),
    button(t("ui.cm.open"), () => openList(list.id), "text-button"),
  ]);
}

async function openList(id) {
  showView("community");
  const page = document.getElementById("community-body");
  clear(page);
  let list;
  try {
    list = await cq("GET", `/v0/lists/${id}`);
  } catch (error) {
    toast(error);
    return;
  }
  const mine = list.owner === COMMUNITY.username;
  page.appendChild(button(`← ${t("ui.cm.lists")}`, () => openCommunity("lists"), "text-button"));
  page.appendChild(el("h3", { className: "section-title", text: list.kind === "watchlist" ? t("ui.cm.watchlist") : list.name }));
  page.appendChild(
    el("p", {
      className: "hint",
      text: [t("ui.cm.bywho", list.owner), t("ui.cm.titles", list.count), list.ranked ? t("ui.cm.ranked") : null, list.public ? t("ui.cm.public") : t("ui.cm.private")]
        .filter(Boolean)
        .join(" · "),
    })
  );
  if (list.description) page.appendChild(el("p", { className: "lede", text: list.description }));
  const actions = el("div", { className: "loading-actions" });
  if (mine) {
    actions.appendChild(button(t("ui.cm.editlist"), () => editList(list)));
    if (list.kind !== "watchlist") {
      actions.appendChild(
        button(t("ui.cm.deletelist"), async (event) => {
          const press = event.currentTarget;
          if (!press.dataset.armed) {
            press.dataset.armed = "true";
            press.textContent = t("ui.cm.deletesure");
            return;
          }
          try {
            await cq("DELETE", `/v0/lists/${list.id}`);
            openCommunity("lists");
          } catch (error) {
            toast(error);
          }
        }, "button danger")
      );
    }
  } else if (COMMUNITY.username && list.public) {
    const like = button(list.liked ? t("ui.cm.unlike") : t("ui.cm.like"), async () => {
      try {
        const answer = await cq(list.liked ? "DELETE" : "PUT", `/v0/lists/${list.id}/like`);
        list.liked = !list.liked;
        like.textContent = `${list.liked ? t("ui.cm.unlike") : t("ui.cm.like")} (${answer.likes})`;
      } catch (error) {
        toast(error);
      }
    });
    like.textContent = `${like.textContent} (${list.likes})`;
    actions.appendChild(like);
    actions.appendChild(button(t("ui.community.report"), () => report({ kind: "list", target: { id: list.id } }), "text-button quiet"));
  }
  page.appendChild(actions);
  const items = el("div", { className: "stack" });
  page.appendChild(items);
  const draw = (entries) => {
    entries.forEach((item) => {
      const title = item.title || {};
      const line = el("div", { className: "score-line" }, [
        list.ranked ? el("span", { className: "rank", text: String(item.position) }) : null,
        el("div", { className: "cal-thumb" }, [title.poster ? image(title.poster, "") : null]),
        el("div", { className: "grow" }, [
          el("div", { className: "title", text: title.name || item.id }),
          el("div", { className: "when", text: title.year || "" }),
          item.note ? el("p", { className: "synopsis", text: item.note }) : null,
        ]),
        button(t("ui.action.details"), () => openDetail(item.type, item.id), "text-button"),
        mine
          ? button(t("ui.cm.remove"), async () => {
              try {
                await cq("DELETE", `/v0/lists/${list.id}/items/${enc(item.type)}/${enc(item.id)}`);
                line.remove();
              } catch (error) {
                toast(error);
              }
            }, "text-button quiet")
          : null,
      ]);
      items.appendChild(line);
    });
  };
  if (!list.items.length) items.appendChild(empty(t("ui.cm.listempty")));
  draw(list.items);
  let at = 1;
  if (list.more) {
    page.appendChild(
      moreButton(async () => {
        const next = await cq("GET", `/v0/lists/${list.id}?page=${at}`);
        at += 1;
        draw(next.items);
        return !next.more;
      })
    );
  }
}

/** Make a list, or change one. */
function editList(list) {
  const page = document.getElementById("community-body");
  clear(page);
  const [nameField, name] = field(t("ui.cm.listname"), { maxlength: "100" });
  const [descField, desc] = field(t("ui.cm.listdesc"), { maxlength: "1000", rows: "4" }, "textarea");
  const publicBox = el("input", { attrs: { type: "checkbox" } });
  const rankedBox = el("input", { attrs: { type: "checkbox" } });
  name.value = list ? list.name : "";
  desc.value = list && list.description ? list.description : "";
  publicBox.checked = list ? list.public : true;
  rankedBox.checked = list ? list.ranked : false;
  const watch = list && list.kind === "watchlist";
  const form = el("form", { className: "card-panel auth-form" }, [
    el("h3", { text: list ? t("ui.cm.editlist") : t("ui.cm.newlist") }),
    watch ? null : nameField,
    descField,
    el("label", { className: "check" }, [publicBox, el("span", { text: t("ui.cm.public") })]),
    watch ? null : el("label", { className: "check" }, [rankedBox, el("span", { text: t("ui.cm.ranked") })]),
    el("div", { className: "loading-actions" }, [
      el("button", { className: "button play", attrs: { type: "submit" }, text: t("ui.action.save") }),
      button(t("ui.action.cancel"), () => (list ? openList(list.id) : openCommunity("lists"))),
    ]),
  ]);
  form.addEventListener("submit", async (event) => {
    event.preventDefault();
    const body = { name: watch ? "Watchlist" : name.value.trim(), description: desc.value.trim() || null, public: publicBox.checked, ranked: rankedBox.checked };
    try {
      const saved = list ? await cq("PUT", `/v0/lists/${list.id}`, body) : await cq("POST", "/v0/lists", body);
      openList(saved.id);
    } catch (error) {
      toast(error);
    }
  });
  page.appendChild(form);
  (watch ? desc : name).focus();
}

/** Put a title on one of the viewer's lists, from its page. */
async function addToList(detail, anchor) {
  if (!COMMUNITY.username) {
    toast(t("ui.cm.signinfirst"));
    return;
  }
  let lists;
  try {
    lists = (await cq("GET", `/v0/lists?owner=${enc(COMMUNITY.username)}`)).lists;
  } catch (error) {
    toast(error);
    return;
  }
  const put = async (list) => {
    try {
      await cq("PUT", `/v0/lists/${list.id}/items/${enc(detail.content_type)}/${enc(detail.id)}`, { title: titlePayload(detail) });
      toast(t("ui.cm.added", list.kind === "watchlist" ? t("ui.cm.watchlist") : list.name));
    } catch (error) {
      toast(error);
    }
  };
  openMenu(anchor, [
    ...lists.map((list) => ({ icon: list.kind === "watchlist" ? "bookmark" : "plus", label: list.kind === "watchlist" ? t("ui.cm.watchlist") : list.name, run: () => put(list) })),
    { icon: "plus", label: t("ui.cm.newlist"), run: () => editList(null) },
  ]);
}

// ------------------------------------------------------------------ awards

async function drawAwards(body, event = null, year = null) {
  clear(body);
  const answer = await cq("GET", "/v0/awards");
  if (!answer.events.length) {
    body.appendChild(empty(t("ui.cm.noawards")));
  } else {
    const current = answer.events.find((e) => e.id === event) || answer.events[0];
    const shownYear = year || current.years[0];
    body.appendChild(chipRow(answer.events.map((e) => [e.id, e.name]), current.id, (value) => drawAwards(body, value, null)));
    body.appendChild(chipRow(current.years.map((y) => [y, String(y)]), shownYear, (value) => drawAwards(body, current.id, value)));
    const ceremony = await cq("GET", `/v0/awards/${enc(current.id)}/${shownYear}`);
    for (const category of ceremony.categories) {
      body.appendChild(el("h3", { className: "section-title", text: category.category }));
      for (const nomination of category.nominations) body.appendChild(nominationLine(nomination));
    }
  }
  if (COMMUNITY.username) body.appendChild(nominateForm());
}

function nominationLine(nomination) {
  const title = nomination.title || {};
  return el("div", { className: `score-line${nomination.won ? " won" : ""}` }, [
    el("span", { className: "rank", text: nomination.won ? "★" : "·" }),
    el("div", { className: "grow" }, [
      el("div", { className: "title", text: [title.name, nomination.person].filter(Boolean).join(" — ") }),
      el("div", { className: "when", text: `${nomination.won ? t("ui.cm.won") : t("ui.cm.nominated")} · ${nomination.event_name || nomination.event} ${nomination.year}${nomination.category ? ` · ${nomination.category}` : ""}` }),
    ]),
    nomination.title_id ? button(t("ui.action.details"), () => openDetail(nomination.type, nomination.title_id), "text-button") : null,
    nomination.person_id ? button(t("ui.cm.person"), () => openPerson(nomination.person_id), "text-button") : null,
  ]);
}

function nominateForm(detail = null) {
  const [eventField, eventName] = field(t("ui.cm.event"), { maxlength: "100", placeholder: "Academy Awards" });
  const [yearField, year] = field(t("ui.cm.year"), { type: "number", min: "1900", max: "2100" });
  const [categoryField, category] = field(t("ui.cm.category"), { maxlength: "200", placeholder: "Best Picture" });
  const [personField, person] = field(t("ui.cm.personopt"), { maxlength: "120" });
  const won = el("input", { attrs: { type: "checkbox" } });
  const form = el("form", { className: "card-panel auth-form" }, [
    el("h3", { text: t("ui.cm.nominate") }),
    el("p", { className: "hint", text: detail ? t("ui.cm.nominatefor", detail.name) : t("ui.cm.nominatehint") }),
    eventField,
    yearField,
    categoryField,
    personField,
    el("label", { className: "check" }, [won, el("span", { text: t("ui.cm.won") })]),
    el("button", { className: "button ghost", attrs: { type: "submit" }, text: t("ui.cm.send") }),
  ]);
  form.addEventListener("submit", async (event) => {
    event.preventDefault();
    const id = personId(eventName.value);
    if (!id || !year.value || !category.value.trim()) return;
    const body = { event_name: eventName.value.trim(), category: category.value.trim(), person: person.value.trim() || null, won: won.checked };
    if (detail) Object.assign(body, { type: detail.content_type, id: detail.id, title: titlePayload(detail) });
    try {
      await cq("POST", `/v0/awards/${enc(id)}/${enc(year.value)}`, body);
      toast(t("ui.cm.pending"));
      form.reset();
    } catch (error) {
      toast(error);
    }
  });
  return form;
}

// ------------------------------------------------------------------- polls

async function drawPolls(body) {
  if (COMMUNITY.username) body.appendChild(el("div", { className: "loading-actions" }, [button(t("ui.cm.newpoll"), () => newPoll(body), "button play")]));
  const holder = el("div", { className: "stack" });
  body.appendChild(holder);
  let page = 0;
  const load = async () => {
    const answer = await cq("GET", `/v0/polls?page=${page}`);
    if (page === 0 && !answer.polls.length) holder.appendChild(empty(t("ui.cm.nopolls")));
    for (const poll of answer.polls) {
      holder.appendChild(
        el("div", { className: "score-line" }, [
          el("div", { className: "grow" }, [el("div", { className: "title", text: poll.question }), el("div", { className: "when", text: t("ui.cm.votes", poll.total) })]),
          button(t("ui.cm.open"), () => openPoll(poll.id), "text-button"),
        ])
      );
    }
    page += 1;
    return !answer.more;
  };
  if (!(await load())) body.appendChild(moreButton(load));
}

async function openPoll(id) {
  showView("community");
  const page = document.getElementById("community-body");
  clear(page);
  page.appendChild(button(`← ${t("ui.cm.polls")}`, () => openCommunity("polls"), "text-button"));
  let poll;
  try {
    poll = await cq("GET", `/v0/polls/${id}`);
  } catch (error) {
    toast(error);
    return;
  }
  page.appendChild(el("h3", { className: "section-title", text: poll.question }));
  page.appendChild(el("p", { className: "hint", text: `${t("ui.cm.bywho", poll.author || "—")} · ${t("ui.cm.votes", poll.total)}` }));
  for (const option of poll.options) {
    const share = poll.total ? Math.round((option.votes / poll.total) * 100) : 0;
    const meter = el("div", { className: "meter" }, [el("i")]);
    meter.firstChild.style.width = `${share}%`;
    const label = option.title ? option.title.name : option.label;
    page.appendChild(
      el("div", { className: `score-line poll-option${poll.mine === option.index ? " mine" : ""}` }, [
        el("div", { className: "grow" }, [el("div", { className: "title", text: label }), meter, el("div", { className: "when", text: `${share}% · ${option.votes}` })]),
        option.id ? button(t("ui.action.details"), () => openDetail(option.type, option.id), "text-button") : null,
        COMMUNITY.username
          ? button(poll.mine === option.index ? t("ui.cm.yourvote") : t("ui.cm.vote"), async () => {
              try {
                await cq("PUT", `/v0/polls/${poll.id}/vote`, { option: option.index });
                openPoll(poll.id);
              } catch (error) {
                toast(error);
              }
            }, poll.mine === option.index ? "button play" : "button ghost")
          : null,
      ])
    );
  }
  if (COMMUNITY.username) page.appendChild(button(t("ui.community.report"), () => report({ kind: "poll", target: { id: poll.id } }), "text-button quiet"));
}

function newPoll(body) {
  clear(body);
  const [questionField, question] = field(t("ui.cm.question"), { maxlength: "300" });
  const options = el("div", { className: "stack" });
  const addOption = () => {
    const [optionField] = field(t("ui.cm.option", options.children.length + 1), { maxlength: "200" });
    options.appendChild(optionField);
  };
  addOption();
  addOption();
  const form = el("form", { className: "card-panel auth-form" }, [
    el("h3", { text: t("ui.cm.newpoll") }),
    el("p", { className: "hint", text: t("ui.cm.moderated") }),
    questionField,
    options,
    button(t("ui.cm.addoption"), () => options.children.length < 20 && addOption()),
    el("button", { className: "button play", attrs: { type: "submit" }, text: t("ui.cm.send") }),
  ]);
  form.addEventListener("submit", async (event) => {
    event.preventDefault();
    const labels = [...options.querySelectorAll("input")].map((input) => input.value.trim()).filter(Boolean);
    try {
      await cq("POST", "/v0/polls", { question: question.value.trim(), options: labels.map((label) => ({ label })) });
      toast(t("ui.cm.pending"));
      openCommunity("polls");
    } catch (error) {
      toast(error);
    }
  });
  body.appendChild(form);
  question.focus();
}

// ------------------------------------------------------------------ people

async function drawPeople(body) {
  const [searchField, query] = field(t("ui.cm.findperson"), { type: "search", autocomplete: "off", maxlength: "80" });
  const results = el("div", { className: "stack" });
  let typing = 0;
  query.addEventListener("input", () => {
    clearTimeout(typing);
    typing = setTimeout(async () => {
      clear(results);
      const text = query.value.trim();
      if (text.length < 2) return;
      try {
        const found = await cq("GET", `/v0/people?q=${enc(text)}`);
        for (const person of found.items) {
          results.appendChild(
            el("div", { className: "person" }, [
              el("button", { className: "name-link", text: person.name, attrs: { type: "button" }, onClick: () => openPerson(person.id) }),
              el("span", { className: "hint", text: person.known_for.map((k) => k.title.name).join(", ") }),
            ])
          );
        }
        if (!found.items.length) results.appendChild(el("div", { className: "hint", text: t("ui.community.nobody") }));
      } catch (error) {
        toast(error);
      }
    }, 300);
  });
  body.appendChild(el("p", { className: "hint", text: t("ui.cm.peoplenote") }));
  body.appendChild(searchField);
  body.appendChild(results);
  query.focus();
}

/** A person's page: details, what they are known for, filmography, awards,
 *  and the facts people added. `who` is a name or an id. */
async function openPerson(who) {
  showView("community");
  await refreshCommunity();
  const page = document.getElementById("community-body");
  clear(page);
  const id = personId(who);
  let person;
  try {
    person = await cq("GET", `/v0/people/${enc(id)}`);
  } catch (error) {
    // Not known to the server yet: offer the addons' search instead.
    page.appendChild(el("h3", { className: "section-title", text: who }));
    page.appendChild(el("p", { className: "lede", text: t("ui.cm.personunknown") }));
    page.appendChild(button(t("ui.detail.searchname", who), () => runSearch(who), "button play"));
    return;
  }
  page.appendChild(el("h3", { className: "section-title person-name", text: person.name }));
  const facts = [
    person.born ? t("ui.cm.born", person.born) : null,
    person.birthplace ? person.birthplace : null,
    person.died ? t("ui.cm.died", person.died) : null,
  ].filter(Boolean);
  if (facts.length) page.appendChild(el("p", { className: "hint", text: facts.join(" · ") }));
  if (person.bio) page.appendChild(el("p", { className: "lede", text: person.bio }));
  page.appendChild(
    el("div", { className: "loading-actions" }, [
      button(t("ui.detail.searchname", person.name), () => runSearch(person.name), "button ghost"),
      COMMUNITY.username ? button(t("ui.cm.suggestedit"), () => suggestPersonEdit(person), "text-button") : null,
    ])
  );
  if (person.known_for.length) {
    page.appendChild(el("h3", { className: "section-title", text: t("ui.cm.knownfor") }));
    page.appendChild(titleRow(person.known_for));
  }
  page.appendChild(el("h3", { className: "section-title", text: t("ui.cm.filmography", person.filmography.length) }));
  page.appendChild(titleRow(person.filmography, (item) => (item.role === "director" ? t("ui.cm.roledirector") : t("ui.cm.rolecast"))));
  if (person.awards.length) {
    page.appendChild(el("h3", { className: "section-title", text: t("ui.cm.awards") }));
    for (const award of person.awards) page.appendChild(nominationLine({ ...award, title: null, person: null }));
  }
  const factsBox = el("div");
  page.appendChild(factsBox);
  drawFacts(factsBox, `/v0/people/${enc(id)}/facts`, ["trivia", "quote", "bio", "nickname", "trademark", "salary"], { name: person.name });
}

async function suggestPersonEdit(person) {
  const fields = ["born", "died", "birthplace", "bio"];
  const anchor = document.querySelector(".person-name");
  openMenu(
    anchor,
    fields.map((name) => ({
      icon: "plus",
      label: t(`ui.cm.field${name}`),
      run: async () => {
        const value = await promptText(t(`ui.cm.field${name}`), t("ui.cm.moderated"), { min: 1, max: name === "bio" ? 5000 : 120, multiline: name === "bio", value: person[name] || "" });
        if (value === null) return;
        try {
          await cq("POST", `/v0/people/${enc(person.id)}/edits`, { field: name, value, name: person.name });
          toast(t("ui.cm.pending"));
        } catch (error) {
          toast(error);
        }
      },
    }))
  );
}

/** Trivia, goofs, quotes and the rest, for a title or a person: chips per
 *  kind, interest votes, and a form to add one. */
async function drawFacts(box, path, kinds, subject) {
  clear(box);
  let kind = kinds[0];
  const head = el("h3", { className: "section-title", text: t("ui.cm.facts") });
  const chips = el("div");
  const list = el("div", { className: "stack" });
  box.appendChild(head);
  box.appendChild(chips);
  box.appendChild(list);
  const draw = async () => {
    clear(list);
    let answer;
    try {
      answer = await cq("GET", `${path}?kind=${enc(kind)}`);
    } catch (error) {
      list.appendChild(empty(String(error)));
      return;
    }
    clear(chips);
    chips.appendChild(
      chipRow(
        kinds.map((k) => [k, answer.counts[k] ? `${t(`ui.fact.${k}`)} ${answer.counts[k]}` : t(`ui.fact.${k}`)]),
        kind,
        (value) => {
          kind = value;
          draw();
        }
      )
    );
    if (!answer.facts.length) list.appendChild(el("div", { className: "hint", text: t("ui.cm.nofacts") }));
    for (const fact of answer.facts) list.appendChild(factLine(fact));
    if (COMMUNITY.username) list.appendChild(factForm(path, kind, subject, draw));
  };
  draw();
}

function factLine(fact) {
  const votes = el("span", { className: "hint" });
  const counted = (up, down) => {
    votes.textContent = up + down ? t("ui.cm.interesting", up, up + down) : "";
  };
  counted(fact.interesting, fact.not_interesting);
  const vote = (value) => async () => {
    try {
      const answer = await cq("PUT", `/v0/facts/${fact.id}/vote`, { interesting: value });
      counted(answer.interesting, answer.not_interesting);
    } catch (error) {
      toast(error);
    }
  };
  const mine = fact.author === COMMUNITY.username;
  return el("div", { className: "fact" }, [
    spoilerText(fact.text, fact.spoiler),
    el("div", { className: "fact-foot" }, [
      fact.state === "pending" ? el("span", { className: "badge", text: t("ui.cm.pendingbadge") }) : null,
      votes,
      COMMUNITY.username && !mine ? el("span", { className: "hint", text: t("ui.cm.wasinteresting") }) : null,
      COMMUNITY.username && !mine ? button(t("ui.cm.yes"), vote(true), "text-button") : null,
      COMMUNITY.username && !mine ? button(t("ui.cm.no"), vote(false), "text-button") : null,
      mine
        ? button(t("ui.cm.withdraw"), async (event) => {
            try {
              await cq("DELETE", `/v0/facts/${fact.id}`);
              event.currentTarget.closest(".fact").remove();
            } catch (error) {
              toast(error);
            }
          }, "text-button quiet")
        : COMMUNITY.username
          ? button(t("ui.community.report"), () => report({ kind: "fact", target: { id: fact.id } }), "text-button quiet")
          : null,
    ]),
  ]);
}

function factForm(path, kind, subject, done) {
  const [textField, text] = field(t("ui.cm.addfact", t(`ui.fact.${kind}`)), { maxlength: "2000", rows: "3" }, "textarea");
  const spoiler = el("input", { attrs: { type: "checkbox" } });
  const form = el("form", { className: "fact-form" }, [
    textField,
    el("label", { className: "check" }, [spoiler, el("span", { text: t("ui.cm.spoiler") })]),
    el("button", { className: "button ghost", attrs: { type: "submit" }, text: t("ui.cm.send") }),
  ]);
  form.addEventListener("submit", async (event) => {
    event.preventDefault();
    try {
      await cq("POST", path, { kind, text: text.value.trim(), spoiler: spoiler.checked, ...subject });
      toast(t("ui.cm.pending"));
      done();
    } catch (error) {
      toast(error);
    }
  });
  return form;
}

// ------------------------------------------------------------------ friends

async function drawFriends(body) {
  const [searchField, query] = field(t("ui.community.find"), { type: "search", autocomplete: "off", maxlength: "24" });
  const results = el("div", { className: "stack" });
  let typing = 0;
  query.addEventListener("input", () => {
    clearTimeout(typing);
    typing = setTimeout(async () => {
      clear(results);
      const text = query.value.trim();
      if (text.length < 2) return;
      try {
        const found = await cq("GET", `/v0/users?q=${enc(text)}`);
        for (const person of found) results.appendChild(personRow(person.username, t("ui.community.scores", person.count)));
        if (!found.length) results.appendChild(el("div", { className: "hint", text: t("ui.community.nobody") }));
      } catch (error) {
        toast(error);
      }
    }, 350);
  });
  body.appendChild(searchField);
  body.appendChild(results);

  const lists = await cq("GET", "/v0/friends");
  const section = (key, names, actions) => {
    if (!names.length) return;
    body.appendChild(el("h3", { className: "section-title", text: t(key) }));
    for (const name of names) body.appendChild(personRow(name, "", actions(name)));
  };
  section("ui.community.incoming", lists.incoming, (name) => [
    button(t("ui.community.accept"), () => befriend(name, true), "button play"),
    button(t("ui.community.decline"), () => befriend(name, false), "text-button quiet"),
  ]);
  section("ui.community.friendlist", lists.friends, (name) => [button(t("ui.community.unfriend"), () => befriend(name, false), "text-button quiet")]);
  section("ui.community.outgoing", lists.outgoing, (name) => [button(t("ui.community.withdraw"), () => befriend(name, false), "text-button quiet")]);
  if (!lists.friends.length && !lists.incoming.length && !lists.outgoing.length) {
    body.appendChild(el("div", { className: "hint", text: t("ui.community.nofriends") }));
  }
}

function personRow(username, note, actions = []) {
  return el("div", { className: "person" }, [
    el("button", { className: "name-link", text: username, attrs: { type: "button" }, onClick: () => openUser(username) }),
    note ? el("span", { className: "hint", text: note }) : null,
    el("span", { className: "grow" }),
    ...actions,
  ]);
}

async function befriend(username, yes) {
  try {
    if (yes) await cq("POST", `/v0/friends/${enc(username)}`);
    else await cq("DELETE", `/v0/friends/${enc(username)}`);
    openCommunity("friends");
  } catch (error) {
    toast(error);
  }
}

/** Someone's page: their numbers, their lists and newest scores, a friend
 *  request, a block and a report. */
async function openUser(username) {
  showView("community");
  const page = document.getElementById("community-body");
  clear(page);
  let person;
  try {
    person = await cq("GET", `/v0/users/${enc(username)}`);
  } catch (error) {
    toast(error);
    return;
  }
  page.appendChild(el("h3", { className: "section-title", text: person.username }));
  if (person.bio) page.appendChild(el("p", { className: "lede", text: person.bio }));
  page.appendChild(
    el("div", { className: "stat-row" }, [
      el("div", { className: "stat" }, [el("strong", { text: String(person.count) }), el("span", { text: t("ui.profile.count") })]),
      el("div", { className: "stat" }, [el("strong", { text: person.average === null ? "—" : Number(person.average).toFixed(1) }), el("span", { text: t("ui.community.theiraverage") })]),
    ])
  );
  if (person.count) page.appendChild(histogramBars(person.histogram));
  if (COMMUNITY.username && COMMUNITY.username !== person.username) {
    const state = person.friend;
    page.appendChild(
      el("div", { className: "loading-actions" }, [
        state === "friends"
          ? button(t("ui.community.unfriend"), () => befriend(person.username, false))
          : button(state === "asked" ? t("ui.cm.asked") : state === "incoming" ? t("ui.community.accept") : t("ui.community.addfriend"), async () => {
              try {
                const answer = await cq("POST", `/v0/friends/${enc(person.username)}`);
                toast(answer.state === "friends" ? t("ui.community.nowfriends", person.username) : t("ui.community.asked", person.username));
              } catch (error) {
                toast(error);
              }
            }, "button play", { disabled: state === "asked" ? "true" : null }),
        button(t("ui.cm.block"), async () => {
          try {
            await cq("PUT", `/v0/blocks/${enc(person.username)}`);
            toast(t("ui.cm.blocked", person.username));
          } catch (error) {
            toast(error);
          }
        }, "text-button quiet"),
        button(t("ui.community.report"), () => report({ kind: "user", target: { username: person.username } }), "text-button quiet"),
      ])
    );
  }
  if (person.lists.length) {
    page.appendChild(el("h3", { className: "section-title", text: t("ui.cm.lists") }));
    for (const list of person.lists) page.appendChild(personRow(list.name, t("ui.cm.titles", list.count), [button(t("ui.cm.open"), () => openList(list.id), "text-button")]));
  }
  page.appendChild(el("h3", { className: "section-title", text: t("ui.cm.recentscores") }));
  for (const rating of person.ratings) page.appendChild(scoreLine(null, rating));
}

// ------------------------------------------------------------ notifications

function notificationText(n) {
  const b = n.body || {};
  switch (n.kind) {
    case "friend":
      return b.state === "friends" ? t("ui.community.nowfriends", b.username) : t("ui.cm.nfriend", b.username);
    case "helpful":
      return t("ui.cm.nhelpful", b.username, (b.title && b.title.name) || b.id);
    case "list_like":
      return t("ui.cm.nlistlike", b.username, b.name);
    case "decision":
      if (b.report) return t(b.outcome === "upheld" ? "ui.cm.nreportupheld" : "ui.cm.nreportdismissed", b.note || "");
      return t("ui.cm.ndecision", t(`ui.cm.action.${b.action}`) || b.action, b.note || "");
    default:
      return n.kind;
  }
}

async function drawNotifications(body) {
  const answer = await cq("GET", "/v0/me/notifications");
  if (!answer.items.length) {
    body.appendChild(empty(t("ui.cm.nonotes")));
    return;
  }
  for (const n of answer.items) {
    const b = n.body || {};
    body.appendChild(
      el("div", { className: `score-line${n.read ? "" : " unread"}` }, [
        el("div", { className: "grow" }, [el("div", { className: "title", text: notificationText(n) }), el("div", { className: "when", text: when(n.at) })]),
        n.kind === "decision" && b.log && !b.report ? button(t("ui.cm.appeal"), () => appeal(b.log), "text-button") : null,
        b.list ? button(t("ui.cm.open"), () => openList(b.list), "text-button") : null,
        n.kind === "friend" ? button(t("ui.cm.open"), () => openUser(b.username), "text-button") : null,
      ])
    );
  }
  try {
    await cq("POST", "/v0/me/notifications", { read: "all" });
    refreshBadge();
  } catch (error) {
    // Unread stays unread.
  }
}

async function appeal(decision) {
  const text = await promptText(t("ui.cm.appeal"), t("ui.cm.appealhint"), { min: 10 });
  if (!text) return;
  try {
    await cq("POST", "/v0/appeals", { decision, text });
    toast(t("ui.cm.appealsent"));
  } catch (error) {
    toast(error);
  }
}

/** The number of unread notifications, on the side menu's Community. */
async function refreshBadge() {
  const dot = document.getElementById("community-dot");
  if (!dot) return;
  dot.hidden = true;
  if (!COMMUNITY.username) return;
  try {
    const answer = await cq("GET", "/v0/me/notifications");
    dot.hidden = answer.unread === 0;
    dot.textContent = String(answer.unread);
    dot.setAttribute("aria-label", t("ui.cm.unread", answer.unread));
  } catch (error) {
    // No badge.
  }
}

// ------------------------------------------------------------------ account

async function drawAccount(body) {
  const me = await cq("GET", "/v0/me");
  body.appendChild(el("p", { text: t("ui.community.me", me.username, me.email) }));
  if (me.suspended_until && me.suspended_until * 1000 > Date.now()) body.appendChild(el("p", { className: "lede warn", text: t("ui.cm.suspended", when(me.suspended_until)) }));
  const [bioField, bio] = field(t("ui.cm.bio"), { maxlength: "300", rows: "3" }, "textarea");
  bio.value = me.bio || "";
  const bioForm = el("form", { className: "card-panel auth-form" }, [bioField, el("button", { className: "button ghost", attrs: { type: "submit" }, text: t("ui.action.save") })]);
  bioForm.addEventListener("submit", async (event) => {
    event.preventDefault();
    try {
      await cq("PUT", "/v0/me", { bio: bio.value.trim() || null });
      toast(t("ui.cm.saved"));
    } catch (error) {
      toast(error);
    }
  });
  body.appendChild(bioForm);

  const [oldField, old] = field(t("ui.cm.oldpassword"), { type: "password", autocomplete: "current-password" });
  const [newField, fresh] = field(t("ui.cm.newpassword"), { type: "password", autocomplete: "new-password", minlength: "10" });
  const passForm = el("form", { className: "card-panel auth-form" }, [
    el("h3", { text: t("ui.cm.changepassword") }),
    oldField,
    newField,
    el("button", { className: "button ghost", attrs: { type: "submit" }, text: t("ui.cm.changepassword") }),
  ]);
  passForm.addEventListener("submit", async (event) => {
    event.preventDefault();
    try {
      await cq("PUT", "/v0/me/password", { old: old.value, new: fresh.value });
      toast(t("ui.cm.passwordchanged"));
      openCommunity("signin");
    } catch (error) {
      toast(error);
    }
  });
  body.appendChild(passForm);

  const out = el("pre", { className: "export" });
  out.hidden = true;
  body.appendChild(
    el("div", { className: "loading-actions" }, [
      button(t("ui.community.signout"), async () => {
        try {
          await cq("DELETE", "/v0/sessions");
        } catch (error) {
          // Signed out here either way.
        }
        openCommunity();
        refreshBadge();
      }),
      button(t("ui.community.export"), async () => {
        try {
          out.textContent = JSON.stringify(await cq("GET", "/v0/me/export"), null, 2);
          out.hidden = false;
        } catch (error) {
          toast(error);
        }
      }),
      button(t("ui.community.delete"), async (event) => {
        const press = event.currentTarget;
        // Two presses: the second one is the decision.
        if (!press.dataset.armed) {
          press.dataset.armed = "true";
          press.textContent = t("ui.community.deletesure");
          return;
        }
        try {
          await cq("DELETE", "/v0/me");
          toast(t("ui.community.deleted"));
          openCommunity();
        } catch (error) {
          toast(error);
        }
      }, "button danger"),
    ])
  );
  body.appendChild(out);
}

// --------------------------------------------------------------- moderation

async function drawModeration(body, kind = "") {
  clear(body);
  body.appendChild(
    chipRow(
      [["", t("ui.cm.allqueue")], ["report", t("ui.cm.q.report")], ["fact", t("ui.cm.q.fact")], ["edit", t("ui.cm.q.edit")], ["nomination", t("ui.cm.q.nomination")], ["poll", t("ui.cm.q.poll")], ["appeal", t("ui.cm.q.appeal")], ["log", t("ui.cm.log")]],
      kind,
      (value) => drawModeration(body, value)
    )
  );
  body.appendChild(el("p", { className: "hint", text: t("ui.cm.modnote") }));
  if (kind === "log") {
    const log = await cq("GET", "/v0/mod/log");
    for (const entry of log.entries) {
      body.appendChild(
        el("div", { className: "score-line" }, [
          el("div", { className: "grow" }, [
            el("div", { className: "title", text: `${entry.moderator} · ${t(`ui.cm.action.${entry.action}`)}${entry.subject ? ` · ${entry.subject}` : ""}` }),
            el("div", { className: "when", text: `${when(entry.at)}${entry.appealed ? ` · ${t("ui.cm.appealed")}` : ""}` }),
            entry.note ? el("p", { className: "synopsis", text: entry.note }) : null,
          ]),
        ])
      );
    }
    return;
  }
  const queue = await cq("GET", `/v0/mod/queue${kind ? `?kind=${kind}` : ""}`);
  if (!queue.items.length) body.appendChild(empty(t("ui.cm.queueempty")));
  for (const item of queue.items) body.appendChild(queueItem(item, () => drawModeration(body, kind)));
}

function queueItem(item, done) {
  const c = item.content || {};
  const lines = [];
  if (item.kind === "fact") {
    lines.push(`${t(`ui.fact.${c.fact_kind}`)} · ${c.about && c.about.name ? c.about.name : c.subject}`);
    lines.push(c.text);
  } else if (item.kind === "edit") {
    lines.push(`${c.name} · ${t(`ui.cm.field${c.field}`)}`);
    lines.push(c.value || "—");
  } else if (item.kind === "nomination") {
    lines.push(`${c.event} ${c.year} · ${c.category}`);
    lines.push([c.title && c.title.name, c.person, c.won ? t("ui.cm.won") : t("ui.cm.nominated")].filter(Boolean).join(" — "));
  } else if (item.kind === "poll") {
    lines.push(c.question);
    lines.push(c.options.map((o) => o.label || o.id).join(" / "));
  } else if (item.kind === "report") {
    const target = c.target || {};
    const got = c.reported || {};
    const what = [t(`ui.cm.r.${c.report_kind}`)];
    if (target.username) what.push(target.username);
    if (got.title && got.title.name) what.push(got.title.name);
    lines.push(what.join(" · "));
    lines.push(t("ui.cm.reason", c.reason));
    // What was reported, as it reads now -- or that it is already gone.
    const shown =
      (got.review && (got.review.review ? `★ ${got.review.score} — ${got.review.review}` : `★ ${got.review.score}`)) ||
      (got.fact && got.fact.text) ||
      (got.list && [got.list.name, got.list.description].filter(Boolean).join(" — ")) ||
      got.poll ||
      got.bio ||
      t("ui.cm.gone");
    lines.push(shown);
  } else if (item.kind === "appeal") {
    lines.push(`${t(`ui.cm.action.${c.decision.action}`)} · ${c.decision.moderator}${c.decision.note ? ` — ${c.decision.note}` : ""}`);
    lines.push(c.text);
  }
  const decide = (decision) => async () => {
    const note = await promptText(t(`ui.cm.d.${decision}`), t("ui.cm.notehint"), { min: 0 });
    if (note === null) return;
    try {
      await cq("POST", `/v0/mod/queue/${item.item}`, { decision, note: note || null });
      done();
    } catch (error) {
      toast(error);
    }
  };
  const actions =
    item.kind === "report"
      ? [button(t("ui.cm.d.remove"), decide("remove"), "button danger"), button(t("ui.cm.d.dismiss"), decide("dismiss"))]
      : [button(t("ui.cm.d.approve"), decide("approve"), "button play"), button(t("ui.cm.d.reject"), decide("reject"))];
  return el("div", { className: "queue-item card-panel" }, [
    el("div", { className: "when", text: `${t(`ui.cm.q.${item.kind}`)} · ${item.author || "—"} · ${when(item.at)}` }),
    ...lines.map((text, at) => el(at === 0 ? "div" : "p", { className: at === 0 ? "title" : "synopsis", text })),
    el("div", { className: "loading-actions" }, actions),
  ]);
}

// ------------------------------------------------------- a title's page

/** What the community says about a title, on its page: the score and its
 *  spread, the reviews with their votes, and tabs for facts, the parental
 *  guide, awards, lists and "more like this". Drawn only when a server is
 *  set; nothing is asked otherwise. */
async function communityBox(detail, container) {
  await refreshCommunity();
  if (!COMMUNITY.server) return;
  const base = `/v0/titles/${enc(detail.content_type)}/${enc(detail.id)}`;
  let title;
  try {
    title = await cq("GET", base);
  } catch (error) {
    return;
  }
  if (DETAIL !== detail) return;
  const box = el("section", { className: "community-box" }, [el("h2", { className: "section-title", text: t("ui.community.title") })]);
  box.appendChild(
    el("div", { className: "cm-score" }, [
      el("p", { className: "lede", text: title.average === null ? t("ui.community.few", title.count) : t("ui.community.average", Number(title.average).toFixed(1), title.count) }),
      title.count ? histogramBars(title.histogram) : null,
    ])
  );
  if (COMMUNITY.username) {
    const add = button(t("ui.cm.addtolist"), () => addToList(detail, add), "button ghost");
    box.appendChild(el("div", { className: "loading-actions" }, [add]));
  }

  const tabs = [
    ["reviews", t("ui.cm.reviews")],
    ["facts", t("ui.cm.facts")],
    ["parental", t("ui.cm.parental")],
    ["similar", t("ui.cm.similar")],
    ["awards", t("ui.cm.awards")],
    ["lists", t("ui.cm.lists")],
  ];
  const panel = el("div", { className: "cm-panel" });
  const strip = el("div");
  const show = async (name) => {
    clear(strip);
    strip.appendChild(chipRow(tabs, name, show));
    clear(panel);
    try {
      if (name === "reviews") await drawReviews(panel, detail, base);
      else if (name === "facts") await drawFacts(panel, `${base}/facts`, ["trivia", "goof", "quote", "crazy_credit", "alternate_version", "soundtrack", "connection", "keyword", "tagline", "synopsis", "plot", "location", "release", "technical", "box_office", "faq"], { title: titlePayload(detail) });
      else if (name === "parental") await drawParental(panel, detail, base);
      else if (name === "similar") {
        const similar = await cq("GET", `${base}/similar`);
        panel.appendChild(el("p", { className: "hint", text: t("ui.cm.similarnote") }));
        if (!similar.items.length) panel.appendChild(empty(t("ui.cm.nosimilar")));
        else panel.appendChild(titleRow(similar.items, (item) => t("ui.cm.incommon", item.count)));
      } else if (name === "awards") {
        const awards = await cq("GET", `${base}/awards`);
        panel.appendChild(el("p", { className: "lede", text: t("ui.cm.awardcount", awards.wins, awards.nominations) }));
        for (const nomination of awards.items) panel.appendChild(nominationLine(nomination));
        if (COMMUNITY.username) panel.appendChild(nominateForm(detail));
      } else if (name === "lists") {
        const lists = await cq("GET", `${base}/lists`);
        if (!lists.lists.length) panel.appendChild(empty(t("ui.cm.onnolists")));
        for (const list of lists.lists) panel.appendChild(listLine(list));
      }
    } catch (error) {
      panel.appendChild(empty(String(error)));
    }
  };
  box.appendChild(strip);
  box.appendChild(panel);
  container.appendChild(box);
  show("reviews");
}

async function drawReviews(panel, detail, base, sort = "helpful") {
  clear(panel);
  panel.appendChild(chipRow([["helpful", t("ui.cm.mosthelpful")], ["newest", t("ui.cm.newest")]], sort, (value) => drawReviews(panel, detail, base, value)));
  const list = el("div", { className: "stack" });
  panel.appendChild(list);
  let page = 0;
  const load = async () => {
    const answer = await cq("GET", `${base}/reviews?page=${page}&sort=${sort}`);
    if (page === 0 && !answer.reviews.length) list.appendChild(empty(t("ui.cm.noreviews")));
    for (const review of answer.reviews) list.appendChild(reviewLine(review, detail));
    page += 1;
    return !answer.more;
  };
  if (!(await load())) panel.appendChild(moreButton(load));
}

function reviewLine(review, detail) {
  const path = `/v0/reviews/${enc(review.username)}/${enc(detail.content_type)}/${enc(detail.id)}/helpful`;
  const count = el("span", { className: "hint" });
  const counted = (up, down) => {
    count.textContent = up + down ? t("ui.cm.helpfulcount", up, up + down) : "";
  };
  counted(review.helpful, review.unhelpful);
  const vote = (helpful) => async () => {
    try {
      const answer = await cq("PUT", path, { helpful });
      counted(answer.helpful, answer.unhelpful);
    } catch (error) {
      toast(error);
    }
  };
  const mine = review.username === COMMUNITY.username;
  const foot = el("div", { className: "fact-foot" }, [
    count,
    COMMUNITY.username && !mine ? el("span", { className: "hint", text: t("ui.cm.washelpful") }) : null,
    COMMUNITY.username && !mine ? button(t("ui.cm.yes"), vote(true), "text-button") : null,
    COMMUNITY.username && !mine ? button(t("ui.cm.no"), vote(false), "text-button") : null,
    COMMUNITY.username && !mine ? button(t("ui.community.report"), () => report({ kind: "review", target: { username: review.username, type: detail.content_type, id: detail.id } }), "text-button quiet") : null,
  ]);
  return scoreLine(review.username, { ...review, type: detail.content_type, id: detail.id, title: { name: detail.name } }, [foot], true);
}

async function drawParental(panel, detail, base) {
  clear(panel);
  const guide = await cq("GET", `${base}/parental`);
  panel.appendChild(el("p", { className: "hint", text: t("ui.cm.parentalnote") }));
  for (const category of guide.categories) {
    const total = Object.values(category.votes).reduce((a, b) => a + b, 0);
    const verdict = category.verdict ? t(`ui.cm.level.${category.verdict}`) : t("ui.cm.toofew");
    const row = el("div", { className: "parental-row" }, [
      el("div", { className: "title", text: t(`ui.cm.cat.${category.category}`) }),
      el("div", { className: `verdict level-${category.verdict || "none"}`, text: verdict }),
      el("div", { className: "hint", text: t("ui.cm.votes", total) }),
    ]);
    if (COMMUNITY.username) {
      row.appendChild(
        chipRow(
          ["none", "mild", "moderate", "severe"].map((level) => [level, t(`ui.cm.level.${level}`)]),
          category.mine,
          async (level) => {
            try {
              await cq("PUT", `${base}/parental/${category.category}`, { level, title: titlePayload(detail) });
              drawParental(panel, detail, base);
            } catch (error) {
              toast(error);
            }
          },
          t(`ui.cm.cat.${category.category}`)
        )
      );
    }
    panel.appendChild(row);
  }
}

/** Share a score the viewer just gave, when they are signed in. */
async function shareRating(rating, detail) {
  if (!COMMUNITY.username) return;
  const path = `/v0/ratings/${enc(detail.content_type)}/${enc(detail.id)}`;
  try {
    if (!rating) await cq("DELETE", path);
    else await cq("PUT", path, { score: rating.score, review: rating.review || null, spoiler: Boolean(rating.spoiler), title: titlePayload(detail) });
  } catch (error) {
    toast(t("ui.community.notshared", String(error)));
  }
}
