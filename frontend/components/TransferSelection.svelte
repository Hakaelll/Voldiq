<script lang="ts">
  import { onDestroy } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import type { IndexId } from '../lib/index-context';
  import { bytes, type Volume } from '../lib/types';
  import { volumeCaller, volumeMeasurement } from '../lib/volume-space';
  import Icon from './Icon.svelte';

  let { indexId, nodes, disabled, onreview }: {
    indexId: IndexId;
    nodes: ReadonlySet<number>;
    disabled: boolean;
    onreview: (id: string) => void;
  } = $props();

  let open = $state(false);
  let loading = $state(false);
  let preparing = $state(false);
  let error = $state('');
  let drives = $state.raw<Volume[]>([]);
  let recycle = $state(false);
  let dragging = $state(false);
  let highlighted = $state<string | null>(null);
  let request = 0;
  let pressed: { pointer: number; x: number; y: number; selection: ReadonlySet<number> } | null = null;
  let suppressClick = false;

  function usable(): boolean {
    return !disabled && !preparing && nodes.size >= 1 && nodes.size <= 500;
  }

  async function refresh(): Promise<void> {
    const owner = ++request;
    loading = true;
    error = '';
    drives = [];
    try {
      const result = await invoke<Volume[]>('transfer_destinations', { indexId });
      if (owner === request) drives = result;
    } catch (problem) {
      if (owner === request) error = String(problem);
    } finally {
      if (owner === request) loading = false;
    }
  }

  function openPanel(): void {
    if (open) return;
    open = true;
    void refresh();
  }

  function stopDrag(): void {
    dragging = false;
    highlighted = null;
    pressed = null;
  }

  function press(event: PointerEvent): void {
    suppressClick = false;
    if (!usable() || event.button !== 0 || event.pointerType !== 'mouse') return;
    pressed = { pointer: event.pointerId, x: event.clientX, y: event.clientY, selection: nodes };
    if (event.currentTarget instanceof HTMLButtonElement) {
      event.currentTarget.setPointerCapture(event.pointerId);
    }
  }

  function move(event: PointerEvent): void {
    if (!pressed || pressed.pointer !== event.pointerId) return;
    if (!dragging && Math.hypot(event.clientX - pressed.x, event.clientY - pressed.y) >= 6) {
      dragging = true;
      suppressClick = true;
      openPanel();
    }
    if (dragging && pressed.selection === nodes && usable()) {
      highlighted = document.elementFromPoint(event.clientX, event.clientY)
        ?.closest<HTMLElement>('[data-transfer-drive]')?.dataset.transferDrive ?? null;
    }
  }

  function release(event: PointerEvent): void {
    if (!pressed || pressed.pointer !== event.pointerId) return;
    move(event);
    const drive = dragging && usable() && pressed.selection === nodes
      ? drives.find((drive) => drive.path === highlighted)
      : undefined;
    stopDrag();
    if (drive) void prepare(drive);
  }

  async function prepare(drive: Volume): Promise<void> {
    if (!usable() || !drives.includes(drive)) return;
    const expectedVolume = volumeMeasurement(drive)?.volume ??
      (drive.space.kind === 'unavailable' ? drive.space.caller?.volume : undefined);
    if (!expectedVolume) return;
    const owner = ++request;
    const selection = nodes;
    preparing = true;
    error = '';
    try {
      const id = await invoke<string | null>('prepare_desktop_transfer', {
        indexId, nodes: [...selection], drive: drive.path, expectedVolume,
        originals: recycle ? 'recycle_after_verification' : 'keep',
      });
      if (owner === request && selection === nodes && id) onreview(id);
    } catch (problem) {
      if (owner === request) error = String(problem);
    } finally {
      if (owner === request) preparing = false;
    }
  }

  onDestroy(() => { request += 1; });
</script>

<button type="button" class="quiet-action transfer-handle" class:dragging disabled={!usable()}
  aria-expanded={open} aria-controls="transfer-destinations" title="Choose a destination or drag this selection"
  onclick={(event) => {
    if (suppressClick && event.detail !== 0) { suppressClick = false; return; }
    if (open) open = false;
    else openPanel();
  }} onpointerdown={press} onpointermove={move} onpointerup={release}
  onpointercancel={stopDrag} onlostpointercapture={stopDrag}>
  <Icon name="disk" size={16} /> Transfer {nodes.size} {nodes.size === 1 ? 'item' : 'items'}
</button>
{#if open}
  <section id="transfer-destinations" aria-label="Transfer destinations" class="destinations">
    <div class="controls">
      <strong>Transfer destinations</strong>
      <button class="quiet-action" onclick={() => void refresh()} disabled={loading || preparing}>Refresh</button>
    </div>
    {#if error}<p role="alert" class="error">{error}</p>{/if}
    {#if preparing}<p role="status">Preparing the exact transfer plan…</p>
    {:else if loading}<p role="status">Checking connected drives…</p>
    {:else if drives.length}
      <label><input type="checkbox" bind:checked={recycle} disabled={dragging} /> Recycle originals after verification</label>
      <ul>
        {#each drives as drive (drive.path)}
          {@const available = volumeCaller(drive)?.available}
          <li>
            <button class="quiet-action destination" class:highlighted={highlighted === drive.path}
              data-transfer-drive={drive.path} onclick={() => void prepare(drive)}
              disabled={!usable()}>
              <Icon name="disk" size={18} />
              <span><strong>{drive.name || 'NTFS drive'} · {drive.path}</strong>
              <small>{drive.drive_type === 'Removable' ? 'Removable' : 'Local'}{available ? ` · ${bytes(Number(available))} available` : ''}</small></span>
              <span class="choose">Choose folder</span>
            </button>
          </li>
        {/each}
      </ul>
    {:else if !error}<p>Connect another NTFS volume, then refresh.</p>{/if}
  </section>
{/if}

<style>
  .transfer-handle { user-select: none; }
  .transfer-handle.dragging { cursor: grabbing; }
  .destinations { flex: 1 0 100%; display: grid; gap: 8px; border-top: 1px solid var(--border); padding-top: 8px; margin-block: 4px; }
  .controls, .destination, label { display: flex; align-items: center; gap: 8px; }
  .controls, .destination { justify-content: space-between; }
  label { color: var(--secondary); }
  ul { margin: 0; padding: 0; list-style: none; max-height: 156px; overflow: auto; }
  .destination { width: 100%; min-width: 0; text-align: start; }
  .destination > span:nth-child(2) { display: grid; flex: 1; min-width: 0; }
  .destination strong { overflow: hidden; text-overflow: ellipsis; }
  .destination.highlighted { background: var(--selected); color: var(--accent); outline: 1px solid var(--accent); outline-offset: -1px; }
  .choose { font-size: 12px; }
  .error { color: var(--error); }
</style>
