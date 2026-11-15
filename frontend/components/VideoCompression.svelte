<script lang="ts">
  import { Channel, invoke } from '@tauri-apps/api/core';
  import { onDestroy } from 'svelte';
  import type { IndexId } from '../lib/index-context';
  import { bytes, type NodeId } from '../lib/types';

  let { indexId, id, paused }: { indexId: IndexId; id: NodeId; paused: boolean } = $props();

  const phases = {
    choosing: 'Choose the encoder and output file…',
    checking: 'Checking the video and encoder…',
    encoding: 'Compressing the video…',
    verifying: 'Decoding and checking the copy…',
  };
  type Progress = { id: number; phase: keyof typeof phases };
  type Result =
    | { kind: 'cancelled' | 'no_reduction' }
    | {
        kind: 'published';
        original: number;
        encoded: number;
        width: number;
        height: number;
        duration_ms: number;
        hardware: boolean;
      };
  type Receipt = { result: Result; path: string | null };

  let operation = $state<Progress | null>(null);
  let working = $state(false);
  let cancelling = $state(false);
  let receipt = $state<Receipt | null>(null);
  let cancelled = $state(false);
  let problem = $state('');
  let destroyed = false;

  function cancelOperation(id: number): Promise<void> {
    return invoke('cancel_operation', { id });
  }

  async function compress(): Promise<void> {
    if (working || paused) return;
    working = true;
    problem = '';
    cancelled = false;
    receipt = null;
    const progress = new Channel<Progress>((message) => {
      if (destroyed) void cancelOperation(message.id).catch(() => {});
      else if (working) operation = message;
    });
    try {
      receipt = await invoke<Receipt | null>('compress_local_video', { indexId, id, progress });
      cancelled = receipt === null || receipt.result.kind === 'cancelled';
    } catch (error) {
      problem = String(error);
    } finally {
      operation = null;
      working = false;
      cancelling = false;
    }
  }

  async function activate(): Promise<void> {
    if (!working) {
      await compress();
      return;
    }
    if (operation === null || operation.phase === 'choosing' || cancelling) return;
    cancelling = true;
    try {
      await cancelOperation(operation.id);
    } catch (error) {
      problem = String(error);
      cancelling = false;
    }
  }

  onDestroy(() => {
    destroyed = true;
    if (operation !== null) void cancelOperation(operation.id).catch(() => {});
  });

  let unavailable = $derived(
    working ? operation === null || operation.phase === 'choosing' || cancelling : paused,
  );
</script>

<details>
  <summary>Compress video</summary>
  <p>
    Save a separate MP4 copy. Recompression can reduce quality. Choose a trusted FFmpeg installation
    with ffprobe in the same folder.
  </p>
  <button class="quiet-action" aria-disabled={unavailable} onclick={() => void activate()}>
    {working && operation?.phase !== 'choosing'
      ? 'Cancel compression'
      : 'Choose encoder and save location'}
  </button>
  <div role="status" aria-live="polite">
    {#if working}
      <p>{cancelling ? 'Cancelling…' : phases[operation?.phase ?? 'choosing']}</p>
    {:else if cancelled}
      <p>Compression cancelled.</p>
    {:else if receipt?.result.kind === 'no_reduction'}
      <p>The copy reached the original size. No MP4 was saved.</p>
    {:else if receipt?.result.kind === 'published'}
      {@const result = receipt.result}
      <p>
        <strong>Copy is {bytes(result.original - result.encoded)} smaller</strong>
        ({((1 - result.encoded / result.original) * 100).toFixed(1)}% smaller)
      </p>
      <dl>
        <div>
          <dt>Original</dt>
          <dd>{bytes(result.original)}</dd>
        </div>
        <div>
          <dt>MP4 copy</dt>
          <dd>{bytes(result.encoded)}</dd>
        </div>
      </dl>
      <p>
        {result.width} × {result.height} · {(result.duration_ms / 1000).toFixed(1)} s · {result.hardware
          ? 'Hardware H.264'
          : 'CPU H.264'}
      </p>
      <p>Open the copy in your media player to compare quality.</p>
      <code>{receipt.path}</code>
    {/if}
  </div>
  {#if problem}<p class="problem" role="alert">{problem}</p>{/if}
</details>

<style>
  details {
    min-width: 0;
    border-top: 1px solid var(--border);
    margin-top: 8px;
    padding-top: 8px;
    font-size: 13px;
  }

  summary {
    width: fit-content;
    font-weight: 600;
    cursor: pointer;
  }

  p {
    margin: 8px 0;
  }

  button[aria-disabled='true'] {
    opacity: 0.55;
    cursor: default;
  }

  dl {
    display: flex;
    flex-wrap: wrap;
    gap: 6px 22px;
    margin: 0;
  }

  dt {
    color: var(--secondary);
  }

  dd {
    margin: 0;
    font-variant-numeric: tabular-nums;
  }

  code,
  .problem {
    overflow-wrap: anywhere;
  }

  code {
    user-select: text;
  }

  .problem {
    color: var(--error);
  }
</style>
