<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { invoke, isTauri } from '@tauri-apps/api/core';
  import { date } from '../lib/types';

  interface Connection {
    id: number;
    name: string;
    policy: { roots: { path: string }[]; exclusions: { path: string }[]; capabilities: string[]; limits: { page_size: number; scan_entries: number; plan_files: number; plan_bytes: number } };
    revoked: boolean;
    created: number;
    last_used: number | null;
  }
  interface Credential { id: number; credential: string }
  let connections = $state<Connection[]>([]);
  let loading = $state(true), saving = $state(false), error = $state(''), notice = $state('');
  let name = $state(''), roots = $state(''), exclusions = $state(''), allowScan = $state(true);
  let pageSize = $state(100), scanEntries = $state(1_000_000);
  let allowPlans = $state(false), planFiles = $state(100), planBytes = $state(10_000_000_000);
  let credential = $state<Credential | null>(null), revealed = $state(false);
  let alive = true;
  const native = isTauri();
  const lines = (value: string): string[] => value.split(/\r?\n/).map(value => value.trim()).filter(Boolean);

  async function refresh(): Promise<void> {
    if (!native) { loading = false; return; }
    loading = true;
    try {
      const items = await invoke<Connection[]>('list_agent_connections');
      if (alive) connections = items;
    } catch (problem) { if (alive) error = String(problem); }
    finally { if (alive) loading = false; }
  }
  async function choose(): Promise<void> {
    try {
      const path = await invoke<string | null>('choose_folder');
      if (alive && path) roots = [...lines(roots), path].join('\n');
    } catch (problem) { if (alive) error = String(problem); }
  }
  async function create(event: SubmitEvent): Promise<void> {
    event.preventDefault();
    saving = true; error = ''; notice = '';
    try {
      const result = await invoke<Credential | null>('create_agent_connection', { request: { name, roots: lines(roots), exclusions: lines(exclusions), scan: allowScan, plan: allowPlans, page_size: pageSize, scan_entries: scanEntries, plan_files: planFiles, plan_bytes: planBytes } });
      if (!alive) return;
      if (result) { credential = result; revealed = false; name = ''; notice = 'Connection created. Save its credential before dismissing it.'; }
      await refresh();
    } catch (problem) { if (alive) error = String(problem); }
    finally { if (alive) saving = false; }
  }
  async function manage(id: number, action: 'rotate' | 'revoke'): Promise<void> {
    saving = true; error = ''; notice = '';
    try {
      if (action === 'rotate') {
        const result = await invoke<Credential | null>('rotate_agent_credential', { id });
        if (alive && result) { credential = result; revealed = false; }
      } else {
        await invoke('revoke_agent_connection', { id });
        if (alive) { if (credential?.id === id) credential = null; notice = 'Connection revoked.'; }
      }
      if (alive) await refresh();
    } catch (problem) { if (alive) error = String(problem); }
    finally { if (alive) saving = false; }
  }
  async function copy(): Promise<void> {
    if (!credential) return;
    try { await navigator.clipboard.writeText(credential.credential); notice = 'Credential copied.'; }
    catch { error = 'Clipboard unavailable. Reveal the credential and copy it manually.'; }
  }
  onMount(() => { void refresh(); });
  onDestroy(() => { alive = false; credential = null; });
</script>

