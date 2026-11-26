<script lang="ts">
  import { onMount, onDestroy, tick } from 'svelte';
  import { invoke, isTauri } from '@tauri-apps/api/core';
  import { date } from '../lib/types';
  import {
    createTransferCatalogue,
    type CatalogueState,
    type TransferPage,
    type TransferRecord,
    type TransferFilter,
  } from '../lib/transfer-catalogue';
  import Icon from './Icon.svelte';
  import VaultQuota from './VaultQuota.svelte';
  let { onreview }: { onreview: (id: string) => void } = $props();
  const native = isTauri();
  let catalogueState = $state.raw<CatalogueState>({
    page: null,
    loading: false,
    error: '',
    before: null,
    selection: { kind: 'none' },
  });
  let cursors = $state<(string | null)[]>([null]);
  let fileCursors = $state<(number | null)[]>([null]);
  let destinationsOpen = $state(false);
  let search = $state('');
  let copied = $state(false);
  let dueOnly = $state(false);
  let restoreNodes = $state.raw<ReadonlySet<number>>(new Set());
  let keepBoth = $state(false);
  let preparingRestore = $state(false);
  let restoreError = $state('');
  let restoreRequest = 0;
  let filter = $state<TransferFilter>({ text: '', published_only: false });
  const filtered = $derived(
    Boolean(filter.text || filter.published_only || filter.review_due_only),
  );
  let resultsHeading: HTMLHeadingElement;
  const catalogue = createTransferCatalogue({
    list: (before, filter) =>
      invoke<TransferPage>('list_transfer_records', { before, limit: 20, filter }),
    read: (id, after, filter) =>
      invoke<TransferRecord>('get_transfer_record', { id, after, limit: 50, filter }),
    onstate: (next) => {
      catalogueState = next;
    },
  });
  const selectedId = $derived(
    catalogueState.selection.kind === 'ready'
      ? catalogueState.selection.record.manifest.id
      : catalogueState.selection.kind === 'loading' || catalogueState.selection.kind === 'failed'
        ? catalogueState.selection.id
        : null,
  );
  const exactBytes = (value: string | null): string =>
    value === null ? 'Not recorded' : `${BigInt(value).toLocaleString()} bytes`;
  const label = (value: string): string => value.replaceAll('_', ' ');
  function resetRestore(): void {
    ++restoreRequest;
    restoreNodes = new Set();
    keepBoth = false;
    preparingRestore = false;
    restoreError = '';
  }
  function toggleRestore(node: number, checked: boolean): void {
    const next = new Set(restoreNodes);
    if (checked) next.add(node);
    else next.delete(node);
    restoreNodes = next;
  }
  async function prepareRestore(record: TransferRecord): Promise<void> {
    if (preparingRestore || !restoreNodes.size || record.manifest.id !== selectedId) return;
    const owner = ++restoreRequest;
    preparingRestore = true;
    restoreError = '';
    try {
      const id = await invoke<string>('prepare_desktop_restore', {
        hash: record.manifest_hash,
        request: {
          transfer_plan_id: record.manifest.id,
          node_ids: [...restoreNodes],
          on_collision: keepBoth ? 'keep_both' : 'fail',
        },
      });
      if (owner === restoreRequest) onreview(id);
    } catch (problem) {
      if (owner === restoreRequest) restoreError = String(problem);
    } finally {
      if (owner === restoreRequest) preparingRestore = false;
    }
  }
  function open(id: string): void {
    resetRestore();
    destinationsOpen = false;
    fileCursors = [null];
    void catalogue.select(id);
  }
  function reload(): void {
    resetRestore();
    cursors = [null];
    fileCursors = [null];
    void catalogue.load(null, filter);
  }
  function searchRecords(event: SubmitEvent): void {
    event.preventDefault();
    filter = { text: search.trim(), published_only: copied, review_due_only: dueOnly };
    reload();
  }
  async function pageTo(before: string | null): Promise<void> {
    resetRestore();
    await catalogue.load(before);
    await tick();
    if (document.activeElement === document.body && resultsHeading?.isConnected) {
      resultsHeading.focus();
    }
  }
  function older(): void {
    if (!catalogueState.page?.next_before || catalogueState.loading) return;
    cursors = [...cursors, catalogueState.page.next_before];
    void pageTo(catalogueState.page.next_before);
  }
  function newer(): void {
    if (cursors.length < 2 || catalogueState.loading) return;
    cursors = cursors.slice(0, -1);
    void pageTo(cursors.at(-1) ?? null);
  }
  function files(after: number | null, back = false): void {
    if (!selectedId || catalogueState.selection.kind !== 'ready') return;
    fileCursors = back ? fileCursors.slice(0, -1) : [...fileCursors, after];
    void catalogue.select(selectedId, back ? (fileCursors.at(-1) ?? null) : after);
  }
  onMount(() => {
    if (native) reload();
  });
  onDestroy(() => { ++restoreRequest; catalogue.dispose(); });
