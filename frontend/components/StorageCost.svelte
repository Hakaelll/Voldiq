<script lang="ts">
  import { tick } from 'svelte';
  import { bytes, type Details } from '../lib/types';
  import {
    enteredUsage,
    monthlyEstimate,
    parseTariff,
    scannedUsage,
    tariffLabel,
    type StorageUnit,
    type TariffDraft,
    type Usage,
  } from '../lib/storage-cost';

  let { folder }: { folder: Details | null } = $props();
  type Tariff = Extract<ReturnType<typeof parseTariff>, { kind: 'valid' }>['tariff'];
  const limit = 8;
  const initialDraft: TariffDraft = {
    name: '',
    date: '',
    currency: '',
    unit: 'GB',
    model: 'metered',
    price: '',
    capacity: '',
  };
  let tariffs = $state.raw<{ id: number; tariff: Tariff }[]>([]);
  let sequence = 0;
  let draft = $state({ ...initialDraft });
  let editing = $state(false);
  let problem = $state('');
  let unit = $state<StorageUnit>('GB');
  let current = $state('');
  let alternative = $state('');
  let copied = $state.raw<{
    usage: Usage;
    label: string;
    path: string;
    incomplete: boolean;
  } | null>(null);
  let addButton: HTMLButtonElement;
  let nameInput = $state<HTMLInputElement>();

  const available = $derived(folder ? scannedUsage(folder.entry.logical) : null);
  const baseline = $derived(copied?.usage ?? enteredUsage(current, unit));
  const scenario = $derived(enteredUsage(alternative, unit));

  function copyFolder(): void {
    if (!folder || !available) return;
    copied = {
      usage: available,
      label: bytes(folder.entry.logical),
      path: folder.path,
      incomplete: folder.entry.incomplete,
    };
  }

  async function toggleForm(): Promise<void> {
    editing = !editing;
    problem = '';
    if (editing) {
      await tick();
      nameInput?.focus();
    }
  }

  async function addTariff(event: SubmitEvent): Promise<void> {
    event.preventDefault();
    if (tariffs.length >= limit) return;
    const result = parseTariff(draft);
    if (result.kind === 'invalid') {
      problem = result.message;
      return;
    }
    tariffs = [...tariffs, { id: ++sequence, tariff: result.tariff }];
    draft = { ...initialDraft };
    problem = '';
    editing = false;
    await tick();
    addButton.focus();
  }

  async function removeTariff(id: number): Promise<void> {
    tariffs = tariffs.filter((row) => row.id !== id);
    await tick();
    addButton.focus();
  }
</script>