<section class="agent-connections" aria-labelledby="agent-heading">
  <div class="agent-title"><h2 id="agent-heading">Agents and permissions</h2><button onclick={refresh} disabled={!native || loading || saving}>Refresh</button></div>
  <p class="agent-description">Allow a local client to inspect selected folders. The client may send metadata to its AI provider. File modification requires a separate approved plan.</p>
  {#if !native}<p class="notice">Connection management requires the Windows application.</p>{/if}
  {#if error}<p role="alert" class="error">{error}</p>{/if}
  {#if notice}<p role="status">{notice}</p>{/if}
  {#if credential}
    <div class="agent-credential">
      <label for="agent-credential">Credential for connection #{credential.id}<small>Shown only in this session. Keep it out of chat, logs and source control.</small></label>
      <input id="agent-credential" type={revealed ? 'text' : 'password'} value={credential.credential} readonly autocomplete="off" spellcheck="false" />
      <div class="agent-actions"><button onclick={() => revealed = !revealed}>{revealed ? 'Hide' : 'Reveal'}</button><button onclick={copy}>Copy credential</button><button onclick={() => { credential = null; revealed = false; }}>Dismiss</button></div>
    </div>
  {/if}
  {#if loading}<p role="status">Loading connections…</p>
  {:else if connections.length === 0}<p class="agent-empty">No authorized connections.</p>
  {:else}
    <ul class="agent-list">
      {#each connections as connection (connection.id)}
        <li>
          <div class="agent-identity"><strong>{connection.name}</strong><span>{connection.revoked ? 'Revoked' : 'Authorized'} · #{connection.id}</span></div>
          <div class="agent-paths">{#each connection.policy.roots as root}<span>{root.path}</span>{/each}</div>
          <small>{connection.policy.capabilities.join(', ')} · Last used: {connection.last_used === null ? 'Never' : date(connection.last_used)}</small>
          <details><summary>Scope and limits</summary><p>Page: {connection.policy.limits.page_size} entries · Scan: {connection.policy.limits.scan_entries.toLocaleString()} entries</p><p>Plan: {connection.policy.limits.plan_files} files · {connection.policy.limits.plan_bytes.toLocaleString()} logical bytes</p>{#if connection.policy.exclusions.length}<p>Excluded folders</p>{#each connection.policy.exclusions as root}<p class="agent-paths">{root.path}</p>{/each}{:else}<p>No folder exclusions.</p>{/if}</details>
          <div class="agent-actions"><button onclick={() => manage(connection.id, 'rotate')} disabled={connection.revoked || saving}>Rotate credential</button><button onclick={() => manage(connection.id, 'revoke')} disabled={connection.revoked || saving}>Revoke</button></div>
        </li>
      {/each}
    </ul>
  {/if}
  <form onsubmit={create}>
    <h3>New connection</h3>
    <fieldset disabled={!native || saving || credential !== null}>
      <label for="agent-name">Name<input id="agent-name" bind:value={name} required maxlength="128" autocomplete="off" placeholder="Local analysis client" /></label>
      <label for="agent-roots">Allowed folders<small>One existing folder per line; overlapping roots are rejected.</small><textarea id="agent-roots" bind:value={roots} required rows="3" spellcheck="false"></textarea></label>
      <button type="button" onclick={choose}>Add folder…</button>
      <label for="agent-exclusions">Excluded folders<textarea id="agent-exclusions" bind:value={exclusions} rows="2" spellcheck="false"></textarea></label>
      <label class="agent-check"><input type="checkbox" checked disabled />Read indexed metadata</label>
      <label class="agent-check"><input type="checkbox" bind:checked={allowScan} />Scan allowed folders</label>
      <label class="agent-check"><input type="checkbox" bind:checked={allowPlans} />Prepare recycling plans and request human review</label>
      <small>Each plan needs separate approval in Voldiq. This connection cannot execute file changes.</small>
      <div class="agent-limits"><label for="agent-page">Entries per page<input id="agent-page" type="number" min="1" max="500" step="1" bind:value={pageSize} required /></label><label for="agent-scan-limit">Entries per scan<input id="agent-scan-limit" type="number" min="1" max="5000000" step="1" bind:value={scanEntries} required /></label></div>
      {#if allowPlans}<div class="agent-limits"><label for="agent-plan-files">Files per plan<input id="agent-plan-files" type="number" min="1" max="500" step="1" bind:value={planFiles} required /></label><label for="agent-plan-bytes">Logical bytes per plan<input id="agent-plan-bytes" type="number" min="1" max="1000000000000" step="1" bind:value={planBytes} required /></label></div>{/if}
      <button class="primary" type="submit">{saving ? 'Waiting for confirmation…' : 'Review and create connection'}</button>
    </fieldset>
  </form>
</section>

<style>
  .agent-title, .agent-actions, .agent-identity { display: flex; align-items: center; flex-wrap: wrap; gap: 8px; }
  .agent-title { justify-content: space-between; }
  .agent-description, .agent-empty { margin: 0; color: var(--secondary); }
  .agent-list { padding: 0; margin: 0; list-style: none; }
  .agent-list li { display: grid; gap: 8px; padding: 16px 0; border-bottom: 1px solid var(--border); }
  .agent-identity span, summary { color: var(--secondary); font-size: 12px; }
  .agent-paths { overflow-wrap: anywhere; display: grid; gap: 4px; font-size: 12px; }
  details p { margin: 8px 0 0; font-size: 12px; }
  .agent-credential { display: grid; gap: 12px; padding: 16px; background: var(--surface); border: 1px solid var(--border); border-radius: var(--radius); }
  input { min-width: 0; width: 100%; }
  .agent-credential input { font-family: Consolas, monospace; }
  h3 { margin: 0 0 12px; font-size: 15px; }
  fieldset { border: 0; padding: 0; margin: 0; display: grid; gap: 16px; min-width: 0; }
  fieldset > label { display: grid; gap: 8px; }
  fieldset > button { justify-self: start; }
  .agent-check { display: flex; align-items: center; }
  .agent-limits { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 16px; }
  .agent-limits label { display: grid; gap: 8px; }
  @media (max-width: 700px) { .agent-limits { grid-template-columns: 1fr; } }
</style>
