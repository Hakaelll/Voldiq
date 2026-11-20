<script lang="ts">
  import { invoke, isTauri } from '@tauri-apps/api/core';
  import { onMount, tick } from 'svelte';
  import { applicationBytes as formatBytes } from '../lib/applications';
  import { trayPercent } from '../lib/cleanup-tray';
  import Icon from './Icon.svelte';

  type Item = {
    id: string;
    name: string;
    kind: 'folder' | 'file' | 'reference';
    bytes: string | null;
  };
  type Quota = { used: string | null; total: string | null; remaining: string | null };
  type Page = {
    folder: string;
    number: number;
    has_parent: boolean;
    has_next: boolean;
    items: Item[];
  };
  type View =
    | { kind: 'disconnected' }
    | { kind: 'saved' }
    | { kind: 'waiting'; code: string; retry_seconds: number }
    | { kind: 'connected'; work_account: boolean; page: Page; quota: Quota };
  type Action =
    | { kind: 'status' | 'poll' | 'open_sign_in' | 'cancel' | 'browse' | 'disconnect' }
    | { kind: 'parent' | 'first_page' | 'next_page' }
    | { kind: 'begin'; client_id: string }
    | { kind: 'enter_folder'; id: string }
    | { kind: 'open_permissions'; work_account: boolean };

  const native = isTauri();
  const labels: Record<View['kind'], string> = {
    disconnected: 'Off',
    saved: 'Account saved',
    waiting: 'Waiting for sign-in',
    connected: 'Connected',
  };
  const itemLabels: Record<Item['kind'], string> = {
    folder: 'Folder',
    file: 'File',
    reference: 'Reference',
  };
  let expanded = $state(false);
  let view = $state<View | null>(null);
  let busy = $state<Action['kind'] | null>(null);
  let status = $state('');
  let error = $state('');
  let clientId = $state('');
  let destination = $state<HTMLElement>();
  let connected = $derived(view?.kind === 'connected' ? view : null);
  let quotaPercent = $derived.by(() => {
    const quota = connected?.quota;
    if (!quota || quota.total === null || quota.used === null || quota.total === '0') return null;
    return trayPercent(quota.used, quota.total);
  });

  function command(action: Action): Promise<View> {
    return invoke<View>('one_drive', { action });
  }

  onMount(() => {
    if (native) void run({ kind: 'status' });
  });

  $effect(() => {
    if (!expanded || view?.kind !== 'waiting' || busy || error) return;
    const timer = setTimeout(() => void run({ kind: 'poll' }), view.retry_seconds * 1000);
    return () => clearTimeout(timer);
  });

  async function run(action: Action): Promise<void> {
    if (busy) return;
    const previous = view?.kind;
    busy = action.kind;
    error = '';
    status = '';
    let focus = false;
    try {
      view = await command(action);
      if (action.kind === 'poll' && view.kind === 'disconnected')
        status = 'The Microsoft sign-in code expired. Start sign-in again.';
      if (action.kind === 'disconnect') status = 'OneDrive disconnected from Voldiq.';
      if (view.kind === 'waiting') clientId = '';
      focus =
        action.kind !== 'status' &&
        (view.kind !== previous ||
          ['browse', 'enter_folder', 'parent', 'first_page', 'next_page'].includes(action.kind));
    } catch (failure) {
      error = String(failure);
      if (action.kind === 'poll') {
        try {
          view = await command({ kind: 'status' });
        } catch {
          // Keep the last known state alongside the error.
        }
      }
    } finally {
      busy = null;
    }
    if (focus) {
      await tick();
      destination?.focus({ preventScroll: true });
    }
  }

  function size(value: string | null): string {
    return value === null ? 'Unknown' : formatBytes(value);
  }
</script>

