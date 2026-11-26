<script lang="ts">
  import { Channel, invoke } from '@tauri-apps/api/core';
  import { onMount, tick } from 'svelte';
  import { formatStorageBytes } from '../lib/storage-units';
  import { date, type Snapshot } from '../lib/types';

  type Scan = {
    id: string;
    root: string;
    origin: 'desktop' | 'headless';
    state: 'running' | 'completed' | 'cancelled' | 'failed' | 'interrupted';
    started: number;
    finished: number | null;
    cancel_requested: boolean;
    saved_result: boolean;
    snapshot_recorded: boolean;
    progress: {
      phase: 'scanning' | 'complete' | 'cancelled' | 'failed';
      files: number;
      logical: string;
      errors: number;
    } | null;
  };
  let {
    blocked,
    onbusy,
    onload,
  }: {
    blocked: boolean;
    onbusy: (busy: boolean) => void;
    onload: (snapshot: Snapshot, index: number) => Promise<void>;
  } = $props();

  let expanded = $state(false);
  let visible = $state(false);
  let scans = $state.raw<Scan[] | null>(null);
  let readError = $state(false);
  let refresh = $state(0);
  let action = $state<{ id: string; kind: 'open' | 'stop' } | null>(null);
  let operationId = $state<number | null>(null);
  let stopping = $state(false);
  let problem = $state('');
  let summary: HTMLElement;
  let alive = false;
  let openRequest = 0;
  let inFlight: Promise<Scan[]> | null = null;
  const activeScan = $derived(scans?.some((scan) => scan.state === 'running') ?? false);

  onMount(() => {
    alive = true;
    const updateVisibility = () => (visible = !document.hidden);
    updateVisibility();
    document.addEventListener('visibilitychange', updateVisibility);
    return () => {
      alive = false;
      document.removeEventListener('visibilitychange', updateVisibility);
    };
  });

  $effect(() => {
    void refresh;
    if (!expanded || !visible || action !== null) return;
    let current = true;
    let timer: ReturnType<typeof setTimeout> | undefined;
    async function poll(): Promise<void> {
      try {
        inFlight ??= invoke<Scan[]>('list_shared_scans').finally(() => (inFlight = null));
        const next = await inFlight;
        if (!current) return;
        scans = next;
        readError = false;
        timer = setTimeout(poll, next.some((scan) => scan.state === 'running') ? 2000 : 8000);
      } catch {
        if (current) readError = true;
      }
    }
    void poll();
    return () => {
      current = false;
      clearTimeout(timer);
    };
  });

  function status(scan: Scan): string {
    switch (scan.state) {
      case 'running':
        if (scan.cancel_requested) return 'Cancellation requested';
        return scan.progress?.phase === 'complete' ? 'Finishing scan' : 'Scanning';
      case 'completed':
        if (scan.saved_result) return 'Saved snapshot';
        return scan.snapshot_recorded ? 'Completed · snapshot unavailable' : 'Completed · no saved snapshot';
      case 'cancelled':
        return 'Cancelled';
      case 'failed':
        return 'Scan failed';
      case 'interrupted':
        return 'Interrupted';
    }
  }

  async function requestStop(scan: Scan): Promise<void> {
    if (action || readError || scan.origin !== 'headless' || scan.cancel_requested || scan.state !== 'running') return;
    action = { id: scan.id, kind: 'stop' };
    problem = '';
    try {
      await invoke('cancel_shared_scan', { id: scan.id });
      if (alive && scans)
        scans = scans.map((row) => (row.id === scan.id ? { ...row, cancel_requested: true } : row));
    } catch (error) {
      if (alive) problem = String(error);
    } finally {
      await done();
    }
  }

  async function open(scan: Scan): Promise<void> {
    if (blocked || activeScan || action || readError || !scan.saved_result) return;
    problem = '';
    action = { id: scan.id, kind: 'open' };
    const request = ++openRequest;
    onbusy(true);
    const operation = new Channel<number>();
    operation.onmessage = (id) => {
      if (alive && openRequest === request) operationId = id;
    };
    try {
      const saved = await invoke<{ snapshot: Snapshot; index_id: number } | null>(
        'open_shared_scan',
        { id: scan.id, operation },
      );
      if (saved && alive) await onload(saved.snapshot, saved.index_id);
    } catch (error) {
      if (alive) problem = String(error);
    } finally {
      openRequest += 1;
      operationId = null;
      stopping = false;
      onbusy(false);
      await done();
    }
  }

  async function stopOpening(): Promise<void> {
    if (operationId === null || stopping) return;
    const request = openRequest;
    stopping = true;
    try {
      await invoke('cancel_operation', { id: operationId });
    } catch (error) {
      if (alive && openRequest === request) {
        problem = String(error);
        stopping = false;
      }
    }
  }

  async function done(): Promise<void> {
    if (!alive) return;
    action = null;
    await tick();
    if (alive && document.activeElement === document.body) summary.focus({ preventScroll: true });
  }
</script>

<details class="shared-scans overview-diagnostics" bind:open={expanded}>
  <summary bind:this={summary}>Scan activity{#if expanded && activeScan}<span> · Active</span>{/if}</summary>
  {#if expanded}
    {#if readError}<p role="alert">Scan activity is unavailable. <button class="quiet-action" onclick={() => (refresh += 1)}>Retry</button></p>{/if}
    {#if problem}<p role="alert">{problem}</p>{/if}
    {#if scans === null && !readError}<p role="status">Loading scan activity…</p>
    {:else if scans?.length === 0}<p>No recorded scans in this profile.</p>
    {:else if scans}
      <ul aria-label="Recent scans">
        {#each scans as scan (scan.id)}
          <li>
            <div class="shared-scan-root">
              <strong title={scan.root}>{scan.root}</strong>
              <span>{scan.origin === 'desktop' ? 'Voldiq' : 'CLI / MCP'} · {date(scan.finished ?? scan.started)}</span>
            </div>
            <div class="shared-scan-state">
              <strong class:active={scan.state === 'running'}>{status(scan)}</strong>
              {#if scan.progress}
                <span>{scan.progress.files.toLocaleString()} files · {formatStorageBytes(scan.progress.logical)} logical{#if scan.progress.errors} · {scan.progress.errors.toLocaleString()} access issues{/if}</span>
              {:else if scan.state === 'running'}<span>Waiting for progress</span>{/if}
            </div>
            <div class="shared-scan-action">
              {#if action?.kind === 'open' && action.id === scan.id}
                <button class="quiet-action" onclick={stopOpening} disabled={operationId === null || stopping}>{stopping ? 'Stopping…' : 'Cancel opening'}</button>
              {:else if scan.origin === 'headless' && scan.state === 'running' && !scan.cancel_requested}
                <button class="quiet-action" onclick={() => requestStop(scan)} disabled={action !== null || readError}>Cancel scan</button>
              {:else if scan.saved_result}
                <button class="quiet-action" onclick={() => open(scan)} disabled={blocked || activeScan || action !== null || readError}>Open snapshot</button>
              {/if}
            </div>
          </li>
        {/each}
      </ul>
    {/if}
  {/if}
</details>
