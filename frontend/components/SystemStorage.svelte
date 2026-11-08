<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { utcDate } from '../lib/history-activity';
  import type { IndexId } from '../lib/index-context';
  import { systemStorageQueries } from '../lib/queries';
  import type { SystemStorageObservation, SystemStorageReport } from '../lib/system-storage';
  import { bytes } from '../lib/types';

  let { indexId, paused }: { indexId: IndexId; paused: boolean } = $props();
  type State =
    | { kind: 'idle' | 'loading' | 'cancelled' }
    | { kind: 'error'; message: string }
    | { kind: 'ready'; report: SystemStorageReport };
  let model = $state.raw<State>({ kind: 'idle' });
  let open = $state(false);
  let reload = $state(0);
  let cancelRead = () => {};
  let settingsError = $state('');
  let diskError = $state('');
  let pickingDisk = $state(false);
  let selectedDisk = $state.raw<{ owner: IndexId; path: string } | null>(null);
  const diskPath = $derived(selectedDisk?.owner === indexId ? selectedDisk.path : null);

  const missing: Record<Exclude<SystemStorageObservation['kind'], 'indexed'>, string> = {
    not_observed: 'Not observed in the index',
    outside_scope: 'Outside the scanned folder',
    location_unavailable: 'Location unavailable',
  };
  const help = {
    paging: 'Paging and swap observations on indexed paths.',
    hibernation: 'Windows stores hibernation and Fast Startup data here.',
    previous_windows: 'Windows.old can support rollback. Manage it in Windows Storage settings.',
    component_store:
      'WinSxS shares hard links with Windows. DISM can analyze component-store usage.',
    docker: 'Host virtual disk. Review disk settings in Docker Desktop.',
    wsl: 'Host virtual disk at a registered WSL 2 location.',
  };

  $effect(() => {
    void reload;
    if (!open || paused) {
      model = { kind: 'idle' };
      return;
    }
    const request = systemStorageQueries.request({ indexId, dockerDisk: diskPath });
    cancelRead = request.cancel;
    model = { kind: 'loading' };
    let current = true;
    void request.result.then(
      (report) => {
        if (current) model = report ? { kind: 'ready', report } : { kind: 'cancelled' };
      },
      (error: unknown) => {
        if (current) model = { kind: 'error', message: String(error) };
      },
    );
    return () => {
      current = false;
      request.cancel();
    };
  });

  async function openSettings(): Promise<void> {
    settingsError = '';
    try {
      await invoke('open_windows_storage');
    } catch (error) {
      settingsError = String(error);
    }
  }

  async function chooseDisk(): Promise<void> {
    if (pickingDisk) return;
    const owner = indexId;
    diskError = '';
    pickingDisk = true;
    try {
      const path = await invoke<string | null>('choose_docker_disk');
      if (path && owner === indexId && open && !paused) selectedDisk = { owner, path };
    } catch (error) {
      if (owner === indexId) diskError = String(error);
    } finally {
      pickingDisk = false;
    }
  }
</script>

