<script lang="ts">
  import { invoke, isTauri } from '@tauri-apps/api/core';
  import { onMount } from 'svelte';

  type CloseBehavior = 'exit' | 'tray';

  let behavior = $state<CloseBehavior | null>(null),
    pending = $state(false),
    error = $state('');

  async function load(): Promise<void> {
    pending = true;
    error = '';
    try {
      behavior = await invoke<CloseBehavior>('get_window_close_behavior');
    } catch (failure) {
      error = String(failure);
    } finally {
      pending = false;
    }
  }

  onMount(() => {
    if (isTauri()) void load();
  });

  async function change(event: Event & { currentTarget: HTMLSelectElement }): Promise<void> {
    const select = event.currentTarget;
    const next = select.value;
    select.value = behavior ?? '';
    if (pending || behavior === null || (next !== 'exit' && next !== 'tray')) return;
    pending = true;
    error = '';
    try {
      behavior = await invoke<CloseBehavior>('set_window_close_behavior', { behavior: next });
    } catch (failure) {
      error = String(failure);
    } finally {
      pending = false;
    }
  }
</script>

<section aria-labelledby="window-settings-heading">
  <h2 id="window-settings-heading">Window</h2>
  <label>
    <span>
      When I close Voldiq
      <small id="window-close-help">
        {#if behavior === 'tray'}
          The tray menu can reopen or exit Voldiq. Live monitoring stops when the window closes.
          Work already started may finish.
        {:else}
          The system tray menu lets you reopen Voldiq or exit.
        {/if}
      </small>
    </span>
    <select
      value={behavior ?? ''}
      onchange={change}
      disabled={behavior === null}
      aria-disabled={pending}
      aria-describedby="window-close-help"
    >
      {#if behavior === null}
        <option value="">{pending ? 'Loading…' : 'Unavailable'}</option>
      {/if}
      <option value="exit">Exit Voldiq</option>
      <option value="tray">Keep Voldiq in the system tray</option>
    </select>
  </label>
  {#if error}
    <div class="settings-action">
      <span role="alert">{error}</span>
      {#if behavior === null}<button onclick={load}>Retry</button>{/if}
    </div>
  {/if}
</section>