<section aria-labelledby="cloud-accounts-heading">
  <h2 id="cloud-accounts-heading">Cloud accounts</h2>
  <details bind:open={expanded}>
    <summary>
      <span>OneDrive</span>
      {#if view}<span class="connection-state">{labels[view.kind]}</span>{/if}
    </summary>
    <div class="cloud-content">
      {#if !native}
        <p>Cloud accounts are available in Voldiq for Windows.</p>
      {:else if view?.kind === 'disconnected'}
        <p>Browse OneDrive file names, sizes and storage quota with read-only access.</p>
        <form
          onsubmit={(event) => {
            event.preventDefault();
            void run({ kind: 'begin', client_id: clientId });
          }}
        >
          <label>
            Microsoft Entra application ID
            <input
              bind:this={destination}
              bind:value={clientId}
              aria-describedby="cloud-registration"
              autocomplete="off"
              spellcheck="false"
              maxlength="36"
              required
            />
          </label>
          <button type="submit" class="primary" disabled={busy !== null || !clientId.trim()}>
            {busy === 'begin' ? 'Requesting code…' : 'Connect OneDrive'}
          </button>
        </form>
        <p id="cloud-registration">
          Use a Microsoft Entra app registration with public client flows enabled and delegated
          Files.Read access.
        </p>
      {:else if view?.kind === 'waiting'}
        <div class="sign-in">
          <p>Open Microsoft sign-in and enter this code to grant read access.</p>
          <code tabindex="-1" bind:this={destination}>{view.code}</code>
          <div class="actions">
            <button
              class="primary"
              disabled={busy !== null}
              onclick={() => void run({ kind: 'open_sign_in' })}>Open Microsoft sign-in</button
            >
            {#if error}<button disabled={busy !== null} onclick={() => void run({ kind: 'poll' })}
                >Check sign-in</button
              >{/if}
            <button
              class="quiet-action"
              disabled={busy !== null}
              onclick={() => void run({ kind: 'cancel' })}>Cancel</button
            >
          </div>
        </div>
      {:else if view?.kind === 'saved'}
        <p>
          Your Windows user profile holds the saved credential. Open OneDrive to request current
          metadata.
        </p>
        <div class="actions">
          <button
            bind:this={destination}
            class="primary"
            disabled={busy !== null}
            onclick={() => void run({ kind: 'browse' })}>Open OneDrive</button
          >
          <button
            class="quiet-action"
            disabled={busy !== null}
            onclick={() => void run({ kind: 'disconnect' })}>Disconnect</button
          >
        </div>
      {:else if connected}
        <div class="account">
          <div class="line">
            <h3>Remote storage quota</h3>
            <button
              class="quiet-action"
              disabled={busy !== null}
              onclick={() => void run({ kind: 'browse' })}>Reload OneDrive</button
            >
          </div>
          {#if quotaPercent !== null}
            <div class="quota-track" aria-hidden="true">
              <span style:width={quotaPercent + '%'}></span>
            </div>
          {/if}
          <dl>
            <div>
              <dt>Used</dt>
              <dd>{size(connected.quota.used)}</dd>
            </div>
            <div>
              <dt>Available</dt>
              <dd>{size(connected.quota.remaining)}</dd>
            </div>
            <div>
              <dt>Capacity</dt>
              <dd>{size(connected.quota.total)}</dd>
            </div>
          </dl>
          <p>OneDrive reports this quota separately from local scans, including synced files.</p>
        </div>
        <div class="folder" aria-busy={busy !== null}>
          <div class="line">
            <h3 tabindex="-1" bind:this={destination}>{connected.page.folder}</h3>
            {#if connected.page.has_parent}
              <button
                class="quiet-action"
                disabled={busy !== null}
                onclick={() => void run({ kind: 'parent' })}>Parent folder</button
              >
            {/if}
          </div>
          {#if connected.page.items.length}
            <!-- svelte-ignore a11y_no_noninteractive_tabindex (Keyboard users need to scroll the named folder list.) -->
            <div class="items" role="region" aria-label="OneDrive folder contents" tabindex="0">
              <ul>
                {#each connected.page.items as item}
                  <li>
                    <div class="item-name">
                      <Icon name={item.kind === 'folder' ? 'folder' : 'file'} size={16} />
                      {#if item.kind === 'folder'}
                        <button
                          class="quiet-action"
                          disabled={busy !== null}
                          onclick={() => void run({ kind: 'enter_folder', id: item.id })}
                          >{item.name}</button
                        >
                      {:else}
                        <span>{item.name}</span>
                      {/if}
                    </div>
                    <span class="item-kind">{itemLabels[item.kind]}</span>
                    <span class="item-size">{size(item.bytes)}</span>
                  </li>
                {/each}
              </ul>
            </div>
          {:else}
            <p>
              {connected.page.has_next
                ? 'There are no items on this page.'
                : 'This folder is empty.'}
            </p>
          {/if}
          {#if connected.page.number > 1 || connected.page.has_next}
            <div class="line pages">
              <span>Page {connected.page.number}</span>
              <div class="actions">
                {#if connected.page.number > 1}<button
                    class="quiet-action"
                    disabled={busy !== null}
                    onclick={() => void run({ kind: 'first_page' })}>First page</button
                  >{/if}
                {#if connected.page.has_next}<button
                    disabled={busy !== null}
                    onclick={() => void run({ kind: 'next_page' })}>Next page</button
                  >{/if}
              </div>
            </div>
          {/if}
        </div>
        <div class="actions">
          <button
            class="quiet-action"
            disabled={busy !== null}
            onclick={() => void run({ kind: 'disconnect' })}>Disconnect</button
          >
          <button
            class="quiet-action"
            disabled={busy !== null}
            onclick={() => {
              if (connected)
                void run({ kind: 'open_permissions', work_account: connected.work_account });
            }}>Manage Microsoft permissions</button
          >
        </div>
      {:else if busy === 'status'}
        <p>Checking for a saved account…</p>
      {:else}
        <button class="quiet-action retry" onclick={() => void run({ kind: 'status' })}
          >Retry</button
        >
      {/if}
      {#if view?.kind === 'saved' || view?.kind === 'disconnected'}
        <details class="permissions">
          <summary>Revoke access in Microsoft</summary>
          <div class="actions">
            <button
              class="quiet-action"
              disabled={busy !== null}
              onclick={() => void run({ kind: 'open_permissions', work_account: false })}
              >Personal account</button
            >
            <button
              class="quiet-action"
              disabled={busy !== null}
              onclick={() => void run({ kind: 'open_permissions', work_account: true })}
              >Work or school account</button
            >
          </div>
        </details>
      {/if}
      <div role="status" class="cloud-status">
        {busy === 'browse' ? 'Loading current OneDrive metadata…' : status}
      </div>
      {#if error}<p role="alert" class="problem">{error}</p>{/if}
    </div>
  </details>
</section>

<style>
  details {
    border-block: 1px solid var(--border);
  }
  summary {
    cursor: pointer;
    padding-block: 10px;
    font-weight: 600;
  }
  .connection-state {
    margin-inline-start: 8px;
    font-size: 12px;
    font-weight: 400;
    color: var(--secondary);
  }
  .cloud-content,
  .account,
  .folder,
  .sign-in {
    display: grid;
    min-width: 0;
    gap: 10px;
  }
  .cloud-content {
    padding: 2px 0 12px 17px;
  }
  h3,
  p,
  ul,
  dl,
  dd {
    margin: 0;
  }
  h3 {
    font-size: 14px;
    overflow-wrap: anywhere;
  }
  p,
  dt,
  .item-kind,
  .pages {
    color: var(--secondary);
  }
  .actions,
  form,
  dl,
  .line {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 6px 12px;
  }
  form {
    align-items: end;
  }
  form label {
    display: grid;
    flex: 1 1 250px;
    max-width: 350px;
    gap: 4px;
  }
  .line {
    justify-content: space-between;
  }
  dl {
    gap: 8px 22px;
  }
  dt {
    font-size: 12px;
  }
  dd,
  .item-size,
  code {
    font-variant-numeric: tabular-nums;
  }
  code {
    justify-self: start;
    color: var(--accent);
    font-size: 21px;
    font-weight: 600;
    user-select: text;
  }
  .quota-track {
    height: 5px;
    overflow: hidden;
    border-radius: 3px;
    background: var(--border);
  }
  .quota-track span {
    display: block;
    height: 100%;
    background: var(--accent);
  }
  .account,
  .folder {
    padding-bottom: 10px;
    border-bottom: 1px solid var(--border);
  }
  ul {
    list-style: none;
    padding: 0;
  }
  .items {
    max-block-size: min(38vh, 360px);
    overflow: auto;
  }
  li {
    display: grid;
    grid-template-columns: minmax(0, 1fr) minmax(56px, 12%) auto;
    align-items: center;
    gap: 9px;
    min-height: 32px;
    padding-inline: 0 5px;
    border-bottom: 1px solid var(--border);
  }
  li:last-child {
    border-bottom: 0;
  }
  .item-name {
    display: flex;
    gap: 5px;
    align-items: center;
    min-width: 0;
    overflow-wrap: anywhere;
  }
  .item-name button {
    min-width: 0;
    min-height: 30px;
    padding-inline: 2px 5px;
    text-align: start;
    overflow-wrap: anywhere;
  }
  .item-kind,
  .pages {
    font-size: 12px;
  }
  .item-size {
    text-align: end;
  }
  .permissions {
    border: 0;
  }
  .permissions summary {
    padding-block: 3px;
    color: var(--secondary);
    font-size: 13px;
    font-weight: 400;
  }
  .retry {
    justify-self: start;
  }
  .problem {
    color: var(--error);
    overflow-wrap: anywhere;
  }
  .cloud-status:empty {
    position: absolute;
  }
</style>
