<script lang="ts">
  import { invoke, isTauri } from '@tauri-apps/api/core';
  import { onMount } from 'svelte';

  type StartupStatus =
    | { kind: 'ready'; registered: boolean }
    | { kind: 'not_installed' }
    | { kind: 'unsupported_path' }
    | { kind: 'conflict' };

  let status = $state<StartupStatus | null>(null),
    pending = $state(false),
    error = $state('');

  onMount(() => {
    if (isTauri()) void load();
  });

  async function load(): Promise<void> {
    pending = true;
    error = '';
    try {
      status = await invoke<StartupStatus>('windows_startup', { registered: null });
    } catch (failure) {
      error = String(failure);
    } finally {
      pending = false;
    }
  }

  async function change(event: Event & { currentTarget: HTMLInputElement }): Promise<void> {
    const checkbox = event.currentTarget;
    const registered = checkbox.checked;
    checkbox.checked = status?.kind === 'ready' && status.registered;
    if (pending || status?.kind !== 'ready') return;
    pending = true;
    error = '';
    try {
      status = await invoke<StartupStatus>('windows_startup', { registered });
    } catch (failure) {
      error = String(failure);
    } finally {
      pending = false;
    }
  }
</script>

<label>
  <span>
    Start with Windows
    <small id="windows-startup-help">
      {#if status?.kind === 'ready'}
        Register Voldiq to open when you sign in. Windows Startup apps can disable this entry.
      {:else if status?.kind === 'not_installed'}
        Available from the installed copy of Voldiq.
      {:else if status?.kind === 'unsupported_path'}
        Startup needs a local fixed drive and a command within Windows' length limit.
      {:else if status?.kind === 'conflict'}
        A conflicting Windows startup entry needs to be resolved first.
      {:else if pending}
        Reading Windows startup settings…
      {:else}
        Windows startup settings are unavailable.
      {/if}
    </small>
  </span>
  <input
    type="checkbox"
    checked={status?.kind === 'ready' && status.registered}
    disabled={status?.kind !== 'ready'}
    aria-disabled={pending}
    aria-describedby="windows-startup-help"
    onchange={change}
  />
</label>
{#if error}
  <div class="settings-action">
    <span role="alert">{error}</span>
    {#if status === null}<button onclick={load}>Retry</button>{/if}
  </div>
{/if}
