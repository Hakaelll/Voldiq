<script lang="ts">
  import { invoke, isTauri } from '@tauri-apps/api/core';
  import { listen } from '@tauri-apps/api/event';
  import { onMount } from 'svelte';
  import { date } from '../lib/types';

  type Run = {
    started: number;
    outcome: 'running' | 'saved' | 'failed' | 'cancelled' | 'interrupted';
    snapshot: number | null;
    partial: boolean | null;
    problem: string | null;
  };
  type Schedule = {
    id: number;
    revision: number;
    path: string;
    exclusions: string[];
    hours: number;
    enabled: boolean;
    next_due: number | null;
    runs: Run[];
  };
  type Change =
    | { kind: 'add'; path: string; hours: number; exclusions: string[] }
    | { kind: 'edit'; id: number; revision: number; hours: number; exclusions: string[] }
    | { kind: 'enable'; id: number; revision: number; enabled: boolean }
    | { kind: 'remove'; id: number; revision: number };
  let { available }: { available: boolean } = $props();
  let schedules = $state<Schedule[] | null>(null),
    pending = $state(false),
    error = $state(''),
    status = $state('');
  let editing = $state<Pick<Schedule, 'id' | 'revision'> | null>(null),
    path = $state(''),
    hours = $state(24),
    exclusions = $state('');
  let request = 0;
  let alive = false;
  let refreshAfterChange = false;

  async function load(): Promise<void> {
    if (!alive) return;
    if (pending) {
      refreshAfterChange = true;
      return;
    }
    const owner = ++request;
    try {
      const result = await invoke<Schedule[]>('list_scan_schedules');
      if (owner === request) schedules = result;
    } catch (failure) {
      if (owner === request) error = String(failure);
    }
  }

  onMount(() => {
    if (!available || !isTauri()) return;
    alive = true;
    const events = listen('scheduled-scans-changed', () => void load()).catch((failure) => {
      error = String(failure);
      return null;
    });
    void events.then(() => load());
    return () => {
      alive = false;
      request += 1;
      void events.then((unlisten) => unlisten?.());
    };
  });

  async function choose(): Promise<void> {
    pending = true;
    error = '';
    try {
      const selected = await invoke<string | null>('choose_folder');
      if (selected) path = selected;
    } catch (failure) {
      error = String(failure);
    } finally {
      pending = false;
      if (refreshAfterChange) {
        refreshAfterChange = false;
        void load();
      }
    }
  }

  function reset(): void {
    editing = null;
    path = '';
    hours = 24;
    exclusions = '';
  }

  async function change(command: Change): Promise<void> {
    if (pending) return;
    request += 1;
    pending = true;
    error = '';
    status = '';
    try {
      schedules = await invoke<Schedule[]>('change_scan_schedule', { change: command });
      status = command.kind === 'remove' ? 'Schedule removed.' : 'Schedule saved.';
      if (command.kind === 'add' || command.kind === 'edit' || command.id === editing?.id) reset();
    } catch (failure) {
      error = String(failure);
    } finally {
      pending = false;
      if (refreshAfterChange) {
        refreshAfterChange = false;
        void load();
      }
    }
  }

  function save(): void {
    const ignored = exclusions
      .split(/\r?\n/u)
      .map((line) => line.trim())
      .filter(Boolean);
    void change(
      editing
        ? { kind: 'edit', ...editing, hours, exclusions: ignored }
        : { kind: 'add', path, hours, exclusions: ignored },
    );
  }

  function edit(schedule: Schedule): void {
    editing = { id: schedule.id, revision: schedule.revision };
    path = schedule.path;
    hours = schedule.hours;
    exclusions = schedule.exclusions.join('\n');
  }

  function outcome(run: Run): string {
    if (run.outcome === 'saved') {
      if (run.snapshot === null) return 'Snapshot removed from History';
      return run.partial ? `Partial snapshot #${run.snapshot}` : `Snapshot #${run.snapshot} saved`;
    }
    return {
      running: 'Scanning',
      failed: 'Failed',
      cancelled: 'Cancelled',
      interrupted: 'Interrupted',
    }[run.outcome];
  }
