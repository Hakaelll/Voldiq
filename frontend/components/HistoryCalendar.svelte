<script lang="ts">
  import { tick, untrack } from 'svelte';
  import ActivityCaptureSummary from './ActivityCaptureSummary.svelte';
  import { calendarQueries, calendarEntryQueries } from '../lib/queries';
  import { shiftMonth, utcDate, validMonth } from '../lib/history-activity';
  import { cloudLabel } from '../lib/cloud';
  import { bytes, type Snapshot, type CalendarReport, type CalendarPage } from '../lib/types';
  let {
    captures,
    initialSnapshot,
    disabled,
    onopen,
  }: {
    captures: Snapshot[];
    initialSnapshot: number;
    disabled: boolean;
    onopen: (snapshot: Snapshot) => Promise<void>;
  } = $props();
  let selected = $state(untrack(() => initialSnapshot));
  const chosen = $derived(captures.find((capture) => capture.id === selected) ?? captures[0]);
  const snapshot = $derived(chosen?.id ?? 0);
  let month = $state(new Date().toISOString().slice(0, 7)),
    day = $state<number | null>(null),
    offset = $state(0),
    retry = $state(0),
    entryRetry = $state(0);
  let region = $state<HTMLElement>();
  type MonthContext = { snapshot: number; month: string };
  type MonthModel =
    | { kind: 'loading' }
    | { kind: 'ready'; context: MonthContext; value: CalendarReport }
    | { kind: 'failed'; context: MonthContext; message: string };
  type DayContext = MonthContext & { day: number; offset: number };
  type DayModel =
    | { kind: 'loading' }
    | { kind: 'ready'; context: DayContext; value: CalendarPage }
    | { kind: 'failed'; context: DayContext; message: string };
  let model = $state.raw<MonthModel>({ kind: 'loading' }),
    entries = $state.raw<DayModel>({ kind: 'loading' });
  const current = $derived(
    model.kind !== 'loading' && model.context.snapshot === snapshot && model.context.month === month
      ? model
      : null,
  );
  const report = $derived(current?.kind === 'ready' && !disabled ? current.value : null);
  const currentDay = $derived(
    entries.kind !== 'loading' &&
      entries.context.snapshot === snapshot &&
      entries.context.month === month &&
      entries.context.day === day &&
      entries.context.offset === offset
      ? entries
      : null,
  );
  const page = $derived(currentDay?.kind === 'ready' && report ? currentDay.value : null);
  const monthLoading = $derived(!disabled && snapshot > 0 && validMonth(month) && current === null);
  const dayLoading = $derived(!disabled && report !== null && day !== null && currentDay === null);
  const padding = $derived(
    report ? (new Date(`${report.month}-01T00:00:00Z`).getUTCDay() + 6) % 7 : 0,
  );
  const maximum = $derived(
    Math.max(0, ...(report?.days.map((value) => Number(value.totals.logical)) ?? [])),
  );
  async function restoreFocus(retain: boolean, current: () => boolean): Promise<void> {
    if (!retain) return;
    await tick();
    if (current() && document.activeElement === document.body)
      (
        region?.querySelector<HTMLElement>(
          '[data-activity-reload]:not(:disabled),select:not(:disabled),button[aria-pressed="true"]',
        ) ?? region
      )?.focus();
  }
  $effect(() => {
    const context = { snapshot, month };
    void retry;
    const retain = region?.contains(document.activeElement) ?? false;
    model = { kind: 'loading' };
    if (disabled || !snapshot || !validMonth(month)) return;
    let alive = true;
    void calendarQueries.request(context).result.then(
      async (value) => {
        if (!alive) return;
        const focus = retain || (region?.contains(document.activeElement) ?? false);
        model =
          value?.capture.id === context.snapshot && value.month === context.month
            ? { kind: 'ready', context, value }
            : { kind: 'failed', context, message: 'Calendar query cancelled. Reload to retry.' };
        await tick();
        if (alive) await restoreFocus(focus, () => alive);
      },
      async (problem: unknown) => {
        if (!alive) return;
        const focus = retain || (region?.contains(document.activeElement) ?? false);
        model = { kind: 'failed', context, message: String(problem) };
        await tick();
        if (alive) await restoreFocus(focus, () => alive);
      },
    );
    return () => {
      alive = false;
      calendarQueries.cancel();
    };
  });
  $effect(() => {
    const loaded = report,
      selectedDay = day,
      position = offset;
    void entryRetry;
    const retain = region?.contains(document.activeElement) ?? false;
    entries = { kind: 'loading' };
    if (!loaded || selectedDay === null || disabled) return;
    const context = {
      snapshot: loaded.capture.id,
      month: loaded.month,
      day: selectedDay,
      offset: position,
    };
    let alive = true;
    void calendarEntryQueries.request({ ...context, limit: 100 }).result.then(
      async (value) => {
        if (!alive) return;
        const focus = retain || (region?.contains(document.activeElement) ?? false);
        const expected = `${context.month}-${String(context.day).padStart(2, '0')}`;
        entries =
          value?.snapshot === context.snapshot &&
          value.date === expected &&
          value.offset === context.offset
            ? { kind: 'ready', context, value }
            : { kind: 'failed', context, message: 'Day query cancelled. Reload to retry.' };
        await tick();
        if (alive) await restoreFocus(focus, () => alive);
      },
      async (problem: unknown) => {
        if (!alive) return;
        const focus = retain || (region?.contains(document.activeElement) ?? false);
        entries = { kind: 'failed', context, message: String(problem) };
        await tick();
        if (alive) await restoreFocus(focus, () => alive);
      },
    );
    return () => {
      alive = false;
      calendarEntryQueries.cancel();
    };
  });
  function chooseMonth(value: string): void {
    month = value;
    day = null;
    offset = 0;
  }
  function chooseDay(value: number): void {
    day = value;
    offset = 0;
  }
  function cancel(): void {
    calendarQueries.cancel();
    calendarEntryQueries.cancel();
  }
  async function open(): Promise<void> {
    if (chosen && !disabled) await onopen(chosen);
  }
