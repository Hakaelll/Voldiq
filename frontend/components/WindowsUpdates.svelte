<script lang="ts">
  import { invoke, isTauri } from '@tauri-apps/api/core';
  import { onMount } from 'svelte';

  type Availability =
    { kind: 'ready'; version: string } | { kind: 'not_installed' } | { kind: 'unconfigured' };

  let availability = $state<Availability | null>(null);
  let pending = $state<'loading' | 'checking' | 'installing' | null>(null);
  let offer = $state<string | null>(null);
  let checked = $state(false);
  let cancelled = $state(false);
  let error = $state('');

  async function load(): Promise<void> {
    pending = 'loading';
    error = '';
    try {
      availability = await invoke<Availability>('update_availability');
    } catch (failure) {
      error = String(failure);
    } finally {
      pending = null;
    }
  }

  onMount(() => {
    if (isTauri()) void load();
  });

  async function check(): Promise<void> {
    if (pending || availability?.kind !== 'ready') return;
    pending = 'checking';
    offer = null;
    checked = false;
    cancelled = false;
    error = '';
    try {
      offer = await invoke<string | null>('check_windows_update');
      checked = true;
    } catch (failure) {
      error = String(failure);
    } finally {
      pending = null;
    }
  }

  async function install(): Promise<void> {
    if (pending || !offer) return;
    const version = offer;
    pending = 'installing';
    offer = null;
    checked = false;
    error = '';
    try {
      await invoke('install_windows_update', { version });
      cancelled = true;
    } catch (failure) {
      error = String(failure);
    } finally {
      pending = null;
    }
  }
</script>

<section aria-labelledby="updates-heading">
  <h2 id="updates-heading">Updates</h2>
  <div class="settings-row">
    <span>
      {#if availability?.kind === 'ready'}
        Installed version {availability.version}
        <small>Check GitHub Releases for a signed Windows installer.</small>
      {:else if availability?.kind === 'unconfigured'}
        Updates unavailable
        <small>This build has no trusted release key.</small>
      {:else if pending === 'loading'}
        Loading update settings…
      {:else}
        Updates are available from installed release builds.
      {/if}
    </span>
    {#if availability?.kind === 'ready'}
      <div>
        <button type="button" onclick={check} aria-disabled={pending !== null}>
          Check for updates
        </button>
      </div>
    {:else if error}
      <div><button type="button" onclick={load}>Retry</button></div>
    {/if}
  </div>
  {#if availability?.kind === 'ready' && (pending || offer || checked || cancelled)}
    <div class="settings-action">
      <span role="status">
        {#if pending === 'checking'}
          Checking GitHub Releases…
        {:else if pending === 'installing'}
          Preparing the installer for review…
        {:else if offer}
          Version {offer} is available. Installation requires confirmation after verification.
        {:else if checked}
          Voldiq is up to date.
        {:else if cancelled}
          Installation cancelled.
        {/if}
      </span>
      {#if offer}
        <div>
          <button type="button" class="primary" onclick={install}>Download and review</button>
        </div>
      {/if}
    </div>
  {/if}
  {#if error}<div class="settings-action" role="alert">{error}</div>{/if}
</section>
