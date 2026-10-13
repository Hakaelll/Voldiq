<script lang="ts">
  import { onDestroy } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { bytes } from '../lib/types';

  const labels = {
    images: 'Images',
    containers: 'Containers',
    volumes: 'Volumes · persistent data',
    build_cache: 'Build cache',
  };
  type DockerStorage = {
    endpoint: string;
    categories: { category: keyof typeof labels; count: number | null; size: number | null }[];
  };
  type LoadState =
    | { kind: 'idle' }
    | { kind: 'loading' }
    | { kind: 'ready'; data: DockerStorage }
    | { kind: 'error'; message: string };
  let state = $state<LoadState>({ kind: 'idle' });
  let largest = $derived(
    state.kind === 'ready'
      ? Math.max(0, ...state.data.categories.map((item) => item.size ?? 0))
      : 0,
  );
  const colors = {
    images: '#075bdd',
    containers: '#087f8c',
    volumes: '#ed701b',
    build_cache: '#717a84',
  };
  let generation = 0;
  async function load(): Promise<void> {
    const token = ++generation;
    state = { kind: 'loading' };
    try {
      const data = await invoke<DockerStorage>('query_docker_storage');
      if (token === generation) state = { kind: 'ready', data };
    } catch (problem) {
      if (token === generation) state = { kind: 'error', message: String(problem) };
    }
  }
  onDestroy(() => {
    generation++;
  });
</script>

<section class="docker-storage" aria-label="Docker storage" aria-busy={state.kind === 'loading'}>
  <div class="docker-summary">
    <h2>Docker <small>· Local engine</small></h2>
    <button onclick={load} disabled={state.kind === 'loading'}>
      {state.kind === 'loading' ? 'Analyzing…' : state.kind === 'ready' ? 'Refresh' : 'Analyze'}
    </button>
  </div>
  {#if state.kind === 'ready'}
    <div class="docker-endpoint" title={state.data.endpoint}>{state.data.endpoint}</div>
    <table>
      <caption class="sr-only"
        >Docker category sizes may overlap. Volumes contain persistent data.</caption
      >
      <thead
        ><tr><th>Category</th><th class="number">Items</th><th class="number">Size</th></tr></thead
      >
      <tbody
        >{#each state.data.categories as item (item.category)}
          <tr
            ><td>{labels[item.category]}</td><td class="number">{item.count ?? 'Unknown'}</td>
            <td class="number"
              ><div class="docker-size">
                {#if item.size !== null}<span class="docker-bar" aria-hidden="true"
                    ><span
                      style:width={`${largest ? (item.size / largest) * 100 : 0}%`}
                      style:background={colors[item.category]}
                    ></span></span
                  >{/if}<span>{item.size === null ? 'Unknown' : bytes(item.size)}</span>
              </div></td
            ></tr
          >
        {/each}</tbody
      >
    </table>
    <div class="docker-note">Category sizes may overlap.</div>
  {:else if state.kind === 'error'}
    <p role="status">{state.message}</p>
  {/if}
</section>

<style>
  .docker-storage {
    margin-top: 16px;
    min-width: 0;
    overflow: hidden;
    border-top: 1px solid var(--border);
    background: var(--canvas);
  }
  .docker-summary {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    padding: 20px 0 16px;
  }
  h2 {
    font-size: 18px;
  }
  h2 small {
    font-size: 13px;
    font-weight: 400;
    color: var(--secondary);
  }
  .docker-size {
    display: flex;
    align-items: center;
    gap: 10px;
    justify-content: flex-end;
  }
  .docker-bar {
    width: 70px;
    height: 14px;
    background: var(--surface);
    flex-shrink: 0;
  }
  .docker-bar > span {
    display: block;
    height: 100%;
  }
  .docker-endpoint,
  .docker-note,
  p {
    color: var(--muted);
    font-size: 12px;
  }
  .docker-endpoint {
    padding: 0 0 12px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  table {
    width: 100%;
    border-collapse: collapse;
    font-size: 12px;
  }
  th,
  td {
    padding: 10px 0;
    border-top: 1px solid var(--border);
    text-align: left;
  }
  th {
    color: var(--muted);
    font-weight: 500;
  }
  .number {
    text-align: right;
    font-variant-numeric: tabular-nums;
  }
  .docker-note {
    padding: 10px 0;
    border-top: 1px solid var(--border);
  }
  p {
    margin: 0;
    padding: 0 0 12px;
    overflow-wrap: anywhere;
  }
  .sr-only {
    position: absolute;
    width: 1px;
    height: 1px;
    padding: 0;
    overflow: hidden;
    clip-path: inset(50%);
    white-space: nowrap;
  }
</style>
