<script lang="ts">
  import { getIndexContext } from '../lib/index-context';
  import { applicationBytes } from '../lib/applications';
  import { gitStorageQueries, type GitStorageReport } from '../lib/queries';

  let {
    enabled,
    revision,
    onopen,
  }: {
    enabled: boolean;
    revision: number;
    onopen: (id: number) => void;
  } = $props();

  const limit = 30;
  let position = $state<{ offset: number; revision: number } | null>(null);
  const offset = $derived(position?.revision === revision ? position.offset : 0);
  let retry = $state(0);
  let report = $state<GitStorageReport | null>(null);
  let status = $state<'idle' | 'loading' | 'ready' | 'error'>('idle');

  $effect(() => {
    retry;
    const requestedRevision = revision;
    const start = offset;
    const indexId = getIndexContext();
    if (!enabled || indexId === null) {
      report = null;
      status = 'idle';
      return;
    }
    status = 'loading';
    const request = gitStorageQueries.request({
      indexId,
      query: { scope: 0, offset: start, limit },
    });
    let current = true;
    void request.result.then(
      (result) => {
        if (!current) return;
        if (!result || getIndexContext() !== indexId) {
          report = null;
          status = 'idle';
          return;
        }
        if (start > 0 && start >= result.total) {
          position = { offset: 0, revision: requestedRevision };
          return;
        }
        report = result;
        status = 'ready';
      },
      () => {
        if (!current) return;
        report = null;
        status = 'error';
      },
    );
    return () => {
      current = false;
      request.cancel();
    };
  });
</script>

<section aria-label="Git repository storage" aria-busy={status === 'loading'}>
  <div class="git-heading">
    <div>
      <h2>Git object storage</h2>
      <p>Logical size of stored objects in the scan, grouped by repository.</p>
    </div>
    {#if report}<span>{report.total.toLocaleString()} repositories</span>{/if}
  </div>
  {#if status === 'error'}
    <p role="status">Git storage is unavailable. <button onclick={() => retry++}>Retry</button></p>
  {:else if status === 'idle'}
    <p role="status">Complete a scan to see Git storage.</p>
  {:else if status === 'loading' && !report}
    <p role="status">Looking for Git stores…</p>
  {:else if report}
    {#if report.coverage_partial}
      <p>Scan coverage is partial. Some stores or files may be missing.</p>
    {/if}
    {#if report.git_file_markers || report.shared_store_markers}
      <p>
        Unresolved in this scan: {report.git_file_markers.toLocaleString()} .git file or link markers,
        {report.shared_store_markers.toLocaleString()} shared Git directory markers.
      </p>
    {/if}
    {#if report.items.length}
      <!-- svelte-ignore a11y_no_noninteractive_tabindex (The named overflow region needs keyboard scrolling.) -->
      <div class="git-table" role="region" aria-label="Git repository sizes" tabindex="0">
        <table>
          <thead
            ><tr>
              <th scope="col">Repository</th><th scope="col">Loose objects</th>
              <th scope="col">Packs</th><th scope="col">Pack metadata</th>
              <th scope="col">Local LFS</th><th scope="col" aria-sort="descending"
                >Observed total</th
              >
            </tr></thead
          >
          <tbody>
            {#each report.items as item (item.store_id)}
              <tr>
                <th scope="row">
                  <button class="repository" title={item.path} onclick={() => onopen(item.id)}>
                    {item.path}
                  </button>
                  {#if item.partial}<small>Partial store</small>{/if}
                  {#if item.alternates}<small>External objects excluded</small>{/if}
                </th>
                {#each [item.loose_bytes, item.pack_bytes, item.pack_metadata_bytes, item.lfs_bytes, item.total_bytes] as value}
                  <td title={value === null ? undefined : `${value} bytes`}>
                    {value === null ? 'Unobserved' : applicationBytes(value)}
                  </td>
                {/each}
              </tr>
            {/each}
          </tbody>
        </table>
      </div>
      {#if report.total > limit}
        <nav class="git-paging" aria-label="Repository pages">
          <button
            disabled={offset === 0 || status === 'loading'}
            onclick={() => (position = { offset: offset - limit, revision })}>Previous</button
          >
          <span role="status"
            >{status === 'loading'
              ? 'Loading…'
              : `Page ${Math.floor(offset / limit) + 1} of ${Math.ceil(report.total / limit)}`}</span
          >
          <button
            disabled={offset + limit >= report.total || status === 'loading'}
            onclick={() => (position = { offset: offset + limit, revision })}>Next</button
          >
        </nav>
      {/if}
    {:else}
      <p role="status">No Git object stores recognized in this scan.</p>
    {/if}
    <p class="git-note">
      Local LFS covers the default objects folder. Alternate or custom stores are outside these
      totals. Sizes describe the files observed in the scan and cannot estimate recoverable space.
    </p>
  {/if}
</section>

<style>
  .git-heading,
  .git-paging {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: space-between;
    gap: 8px 24px;
  }
  h2 {
    margin: 0;
    font-size: 18px;
  }
  p,
  .git-heading > span,
  small {
    color: var(--secondary);
  }
  p {
    margin: 4px 0 12px;
  }
  .git-table {
    overflow-x: auto;
  }
  table {
    width: 100%;
    border-collapse: collapse;
    font-variant-numeric: tabular-nums;
  }
  th,
  td {
    padding: 8px 10px;
    border-bottom: 1px solid var(--border);
    text-align: right;
    white-space: nowrap;
  }
  th:first-child {
    min-width: 180px;
    width: 100%;
    padding-left: 0;
    text-align: left;
  }
  td:last-child {
    font-weight: 600;
  }
  .repository {
    max-width: clamp(180px, 32vw, 530px);
    padding: 3px 0;
    overflow: hidden;
    border: 0;
    background: transparent;
    color: var(--accent);
    text-align: left;
    text-overflow: ellipsis;
  }
  .repository:hover {
    text-decoration: underline;
  }
  small {
    display: block;
    font-weight: 400;
  }
  .git-paging {
    justify-content: end;
    margin-top: 10px;
  }
  .git-note {
    margin-top: 14px;
  }
</style>
