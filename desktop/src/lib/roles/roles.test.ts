import { beforeEach, describe, expect, it, vi } from 'vitest';
import { render } from 'svelte/server';
import type { RoleRow, SessionRole } from '../studio/types';

const rolesList = vi.fn();
vi.mock('../api', () => ({ api: { rolesList: (...a: unknown[]) => rolesList(...a) } }));
vi.mock('../store.svelte', () => ({ app: { roles: {} as Record<string, SessionRole> } }));

import { app } from '../store.svelte';
import RoleChip from './RoleChip.svelte';
import RolePicker from './RolePicker.svelte';
import { loadRoles, roleOption, roleTitle, roles } from './roles.svelte';

const runner: RoleRow = { id: 'runner', label: 'Runner', model: 'haiku', permission: 'allowlist', tier: 3, department: 'audio' };
const lead: RoleRow = { id: 'lead', label: 'Lead', model: null, permission: 'prompt', tier: 2, department: null };
const held = (rows: RoleRow[], error: string | null = null) => ({ rows, loading: false, error, at: Date.now() });
const session = (permission: string): SessionRole => ({
  agent: 'a1',
  card: 'runner',
  label: 'Runner',
  tier: 3,
  department: null,
  permission,
  context_budget: null,
});

beforeEach(() => {
  rolesList.mockReset();
  app.roles = {};
});

describe('RolePicker', () => {
  it('offers no role, then each role with its model and permission', () => {
    roles['two'] = held([runner, lead]);
    const { body } = render(RolePicker, { props: { repo: 'two', value: null, bar: true } });
    expect(body).toContain('No role');
    expect(body).toContain('Runner \u00b7 haiku \u00b7 allowlist');
    expect(body).toContain('Lead \u00b7 default model \u00b7 prompt');
    expect(body.indexOf('No role')).toBeLessThan(body.indexOf('Runner'));
    expect(body).toContain('aria-label="Role"');
  });

  it('is a labelled field outside the quick prompt', () => {
    roles['two'] = held([runner]);
    const { body } = render(RolePicker, { props: { repo: 'two', value: 'runner' } });
    expect(body).toContain('<label');
    expect(body).toContain('title="Runner: allowlist, tier 3"');
  });

  it('renders nothing for a repo with no roles, or while they load', () => {
    roles['none'] = held([]);
    for (const repo of ['none', 'unseen']) {
      const { body } = render(RolePicker, { props: { repo, value: null } });
      expect(body).not.toContain('<select');
      expect(body).not.toContain('<span');
    }
  });

  it('says so when the roles file is invalid', () => {
    roles['bad'] = held([], '.kete/roles.toml: line 3');
    const { body } = render(RolePicker, { props: { repo: 'bad', value: null } });
    expect(body).toContain('roles file invalid');
    expect(body).not.toContain('<select');
  });
});

describe('RoleChip', () => {
  it('names the role, with its permission and tier in the tooltip', () => {
    app.roles = { a1: session('allowlist') };
    const { body } = render(RoleChip, { props: { agent: 'a1' } });
    expect(body).toContain('title="Runner: allowlist, tier 3"');
    expect(body).toContain('>Runner<');
    expect(body).not.toContain('unattended</span>');
  });

  it('says unattended in a word', () => {
    app.roles = { a1: session('unattended') };
    const { body } = render(RoleChip, { props: { agent: 'a1' } });
    expect(body).toContain('>unattended</span>');
  });

  it('renders nothing for a session without a role', () => {
    app.roles = { other: session('prompt') };
    const { body } = render(RoleChip, { props: { agent: 'a1' } });
    expect(body).not.toContain('<span');
    expect(body).not.toContain('Runner');
  });
});

describe('roles store', () => {
  it('words', () => {
    expect(roleOption(runner)).toBe('Runner \u00b7 haiku \u00b7 allowlist');
    expect(roleOption(lead)).toBe('Lead \u00b7 default model \u00b7 prompt');
    expect(roleTitle(runner)).toBe('Runner: allowlist, tier 3');
  });

  it('asks once per repo while the answer is fresh, and again when forced', async () => {
    rolesList.mockResolvedValue([runner]);
    await loadRoles('fresh');
    await loadRoles('fresh');
    expect(rolesList).toHaveBeenCalledTimes(1);
    expect(roles['fresh'].rows).toEqual([runner]);
    await loadRoles('fresh', true);
    expect(rolesList).toHaveBeenCalledTimes(2);
  });

  it('keeps the error of an invalid file, and asks again next time', async () => {
    rolesList.mockRejectedValueOnce('.kete/roles.toml: line 3').mockResolvedValueOnce([lead]);
    await loadRoles('flaky');
    expect(roles['flaky']).toMatchObject({ rows: [], loading: false, error: '.kete/roles.toml: line 3' });
    await loadRoles('flaky');
    expect(roles['flaky']).toMatchObject({ rows: [lead], error: null });
  });
});