<section class="type-panel system-storage" aria-labelledby="system-storage-heading">
  <div class="panel-heading">
    <h2 id="system-storage-heading">System storage</h2>
    <button aria-expanded={open} aria-controls="system-storage-body" onclick={() => (open = !open)}>
      {open ? 'Close' : 'Check system storage'}
    </button>
  </div>
  <div id="system-storage-body" hidden={!open}>
    {#if open}
      {#if paused}
        <p role="status">Available after the current operation.</p>
      {:else if model.kind === 'loading'}
        <div class="system-controls">
          <p role="status">Checking indexed system paths…</p>
          <button onclick={() => cancelRead()}>Cancel</button>
        </div>
      {:else if model.kind === 'cancelled'}
        <div class="system-controls">
          <p role="status">Check cancelled.</p>
          <button onclick={() => (reload += 1)}>Retry</button>
        </div>
      {:else if model.kind === 'error'}
        <div class="system-controls">
          <p role="alert">{model.message}</p>
          <button onclick={() => (reload += 1)}>Retry</button>
        </div>
      {:else if model.kind === 'ready' && model.report.kind === 'recorded_scan'}
        <p>Scan a live folder to check it against current Windows locations.</p>
      {:else if model.kind === 'ready' && model.report.kind === 'live'}
        {@const report = model.report}
        <div class="system-controls">
          <p title={report.root}>
            {report.root} · {utcDate(report.indexed_at)}{report.scope_complete
              ? ''
              : ' · Incomplete index coverage'}
          </p>
          <button onclick={() => (reload += 1)}>Refresh</button>
        </div>
        <p class="system-note">
          {#if report.paging_registry === 'unavailable'}The Windows paging registry was unavailable.
            Default paths are still checked.
          {:else if report.paging_registry === 'partial'}The Windows paging registry returned
            partial results.
          {:else}Existing paging files reported by Windows are matched to this scan.{/if}
        </p>
        <p class="system-note">
          These observations come from the scan and can overlap with its totals. Sizes inside
          virtual disks and recoverable space are unmeasured.
        </p>
        <!-- svelte-ignore a11y_no_noninteractive_tabindex (Long virtual-disk lists scroll.) -->
        <div class="system-list" tabindex="0" role="region" aria-label="Indexed system locations">
          <ul>
            {#each report.items as item}
              <li>
                <span>
                  <strong><bdi>{item.label}</bdi></strong>
                  {#if item.observation.kind === 'indexed'}
                    <small title={item.observation.path}>{item.observation.path}</small>
                  {/if}
                  <small>{help[item.category]}</small>
                </span>
                {#if item.observation.kind === 'indexed'}
                  <span class="system-size">
                    <strong title={`${item.observation.logical} logical bytes`}
                      >{bytes(Number(item.observation.logical))} logical</strong
                    >
                    {#if item.observation.allocated !== null}
                      <small title={`${item.observation.allocated} allocated bytes`}
                        >{bytes(Number(item.observation.allocated))} allocated</small
                      >
                    {/if}
                    {#if item.observation.incomplete}<small>Partial</small>{/if}
                  </span>
                {:else}
                  <small class="system-size">{missing[item.observation.kind]}</small>
                {/if}
              </li>
            {/each}
          </ul>
        </div>
        <p class="system-note">
          {#if !report.items.some((item) => item.category === 'docker')}No Docker Desktop disk paths
            observed in this scope.
          {/if}
          {#if !report.docker_defaults_available}Some Docker defaults were unavailable.
          {/if}
          For a custom Docker location, select the VHDX shown in Docker Desktop’s disk settings.
        </p>
        <div class="system-disk-actions">
          <button class="quiet-action" disabled={pickingDisk} onclick={chooseDisk}
            >Choose Docker disk</button
          >
          {#if diskPath}<button
              class="quiet-action"
              onclick={() => {
                selectedDisk = null;
                diskError = '';
              }}>Clear selected disk</button
            >{/if}
        </div>
        {#if diskError}<p role="alert">{diskError}</p>{/if}
        <p class="system-note">
          {#if report.wsl_registrations === 'unavailable'}WSL registrations were unavailable.
          {:else if report.wsl_registrations === 'partial'}Current-user WSL registration coverage
            was partial.
          {:else}WSL checks disk locations registered by the current user.{/if}
          {#if !report.items.some((item) => item.category === 'wsl')}
            No matching WSL disk paths observed.{/if}
        </p>
        <button class="quiet-action" onclick={openSettings}>Open Windows Storage settings</button>
        {#if settingsError}<p role="alert">{settingsError}</p>{/if}
      {/if}
    {/if}
  </div>
</section>

<style>
  .system-controls {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
  }
  .system-controls p {
    overflow-wrap: anywhere;
  }
  .system-disk-actions {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }
  .system-list {
    max-height: 340px;
    overflow: auto;
  }
  ul {
    list-style: none;
    padding: 0;
    margin: 0;
  }
  li {
    display: flex;
    align-items: start;
    justify-content: space-between;
    gap: 12px;
    padding-block: 8px;
    border-bottom: 1px solid var(--border);
    overflow-wrap: anywhere;
  }
  li strong,
  li small {
    display: block;
  }
  .system-note,
  li small {
    color: var(--muted);
  }
  .system-size {
    flex: 0 0 auto;
    text-align: right;
    max-width: 40%;
  }
  @media (max-width: 550px) {
    li {
      flex-wrap: wrap;
      gap: 4px;
    }
    .system-size {
      text-align: left;
      max-width: 100%;
    }
  }
</style>
