<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { applicationBytes } from '../lib/applications';
  import type { GameLibraryReport } from '../lib/game-libraries';
  import type { IndexId } from '../lib/index-context';
  import { gameLibraryQueries } from '../lib/queries';

  let {
    indexId,
    scope,
    paused,
    manual = $bindable(),
    onopen,
  }: {
    indexId: IndexId | null;
    scope: number;
    paused: boolean;
    manual: string[];
    onopen: (id: number) => void;
  } = $props();

  const limit = 25;
  const providers = { steam: 'Steam', epic: 'Epic Games', gog: 'GOG', itch: 'itch.io' };
  let position = $state<{ identity: string; offset: number } | null>(null);
  const identity = $derived(`${indexId}:${scope}`);
  const offset = $derived(position?.identity === identity ? position.offset : 0);
  let reload = $state(0);
  let report = $state.raw<GameLibraryReport | null>(null);
  let loadState = $state<'idle' | 'loading' | 'ready' | 'failed'>('idle');
  let selecting = $state(false);
  let notice = $state('');
  const size = (value: string | null) => (value === null ? 'Unobserved' : applicationBytes(value));

  $effect(() => {
    reload;
    const current = identity;
    const start = offset;
    if (indexId === null || paused) {
      report = null;
      loadState = 'idle';
      return;
    }
    loadState = 'loading';
    const job = gameLibraryQueries.request({
      indexId,
      query: { scope, offset: start, limit, manual: [...manual] },
    });
    let live = true;
    void job.result.then(
      (value) => {
        if (!live) return;
        if (value?.kind === 'available' && start > 0 && start >= value.total) {
          position = { identity: current, offset: 0 };
          return;
        }
        report = value;
        loadState = value ? 'ready' : 'idle';
      },
      () => {
        if (!live) return;
        report = null;
        loadState = 'failed';
      },
    );
    return () => {
      live = false;
      job.cancel();
    };
  });

  async function addFolder(): Promise<void> {
    selecting = true;
    notice = '';
    try {
      const chosen = await invoke<string | null>('choose_folder');
      if (!chosen) return;
      if (manual.some((path) => path.toLowerCase() === chosen.toLowerCase())) {
        notice = 'This folder is already listed.';
      } else if (manual.length < 8) manual = [...manual, chosen];
    } catch {
      notice = 'The folder picker is unavailable.';
    } finally {
      selecting = false;
    }
  }
</script>

