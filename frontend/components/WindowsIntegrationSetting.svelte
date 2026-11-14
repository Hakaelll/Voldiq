<script lang="ts">
  import { invoke, isTauri } from '@tauri-apps/api/core';
  import { onMount } from 'svelte';

  const integrations = {
    startup: {
      command: 'windows_startup',
      label: 'Start with Windows',
      help: 'Register Voldiq to open when you sign in. Windows Startup apps can disable this entry.',
      pathHelp: "Startup needs a local fixed drive and a command within Windows' length limit.",
    },
    explorer: {
      command: 'windows_explorer',
      label: 'Explorer context menu',
      help: 'Adds “Analizar con Voldiq” to the classic folder menu (Show more options on Windows 11).',
      pathHelp: 'Requires a supported install path on a local fixed drive.',
    },
    notifications: {
      command: 'scan_notifications',
      label: 'Scan and growth notifications',
      help: 'Show Windows notifications for completed manual scans and scheduled growth alerts. Windows can silence notifications.',
      pathHelp: 'Requires a supported install path on a local fixed drive.',
    },
  };
  let { integration }: { integration: keyof typeof integrations } = $props();
  const setting = $derived(integrations[integration]);
  const helpId = $derived(`windows-${integration}-help`);

  type IntegrationStatus =
    | { kind: 'ready'; registered: boolean }
    | { kind: 'not_installed' }
    | { kind: 'unsupported_path' }
    | { kind: 'conflict' };

  let status = $state<IntegrationStatus | null>(null),
    pending = $state(false),
    error = $state('');

  onMount(() => {
    if (isTauri()) void load();
  });

  async function load(): Promise<void> {
    pending = true;
    error = '';
    try {
      status = await invoke<IntegrationStatus>(setting.command, { registered: null });
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
      status = await invoke<IntegrationStatus>(setting.command, { registered });
    } catch (failure) {
      error = String(failure);
    } finally {
      pending = false;
    }
  }
</script>

<label>
  <span>
    {setting.label}
    <small id={helpId}>
      {#if status?.kind === 'ready'}
        {setting.help}
      {:else if status?.kind === 'not_installed'}
        Available from the installed copy of Voldiq.
      {:else if status?.kind === 'unsupported_path'}
        {setting.pathHelp}
      {:else if status?.kind === 'conflict'}
        A conflicting Windows entry needs to be resolved first.
      {:else if pending}
        Reading Windows settings…
      {:else}
        Windows settings are unavailable.
      {/if}
    </small>
  </span>
  <input
    type="checkbox"
    checked={status?.kind === 'ready' && status.registered}
    disabled={status?.kind !== 'ready'}
    aria-disabled={pending}
    aria-describedby={helpId}
    onchange={change}
  />
</label>
{#if error}
  <div class="settings-action">
    <span role="alert">{error}</span>
    {#if status === null}<button onclick={load}>Retry</button>{/if}
  </div>
{/if}
