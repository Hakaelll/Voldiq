<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { onMount } from 'svelte';
  import { bytes } from '../lib/types';

  type Quota = { limit_bytes: string | null; charged_bytes: string; charged_plans: number };
  let quota = $state<Quota | null>(null);
  let pending = $state(false);
  let input = $state('');
  let problem = $state('');
  let saved = $state(false);
  let open = $state(false);
  const exceeded = $derived(
    quota?.limit_bytes != null && BigInt(quota.charged_bytes) >= BigInt(quota.limit_bytes),
  );

  async function load(): Promise<void> {
    if (pending) return;
    pending = true;
    problem = '';
    try {
      quota = await invoke<Quota>('get_recovery_vault_quota');
      input = quota.limit_bytes ?? '';
    } catch (error) {
      problem = String(error);
    } finally {
      pending = false;
    }
  }

  async function save(event: SubmitEvent): Promise<void> {
    event.preventDefault();
    if (!quota || pending) return;
    const value = input.trim();
    if (value === (quota.limit_bytes ?? '')) return;
    if (value && !/^[1-9]\d{0,18}$/.test(value)) {
      problem = 'Enter a positive whole byte count.';
      return;
    }
    pending = true;
    saved = false;
    problem = '';
    try {
      quota = await invoke<Quota>('set_recovery_vault_quota', { bytes: value || null });
      input = quota.limit_bytes ?? '';
      saved = true;
    } catch (error) {
      problem = String(error);
    } finally {
      pending = false;
    }
  }

  onMount(() => void load());
</script>

<details class="vault-quota" bind:open>
  <summary>
    Recovery vault quota
    {#if quota}
      <span title={`${quota.charged_bytes} bytes charged`}>
        {bytes(Number(quota.charged_bytes))} charged ·
        {quota.limit_bytes === null ? 'No limit set' : bytes(Number(quota.limit_bytes)) + ' limit'}
      </span>
    {/if}
  </summary>
  {#if open}
    {#if quota}
      <p>
        Accounts for {quota.charged_plans} started archive {quota.charged_plans === 1
          ? 'plan'
          : 'plans'}, including interrupted copies. Windows free space is checked separately.
      </p>
      {#if exceeded}<p role="status">Quota reached. New archive plans are blocked.</p>{/if}
      <form onsubmit={save}>
        <label
          >Limit in bytes <input
            type="text"
            inputmode="numeric"
            bind:value={input}
            oninput={() => (saved = false)}
            placeholder="Leave blank for no limit"
            maxlength="19"
          /></label
        >
        <button type="submit" aria-disabled={pending || input.trim() === (quota.limit_bytes ?? '')}
          >Save quota</button
        >
      </form>
      {#if saved}<p role="status">Quota saved.</p>{/if}
    {:else if pending}<p role="status">Reading quota…</p>
    {/if}
    {#if problem}
      <p role="alert">{problem}</p>
      {#if !quota}<button onclick={load}>Retry</button>{/if}
    {/if}
  {/if}
</details>

<style>
  .vault-quota {
    border-block: 1px solid var(--border);
    padding-block: 10px;
  }
  summary {
    cursor: pointer;
  }
  summary span,
  p {
    color: var(--secondary);
    font-size: 12px;
  }
  summary span {
    margin-inline-start: 10px;
  }
  form {
    display: flex;
    align-items: end;
    flex-wrap: wrap;
    gap: 10px;
  }
  label {
    display: grid;
    gap: 4px;
  }
</style>