<section aria-labelledby="library-heading" aria-busy={loadState === 'loading'}>
  <div class="library-heading">
    <div>
      <h2 id="library-heading">Game libraries</h2>
      <p>Indexed storage matched to Steam, Epic Games, GOG and itch.io manifests.</p>
    </div>
    <button
      disabled={indexId === null || paused || loadState === 'loading'}
      onclick={() => reload++}>Refresh</button
    >
  </div>

  {#if loadState === 'idle'}
    <p role="status">
      {paused ? 'Waiting for the folder analysis.' : 'Complete a live scan to see game libraries.'}
    </p>
  {:else if loadState === 'failed'}
    <p role="alert">Game library details could not be loaded. Refresh to retry.</p>
  {:else if loadState === 'loading' && !report}
    <p role="status">Reading indexed launcher manifests…</p>
  {:else if report?.kind === 'recorded_scan'}
    <p role="status">Run a live scan to read current launcher manifests.</p>
  {:else if report?.kind === 'unsupported'}
    <p role="status">Launcher attribution requires Windows.</p>
  {:else if report?.kind === 'available'}
    <p class="library-coverage" role="status">
      {loadState === 'loading'
        ? 'Updating results…'
        : `${report.total.toLocaleString()} recognized games · ${size(report.shared_bytes)} shared · ${size(report.unattributed_bytes)} unattributed`}
    </p>
    {#if report.coverage_partial}
      <p>
        Coverage is partial.{report.omitted_manifests
          ? ` ${report.omitted_manifests.toLocaleString()} launcher records could not be attributed.`
          : ''}
      </p>
    {/if}
    {#if report.items.length}
      <ul class="library-list">
        {#each report.items as item (`${item.provider}:${item.product}:${item.id}`)}
          <li>
            <div class="game-row">
              <div class="game-name">
                <button
                  title={item.path}
                  onclick={() => onopen(item.id)}
                  disabled={loadState === 'loading'}>{item.title}</button
                >
                <small
                  >{providers[item.provider]}{item.partial ? ' · Partial attribution' : ''}</small
                >
              </div>
              <div class="installation">
                <small>Installation</small><strong>{size(item.installation_bytes)}</strong>
              </div>
            </div>
            {#if item.shaders_bytes !== null || item.workshop_bytes !== null || item.user_data_bytes !== null || item.dlc_records}
              <dl>
                {#if item.shaders_bytes !== null}<div>
                    <dt>Shader cache</dt>
                    <dd>{size(item.shaders_bytes)}</dd>
                  </div>{/if}
                {#if item.workshop_bytes !== null}<div>
                    <dt>Workshop</dt>
                    <dd>{size(item.workshop_bytes)}</dd>
                  </div>{/if}
                {#if item.user_data_bytes !== null}<div>
                    <dt>Saves and user data</dt>
                    <dd>{size(item.user_data_bytes)}</dd>
                  </div>{/if}
                {#if item.dlc_records}<div>
                    <dt>DLC records · {item.dlc_records}</dt>
                    <dd>{item.dlc_bytes === null ? 'No separate size' : size(item.dlc_bytes)}</dd>
                  </div>{/if}
              </dl>
            {/if}
          </li>
        {/each}
      </ul>
      {#if report.total > limit}
        <nav class="library-pages" aria-label="Game library pages">
          <button
            disabled={offset === 0 || loadState === 'loading'}
            onclick={() => (position = { identity, offset: offset - limit })}>Previous</button
          >
          <span>Page {Math.floor(offset / limit) + 1} of {Math.ceil(report.total / limit)}</span>
          <button
            disabled={offset + limit >= report.total || loadState === 'loading'}
            onclick={() => (position = { identity, offset: offset + limit })}>Next</button
          >
        </nav>
      {/if}
    {:else}
      <p>
        No launcher records matched folders in this analysis. Add a folder below to see its measured
        size.
      </p>
    {/if}
    <p>
      Sizes describe indexed files. Saves, mods and DLC inside an installation may be included in
      its size. Shader caches can regenerate and briefly affect performance.
    </p>
  {/if}

  <div class="manual-libraries">
    <div class="library-heading">
      <div>
        <h3>Manual folders</h3>
        <p>
          Measure a folder in the current analysis. These totals can overlap the game results above.
        </p>
      </div>
      <button
        disabled={indexId === null || paused || selecting || manual.length >= 8}
        onclick={addFolder}>Add folder</button
      >
    </div>
    {#if notice}<p role="status">{notice}</p>{/if}
    {#if manual.length}
      <ul class="library-list">
        {#each manual as path, i (path)}
          {@const measured =
            loadState === 'ready' && report?.kind === 'available' ? report.manual[i] : null}
          <li class="manual-row">
            <div class="manual-path" title={path}>{path}</div>
            {#if measured}
              <span>{measured.partial ? 'At least ' : ''}{size(measured.logical_bytes)}</span>
              <button onclick={() => onopen(measured.id)}>Open</button>
            {:else}
              <span>{loadState === 'loading' ? 'Measuring…' : 'Unobserved in this analysis'}</span>
            {/if}
            <button
              aria-label={`Remove ${path}`}
              onclick={() => (manual = manual.filter((item) => item !== path))}>Remove</button
            >
          </li>
        {/each}
      </ul>
    {/if}
  </div>
</section>

<style>
  .library-heading,
  .game-row,
  .manual-row,
  .library-pages {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 8px 16px;
    flex-wrap: wrap;
  }
  h2,
  h3 {
    margin: 0;
  }
  h2 {
    font-size: 18px;
  }
  h3 {
    font-size: 15px;
  }
  p,
  small,
  dt {
    color: var(--secondary);
  }
  p {
    margin: 4px 0 10px;
  }
  .library-coverage {
    margin-top: 12px;
  }
  .library-list {
    list-style: none;
    padding: 0;
    margin: 8px 0 12px;
  }
  li {
    padding: 9px 0;
    border-top: 1px solid var(--border);
  }
  .game-name {
    display: grid;
    gap: 2px;
    min-width: 0;
  }
  .game-name button {
    padding: 0;
    min-height: 26px;
    color: var(--accent);
    background: transparent;
    border: 0;
    text-align: left;
    overflow-wrap: anywhere;
  }
  .game-name button:hover {
    text-decoration: underline;
  }
  .installation {
    display: grid;
    text-align: right;
  }
  dl {
    display: flex;
    flex-wrap: wrap;
    gap: 4px 18px;
    margin: 6px 0 0;
    font-size: 13px;
  }
  dl div {
    display: flex;
    gap: 5px;
  }
  dd {
    margin: 0;
    font-variant-numeric: tabular-nums;
  }
  .library-pages {
    justify-content: end;
  }
  .manual-libraries {
    border-top: 1px solid var(--border);
    margin-top: 16px;
    padding-top: 12px;
  }
  .manual-path {
    flex: 1 1 240px;
    overflow-wrap: anywhere;
  }
</style>
