<script lang="ts">
  // The rail's ACCOUNTS: each profile, and a harness's own login while it
  // is in use, with its session (5h) and weekly use as two thin bars, how
  // many sessions run on it, an enabled switch and the default star. A
  // row that needs a login shows the command (and can run it in a new
  // terminal); a broken profile shows what is wrong and the way to fix it.
  // The heading opens the Accounts view, where everything else lives.
  import { onMount } from 'svelte';
  import { api, type AccountRow } from '../api';
  import { app, focusQuickPrompt, openSession, refreshAccounts, refreshLaunchables } from '../store.svelte';
  import { nav } from '../shell/history.svelte';
  import { copySignIn, openAccounts, runLogin as startLogin, setAccountEnabled, setDefaultAccount } from './actions';
  import { openAddAccount } from './add.svelte';
  import UsageBar from './UsageBar.svelte';
  import { bars, liveUsage, loginTitle, mergeRows, ownLoginSessionHarnesses, sessionsOn, shownAt, staleAt, starTitle, usageTitle, windowLabel } from './usage';
  import { withKey } from '../shortcuts';

  let { heading }: { heading: string } = $props();

  const rows = $derived(mergeRows(app.accounts, app.model.accounts ?? {}, ownLoginSessionHarnesses(app.model.agents)));
  let loginOpen = $state<string | null>(null);
  let sessionsOpen = $state<string | null>(null);

  const GLYPH: Record<string, string> = { claude: 'C', codex: 'X', copilot: 'G', cursor: 'U', pi: 'P', opencode: 'O' };

  async function runLogin(r: AccountRow) {
    if (await startLogin(r)) loginOpen = null;
  }

  // The list is read from disk: ask again when the window comes back, every
  // minute while it is shown, and when a session names a profile the list
  // has not seen (one a terminal added). Each unknown name is asked about
  // once, so one removed from the config cannot loop.
  onMount(() => {
    const again = () => {
      if (document.visibilityState !== 'visible') return;
      refreshLaunchables();
      // The meter follows the account, not the last statusLine: ask for a
      // fresh reading as the window returns.
      api.nudgeAccountUsage().catch(() => {});
    };
    window.addEventListener('focus', again);
    document.addEventListener('visibilitychange', again);
    const tick = setInterval(() => {
      if (document.visibilityState === 'visible' && app.model.connected) refreshAccounts();
    }, 60_000);
    return () => {
      window.removeEventListener('focus', again);
      document.removeEventListener('visibilitychange', again);
      clearInterval(tick);
    };
  });
  const askedAbout = new Set<string>();
  $effect(() => {
    // A first look, should the boot's ask have raced the daemon.
    if (app.model.connected && !app.accounts.length) refreshAccounts();
  });
  $effect(() => {
    const known = new Set(app.accounts.map((r) => r.key));
    for (const a of app.model.agents) {
      const p = a.profile?.trim();
      if (!a.archived && p && !known.has(p) && !askedAbout.has(p)) {
        askedAbout.add(p);
        refreshLaunchables();
        return;
      }
    }
  });
</script>

<!-- Always drawn, empty too: an empty list is who most needs Add. -->
<div class="flex items-center pr-3 {heading}">
  <h2 class="m-0 min-w-0 flex-1 text-[12px] font-medium">
    <button
      class="border-0 bg-transparent p-0 text-left text-[12px] font-medium hover:text-ink hover:underline hover:underline-offset-4 {nav.current.view === 'accounts'
        ? 'text-ink'
        : 'text-muted'}"
      onclick={openAccounts}
      title={withKey('Open Accounts: every login, its use, its sessions, and the account spread', 'accounts')}
      aria-current={nav.current.view === 'accounts' ? 'page' : undefined}>Accounts</button
    >
  </h2>
  <button
    class="grid size-[18px] flex-none place-items-center border-0 bg-transparent p-0 text-muted hover:text-ink"
    onclick={() => openAddAccount()}
    title="Add an account: a harness, a name, then sign it in"
    aria-label="Add an account"
  >
    <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true"><path d="M5 1v8M1 5h8" stroke="currentColor" stroke-width="1.3" /></svg>
  </button>
