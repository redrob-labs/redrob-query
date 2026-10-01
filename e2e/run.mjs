#!/usr/bin/env node
// End-to-end: the real desktop build, driven over W3C WebDriver through tauri-driver.
//
//   add a SQLite connection -> "Query this table" on people -> Run -> edit row 1's name
//   -> Apply -> Run again -> the new name is on screen (and the CI step then reads it from the file)
//
// Every step is a click or a keystroke a person could make; nothing calls a native command
// directly. Elements are reached with `execute/sync` page scripts, as in redrob-recall's E2E: on
// WebKitWebDriver, element lookup by XPath or CSS failed for elements present in the page source.
//
// No test framework: a handful of HTTP calls, and the WebdriverIO dependency tree carried
// high-severity advisories that no override reached.
//
// Needs: tauri-driver on WEBDRIVER_URL (default http://127.0.0.1:4444), the debug app at APP, and
// REDROB_E2E_DB pointing at a SQLite file with a `people` table (see e2e/seed.sql). Screenshots go
// to SHOTS (default e2e/screenshots).
import { mkdirSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';

const DRIVER = process.env.WEBDRIVER_URL ?? 'http://127.0.0.1:4444';
const APP = process.env.APP;
const DB = process.env.REDROB_E2E_DB;
const SHOTS = process.env.SHOTS ?? 'e2e/screenshots';
const NEW_NAME = 'Ann Lee-Kim';
if (!APP || !DB) {
  console.error('APP and REDROB_E2E_DB must be set');
  process.exit(2);
}
mkdirSync(SHOTS, { recursive: true });

async function wd(method, path, body) {
  const response = await fetch(`${DRIVER}${path}`, {
    method,
    headers: { 'content-type': 'application/json' },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  const json = await response.json();
  if (!response.ok || json.value?.error) throw new Error(`${method} ${path}: ${json.value?.error} ${json.value?.message ?? ''}`);
  return json.value;
}

let session;
const js = (body, ...args) => wd('POST', `/session/${session}/execute/sync`, { script: body, args });

/** Click the element with this data-testid; false when it is not on screen or disabled. */
const click = (id) => js("const el = document.querySelector('[data-testid=\"' + arguments[0] + '\"]'); if (!el || el.disabled) return false; el.click(); return true;", id);
/** Click the button or menu item whose accessible name or visible text is exactly `label`. Exact on
 * purpose: "Connect" must not hit "Connection actions", which a contains-match did on the first run. */
const clickLabel = (label) => js(
  `const want = arguments[0];
   const els = [...document.querySelectorAll('button, [role=menuitem], [role=tab]')];
   const el = els.find((e) => !e.disabled && (e.getAttribute('aria-label') === want || (e.textContent ?? '').trim() === want));
   if (!el) return false; el.click(); return true;`, label);
/** True when some element's visible text includes `fragment`. */
const shows = (fragment) => js("return (document.body.innerText ?? '').includes(arguments[0]);", fragment);
/** Type into a React-controlled field: set through the native setter, then fire `input`. */
const fill = (id, value) => js(
  `const el = document.querySelector('[data-testid="' + arguments[0] + '"]');
   if (!el) return false;
   const proto = el instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
   Object.getOwnPropertyDescriptor(proto, 'value').set.call(el, arguments[1]);
   el.dispatchEvent(new Event('input', { bubbles: true }));
   return true;`, id, value);
/** Text of the grid cell with this accessible name, or null. */
const cell = (name) => js("const el = document.querySelector('[role=cell][aria-label=\"' + arguments[0] + '\"]'); return el ? el.textContent : null;", name);

async function shot(name) {
  const png = await wd('GET', `/session/${session}/screenshot`);
  writeFileSync(join(SHOTS, `${name}.png`), Buffer.from(png, 'base64'));
}
async function until(what, check, seconds = 60) {
  const deadline = Date.now() + seconds * 1000;
  let last = 'the check returned nothing';
  for (;;) {
    try {
      const value = await check();
      if (value) return value;
    } catch (error) {
      if (error.fatal) throw error;
      last = error.message;
    }
    if (Date.now() > deadline) throw new Error(`timed out waiting for ${what}; last: ${last}`);
    await new Promise((resolve) => setTimeout(resolve, 500));
  }
}

let failed = false;
try {
  session = (await wd('POST', '/session', { capabilities: { alwaysMatch: { 'tauri:options': { application: APP } } } })).sessionId;

  // 1. Add the SQLite file as a connection.
  await until('the add-connection button', () => click('open-connection-modal'));
  await until('the SQLite choice', () => click('database-sqlite'));
  await until('the name field', () => fill('connection-name', 'E2E people'));
  await until('the file field', () => fill('connection-file', DB));
  await shot('1-connection');
  await until('Save to accept a click', () => click('save-connection'));
  // The saved connection becomes the active one; connect it from the sidebar's actions menu.
  await until('the new connection to be active', () => shows('E2E people'));
  await until('the connection actions menu', async () => (await clickLabel('Connection actions')) && clickLabel('Connect'));
  await until('the connection to connect', () => shows('E2E people · connected'));

  // 2. "Query this table" on people writes and opens the SELECT.
  // SQLite puts tables under a schema node; open collapsed nodes until the table's menu is there.
  await until('the people table and its menu', async () => {
    if (await clickLabel('Actions for people')) return clickLabel('Query this table');
    await js("const el = document.querySelector('[data-testid=navigator] button[aria-expanded=false]'); if (el) el.click(); return true;");
    return false;
  });
  await until('Run to accept a click', () => click('run-query'));
  const before = await until("row 1's name", () => cell('name, result row 1'));
  if (before !== 'Ann') throw new Error(`row 1 name before the edit is ${JSON.stringify(before)}, not "Ann"`);
  await shot('2-result');

  // 3. Edit the cell in place, as a double-click and typing would, then leave it.
  await until('the name cell to be editable', () => js(
    `const el = document.querySelector('[role=cell][aria-label="name, result row 1"]');
     if (!el || el.getAttribute('contenteditable') !== 'true') return false;
     el.focus(); el.textContent = arguments[0]; el.dispatchEvent(new FocusEvent('blur')); el.blur(); return true;`, NEW_NAME));
  await until('the staged change', () => js("return [...document.querySelectorAll('button')].some((b) => /1 staged change/.test(b.textContent ?? ''));"));
  await shot('3-staged');

  // 4. Apply, then read the table again.
  await until('the changes panel', async () => (await text('changes-panel')) !== null || clickLabel('1 staged change'));
  await until('Apply to accept a click', () => click('apply-changes'));
  // Apply reports a failure as a toast that fades in seconds, so read it while waiting.
  await until('the staged change to clear', async () => {
    const failure = await js("const t = document.body.innerText ?? ''; const i = t.indexOf('Could not apply changes'); return i < 0 ? null : t.slice(i, i + 300);");
    if (failure) throw Object.assign(new Error(`Apply failed: ${failure}`), { fatal: true });
    return js("return ![...document.querySelectorAll('button')].some((b) => /staged change/.test(b.textContent ?? ''));");
  });
  await until('Run to accept a click', () => click('run-query'));
  await until('the new name after a fresh run', async () => (await cell('name, result row 1')) === NEW_NAME);
  await shot('4-applied');
  console.log(`e2e: SQLite connection -> Query this table -> Run -> edit name -> Apply -> Run -> "${NEW_NAME}"`);
} catch (error) {
  failed = true;
  console.error(`e2e failed: ${error.message}`);
  if (session) {
    await shot('failure').catch(() => {});
    const source = await wd('GET', `/session/${session}/source`).catch((e) => `source unavailable: ${e.message}`);
    writeFileSync(join(SHOTS, 'failure.html'), String(source));
  }
} finally {
  if (session) await wd('DELETE', `/session/${session}`).catch(() => {});
}
process.exit(failed ? 1 : 0);

async function text(id) {
  return js("const el = document.querySelector('[data-testid=\"' + arguments[0] + '\"]'); return el ? el.textContent : null;", id);
}
