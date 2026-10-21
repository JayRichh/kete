// The desktop end-to-end harness: the built kete-desktop.exe, opened by
// tauri-driver (msedgedriver underneath, driving its WebView2), over a
// real daemon in temp dirs, with the TUI suite's scripted fake agent as
// the `claude` harness's program.
//
// Every test owns a temp root: the runtime and data dirs (never the
// user's), a home, the WebView2 profile, the repos, and the daemon the
// window starts there (`KETE_BIN` points it at this checkout's `kete`).
// tauri-driver, msedgedriver, the window, the daemon and its agents all
// inherit the environment set here.

import { spawn, spawnSync, execFileSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import net from 'node:net';
import { WebDriver, Key, ELEMENT } from './webdriver.mjs';
import { paths } from './paths.mjs';

/** How long one window state may take to come up: a cold daemon, git
 *  worktrees and agents. */
export const WAIT = 30_000;
const POLL = 100;

/** The fake agent's own switches (crates/nebula/tests/e2e_windows/agent.rs). */
const AGENT = {
  ROLE: 'KETE_E2E_FAKE_AGENT',
  SCRIPT: 'KETE_E2E_SCRIPT',
  LOG: 'KETE_E2E_AGENT_LOG',
  ECHO_LOG: 'KETE_E2E_ECHO_LOG',
  GATES: 'KETE_E2E_GATES',
  // The agent binary is the TUI suite too: if the role were ever lost on
  // the way down, it runs nothing rather than a suite of TUIs.
  SKIP: 'KETE_E2E_SKIP',
};

/** The feature the fake agent builds: a question, a file, a commit. */
/** The board's lanes by id, as their columns are labelled. */
const LANE_TITLE = { running: 'Running', needs: 'Needs you', unseen: 'Review', pr: 'Ship', done: 'Done' };

export const FEATURE = 'ask;edit:src/feature.txt:made by the fake agent;commit:add the feature;stop;echo';

export function git(cwd, ...args) {
  return execFileSync('git', args, { cwd, encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'] });
}

function freePort() {
  return new Promise((resolve, reject) => {
    const s = net.createServer();
    s.unref();
    s.on('error', reject);
    s.listen(0, '127.0.0.1', () => {
      const { port } = s.address();
      s.close(() => resolve(port));
    });
  });
}

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

/** The clipboard's text ('' when it holds none), read through PowerShell.
 *  Base64 on the way out keeps any text whole: no console code page, no
 *  trailing newline. */
export function clipboardText() {
  const out = execFileSync(
    'powershell',
    ['-NoProfile', '-Command', '$t = Get-Clipboard -Raw; if ($t) { [Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes($t)) }'],
    { encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'] },
  );
  return Buffer.from(out.trim(), 'base64').toString('utf8');
}

/** Put `text` on the clipboard. It goes through the environment, never the
 *  command line, so no quoting can touch it. */
export function setClipboard(text) {
  execFileSync('powershell', ['-NoProfile', '-Command', 'Set-Clipboard -Value $env:KETE_E2E_CLIP'], {
    env: { ...process.env, KETE_E2E_CLIP: text },
    stdio: ['ignore', 'pipe', 'pipe'],
  });
}

/** The PATH directories holding `gh`, for a window that must not find it. */
export function ghDirs() {
  const out = spawnSync('where', ['gh'], { encoding: 'utf8' });
  if (out.status !== 0) return [];
  return out.stdout
    .split(/\r?\n/)
    .map((l) => l.trim())
    .filter(Boolean)
    .map((l) => path.dirname(l));
}

export class Desktop {
  /**
   * @param {{ script?: string, scripts?: Record<string,string>, stripPath?: string[], config?: object, env?: Record<string,string> }} options
   *   `env`: variables for the window and everything it starts, set after the
   *   KETE_* strip (`{ KETE_BROWSER_KEYS: 'off' }` gives the release keys).
   */
  static async open(options = {}) {
    const d = new Desktop(options);
    try {
      await d.#start();
    } catch (e) {
      await d.close();
      throw e;
    }
    return d;
  }

  constructor(options) {
    this.options = options;
    const root = fs.realpathSync.native(fs.mkdtempSync(path.join(os.tmpdir(), 'kete-desk-e2e-')));
    this.root = root;
    this.runtimeDir = path.join(root, 'rt');
    this.dataDir = path.join(root, 'data');
    this.home = path.join(root, 'home');
    this.repos = path.join(root, 'repos');
    this.gates = path.join(root, 'gates');
    this.webview = path.join(root, 'webview');
    this.agentLog = path.join(root, 'agent.log');
    this.driverLog = path.join(root, 'driver.log');
    for (const dir of [this.runtimeDir, this.dataDir, this.home, this.repos, this.gates, this.webview]) {
      fs.mkdirSync(dir, { recursive: true });
    }
    // The look and first-run state the assertions read: no tips or What's
    // new over the window, and the fake agent as Claude, so the quick
    // prompt launches it and the daemon installs Claude's hooks for it.
    fs.writeFileSync(
      this.configPath,
      JSON.stringify({
        theme: 'default',
        icons: 'off',
        quick_prompt_new_worktree: true,
        show_tips: false,
        whats_new_seen: '999.0',
        harnesses: { claude: { program: paths().agent } },
        ...options.config,
      }),
    );
    this.driver = null;
    this.wd = null;
  }

  get configPath() {
    return path.join(this.dataDir, 'config.json');
  }

  config() {
    return JSON.parse(fs.readFileSync(this.configPath, 'utf8'));
  }

  #env() {
    const env = { ...process.env };
    // Nothing of the session this runs from, nor of a real Kete or Claude.
    for (const k of Object.keys(env)) {
      if (/^(NEBULA_|KETE_|BRO_|CLAUDE|WEBVIEW2_)/i.test(k)) delete env[k];
    }
    for (const k of ['NO_COLOR', 'FORCE_COLOR', 'CODEX_HOME', 'COPILOT_HOME']) delete env[k];
    Object.assign(env, {
      NEBULA_RUNTIME_DIR: this.runtimeDir,
      NEBULA_DATA_DIR: this.dataDir,
      NEBULA_WORKTREE_SYNC_MS: '100',
      NEBULA_UPDATE_CHECK_SECS: '0',
      NEBULA_LOG: 'debug',
      KETE_BIN: paths().kete,
      HOME: this.home,
      CLAUDE_CONFIG_DIR: path.join(this.home, '.claude'),
      CODEX_HOME: path.join(this.home, '.codex'),
      COPILOT_HOME: path.join(this.home, '.copilot'),
      WEBVIEW2_USER_DATA_FOLDER: this.webview,
      GIT_TERMINAL_PROMPT: '0',
      GCM_INTERACTIVE: 'never',
      [AGENT.ROLE]: '1',
      [AGENT.SKIP]: '1',
      [AGENT.SCRIPT]: this.options.script ?? 'stop;echo',
      [AGENT.LOG]: this.agentLog,
      [AGENT.ECHO_LOG]: path.join(this.root, 'echo.log'),
      [AGENT.GATES]: this.gates,
    });
    for (const [word, script] of Object.entries(this.options.scripts ?? {})) {
      env[`${AGENT.SCRIPT}_${word.toUpperCase()}`] = script;
    }
    // What a flow asks of its window (KETE_BROWSER_KEYS=off for the
    // release keys): after the strip above, so it can set KETE_ ones.
    Object.assign(env, this.options.env ?? {});
    const strip = (this.options.stripPath ?? []).map((d) => d.replace(/\\+$/, '').toLowerCase());
    if (strip.length) {
      const key = Object.keys(env).find((k) => k.toUpperCase() === 'PATH') ?? 'PATH';
      env[key] = env[key]
        .split(';')
        .filter((dir) => !strip.includes(dir.replace(/\\+$/, '').toLowerCase()))
        .join(';');
    }
    return env;
  }

  async #start() {
    const port = await freePort();
    const native = await freePort();
    const log = fs.openSync(this.driverLog, 'a');
    this.driver = spawn(
      paths().tauriDriver,
      ['--port', String(port), '--native-port', String(native), '--native-driver', paths().edgeDriver],
      { env: this.#env(), stdio: ['ignore', log, log], windowsHide: true },
    );
    this.wd = new WebDriver(`http://127.0.0.1:${port}`);
    const deadline = Date.now() + WAIT;
    for (;;) {
      try {
        await this.wd.call('GET', '/status');
        break;
      } catch (e) {
        if (Date.now() > deadline) throw new Error(`tauri-driver never came up: ${e}\n${this.tail(this.driverLog)}`);
        await sleep(200);
      }
    }
    await this.wd.start(paths().desktop);
    // Before anything else: the window is on this test's daemon, never
    // the user's.
    const state = await this.waitFor('the window on the test daemon', async () => {
      const s = await this.invoke('get_state');
      return s.connected ? s : null;
    });
    if (!path.resolve(state.runtime_dir).toLowerCase().startsWith(this.root.toLowerCase())) {
      throw new Error(`the window reached a daemon outside the test root: ${state.runtime_dir}`);
    }
  }

  async close() {
    try {
      await this.wd?.quit();
    } catch {}
    if (this.driver?.pid) {
      spawnSync('taskkill', ['/T', '/F', '/PID', String(this.driver.pid)], { stdio: 'ignore' });
    }
    spawnSync(paths().kete, ['kill'], { env: this.#env(), stdio: 'ignore', timeout: 15_000 });
    // The daemon and its agents let go of the dirs a moment after.
    for (let i = 0; i < 20; i++) {
      try {
        fs.rmSync(this.root, { recursive: true, force: true });
        return;
      } catch {
        await sleep(250);
      }
    }
  }

  tail(file, n = 40) {
    try {
      return fs.readFileSync(file, 'utf8').split('\n').slice(-n).join('\n');
    } catch {
      return '';
    }
  }

  // ---- the page ----

  /** A src-tauri command, called as the window calls it. */
  invoke(cmd, args = {}) {
    return this.wd.exec('return window.__TAURI_INTERNALS__.invoke(arguments[0], arguments[1]);', cmd, args);
  }

  /** The page's text, Monaco's no-break spaces as spaces. */
  async text() {
    return (await this.wd.exec('return document.body.innerText;')).replace(/\u00A0/g, ' ');
  }

  /** Poll `probe` until it returns something truthy. */
  async waitFor(what, probe, timeout = WAIT) {
    const deadline = Date.now() + timeout;
    let last;
    for (;;) {
      try {
        const v = await probe();
        if (v) return v;
      } catch (e) {
        last = e;
      }
      if (Date.now() > deadline) {
        let page = '';
        try {
          page = await this.text();
        } catch {}
        throw new Error(`timed out waiting for ${what}${last ? ` (${last.message})` : ''}\n--- page ---\n${page}\n--- agent log ---\n${this.tail(this.agentLog)}`);
      }
      await sleep(POLL);
    }
  }

  waitForText(needle, timeout) {
    return this.waitFor(JSON.stringify(needle), async () => (await this.text()).includes(needle), timeout);
  }

  waitForGone(needle, timeout) {
    return this.waitFor(`${JSON.stringify(needle)} gone`, async () => !(await this.text()).includes(needle), timeout);
  }

  waitForAgent(needle) {
    return this.waitFor(`agent log ${JSON.stringify(needle)}`, () => this.tail(this.agentLog, 400).includes(needle));
  }

  /** The visible element `css` whose text holds `text` (or the first
   *  visible match), as a WebDriver element. */
  async el(css, text) {
    return this.waitFor(`${css}${text ? ` with ${JSON.stringify(text)}` : ''}`, async () => {
      const found = await this.wd.exec(
        `const [css, text] = arguments;
         for (const e of document.querySelectorAll(css)) {
           if (text && !(e.innerText || e.textContent || e.value || '').includes(text)) continue;
           const r = e.getBoundingClientRect();
           if (r.width && r.height) return e;
         }
         return null;`,
        css,
        text ?? null,
      );
      return found ? found[ELEMENT] : null;
    });
  }

  /** Click it; a node the window redrew between the find and the click
   *  (a card whose model just moved) is found again. */
  async click(css, text) {
    for (let i = 0; ; i++) {
      try {
        return await this.wd.click(await this.el(css, text));
      } catch (e) {
        if (i >= 5 || !/stale element/.test(e.message)) throw e;
      }
    }
  }

  /** Right-click it (`at`: offset from its centre, to land on a spot of
   *  it); a node redrawn between the find and the click is found again. */
  async contextClick(css, text, at) {
    for (let i = 0; ; i++) {
      try {
        return await this.wd.contextClick(await this.el(css, text), at);
      } catch (e) {
        if (i >= 5 || !/stale element/.test(e.message)) throw e;
      }
    }
  }

  async type(css, value) {
    const el = await this.el(css);
    await this.wd.click(el);
    await this.wd.type(el, value);
  }

  press(key, ...mods) {
    return this.wd.press(key, ...mods);
  }

  /** Until Kete's menu named `label` is up and visible (a submenu is
   *  named for its row); resolves to its rows as `{ label, disabled,
   *  submenu, checked }`, read in the page. A greyed row is
   *  aria-disabled, so its tooltip still shows. */
  menu(label) {
    return this.waitFor(`the ${JSON.stringify(label)} menu`, () =>
      this.wd.exec(
        `const label = arguments[0];
         for (const m of document.querySelectorAll('[role="menu"]')) {
           if (m.getAttribute('aria-label') !== label) continue;
           const r = m.getBoundingClientRect();
           if (!r.width || !r.height || getComputedStyle(m).visibility === 'hidden') continue;
           return [...m.querySelectorAll('[role^="menuitem"]')].map((b) => ({
             label: (b.querySelector('[data-label]') ?? b).innerText,
             disabled: b.disabled || b.getAttribute('aria-disabled') === 'true',
             submenu: b.getAttribute('aria-haspopup') === 'menu',
             checked: b.getAttribute('aria-checked'),
           }));
         }
         return null;`,
        label,
      ),
    );
  }

  /** The words of the menu row the keys are on (its menu's active
   *  descendant), or null. */
  activeRow() {
    return this.wd.exec(`const m = [...document.querySelectorAll('[role="menu"][aria-activedescendant]')].pop();
      const id = m && m.getAttribute('aria-activedescendant');
      const b = id && document.getElementById(id);
      return b ? (b.querySelector('[data-label]') ?? b).innerText : null;`);
  }

  /** Let a model the daemon just changed reach the window: src-tauri
   *  pushes it at most every 50 ms. */
  settle() {
    return sleep(250);
  }

  /** The window as a PNG at `file` (W3C Take Screenshot), folders made. */
  async screenshot(file) {
    fs.mkdirSync(path.dirname(file), { recursive: true });
    fs.writeFileSync(file, Buffer.from(await this.wd.screenshot(), 'base64'));
    return file;
  }

  openGate(name) {
    fs.writeFileSync(path.join(this.gates, name), '');
  }

  /** The model's sessions, as the window has them. */
  async agents() {
    return (await this.invoke('get_state')).agents;
  }

  /** The board lane session `id` sits in. */
  async laneOf(id) {
    const s = await this.invoke('get_state');
    return Object.entries(s.lanes).find(([, ids]) => ids.includes(id))?.[0] ?? null;
  }

/** Until session `id` is in `lane` in the model and, while the board
   *  is up, its card is drawn in that lane (the window takes the model a
   *  moment after the daemon has it). */
  async waitForLane(id, lane, timeout) {
    const title = LANE_TITLE[lane];
    await this.waitFor(
      `session in ${lane}`,
      async () =>
        (await this.laneOf(id)) === lane &&
        (await this.wd.exec(
          `const [id, title] = arguments;
           if (!document.querySelector('[data-board]')) return true;
           return document.querySelector('[data-target="agent:' + id + '"]')?.closest('section')?.getAttribute('aria-label') === title;`,
          id,
          title,
        )),
      timeout,
    );
  }

  // ---- repos ----

  /** A repo with one commit and a local bare repo as `origin` (its `main`
   *  pushed), so the base is `origin/main`. `github` names `origin` as a
   *  GitHub URL, pushed to the bare repo underneath. */
  makeRepo(name, { github = false } = {}) {
    const repo = path.join(this.repos, name);
    const bare = path.join(this.repos, `${name}-origin.git`);
    fs.mkdirSync(repo, { recursive: true });
    git(this.repos, 'init', '-q', '--bare', '-b', 'main', bare);
    git(repo, 'init', '-q', '-b', 'main');
    git(repo, 'config', 'user.email', 'e2e@kete.dev');
    git(repo, 'config', 'user.name', 'kete-e2e');
    git(repo, 'config', 'core.autocrlf', 'false');
    fs.writeFileSync(path.join(repo, 'README.md'), 'e2e\n');
    git(repo, 'add', '.');
    git(repo, 'commit', '-q', '-m', 'init');
    if (github) {
      const url = `https://github.com/kete-e2e/${name}.git`;
      git(repo, 'remote', 'add', 'origin', url);
      git(repo, 'config', `url.${bare}.pushInsteadOf`, url);
    } else {
      git(repo, 'remote', 'add', 'origin', bare);
    }
    git(repo, 'push', '-q', '-u', 'origin', 'main');
    git(repo, 'remote', 'set-head', 'origin', 'main');
    return repo;
  }

  /** The repo's linked worktrees (every checkout but its own). */
  linkedWorktrees(repo) {
    return git(repo, 'worktree', 'list', '--porcelain')
      .split(/\r?\n/)
      .filter((l) => l.startsWith('worktree '))
      .map((l) => path.resolve(l.slice('worktree '.length)))
      .slice(1);
  }

  /** Add `repo` through the rail's Add a repo, and wait for its name. */
  async addRepo(repo) {
    await this.click('button', 'Add a repo');
    await this.type('input[aria-label="Repo folder"]', repo + Key.Enter);
    await this.waitFor('the repo in the model', async () => (await this.invoke('get_state')).projects.length > 0);
    await this.waitForText(path.basename(repo));
  }

  // ---- sessions ----

  /** The quick prompt: `task` typed and started. Resolves to the new
   *  session's id. */
  async launch(task) {
    const before = new Set((await this.agents()).map((a) => a.id));
    await this.waitFor('the quick prompt ready', () =>
      this.wd.exec(`const b = [...document.querySelectorAll('form[aria-label="Start a session"] button[type=submit]')]; return b.some((x) => !x.disabled);`),
    );
    await this.type('form[aria-label="Start a session"] textarea[aria-label="Prompt"]', task);
    await this.press(Key.Enter);
    const a = await this.waitFor(`the session for ${JSON.stringify(task)}`, async () =>
      (await this.agents()).find((x) => !before.has(x.id)),
    );
    return a.id;
  }

  /** Type a line into the open session's terminal and press Enter. */
  async replyInPane(text) {
    const el = await this.el('.xterm-helper-textarea');
    await this.wd.type(el, text);
    await this.wd.type(el, Key.Enter);
  }

  // ---- drag ----

  /** A drag, at the event level: WebDriver's pointer actions don't run
   *  HTML5 drag and drop in Chromium, so the page gets the events a real
   *  drag fires, in order, with one DataTransfer, from the card's own
   *  element to the target's; the board's own handlers do the rest. */
  async drag(fromCss, toCss) {
    const out = await this.wd.exec(
      `const [fromCss, toCss] = arguments;
       const frame = () => new Promise((r) => requestAnimationFrame(() => setTimeout(r, 0)));
       const from = document.querySelector(fromCss);
       if (!from) return 'no source ' + fromCss;
       const dt = new DataTransfer();
       const at = (el) => { const r = el.getBoundingClientRect(); return { clientX: r.left + r.width / 2, clientY: r.top + r.height / 2 }; };
       const fire = (el, type) => el.dispatchEvent(new DragEvent(type, { bubbles: true, cancelable: true, dataTransfer: dt, ...at(el) }));
       fire(from, 'dragstart');
       await frame();
       let to = null;
       for (let i = 0; i < 50 && !to; i++) { to = document.querySelector(toCss); if (!to) await frame(); }
       if (!to) { fire(from, 'dragend'); return 'no target ' + toCss; }
       fire(to, 'dragenter');
       fire(to, 'dragover');
       await frame();
       const hint = to.querySelector('[role=status]')?.innerText ?? '';
       const taken = !fire(to, 'drop');
       fire(from, 'dragend');
       return { taken, hint };`,
      fromCss,
      toCss,
    );
    if (!out?.taken) throw new Error(`drag ${fromCss} -> ${toCss}: ${typeof out === 'string' ? out : `refused (${out.hint})`}`);
    return out.hint;
  }
}
export { Key };