</div>
{#if !rows.length}
  <p class="m-0 px-4 py-1 text-[12px] text-muted">Sessions run on each CLI's own login. Add an account to run several.</p>
{:else}
  <ul class="m-0 list-none p-0" aria-label="Accounts">
    {#each rows as r (r.key)}
      {@const u = liveUsage(r, app.model.accounts ?? {})}
      {@const [top, bottom] = bars(u)}
      {@const on = sessionsOn(r, app.model.agents)}
      <li class="group px-4 py-1.5 {r.disabled && !r.problem ? 'opacity-55' : ''}" data-target="account:{r.key}">
        <div class="flex min-w-0 items-center gap-2">
          <span
            class="grid size-[14px] flex-none place-items-center border font-mono text-[9px] leading-none font-semibold {r.problem
              ? 'border-fail text-fail'
              : 'border-rule-strong text-ink'}"
            title={r.problem ? `${r.harness}: broken profile` : `${r.harness}\n${loginTitle(r)}`}
            aria-hidden="true">{r.problem ? '!' : (GLYPH[r.harness] ?? r.harness.slice(0, 1).toUpperCase())}</span
          >
          <button
            class="min-w-0 flex-1 truncate border-0 bg-transparent p-0 text-left text-[13px] text-ink hover:underline hover:underline-offset-4"
            onclick={openAccounts}
            title={r.problem
              ? `Fix ${r.name} in Accounts`
              : r.profile
                ? `Edit ${r.name} in Accounts\n${loginTitle(r)}`
                : `${r.harness}'s own login. Add it as an account in Accounts\n${loginTitle(r)}`}
            >{r.name}{#if !r.profile}<span class="ml-1 text-faint">own login</span>{/if}{#if r.email}<span class="ml-1 text-faint" data-account-email>{r.email}</span>{/if}</button
          >
          {#if on.length}
            <button
              class="flex-none border border-rule bg-transparent px-1 font-mono text-[10px] leading-[14px] text-muted hover:border-rule-strong hover:text-ink"
              onclick={() => (sessionsOpen = sessionsOpen === r.key ? null : r.key)}
              aria-expanded={sessionsOpen === r.key}
              title={`${on.length} ${on.length === 1 ? 'session runs' : 'sessions run'} on ${r.name}:\n${on.map((a) => a.name).join('\n')}`}
              aria-label="{on.length} sessions on {r.name}">{on.length}</button
            >
          {/if}
          {#if r.login === 'needs_login' && r.loginHint}
            <button
              class="flex-none border border-ember bg-transparent px-1 font-mono text-[10px] leading-[14px] text-ember-ink hover:bg-surface"
              onclick={() => (loginOpen = loginOpen === r.key ? null : r.key)}
              aria-expanded={loginOpen === r.key}
              title={`${loginTitle(r)}\nShow the command`}>{r.expired ? 'expired' : 'login'}</button
            >
          {/if}
          {#if !r.problem}
            <button
              class="hidden size-[16px] flex-none place-items-center border-0 bg-transparent p-0 text-muted group-hover:grid hover:text-ink focus-visible:grid"
              onclick={() => focusQuickPrompt({ harness: r.harness, profile: r.profile ? r.name : undefined })}
              title={r.profile ? `Start a session with ${r.name}` : `Start a ${r.harness} session (auto picks the account)`}
              aria-label={r.profile ? `Start a session with ${r.name}` : `Start a ${r.harness} session`}
            >
              <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true"><path d="M5 1v8M1 5h8" stroke="currentColor" stroke-width="1.3" /></svg>
            </button>
          {/if}
          {#if r.profile && !r.problem}
            {@const dimStar = !r.default && !!r.flaggedDefault && r.disabled}
            <button
              class="grid size-[16px] flex-none place-items-center border-0 bg-transparent p-0 {r.default
                ? 'text-ember'
                : dimStar
                  ? 'text-ember opacity-45'
                  : 'text-faint hover:text-ink'}"
              onclick={() => setDefaultAccount(r)}
              title={starTitle(r)}
              aria-label={r.default ? `${r.name} is the default` : dimStar ? `${r.name} is the default (disabled)` : `Make ${r.name} the default`}
              aria-pressed={r.default}
            >
              <svg width="11" height="11" viewBox="0 0 12 12" aria-hidden="true"
                ><path
                  d="M6 .8l1.55 3.3 3.6.45-2.65 2.5.68 3.57L6 8.85l-3.18 1.77.68-3.57L.85 4.55l3.6-.45z"
                  fill={r.default || dimStar ? 'currentColor' : 'none'}
                  stroke="currentColor"
                  stroke-width="1"
                  stroke-linejoin="miter"
                /></svg
              >
            </button>
            <button
              class="relative h-[12px] w-[22px] flex-none border p-0 {r.disabled ? 'border-rule bg-transparent' : 'border-rule-strong bg-ink'}"
              role="switch"
              aria-checked={!r.disabled}
              onclick={() => setAccountEnabled(r)}
              title={r.disabled ? `Off: launches that pick no account skip ${r.name}. Switch on` : `On. Switch ${r.name} off`}
              aria-label="{r.name} enabled"
            >
              <span class="absolute top-[1px] size-[8px] {r.disabled ? 'left-[1px] bg-faint' : 'right-[1px] bg-bg'}" aria-hidden="true"></span>
            </button>
          {/if}
        </div>
        {#if r.problem}
          <div class="mt-1 ml-[22px] flex items-start gap-2">
            <p class="m-0 min-w-0 flex-1 text-[11px] leading-[15px] text-fail">{r.problem}</p>
            <button
              class="flex-none border border-rule bg-bg px-1.5 text-[11px] text-ink hover:border-rule-strong"
              onclick={openAccounts}>Fix</button
            >
          </div>
        {:else}
          <div class="mt-1 flex flex-col gap-[3px] pl-[22px]" title={usageTitle(r, u, app.now)}>
            <UsageBar label={windowLabel(top, 0)} percent={top ? shownAt(top, app.now) : null} stale={!!top && staleAt(top, app.now)} />
            <UsageBar
              label={windowLabel(bottom, 1)}
              percent={bottom ? shownAt(bottom, app.now) : null}
              stale={!!bottom && staleAt(bottom, app.now)}
            />
          </div>
        {/if}
        {#if sessionsOpen === r.key && on.length}
          <ul class="m-0 mt-1.5 ml-[22px] list-none border border-rule p-0" aria-label="Sessions on {r.name}">
            {#each on as a (a.id)}
              <li>
                <button
                  class="block w-full truncate border-0 bg-transparent px-1.5 py-0.5 text-left text-[12px] text-ink hover:bg-surface"
                  onclick={() => openSession({ Agent: a.id })}
                  title="Open {a.name}">{a.name}</button
                >
              </li>
            {/each}
          </ul>
        {/if}
        {#if loginOpen === r.key && r.loginHint}
          <!-- The whole command, wrapped: it is what gets typed. -->
          <div class="mt-1.5 ml-[22px] flex items-start gap-1 border border-rule bg-surface px-1.5 py-1">
            <!-- Wraps between words only: a profile name never splits at its hyphen. -->
            <code class="min-w-0 flex-1 font-mono text-[11px] leading-[15px] whitespace-normal text-ink"
              >{#each r.loginHint.split(' ') as word, i (i)}{#if i > 0}{' '}{/if}<span class="whitespace-nowrap">{word}</span
                >{/each}</code
            >
            <button
              class="flex-none border border-rule bg-bg px-1.5 text-[11px] text-ink hover:border-rule-strong"
              onclick={() => copySignIn(r)}>Copy</button
            >
            <button
              class="flex-none border border-rule-strong bg-bg px-1.5 text-[11px] text-ink hover:bg-surface"
              onclick={() => runLogin(r)}
              title="Open a terminal and run it there">Run</button
            >
          </div>
        {/if}
      </li>
    {/each}
  </ul>
{/if}