</script>

<section
  class="panel history-activity"
  aria-label="Captured modification calendar"
  tabindex="-1"
  bind:this={region}
>
  <div class="activity-toolbar">
    <label
      >Capture<select
        value={snapshot}
        onchange={(event) => {
          selected = Number(event.currentTarget.value);
          day = null;
          offset = 0;
        }}
        {disabled}
        >{#each captures as capture}<option value={capture.id}
            >#{capture.id} · {utcDate(capture.captured)}</option
          >{/each}</select
      ></label
    >
    <label
      >Modification month (UTC)<input
        type="month"
        min="1970-01"
        max="9999-12"
        value={month}
        oninput={(event) => chooseMonth(event.currentTarget.value)}
        {disabled}
      /></label
    >
    <button
      onclick={() => chooseMonth(shiftMonth(month, -1))}
      disabled={disabled || !validMonth(month) || month === '1970-01'}>Previous month</button
    >
    <button
      onclick={() => chooseMonth(shiftMonth(month, 1))}
      disabled={disabled || !validMonth(month) || month === '9999-12'}>Next month</button
    >
    <button data-activity-reload onclick={() => retry++} disabled={disabled || !validMonth(month)}
      >Reload calendar</button
    >
    <button onclick={cancel} disabled={disabled || (!monthLoading && !dayLoading)}
      >Cancel query</button
    >
    <button onclick={open} disabled={disabled || !chosen}>Open full snapshot</button>
  </div>
  <p class="activity-help">
    Capture choices cover this history page. Return to captures to choose another folder or page.
    Dates and cohort boundaries use UTC.
  </p>
  {#if disabled}<p role="status">Calendar paused during the current operation.</p>
  {:else if !chosen}<p>No saved capture selected.</p>
  {:else if !validMonth(month)}<p role="status">Choose a UTC month from 1970 through 9999.</p>
  {:else if current?.kind === 'failed'}<p role="alert">{current.message}</p>
  {:else if !report}<p role="status">Reading captured modification dates…</p>
  {:else}
    <h3 class="activity-root">{report.root}</h3>
    <ActivityCaptureSummary capture={report.capture} />
    <p class="activity-help">
      Recorded file-path sizes grouped by last modification date. These are not bytes written or
      evidence of file use. Creation dates are unavailable. File-placeholder metadata is included
      without establishing local content availability.
    </p>
    <div class="calendar-cohort-summary">
      <span
        >All recorded files <strong
          >{report.total.files.toLocaleString()} · {bytes(Number(report.total.logical))}</strong
        ></span
      >
      <span
        >Date unavailable <strong
          >{report.undated.files.toLocaleString()} · {bytes(Number(report.undated.logical))}</strong
        ></span
      >
      <span
        >Outside this month <strong
          >{report.outside.files.toLocaleString()} · {bytes(Number(report.outside.logical))}</strong
        ></span
      >
    </div>
    <div class="calendar-grid" role="group" aria-label={`${month} UTC modification cohorts`}>
      {#each ['Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat', 'Sun'] as weekday}<span
          class="calendar-weekday">{weekday}</span
        >{/each}
      {#each Array.from({ length: padding }) as _}<span aria-hidden="true"></span>{/each}
      {#each report.days as cell}<button
          class="calendar-day"
          class:selected={day === cell.day}
          aria-pressed={day === cell.day}
          aria-label={`${cell.date} UTC, ${cell.totals.files} recorded files, ${cell.totals.logical} logical bytes`}
          onclick={() => chooseDay(cell.day)}
        >
          <strong>{cell.day}</strong><span>{cell.totals.files.toLocaleString()} files</span><small
            >{bytes(Number(cell.totals.logical))}</small
          ><i
            aria-hidden="true"
            style:width={`${maximum ? (100 * Number(cell.totals.logical)) / maximum : 0}%`}
          ></i>
        </button>{/each}
    </div>
    <p class="activity-help">
      Bars compare recorded logical sizes within this month. Select a day for its stored file
      metadata.
    </p>
    {#if day !== null}<section class="calendar-day-records" aria-label="Selected day records">
        <div class="activity-toolbar">
          <h3>{month}-{String(day).padStart(2, '0')} UTC</h3>
          <button onclick={() => entryRetry++}>Reload day</button><button
            onclick={() => (offset = Math.max(0, offset - 100))}
            disabled={offset === 0 || !page}>Previous records</button
          ><button onclick={() => (offset += 100)} disabled={!page?.has_more || offset >= 10000}
            >Next records</button
          >
        </div>
        {#if currentDay?.kind === 'failed'}<p role="alert">{currentDay.message}</p>
        {:else if !page}<p role="status">Reading day records…</p>
        {:else if !page.total}<p>No recorded files in this UTC day.</p>
        {:else if !page.entries.length}<p>End of recorded files. Return to the previous page.</p>
        {:else}
          <p class="activity-help">
            {offset + 1}–{offset + page.entries.length} of {page.total.toLocaleString()} recorded files.
            At most 100 per page.
          </p>
          <!-- svelte-ignore a11y_no_noninteractive_tabindex (The named scroll region needs keyboard scrolling.) -->
          <div
            class="activity-table"
            role="region"
            tabindex="0"
            aria-label="Captured file metadata"
          >
            <table>
              <thead
                ><tr
                  ><th>Recorded path</th><th>Logical size</th><th>Last modification (UTC)</th><th
                    >Cloud observation</th
                  ></tr
                ></thead
              ><tbody
                >{#each page.entries as entry}<tr
                    ><td>{entry.path}</td><td
                      class="number"
                      title={`${entry.logical} logical bytes`}>{bytes(Number(entry.logical))}</td
                    ><td>{utcDate(entry.modified)}</td><td>{cloudLabel(entry.cloud)}</td></tr
                  >{/each}</tbody
              >
            </table>
          </div>
          {#if page.has_more && offset >= 10000}<p class="activity-help">
              The 10,000-record paging bound is reached. Open the full snapshot to explore all
              records.
            </p>{/if}
        {/if}
      </section>{/if}
  {/if}
</section>