<section class="storage-cost" aria-labelledby="storage-cost-heading">
  <div class="cost-heading">
    <div>
      <h2 id="storage-cost-heading">Monthly storage cost</h2>
      <p>Compare two amounts of storage using your own dated tariffs.</p>
    </div>
    <button
      bind:this={addButton}
      aria-expanded={editing}
      aria-controls={editing ? 'cost-tariff-form' : undefined}
      aria-disabled={tariffs.length >= limit}
      onclick={() => {
        if (tariffs.length < limit) void toggleForm();
      }}>{editing ? 'Close tariff form' : 'Add tariff'}</button
    >
  </div>

  {#if editing}
    <form id="cost-tariff-form" aria-label="Add a tariff" onsubmit={addTariff}>
      <label
        >Tariff name <input
          bind:this={nameInput}
          bind:value={draft.name}
          maxlength="48"
          required
        /></label
      >
      <label>Effective date <input type="date" bind:value={draft.date} required /></label>
      <label
        >Currency code <input
          bind:value={draft.currency}
          maxlength="3"
          placeholder="EUR"
          required
        /></label
      >
      <label
        >Billing model
        <select bind:value={draft.model}>
          <option value="metered">Per unit per month</option>
          <option value="fixed">Fixed monthly fee</option>
        </select>
      </label>
      <label
        >Storage unit
        <select bind:value={draft.unit}><option>GB</option><option>GiB</option></select>
      </label>
      <label
        >{draft.model === 'metered' ? `Monthly rate per ${draft.unit}` : 'Monthly fee'}
        <input inputmode="decimal" bind:value={draft.price} maxlength="19" required />
      </label>
      {#if draft.model === 'fixed'}
        <label
          >Included capacity ({draft.unit})
          <input inputmode="decimal" bind:value={draft.capacity} maxlength="19" required /></label
        >
      {/if}
      <div class="form-footer">
        {#if problem}<p role="alert">{problem}</p>{/if}
        <button class="primary" type="submit">Add to comparison</button>
      </div>
    </form>
  {/if}

  <div class="usage" role="group" aria-label="Storage amounts for comparison">
    <label
      >Input unit <select bind:value={unit}><option>GB</option><option>GiB</option></select></label
    >
    <div class="current-usage">
      {#if copied}
        <div class="copied-usage" title={copied.path}>
          <span>Current storage</span>
          <strong>{copied.label} · {copied.incomplete ? 'Partial scan' : 'Indexed folder'}</strong>
        </div>
      {:else}
        <label
          >Current storage ({unit})
          <input
            inputmode="decimal"
            bind:value={current}
            maxlength="19"
            aria-invalid={current !== '' && !baseline}
          />
        </label>
      {/if}
      {#if copied || available}
        <button
          onclick={() => {
            if (copied) copied = null;
            else copyFolder();
          }}>{copied ? 'Enter manually' : 'Use indexed folder'}</button
        >
      {/if}
    </div>
    <label
      >Scenario storage ({unit})
      <input
        inputmode="decimal"
        bind:value={alternative}
        maxlength="19"
        aria-invalid={alternative !== '' && !scenario}
      />
    </label>
  </div>
  {#if copied}<p class="cost-note path-note">
      Captured from {copied.path}. The indexed logical size is a billing assumption.
    </p>{/if}
  {#if (!baseline && current !== '') || (!scenario && alternative !== '')}
    <p class="cost-note" role="status">
      Enter a nonnegative quantity with up to six decimal places and no grouping separators.
    </p>
  {/if}

  {#if tariffs.length}
    <!-- svelte-ignore a11y_no_noninteractive_tabindex (The named overflow region needs keyboard scrolling.) -->
    <div class="cost-table" role="region" aria-label="Dated tariff comparison" tabindex="0">
      <table>
        <thead
          ><tr>
            <th scope="col">Tariff</th><th scope="col">Effective date</th><th scope="col">Rate</th>
            <th scope="col">Current per month</th><th scope="col">Scenario per month</th><th
              scope="col">Actions</th
            >
          </tr></thead
        >
        <tbody>
          {#each tariffs as { id, tariff } (id)}
            <tr>
              <th scope="row">{tariff.name}</th>
              <td><time datetime={tariff.date}>{tariff.date}</time></td>
              <td>{tariffLabel(tariff)}</td>
              <td class="cost-value">{monthlyEstimate(tariff, baseline)}</td>
              <td class="cost-value">{monthlyEstimate(tariff, scenario)}</td>
              <td
                ><button aria-label={`Remove ${tariff.name}`} onclick={() => removeTariff(id)}
                  >Remove</button
                ></td
              >
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
  {:else}
    <p class="cost-empty">Add a dated tariff to see the comparison.</p>
  {/if}
  {#if tariffs.length >= limit}<p class="cost-note">
      Remove a tariff to make room for another.
    </p>{/if}
  <p class="cost-note">
    Estimates assume constant usage and cover storage charges only. Taxes, transfers, discounts and
    extra charges are excluded. A fixed plan keeps the same fee within its included capacity.
  </p>
  <p class="cost-note">Tariffs and amounts stay in this overview session.</p>
</section>

<style>
  .storage-cost {
    min-width: 0;
  }
  .cost-heading,
  .current-usage {
    display: flex;
    align-items: start;
    gap: 8px 16px;
  }
  .cost-heading {
    justify-content: space-between;
    flex-wrap: wrap;
  }
  h2 {
    margin: 0;
    font-size: 18px;
  }
  p {
    margin: 4px 0 0;
    color: var(--secondary);
  }
  .cost-heading button[aria-disabled='true'] {
    color: var(--muted);
    cursor: default;
  }
  form,
  .usage {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(min(190px, 100%), 1fr));
    gap: 12px 20px;
    padding: 14px 0;
    border-bottom: 1px solid var(--border);
  }
  .usage {
    grid-template-columns: minmax(84px, 0.45fr) minmax(0, 2fr) minmax(0, 1fr);
  }
  .current-usage {
    flex-wrap: wrap;
    align-items: end;
  }
  .current-usage > label,
  .copied-usage {
    flex: 1 1 140px;
  }
  label,
  .copied-usage {
    display: grid;
    gap: 5px;
    min-width: 0;
    font-size: 13px;
  }
  input,
  select {
    width: 100%;
    min-width: 0;
    min-height: 34px;
  }
  input[aria-invalid='true'] {
    border-color: var(--error);
  }
  .form-footer {
    display: flex;
    align-items: center;
    justify-content: end;
    flex-wrap: wrap;
    gap: 10px;
    grid-column: 1 / -1;
  }
  .form-footer p {
    margin: 0;
    color: var(--error);
  }
  .cost-table {
    margin-top: 12px;
    overflow-x: auto;
  }
  table {
    width: 100%;
    border-collapse: collapse;
    font-variant-numeric: tabular-nums;
  }
  th,
  td {
    padding: 8px 12px;
    text-align: left;
    border-bottom: 1px solid var(--border);
  }
  th:first-child {
    padding-left: 0;
  }
  td:nth-child(2),
  .cost-value {
    white-space: nowrap;
  }
  .cost-value {
    font-weight: 600;
  }
  .cost-note,
  .cost-empty {
    margin-top: 12px;
  }
  .cost-note {
    font-size: 13px;
  }
  .path-note {
    overflow-wrap: anywhere;
  }
  @media (max-width: 620px) {
    .usage {
      grid-template-columns: 1fr;
    }
  }
</style>