</script>

<section aria-labelledby="scheduled-scans-title">
  <h2 id="scheduled-scans-title">Scheduled scans</h2>
  {#if !available || !isTauri()}
    <p>Scheduled scans require a persistent desktop profile.</p>
  {:else}
    <p>Voldiq runs due scans when idle. Each finished scan appears in History.</p>
    {#if error}<div class="settings-action">
        <p role="alert">{error}</p>
        <button
          disabled={pending}
          onclick={() => {
            error = '';
            void load();
          }}>Reload</button
        >
      </div>{/if}
    {#if status}<p role="status">{status}</p>{/if}
    {#if schedules === null}
      {#if !error}<p role="status">Loading schedules…</p>{/if}
    {:else}
      <form
        onsubmit={(event) => {
          event.preventDefault();
          save();
        }}
        aria-label={editing ? 'Edit schedule' : 'New schedule'}
      >
        <div class="settings-row">
          <span>Folder</span>
          <div>
            {#if !editing}<button type="button" onclick={choose} disabled={pending}
                >Choose a folder</button
              >{/if}
            {#if path}<p class="schedule-path">{path}</p>{/if}
          </div>
        </div>
        <label class="settings-row">
          <span>Frequency</span>
          <select bind:value={hours} disabled={pending}>
            <option value={1}>Every hour</option>
            <option value={24}>Every day</option>
            <option value={168}>Every week</option>
          </select>
        </label>
        <label class="settings-row">
          <span
            >Exclude folders<small>Full paths inside the chosen folder, one per line</small></span
          >
          <textarea bind:value={exclusions} rows="2" maxlength="32768" disabled={pending}
          ></textarea>
        </label>
        <div class="actions">
          <button class="primary" disabled={pending || !path}
            >{editing ? 'Save changes' : 'Add schedule'}</button
          >
          {#if editing}<button class="quiet-action" type="button" onclick={reset} disabled={pending}
              >Cancel edit</button
            >{/if}
        </div>
      </form>
      {#each schedules as schedule (schedule.id)}
        <article>
          <h3 class="schedule-path">{schedule.path}</h3>
          <p>
            {schedule.hours === 1 ? 'Hourly' : schedule.hours === 24 ? 'Daily' : 'Weekly'}
            · {schedule.runs[0]?.outcome === 'running'
              ? 'Scanning'
              : schedule.next_due === null
                ? 'Disabled'
                : `Due ${date(schedule.next_due)}`}
          </p>
          <div class="actions">
            <button disabled={pending} onclick={() => edit(schedule)}>Edit</button>
            <button
              disabled={pending}
              onclick={() =>
                change({
                  kind: 'enable',
                  id: schedule.id,
                  revision: schedule.revision,
                  enabled: !schedule.enabled,
                })}
            >
              {schedule.enabled ? 'Disable' : 'Enable'}
            </button>
            <button
              class="quiet-action"
              disabled={pending}
              onclick={() =>
                change({ kind: 'remove', id: schedule.id, revision: schedule.revision })}
              >Remove schedule</button
            >
          </div>
          {#if schedule.runs.length}
            <details>
              <summary>Run history ({schedule.runs.length})</summary>
              <ul>
                {#each schedule.runs as run}
                  <li>
                    {date(run.started)} · {outcome(run)}{#if run.problem}<small>{run.problem}</small
                      >{/if}
                  </li>
                {/each}
              </ul>
            </details>
          {/if}
        </article>
      {/each}
    {/if}
  {/if}
</section>

<style>
  form,
  article {
    display: grid;
    gap: 12px;
  }
  article {
    padding-top: 12px;
    border-top: 1px solid var(--border);
  }
  article p,
  section > p {
    color: var(--secondary);
  }
  .schedule-path {
    overflow-wrap: anywhere;
  }
  h3 {
    margin: 0;
    font-size: 14px;
    font-weight: 600;
  }
  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
  }
  ul {
    padding-inline-start: 18px;
  }
  li + li {
    margin-top: 6px;
  }
</style>
