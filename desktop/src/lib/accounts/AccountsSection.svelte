<script lang="ts">
  // The rail's ACCOUNTS: each profile, and a harness's own login when it
  // has none, with its session (5h) and weekly use as two thin bars, an
  // enabled switch and the default star. A profile that needs a login
  // shows the command to run; the window never signs anyone in.
  import { api, type AccountRow } from '../api';
  import { app, focusQuickPrompt, openSettings, refreshAccounts, refreshLaunchables, toast } from '../store.svelte';
  import UsageBar from './UsageBar.svelte';
  import { bars, liveUsage, mergeRows, percentAt, usageTitle, windowLabel } from './usage';

  let { heading }: { heading: string } = $props();

  const sessionHarnesses = $derived([...new Set(app.model.agents.filter((a) => !a.archived).map((a) => a.kind))]);
  const rows = $derived(mergeRows(app.accounts, app.model.accounts ?? {}, sessionHarnesses));
  let loginOpen = $state<string | null>(null);

  const GLYPH: Record<string, string> = { claude: 'C', codex: 'X', copilot: 'G', cursor: 'U', pi: 'P', opencode: 'O' };

  async function setDefault(r: AccountRow) {
    if (r.default) return;
    try {
      await api.profileSetDefault(r.name);
      refreshLaunchables();
    } catch (e) {
      toast(String(e));
    }
  }

  async function setEnabled(r: AccountRow) {
    try {
      await api.profileSetEnabled(r.name, r.disabled);
      toast(r.disabled ? `${r.name} is on.` : `${r.name} is off: launches that pick no profile skip it.`);
      refreshLaunchables();
    } catch (e) {
      toast(String(e));
    }
  }

  async function copy(text: string) {
    try {
      await navigator.clipboard.writeText(text);
      toast('Copied. Run it in a terminal to sign in.');
    } catch {
      toast(text);
    }
  }

  $effect(() => {
    // A first look, should the boot's ask have raced the daemon.
    if (app.model.connected && !app.accounts.length) refreshAccounts();
  });
</script>

{#if rows.length}
  <h2 class="m-0 {heading}">Accounts</h2>
  <ul class="m-0 list-none p-0" aria-label="Accounts">
    {#each rows as r (r.key)}
      {@const u = liveUsage(r, app.model.accounts ?? {})}
      {@const [top, bottom] = bars(u)}
      <li class="group px-4 py-1.5 {r.disabled ? 'opacity-55' : ''}">
        <div class="flex min-w-0 items-center gap-2">
          <span
            class="grid size-[14px] flex-none place-items-center border border-rule-strong font-mono text-[9px] leading-none font-semibold text-ink"
            title={r.harness}
            aria-hidden="true">{GLYPH[r.harness] ?? r.harness.slice(0, 1).toUpperCase()}</span
          >
          <button
            class="min-w-0 flex-1 truncate border-0 bg-transparent p-0 text-left text-[13px] text-ink hover:underline hover:underline-offset-4"
            onclick={() => openSettings('profiles')}
            title={r.profile ? `Edit ${r.name} in Settings, Profiles` : `${r.harness}'s own login. Add a profile in Settings, Profiles`}
            >{r.name}{#if !r.profile}<span class="ml-1 text-faint">own login</span>{/if}</button
          >
          {#if r.login === 'needs_login' && r.loginHint}
            <button
              class="flex-none border border-ember bg-transparent px-1 font-mono text-[10px] leading-[14px] text-ember-ink hover:bg-surface"
              onclick={() => (loginOpen = loginOpen === r.key ? null : r.key)}
              aria-expanded={loginOpen === r.key}
              title="Not signed in: show the command that signs it in">login</button
            >
          {/if}
          <button
            class="hidden size-[16px] flex-none place-items-center border-0 bg-transparent p-0 text-muted group-hover:grid hover:text-ink focus-visible:grid"
            onclick={() => focusQuickPrompt({ harness: r.harness, profile: r.profile ? r.name : undefined })}
            title="Start a session with {r.name}"
            aria-label="Start a session with {r.name}"
          >
            <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true"><path d="M5 1v8M1 5h8" stroke="currentColor" stroke-width="1.3" /></svg>
          </button>
          {#if r.profile}
            <button
              class="grid size-[16px] flex-none place-items-center border-0 bg-transparent p-0 {r.default ? 'text-ember' : 'text-faint hover:text-ink'}"
              onclick={() => setDefault(r)}
              title={r.default ? `${r.name} is the ${r.harness} default` : `Make ${r.name} the ${r.harness} default`}
              aria-label={r.default ? `${r.name} is the default` : `Make ${r.name} the default`}
              aria-pressed={r.default}
            >
              <svg width="11" height="11" viewBox="0 0 12 12" aria-hidden="true"
                ><path
                  d="M6 .8l1.55 3.3 3.6.45-2.65 2.5.68 3.57L6 8.85l-3.18 1.77.68-3.57L.85 4.55l3.6-.45z"
                  fill={r.default ? 'currentColor' : 'none'}
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
              onclick={() => setEnabled(r)}
              title={r.disabled ? `Off: launches that pick no profile skip ${r.name}. Switch on` : `On. Switch ${r.name} off`}
              aria-label="{r.name} enabled"
            >
              <span class="absolute top-[1px] size-[8px] {r.disabled ? 'left-[1px] bg-faint' : 'right-[1px] bg-bg'}" aria-hidden="true"></span>
            </button>
          {/if}
        </div>
        <div class="mt-1 flex flex-col gap-[3px] pl-[22px]" title={usageTitle(r, u, app.now)}>
          <UsageBar label={windowLabel(top, 0)} percent={top ? percentAt(top, app.now) : null} />
          <UsageBar label={windowLabel(bottom, 1)} percent={bottom ? percentAt(bottom, app.now) : null} />
        </div>
        {#if loginOpen === r.key && r.loginHint}
          <div class="mt-1.5 ml-[22px] flex items-center gap-1 border border-rule bg-surface px-1.5 py-1">
            <code class="min-w-0 flex-1 truncate font-mono text-[11px] text-ink" title={r.loginHint}>{r.loginHint}</code>
            <button
              class="flex-none border border-rule bg-bg px-1.5 text-[11px] text-ink hover:border-rule-strong"
              onclick={() => copy(r.loginHint ?? '')}>Copy</button
            >
          </div>
        {/if}
      </li>
    {/each}
  </ul>
{/if}
