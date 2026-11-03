<script lang="ts">
  import { tick } from 'svelte';
  import { recoveryQueries } from '../lib/queries';
  import type { RecoveryReport, RecoveryDiscovery } from '../lib/recovery';
  import type { IndexId } from '../lib/index-context';
  import { utcDate } from '../lib/history-activity';
  let { indexId, paused }: { indexId: IndexId; paused: boolean } = $props();
  type Model =
    | { kind: 'closed' }
    | { kind: 'paused' }
    | { kind: 'loading' }
    | { kind: 'cancelled' }
    | { kind: 'failed'; message: string }
    | { kind: 'ready'; indexId: IndexId; report: RecoveryReport };
  let model = $state.raw<Model>({ kind: 'closed' });
  let expanded = $state(false),
    retry = $state(0);
  let controls: HTMLDivElement;
  let reload = $state<HTMLButtonElement>();
  let cancelRead = () => {};
  const report = $derived(
    model.kind === 'ready' && model.indexId === indexId ? model.report : null,
  );
  const unavailable: Record<Extract<RecoveryDiscovery, { kind: 'unavailable' }>['reason'], string> =
    {
      unsupported: 'Shadow-copy discovery requires Windows.',
      root_unavailable: 'The captured folder cannot be matched. Scan again.',
      access_denied:
        'Windows denied access for the current account. Use Windows recovery tools with the required permissions.',
      provider_unavailable: 'The Windows shadow-copy provider is unavailable.',
      native_failure: 'Windows could not complete this query. Reload to retry.',
      invalid_metadata: 'Windows returned copy metadata that Voldiq could not validate.',
    };
  const flag = (value: boolean | null) => (value === null ? 'Unknown' : value ? 'Yes' : 'No');
  $effect(() => {
    void retry;
    if (!expanded) {
      model = { kind: 'closed' };
      return;
    }
    if (paused) {
      model = { kind: 'paused' };
      return;
    }
    const heldFocus = controls?.contains(document.activeElement);
    const context = indexId;
    model = { kind: 'loading' };
    const request = recoveryQueries.request({ indexId: context });
    cancelRead = request.cancel;
    let alive = true;
    async function restore(): Promise<void> {
      await tick();
      if (alive && heldFocus && document.activeElement === document.body)
        reload?.focus({ preventScroll: true });
    }
    void request.result.then(
      (value) => {
        if (!alive) return;
        model = value ? { kind: 'ready', indexId: context, report: value } : { kind: 'cancelled' };
        void restore();
      },
      (problem: unknown) => {
        if (alive) {
          model = { kind: 'failed', message: String(problem) };
          void restore();
        }
      },
    );
    return () => {
      alive = false;
      request.cancel();
    };
  });
</script>

<section class="type-panel recovery" aria-labelledby="recovery-heading">
  <div class="panel-heading">
    <h2 id="recovery-heading">Recovery copies</h2>
    <div class="recovery-controls" bind:this={controls}>
      <button
        aria-expanded={expanded}
        aria-controls="recovery-body"
        onclick={() => (expanded = !expanded)}
      >
        {expanded ? 'Close' : 'Query copies'}
      </button>
      {#if expanded}<button
          bind:this={reload}
          disabled={model.kind === 'loading' || paused}
          onclick={() => (retry += 1)}>Reload</button
        >{/if}
      {#if model.kind === 'loading'}<button onclick={() => cancelRead()}>Cancel</button>{/if}
    </div>
  </div>
  {#if expanded}<div id="recovery-body" class="recovery-body">
      {#if model.kind === 'loading'}<p role="status">Querying Windows shadow-copy metadata…</p>
      {:else if model.kind === 'paused'}<p>Available after the current operation.</p>
      {:else if model.kind === 'cancelled'}<p role="status">Query cancelled. Reload to retry.</p>
      {:else if model.kind === 'failed'}<p class="recovery-error" role="alert">{model.message}</p>
      {:else if report}
        <p class="recovery-root">Analyzed root · {report.root}</p>
        <p class="recovery-scope">
          Completed VSS records visible to this Windows account for the containing volume. Other
          backup mechanisms and file contents are not checked.
        </p>
        <p>Checked · {utcDate(report.observed_at)}</p>
        {#if report.shadows.kind === 'unavailable'}
          <p class="recovery-unavailable" role="status">{unavailable[report.shadows.reason]}</p>
        {:else}
          {#if report.shadows.coverage === 'item_limit'}
            <p role="status">
              Showing the first {report.copy_limit.toLocaleString()} records returned by Windows, sorted
              by known recorded date. Newer copies may be outside this result.
            </p>
          {:else if report.shadows.coverage === 'time_limit'}
            <p role="status">
              Enumeration stopped at its time limit. These records are an incomplete result; more
              copies may exist.
            </p>
          {/if}
          {#if report.shadows.copies.length > 0}
            <table>
              <caption
                >{report.shadows.copies.length.toLocaleString()} reported {report.shadows.copies
                  .length === 1
                  ? 'copy'
                  : 'copies'}</caption
              >
              <thead
                ><tr
                  ><th scope="col">Recorded date · UTC</th><th scope="col">Copy ID and provider</th
                  ><th scope="col">Windows metadata</th></tr
                ></thead
              >
              <tbody>
                {#each report.shadows.copies as copy (copy.id)}
                  <tr>
                    <td>{utcDate(copy.installed_at)}</td>
                    <td
                      ><span class="copy-id">{copy.id}</span><span class="copy-provider"
                        >Provider · {copy.provider_id ?? 'Unknown'}</span
                      ></td
                    >
                    <td
                      ><span>Previous Versions · {flag(copy.previous_versions)}</span><span
                        >Persistent · {flag(copy.persistent)}</span
                      ></td
                    >
                  </tr>
                {/each}
              </tbody>
            </table>
          {:else if report.shadows.coverage === 'complete'}
            <p role="status">No completed VSS records were returned for this volume.</p>
          {:else}<p>No copy records were returned before this incomplete query stopped.</p>{/if}
        {/if}
        <p class="recovery-guidance">
          Copies can disappear. Their presence does not establish that a particular file can be
          recovered. Use Windows Previous Versions or your backup application's recovery controls to
          check the files and choose a separate destination.
        </p>
      {/if}
    </div>{/if}
</section>

<style>
  .recovery-controls {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
  }
  .recovery-controls button {
    min-height: 28px;
    font-size: 12px;
  }
  .recovery-body {
    font-size: 12px;
    color: var(--secondary);
  }
  .recovery-body p {
    line-height: 1.5;
  }
  .recovery-root,
  td {
    overflow-wrap: anywhere;
  }
  .recovery-unavailable {
    border-left: 2px solid var(--strong);
    padding-left: 12px;
    color: var(--text);
  }
  .recovery-error {
    color: var(--error);
  }
  table {
    width: 100%;
    border-collapse: collapse;
    table-layout: fixed;
  }
  caption {
    text-align: left;
    color: var(--text);
    padding: 12px 0 8px;
  }
  th,
  td {
    text-align: left;
    vertical-align: top;
    padding: 10px 8px;
    border-bottom: 1px solid var(--border);
  }
  th {
    font-weight: 600;
    color: var(--text);
  }
  th:first-child,
  td:first-child {
    width: 25%;
    padding-left: 0;
  }
  td span {
    display: block;
  }
  .copy-id {
    color: var(--text);
  }
  .copy-provider {
    margin-top: 6px;
  }
  .recovery-guidance {
    padding-top: 12px;
    border-top: 1px solid var(--border);
  }
</style>
