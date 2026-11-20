// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 EON contributors
//
// Additional permission under GNU GPL version 3 section 7:
// see LICENSE-EXCEPTION.md (EON Module ABI Exception 1.0).
//
// EON Stream's account flow in the real window, against a real account server
// (plan madde 46): first start asks, the recovery key is shown once, Settings
// shows the server's own list of what it keeps, a wrong password is said at its
// field, and the recovery key sets a new password. The executable is copied to
// a fresh folder first, because EON Stream keeps everything beside itself.
//
//   node tools/account-check.mjs <debug eon-stream.exe> <eon-account-server.exe> <screenshots dir> [eon-community.exe]
//
// With the community server, the window also signs in to it with EON Account.
//
// The window is driven over the Chrome DevTools protocol, which a debug build
// opens when EON_WEBVIEW_ARGS asks; the release build ignores it.
import { spawn } from "node:child_process";
import { copyFileSync, mkdirSync, mkdtempSync, writeFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

const [streamExe, serverExe, shots, communityExe] = process.argv.slice(2);
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const root = mkdtempSync(join(tmpdir(), "eon-stream-account-"));
mkdirSync(join(root, "app"));
copyFileSync(streamExe, join(root, "app", "eon-stream.exe"));
mkdirSync(shots, { recursive: true });
const port = 9400 + Math.floor(Math.random() * 400);
const apiPort = 8800 + Math.floor(Math.random() * 100);
const api = `http://127.0.0.1:${apiPort}`;

const server = spawn(serverExe, ["--data", join(root, "server"), "--listen", `127.0.0.1:${apiPort}`, "--outbox", join(root, "outbox")], { stdio: "ignore" });
let community = null;
const app = spawn(join(root, "app", "eon-stream.exe"), [], { env: { ...process.env, EON_WEBVIEW_ARGS: `--remote-debugging-port=${port}` }, stdio: "ignore" });

let failures = 0;
const check = (what, ok) => { console.log(`${ok ? "  ok  " : "  FAIL"} ${what}`); if (!ok) failures++; };

let cdp;
async function devtools() {
  for (let i = 0; i < 300; i++) {
    try {
      const pages = await (await fetch(`http://127.0.0.1:${port}/json`)).json();
      const page = pages.find((p) => p.type === "page" && p.webSocketDebuggerUrl && !p.url.includes("player"));
      if (page) {
        const ws = new WebSocket(page.webSocketDebuggerUrl);
        await new Promise((ok, no) => { ws.onopen = ok; ws.onerror = no; });
        let next = 1; const waiting = new Map();
        ws.onmessage = (e) => { const m = JSON.parse(e.data); if (m.id && waiting.has(m.id)) { waiting.get(m.id)(m); waiting.delete(m.id); } };
        return { send: (method, params = {}) => new Promise((ok) => { const id = next++; waiting.set(id, ok); ws.send(JSON.stringify({ id, method, params })); }) };
      }
    } catch { /* not yet */ }
    await sleep(200);
  }
  throw new Error("no DevTools endpoint");
}
async function js(expression) {
  const r = await cdp.send("Runtime.evaluate", { expression, awaitPromise: true, returnByValue: true });
  if (r.result?.exceptionDetails) throw new Error(JSON.stringify(r.result.exceptionDetails).slice(0, 400));
  return r.result?.result?.value;
}
async function until(expression, ms = 15000) {
  const end = Date.now() + ms;
  while (Date.now() < end) { if (await js(expression).catch(() => false)) return true; await sleep(150); }
  return false;
}
async function shot(name) {
  for (let i = 0; i < 5; i++) {
    const r = await cdp.send("Page.captureScreenshot", { format: "png" });
    if (r.result?.data) { writeFileSync(join(shots, `account-${name}.png`), Buffer.from(r.result.data, "base64")); return; }
    await sleep(300);
  }
  console.log(`  (no screenshot for ${name})`);
}
const setValue = (selector, value) => js(`(() => { const n = document.querySelector(${JSON.stringify(selector)}); n.value = ${JSON.stringify(value)}; n.dispatchEvent(new Event('input', { bubbles: true })); return true; })()`);
const click = (selector) => js(`(() => { const n = document.querySelector(${JSON.stringify(selector)}); n.click(); return true; })()`);
const clickText = (text) => js(`(() => { const n = [...document.querySelectorAll('button')].find((b) => b.textContent.trim() === ${JSON.stringify(text)} && b.offsetParent !== null); if (!n) return false; n.click(); return true; })()`);

try {
  cdp = await devtools();
  await cdp.send("Emulation.setDeviceMetricsOverride", { width: 1280, height: 860, deviceScaleFactor: 1, mobile: false });
  check("first start asks about the account before anything else", await until(`!document.getElementById('welcome').hidden && !!document.querySelector('.account')`));
  check("the sign-in tab is chosen, the guest choice is there", await js(`document.querySelector('.account-tab[aria-pressed=true]').textContent === t('ui.account.signin') && [...document.querySelectorAll('.account button')].some((b) => b.textContent === t('ui.account.guest'))`));
  check("the server field is open and empty, saying EON runs none", await js(`!document.querySelector('.account-server summary') && document.getElementById('acc-server').value === ''`));
  check("password fields let a password manager fill them", await js(`document.getElementById('acc-password').autocomplete === 'current-password'`));
  await shot("01-signin");

  await clickText(await js(`t('ui.account.create')`));
  await until(`document.getElementById('acc-password').autocomplete === 'new-password'`);
  await setValue("#acc-server", api);
  await setValue("#acc-login", "Deniz");
  await setValue("#acc-password", "short");
  await click(".account button[type=submit]");
  check("a short password is refused at the field, before anything is sent", await until(`document.getElementById('acc-password-error').textContent === t('ui.account.error.weak_password')`, 3000));
  await setValue("#acc-password", "correct horse battery");
  await click(".account .field-toggle");
  check("show reveals the password", await js(`document.getElementById('acc-password').type === 'text'`));
  await shot("02-create");
  await click(".account button[type=submit]");
  check("creating the account shows the recovery key, once", await until(`!!document.querySelector('.recovery-key')`, 20000));
  const key = await js(`[...document.querySelectorAll('.recovery-key span')].map((s) => s.textContent).join('-')`);
  check("the key is eight groups of four", /^([A-Z2-7]{4}-){7}[A-Z2-7]{4}$/.test(key));
  check("continuing waits for the person to say they kept it", await js(`[...document.querySelectorAll('#welcome button')].find((b) => b.textContent === t('ui.account.continue')).disabled`));
  await shot("03-key");
  await click("#acc-kept");
  await clickText(await js(`t('ui.account.continue')`));
  check("then the usual first-start questions", await until(`document.querySelector('.welcome-title')?.textContent === t('ui.welcome.hello')`));
  await clickText(await js(`t('ui.welcome.skip')`));
  await until(`document.getElementById('welcome').hidden`);

  await js(`go('settings')`);
  check("Settings says who is signed in", await until(`document.querySelector('#account-panel .account-who')?.textContent === t('ui.account.signedin', 'deniz')`));
  check("and that it has synced", await until(`(async () => { const s = await invoke('account_state'); return !!s.last_sync; })()`, 20000));
  await js(`loadSettings()`);
  await until(`!!document.querySelector('#account-panel .account-who')`);
  await clickText(await js(`t('ui.account.keeps')`));
  check("the server's own list of what it keeps is shown", await until(`document.querySelectorAll('.keeps-item').length === 7`));
  await js(`document.getElementById('account-panel').scrollIntoView()`);
  await shot("04-settings");

  // Sign out; sign in again from the menu; a wrong password is said at the field.
  await clickText(await js(`t('ui.account.signout')`));
  check("signing out returns to the invitation", await until(`!!document.querySelector('#account-panel') && !document.querySelector('#account-panel .account-who')`));
  await clickText(await js(`t('ui.account.open')`));
  await until(`!!document.querySelector('.account')`);
  check("the server is remembered and folded away", await js(`!document.querySelector('.account-server').open`));
  await setValue("#acc-login", "deniz");
  await setValue("#acc-password", "the wrong password");
  await click(".account button[type=submit]");
  check("a wrong password is said at the password field", await until(`document.getElementById('acc-password-error').textContent === t('ui.account.error.unauthorised')`, 20000));
  await shot("05-wrong");
  await setValue("#acc-password", "correct horse battery");
  await click(".account button[type=submit]");
  check("the right one signs in", await until(`document.getElementById('welcome').hidden`, 20000));

  // Recovery with the key, in Turkish.
  await js(`(async () => { adoptStrings(await invoke('language_set', { code: 'tr' })); })()`);
  await clickText(await js(`t('ui.account.signout')`)).catch(() => {});
  await js(`loadSettings()`);
  await until(`!!document.querySelector('#account-panel')`);
  await js(`(async () => { await invoke('account_logout'); })()`);
  await js(`openAccount(null)`);
  await until(`!!document.querySelector('.account')`);
  await clickText(await js(`t('ui.account.forgot')`));
  await until(`!!document.getElementById('acc-key')`);
  await setValue("#acc-login", "deniz");
  await setValue("#acc-key", key.toLowerCase());
  await setValue("#acc-password", "a new password for deniz");
  await shot("06-recover-tr");
  await click(".account button[type=submit]");
  check("the recovery key sets a new password (in Turkish)", await until(`document.getElementById('welcome').hidden`, 20000));
  check("and the account is signed in again", await js(`(async () => (await invoke('account_state')).username === 'deniz')()`));

  if (communityExe) {
    // A community server that trusts this account server's key, given on its
    // command line -- it asks nothing of the account server itself.
    const keys = await (await fetch(`${api}/v0/keys`)).json();
    const k = keys.keys[0];
    const cPort = apiPort + 100;
    const origin = `http://127.0.0.1:${cPort}`;
    community = spawn(communityExe, ["serve", "--listen", `127.0.0.1:${cPort}`, "--db", join(root, "community.sqlite"), "--origin", origin, "--trust-account", `${keys.issuer},${k.kid},${k.public}`], { stdio: "ignore" });
    await sleep(800);
    await js(`(async () => { await invoke('community_server', { address: ${JSON.stringify(origin)} }); })()`);
    await js(`go('community')`);
    check("the community offers EON Account to a signed-in viewer", await until(`[...document.querySelectorAll('button')].some((b) => b.textContent === t('ui.community.withaccount', 'deniz'))`));
    await shot("07-community-tr");
    await clickText(await js(`t('ui.community.withaccount', 'deniz')`));
    check("one press signs in to the community, under the EON name", await until(`(async () => (await invoke('community_state')).username === 'deniz')()`, 20000));
  }
} catch (error) {
  console.error(error);
  failures++;
} finally {
  app.kill();
  server.kill();
  if (community) community.kill();
  await sleep(500);
  try { rmSync(root, { recursive: true, force: true }); } catch { /* the webview may hold files a moment longer */ }
}
console.log(failures ? `\n${failures} failure(s)` : "\nall account checks pass");
process.exit(failures ? 1 : 0);