</script>

<section class="transfer-catalogue" aria-labelledby="transfer-catalogue-heading">
  <div class="heading">
    <div>
      <h2 id="transfer-catalogue-heading" tabindex="-1" bind:this={resultsHeading}>
        Transfers & restoration records
      </h2>
      <p>Past observations. Device connection and current content have not been checked.</p>
    </div>
    <button disabled={!native || catalogueState.loading} onclick={reload}
      ><Icon name="scan" size={16} />Refresh records</button
    >
  </div>
  {#if native}
    <VaultQuota onretentionchange={reload} />
    <form class="transfer-search" onsubmit={searchRecords}>
      <label>
        Find a file or path
        <input
          type="search"
          bind:value={search}
          maxlength="200"
          placeholder="Name or recorded path"
        />
      </label>
      <label class="transfer-copy-filter"
        ><input type="checkbox" bind:checked={copied} /> Published copies only</label
      >
      <label class="transfer-copy-filter"
        ><input type="checkbox" bind:checked={dueOnly} /> Retention review due</label
      >
      <button type="submit">Search records</button>
    </form>
  {/if}
  {#if !native}<p>Transfer records are available in the Windows application.</p>
  {:else if catalogueState.loading}<p role="status">Loading recorded operations…</p>
  {:else if catalogueState.error}<div class="error" role="alert">
      <span>{catalogueState.error}</span><button
        onclick={() => void catalogue.load(catalogueState.before)}>Retry</button
      >
    </div>
  {:else if catalogueState.page && catalogueState.page.items.length === 0}<div class="empty">
      <Icon name="disk" size={28} />
      {#if filtered}
        <h3>
          {catalogueState.page.next_before || cursors.length > 1
            ? 'No matches in this batch'
            : 'No matching records'}
        </h3>
        {#if catalogueState.page.next_before}<button onclick={older}>Search older records</button
          >{/if}
        {#if cursors.length > 1}<button onclick={newer}>Previous batch</button>{/if}
      {:else}
        <h3>No transfer records yet</h3>
        <p>Prepared transfers and restoration plans will appear here.</p>
      {/if}
    </div>
  {:else if catalogueState.page}
    <div class="catalogue-grid">
      <div class="records">
        <ul aria-label="Recorded transfer operations">
          {#each catalogueState.page.items as item (item.id)}<li>
              {#if item.availability === 'ready'}<button
                  class="record"
                  class:active={selectedId === item.id}
                  aria-pressed={selectedId === item.id}
                  onclick={() => open(item.id)}
                >
                  <span class="record-title"
                    ><Icon name={item.action === 'restore' ? 'history' : 'disk'} /><strong
                      >{item.action === 'restore' ? 'Restoration' : 'Transfer'}</strong
                    ><span class="state" data-state={item.state}>{label(item.state)}</span></span
                  >
                  <span class="path" title={item.source_preview}>{item.source_preview}</span>
                  <span class="destination" title={item.destination_preview}
                    >To {item.destination_preview}{#if item.destinations > 1}
                      · {item.destinations} folders{/if}</span
                  >
                  <span class="record-count"
                    >{item.published_files} of {item.files} publications recorded · {exactBytes(
                      item.published_bytes,
                    )}</span
                  >
                  {#if item.review_due_files > 0}<small
                      >{item.review_due_files} due for retention review</small
                    >{/if}
                  <small>{date(item.created)} · connection #{item.connection_id}</small>
                </button>
              {:else}<div class="record unavailable">
                  <strong>Unavailable record</strong>
                  <p>{item.reason}</p>
                </div>{/if}
            </li>{/each}
        </ul>
        <div class="paging">
          <button disabled={cursors.length < 2} onclick={newer}>Newer</button><span
            >{filtered ? 'Batch' : 'Page'} {cursors.length}</span
          ><button disabled={!catalogueState.page.next_before} onclick={older}>Older</button>
        </div>
      </div>
      <div
        class="record-detail"
        aria-live="polite"
        aria-busy={catalogueState.selection.kind === 'loading'}
      >
        {#if catalogueState.selection.kind === 'none'}<div class="empty">
            <Icon name="list" size={26} />
            <h3>Select a recorded operation</h3>
            <p>
              Review its destination, file phases and recorded verification without reopening the
              archive.
            </p>
          </div>
        {:else if catalogueState.selection.kind === 'loading'}<p role="status">
            Loading recorded files…
          </p>
        {:else if catalogueState.selection.kind === 'failed'}<p class="error" role="alert">
            {catalogueState.selection.error}
          </p>
          <button
            onclick={() => {
              if (catalogueState.selection.kind === 'failed')
                void catalogue.select(catalogueState.selection.id, catalogueState.selection.after);
            }}>Retry record</button
          >
        {:else}
          {@const record = catalogueState.selection.record}
          {@const canRestore = record.restore_available || restoreNodes.size > 0}
          {@const action = record.manifest.action}
          {@const restore =
            typeof action === 'object' && 'restore' in action ? action.restore : null}
          {@const targets =
            typeof action === 'object'
              ? 'offload' in action
                ? [action.offload]
                : action.restore.groups
              : []}
          <div class="heading detail-heading">
            <div>
              <h3>{restore ? 'Recorded restoration' : 'Recorded transfer'}</h3>
              <span class="state" data-state={record.state}>{label(record.state)}</span>
            </div>
            <button onclick={() => onreview(record.manifest.id)}>Open exact plan</button>
          </div>
          <dl class="summary">
            <div>
              <dt>Source folder</dt>
              <dd>{record.manifest.root}</dd>
            </div>
            <div>
              <dt>Selected logical bytes</dt>
              <dd>{exactBytes(record.manifest.logical_bytes)}</dd>
            </div>
            <div>
              <dt>Destination last observed</dt>
              <dd>{date(record.last_observed_at)}</dd>
            </div>
            <div>
              <dt>Current connection</dt>
              <dd>
                {record.currently_connected === null
                  ? 'Not checked'
                  : record.currently_connected
                    ? 'Observed connected'
                    : 'Observed disconnected'}
              </dd>
            </div>
          </dl>
          <details class="destinations" bind:open={destinationsOpen}>
            <summary>{targets.length} recorded destination{targets.length === 1 ? '' : 's'}</summary
            >
            {#if destinationsOpen}{#each targets as target}<dl class="summary">
                  <div>
                    <dt>Folder</dt>
                    <dd>{target.destination.path}</dd>
                  </div>
                  <div>
                    <dt>Volume identity</dt>
                    <dd>
                      <code>{target.destination.volume.guid_root}</code> · {target.destination
                        .volume.serial}
                    </dd>
                  </div>
                  <div>
                    <dt>Filesystem</dt>
                    <dd>{target.destination.filesystem}</dd>
                  </div>
                  <div>
                    <dt>Available when observed</dt>
                    <dd>
                      {exactBytes(target.destination.caller_available_bytes)} · {date(
                        target.destination.observed_at,
                      )}
                    </dd>
                  </div>
                  <div>
                    <dt>Supported metadata</dt>
                    <dd>
                      {target.metadata
                        ? 'NTFS basic timestamps and supported file attributes'
                        : 'No metadata profile recorded'}
                    </dd>
                  </div>
                </dl>{/each}{/if}
          </details>
          {#if filtered}
            <p class="matching-count">
              {record.files.length} matching {record.files.length === 1 ? 'file' : 'files'} on this page
            </p>
          {/if}
          {#if canRestore}
            <div class="restore-actions" aria-label="Restore archived copies" role="group">
              <span>{restoreNodes.size ? `${restoreNodes.size} selected` : 'Select verified copies'}</span>
              <label><input type="checkbox" bind:checked={keepBoth} disabled={preparingRestore} /> Keep both if a name exists</label>
              <button class="primary" disabled={!restoreNodes.size || preparingRestore}
                onclick={() => void prepareRestore(record)}>{preparingRestore ? 'Checking copies…' : 'Review restoration'}</button>
            </div>
            {#if restoreError}<p role="alert" class="error">{restoreError}</p>{/if}
          {/if}
          <!-- svelte-ignore a11y_no_noninteractive_tabindex (The named overflow region needs keyboard scrolling.) -->
          <div class="file-table" tabindex="0" role="region" aria-label="Recorded file table">
            <table>
              <caption>Recorded file phases · {record.files.length} on this page</caption><thead
                ><tr
                  ><th scope="col">File & evidence</th><th scope="col">Recorded phase</th><th
                    scope="col">Verified bytes</th
                  ><th scope="col">Publication recorded</th></tr
                ></thead
              ><tbody>
                {#if record.files.length === 0}
                  <tr><td colspan="4">{filtered ? 'No files in this operation match the filters.' : 'No file records on this page.'}</td></tr>
                {/if}
                {#each record.files as file (file.node)}
                  {@const restored = restore?.files.find((entry) => entry.source_id === file.node)}
                  {@const reviewAt =
                    !restore && record.retention_days !== null && file.published_at !== null
                      ? file.published_at + record.retention_days * 86400
                      : null}
                  <tr
                    ><td
                      >{#if record.restore_available && file.published_at !== null}
                        <label class="restore-file"><input type="checkbox" checked={restoreNodes.has(file.node)}
                          disabled={preparingRestore} onchange={(event) => toggleRestore(file.node, event.currentTarget.checked)} />
                          <strong>{file.output_name}</strong></label>
                      {:else}<strong>{file.output_name}</strong>{/if}{#if restored}<small class="original-path"
                          >Original path: {restored.original_path}</small
                        >{/if}
                      <details>
                        <summary>Verification evidence</summary>
                        <dl>
                          <dt>Recorded node</dt>
                          <dd>#{file.node} · historical reference</dd>
                          <dt>BLAKE3</dt>
                          <dd><code>{file.content_blake3 ?? 'Not recorded'}</code></dd>
                          <dt>Destination identity</dt>
                          <dd><code>{file.destination_identity ?? 'Not recorded'}</code></dd>
                          <dt>Content verified at</dt>
                          <dd>{date(file.verified_at)}</dd>
                        </dl>
                      </details></td
                    ><td
                      ><span class="state">{label(file.phase)}</span
                      >{#if file.interrupted_at !== null}<small
                          >Interrupted · outcome needs review</small
                        >{/if}</td
                    ><td class="number">{exactBytes(file.verified_bytes)}</td><td
                      >{date(file.published_at)}
                      {#if reviewAt !== null}<small>
                          {reviewAt <= record.retention_checked_at ? 'Review due' : 'Review after'} ·
                          {date(reviewAt)}
                        </small>{/if}</td
                    ></tr
                  >
                {/each}
              </tbody>
            </table>
          </div>
          <div class="paging">
            <button disabled={fileCursors.length < 2} onclick={() => files(null, true)}
              >Previous files</button
            ><span>Page {fileCursors.length}</span><button
              disabled={record.next_after === null}
              onclick={() => files(record.next_after)}>Next files</button
            >
          </div>
          <details class="limitations">
            <summary>Restoration requirements & recorded limits</summary>
            <p>
              Restoration requires a freshly scanned archive and a separately reviewed exact plan.
              These records do not prove that a file is currently available or that all metadata can
              be restored. Review the exact plan for its current approval state.
            </p>
            <p>{record.limitations}</p>
            <dl>
              <dt>Manifest hash</dt>
              <dd><code>{record.manifest_hash}</code></dd>
            </dl>
          </details>
        {/if}
      </div>
    </div>
  {/if}
</section>

<style>
  .restore-actions { display: flex; align-items: center; flex-wrap: wrap; gap: 8px 14px; margin-block: 10px; }
  .restore-actions label, .restore-file { display: inline-flex; align-items: center; gap: 6px; }
  .restore-actions button { margin-inline-start: auto; }
  .transfer-catalogue {
    display: grid;
    gap: 20px;
    min-width: 0;
  }
  .heading {
    display: flex;
    align-items: start;
    justify-content: space-between;
    gap: 16px;
    flex-wrap: wrap;
  }
  .heading p {
    font-size: 12px;
    margin-top: 6px;
  }
  .transfer-search {
    display: flex;
    align-items: end;
    gap: 12px;
    flex-wrap: wrap;
  }
  .transfer-search > label:first-child {
    display: grid;
    gap: 4px;
    flex: 1 1 220px;
    color: var(--secondary);
  }
  .transfer-copy-filter {
    min-height: 32px;
    display: inline-flex;
    align-items: center;
    gap: 6px;
  }
  .catalogue-grid {
    display: grid;
    grid-template-columns: minmax(240px, 310px) minmax(0, 1fr);
    gap: 20px;
    align-items: start;
  }
  .records,
  .record-detail {
    min-width: 0;
  }
  ul {
    list-style: none;
    margin: 0;
    padding: 0;
    display: grid;
    gap: 10px;
  }
  .record {
    width: 100%;
    display: flex;
    flex-direction: column;
    align-items: stretch;
    gap: 8px;
    text-align: left;
    white-space: normal;
    padding: 14px;
    background: var(--panel);
    border-radius: 8px;
  }
  .record.active {
    border-color: var(--accent);
    background: var(--selected);
  }
  .record-title {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 8px;
  }
  .record-title > :global(svg) {
    color: var(--chart-blue);
  }
  .record-title strong {
    flex: 1;
    font-size: 13px;
  }
  .path,
  .destination {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 12px;
  }
  .destination,
  .record-count {
    color: var(--secondary);
    font-size: 11px;
  }
  .state {
    display: inline-block;
    font-size: 11px;
    color: var(--secondary);
    border: 1px solid var(--border);
    border-radius: 5px;
    padding: 3px 6px;
  }
  .state[data-state='completed'] {
    color: var(--chart-reduction);
  }
  .state[data-state='partially_completed'],
  .state[data-state='failed'],
  .state[data-state='invalidated'] {
    color: var(--chart-growth);
  }
  .state[data-state='draft'],
  .state[data-state='awaiting_approval'] {
    color: var(--chart-violet);
  }
  .record.active .state {
    color: var(--text);
  }
  .unavailable {
    border: 1px solid var(--border);
    color: var(--secondary);
  }
  .unavailable p {
    font-size: 12px;
  }
  .record-detail {
    border: 1px solid var(--border);
    border-radius: 8px;
    background: var(--panel);
    padding: 20px;
  }
  h3 {
    font-size: 15px;
    margin: 0 0 8px;
    font-weight: 600;
  }
  .empty {
    display: grid;
    justify-items: start;
    gap: 10px;
    padding: 28px 16px;
  }
  .empty p {
    max-width: 560px;
    font-size: 13px;
  }
  .empty :global(svg) {
    color: var(--chart-blue);
  }
  .paging {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 10px;
    margin-top: 14px;
    font-size: 11px;
    color: var(--secondary);
  }
  .summary {
    margin: 16px 0;
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: 14px 20px;
  }
  .summary > div:first-child {
    grid-column: 1 / -1;
  }
  dt {
    font-size: 11px;
    color: var(--secondary);
    margin-bottom: 4px;
  }
  dd {
    margin: 0;
    font-size: 12px;
    overflow-wrap: anywhere;
  }
  summary {
    cursor: pointer;
    font-size: 12px;
    color: var(--secondary);
    padding: 9px 0;
  }
  summary:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 2px;
  }
  .destinations,
  .limitations {
    border-top: 1px solid var(--border);
    margin-top: 16px;
  }
  .destinations > dl + dl {
    padding-top: 16px;
    border-top: 1px solid var(--border);
  }
  .file-table {
    overflow-x: auto;
    margin-top: 16px;
  }
  .file-table:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 2px;
  }
  table {
    width: 100%;
    border-collapse: collapse;
    font-size: 12px;
  }
  caption {
    text-align: left;
    color: var(--secondary);
    font-size: 11px;
    padding: 0 0 12px;
  }
  th {
    text-align: left;
    font-size: 11px;
    color: var(--secondary);
    font-weight: 500;
    white-space: nowrap;
  }
  th,
  td {
    padding: 10px 8px;
    border-bottom: 1px solid var(--border);
    vertical-align: top;
  }
  td:first-child {
    min-width: 170px;
  }
  td strong,
  .original-path {
    display: block;
    max-width: 320px;
    overflow-wrap: anywhere;
    font-weight: 500;
  }
  td > small {
    display: block;
    margin-top: 6px;
  }
  .number {
    font-variant-numeric: tabular-nums;
    text-align: right;
    white-space: nowrap;
  }
  code {
    font-size: 11px;
    overflow-wrap: anywhere;
  }
  .limitations p {
    font-size: 12px;
    margin: 8px 0;
  }
  @media (max-width: 1180px) {
    .catalogue-grid {
      grid-template-columns: minmax(0, 1fr);
    }
    .summary {
      grid-template-columns: minmax(0, 1fr);
    }
  }
</style>
