<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { tick } from 'svelte';
  import { createScanControl, type ScanControlView } from '../lib/scan-control';
  let {
    id,
    supported,
    onstate,
  }: { id: number | null; supported: boolean; onstate: (state: ScanControlView) => void } =
    $props();
  let view = $state<ScanControlView>({ observed: null, activity: null, problem: null });
  let controller: ReturnType<typeof createScanControl> | null = null;
  let button: HTMLButtonElement;
  let retryButton = $state<HTMLButtonElement>();
  $effect(() => {
    const owner = id;
    let alive = true;
    view = { observed: null, activity: null, problem: null };
    onstate({ observed: null, activity: null, problem: null });
    if (owner === null || !supported) {
      controller = null;
      return;
    }
    const session = createScanControl(
      owner,
      {
        query: (id) => invoke('get_scan_control', { id }),
        pause: (id) => invoke('pause_scan', { id }),
        resume: (id) => invoke('resume_scan', { id }),
      },
      (next) => {
        if (alive) {
          view = next;
          onstate(next);
        }
      },
    );
    controller = session;
    void session.load();
    return () => {
      alive = false;
      session.dispose();
      if (controller === session) controller = null;
    };
  });
  async function change() {
    const session = controller;
    const heldFocus = document.activeElement === button;
    if (!session) return;
    if (view.observed === 'paused' || view.observed === 'pause_requested') await session.resume();
    else await session.pause();
    await restoreFocus(session, heldFocus);
  }
  async function retry() {
    const session = controller;
    const heldFocus = document.activeElement === retryButton;
    if (!session) return;
    await session.load();
    await restoreFocus(session, heldFocus);
  }
  async function restoreFocus(session: ReturnType<typeof createScanControl>, heldFocus: boolean) {
    await tick();
    if (!heldFocus || controller !== session || document.activeElement !== document.body) return;
    const target = button && !button.disabled ? button : retryButton;
    if (target && !target.disabled) target.focus({ preventScroll: true });
  }
</script>

<button
  bind:this={button}
  onclick={change}
  disabled={!supported ||
    id === null ||
    (view.activity !== null && view.activity !== 'query') ||
    view.observed === null ||
    view.observed === 'idle' ||
    view.observed === 'finished'}
  title={!supported
    ? 'Pause is available in Directory enumeration mode.'
    : view.observed === 'pause_requested'
      ? 'Waiting for the current metadata batch to finish. Resume withdraws the request.'
      : undefined}
  >{view.activity === 'pause'
    ? 'Requesting pause…'
    : view.activity === 'resume'
      ? 'Resuming…'
      : view.observed === 'pause_requested'
        ? 'Withdraw pause'
        : view.observed === 'paused'
          ? 'Resume scan'
          : 'Pause scan'}</button
>
{#if !supported && id !== null}<small>Pause requires Directory enumeration mode.</small>{/if}
{#if view.problem}<span class="control-problem" role="status">{view.problem}</span><button
    bind:this={retryButton}
    onclick={retry}
    disabled={view.activity !== null}>Retry pause controls</button
  >{/if}

<style>
  .control-problem {
    color: var(--error);
    font-size: 12px;
    max-width: 36ch;
    overflow-wrap: anywhere;
  }
</style>

