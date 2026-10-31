<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { invoke, isTauri } from '@tauri-apps/api/core';
  import { date } from '../lib/types';

  type PlanState = 'draft' | 'awaiting_approval' | 'approved' | 'rejected' | 'expired' | 'executing' | 'completed' | 'partially_completed' | 'failed' | 'cancelled' | 'invalidated';
  interface Evidence { id: number; path: string; identity: string; logical_bytes: string; allocated_bytes: string; hard_links: number; modified_unix_ns: string; change_time: string }
  interface Plan {
    hash: string; state: PlanState; approval_expires: number | null;
    manifest: { id: string; connection_id: number; source_snapshot: number; user: string; root: string; root_identity: string; exclusions_count: number; created: number; expires: number; action: string; risk: string; logical_bytes: string; potential_reclaim_bytes: string; immediate_free_bytes: string; native_record_hash: string; entries: Evidence[] };
  }
  interface Review { connection: string; plan: Plan }
  type Summary = { id: string; connection_id: number; connection: string } & (
    | { availability: 'ready'; state: PlanState; expires: number; files: number; logical_bytes: string }
    | { availability: 'unavailable'; reason: string }
  );
  interface Page { items: Summary[]; next_offset: number | null }
  type Selection = { kind: 'none' } | { kind: 'loading'; id: string } | { kind: 'ready'; review: Review } | { kind: 'error'; message: string };
  let items = $state<Summary[]>([]), offset = $state(0), nextOffset = $state<number | null>(null);
  let selection = $state<Selection>({ kind: 'none' });
  let loading = $state(false), busy = $state(false), error = $state(''), notice = $state('');
  let now = $state(Math.floor(Date.now() / 1000));
  let alive = true, request = 0;
  let timer: ReturnType<typeof setInterval> | undefined;
  const native = isTauri();
  const exactBytes = (value: string): string => `${BigInt(value).toLocaleString()} bytes`;
  const stateLabel = (value: PlanState): string => value.replaceAll('_', ' ');

  async function refresh(): Promise<void> {
    if (!native || loading || busy) return;
    const current = request;
    const selected = selection.kind === 'ready' ? selection.review.plan.manifest.id : null;
    loading = true;
    try {
      const page = await invoke<Page>('list_agent_plans', { offset, limit: 25 });
      if (alive) { items = page.items; nextOffset = page.next_offset; }
      if (selected && alive && current === request) {
        try {
          const review = await invoke<Review>('get_agent_plan', { id: selected });
          if (alive && current === request) selection = { kind: 'ready', review };
        } catch (problem) { if (alive && current === request) selection = { kind: 'error', message: String(problem) }; }
      }
    } catch (problem) { if (alive) error = String(problem); }
    finally { if (alive) loading = false; }
  }
  async function inspect(id: string): Promise<void> {
    const current = ++request;
    selection = { kind: 'loading', id }; error = ''; notice = '';
    try {
      const review = await invoke<Review>('get_agent_plan', { id });
      if (alive && current === request) selection = { kind: 'ready', review };
    } catch (problem) { if (alive && current === request) selection = { kind: 'error', message: String(problem) }; }
  }
  async function decide(action: 'approve' | 'reject'): Promise<void> {
    if (selection.kind !== 'ready' || busy) return;
    const review = selection.review;
    ++request;
    busy = true; error = ''; notice = '';
    try {
      const plan = await invoke<Plan | null>(action === 'approve' ? 'approve_agent_plan' : 'reject_agent_plan', { id: review.plan.manifest.id, hash: review.plan.hash });
      if (!alive) return;
      if (plan) { selection = { kind: 'ready', review: { ...review, plan } }; notice = action === 'approve' ? 'Plan approved for at most two minutes. No files have been changed.' : 'Plan rejected. No files have been changed.'; }
      else notice = 'No approval granted.';
    } catch (problem) { if (alive) error = String(problem); }
    finally { if (alive) { busy = false; void refresh(); } }
  }
  function page(value: number): void { if (loading || busy) return; offset = value; selection = { kind: 'none' }; ++request; void refresh(); }
  onMount(() => {
    void refresh();
    if (native) timer = setInterval(() => { now = Math.floor(Date.now() / 1000); if (now % 5 === 0) void refresh(); }, 1000);
  });
  onDestroy(() => { alive = false; ++request; if (timer) clearInterval(timer); });
</script>

<section class="plan-review" aria-labelledby="plan-review-heading">
  <div class="review-heading"><h2 id="plan-review-heading">Agent plan review</h2><button onclick={refresh} disabled={!native || loading || busy}>Refresh requests</button></div>
  <p class="description">Review exact file selections requested by local connections. Approval is separate from execution and lasts at most two minutes.</p>
  <p class="description">Agent execution is unavailable in this build. Approving a plan does not change files.</p>
  {#if !native}<p>Human review requires the Windows application.</p>{/if}
  {#if error}<p class="error" role="alert">{error}</p>{/if}
  {#if notice}<p role="status">{notice}</p>{/if}
  {#if loading && items.length === 0}<p role="status">Loading requests…</p>
  {:else if native && !error && items.length === 0}<p class="description">No submitted plans. A connection must prepare a plan and request review.</p>{/if}
  <ul class="requests" aria-label="Submitted agent plans">
    {#each items as item (item.id)}
      <li>
        {#if item.availability === 'ready'}
          <button class="request" aria-pressed={selection.kind === 'ready' && selection.review.plan.manifest.id === item.id} disabled={busy} onclick={() => inspect(item.id)}>
            <span><strong>{item.connection}</strong><small>#{item.connection_id} · {item.id.slice(0, 12)}</small></span>
            <span>{item.files} files · {exactBytes(item.logical_bytes)}</span><span class="state">{stateLabel(item.state)}</span>
          </button>
        {:else}<div class="unavailable"><strong>{item.connection} · #{item.connection_id}</strong><p>{item.reason}</p><code>{item.id}</code></div>{/if}
      </li>
    {/each}
  </ul>
  {#if offset > 0 || nextOffset !== null}<div class="pagination"><button disabled={offset === 0 || loading || busy} onclick={() => page(Math.max(0, offset - 25))}>Previous</button><span>Requests {offset + 1}–{offset + items.length}</span><button disabled={nextOffset === null || loading || busy} onclick={() => { if (nextOffset !== null) page(nextOffset); }}>Next</button></div>{/if}
  {#if selection.kind === 'loading'}<p role="status">Loading exact selection…</p>
  {:else if selection.kind === 'error'}<p class="error" role="alert">{selection.message}</p>
  {:else if selection.kind === 'ready'}
    {@const review = selection.review}
    {@const plan = review.plan}
    {@const manifest = plan.manifest}
    <article class="selection" aria-labelledby="selected-plan-heading">
      <div class="review-heading"><h3 id="selected-plan-heading">{review.connection} · exact selection</h3><span class="state">{stateLabel(plan.state)}</span></div>
      <dl>
        <div><dt>Connection</dt><dd>#{manifest.connection_id}</dd></div><div><dt>Action</dt><dd>Send to Recycle Bin · manual review</dd></div>
        <div class="wide"><dt>Folder</dt><dd>{manifest.root}</dd></div>
        <div><dt>Selected</dt><dd>{manifest.entries.length} files · {exactBytes(manifest.logical_bytes)}</dd></div><div><dt>Immediate freed space</dt><dd>{exactBytes(manifest.immediate_free_bytes)}</dd></div>
        <div><dt>Potential recovery after bin removal</dt><dd>{exactBytes(manifest.potential_reclaim_bytes)} · not guaranteed</dd></div>
        <div><dt>Valid until</dt><dd>{date(plan.approval_expires ?? manifest.expires)} · {Math.max(0, (plan.approval_expires ?? manifest.expires) - now)} seconds remaining</dd></div>
        <div class="wide"><dt>Manifest hash</dt><dd><code>{plan.hash}</code></dd></div>
      </dl>
      <!-- svelte-ignore a11y_no_noninteractive_tabindex (The named overflow region needs keyboard scrolling.) -->
      <div class="file-list" role="region" aria-label="Exact selected files" tabindex="0">
        <table><caption>Only these files are included in this plan.</caption><thead><tr><th scope="col">Path and identity</th><th scope="col">Logical size</th><th scope="col">Allocation</th><th scope="col">Links</th></tr></thead><tbody>
          {#each manifest.entries as entry (entry.id)}<tr><td>{entry.path}<small><code>{entry.identity}</code></small></td><td>{exactBytes(entry.logical_bytes)}</td><td>{exactBytes(entry.allocated_bytes)}</td><td>{entry.hard_links}</td></tr>{/each}
        </tbody></table>
      </div>
      <p class="description">Paths, identities, sizes, timestamps, link counts and protections are checked again. Changed evidence invalidates the plan.</p>
      <details><summary>Scope and evidence</summary><dl><div><dt>Snapshot</dt><dd>{manifest.source_snapshot}</dd></div><div><dt>Captured exclusions</dt><dd>{manifest.exclusions_count}</dd></div><div class="wide"><dt>Windows user</dt><dd>{manifest.user}</dd></div><div class="wide"><dt>Folder identity</dt><dd><code>{manifest.root_identity}</code></dd></div><div class="wide"><dt>Native evidence hash</dt><dd><code>{manifest.native_record_hash}</code></dd></div></dl></details>
      <div class="decisions"><button disabled={busy || (plan.state !== 'awaiting_approval' && plan.state !== 'approved') || now >= manifest.expires} onclick={() => decide('reject')}>Reject plan</button><button class="primary" disabled={busy || plan.state !== 'awaiting_approval' || now >= manifest.expires} onclick={() => decide('approve')}>{busy ? 'Working…' : 'Review and approve plan'}</button></div>
    </article>
  {/if}
</section>

<style>
  .plan-review { display: grid; gap: 14px; min-width: 0; }
  .review-heading, .pagination, .decisions { display: flex; flex-wrap: wrap; align-items: center; justify-content: space-between; gap: 10px; }
  h3 { font-size: 16px; margin: 0; }
  .review-heading h3, .request > span { min-width: 0; overflow-wrap: anywhere; }
  .description { color: var(--secondary); margin: 0; }
  .requests { padding: 0; margin: 0; list-style: none; }
  .requests li + li { margin-top: 6px; }
  .request { width: 100%; display: flex; align-items: center; justify-content: space-between; gap: 16px; padding: 12px; text-align: left; min-height: 54px; height: auto; flex-wrap: wrap; }
  .request[aria-pressed='true'] { background: var(--selected); border-color: var(--accent); }
  small { display: block; color: var(--secondary); font-size: 11px; margin-top: 4px; }
  .state { font-size: 12px; color: var(--secondary); text-transform: capitalize; }
  .selection { display: grid; gap: 16px; border-top: 1px solid var(--border); padding-top: 18px; min-width: 0; }
  dl { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 14px; margin: 0; }
  dt { color: var(--secondary); font-size: 12px; }
  dd { margin: 4px 0 0; overflow-wrap: anywhere; }
  .wide { grid-column: 1 / -1; }
  code { font-family: Consolas, monospace; font-size: 12px; overflow-wrap: anywhere; }
  .file-list { overflow: auto; max-height: 360px; border: 1px solid var(--border); border-radius: var(--radius); }
  table { width: 100%; min-width: 620px; border-collapse: collapse; font-size: 12px; }
  caption { text-align: left; color: var(--secondary); padding: 10px; }
  th, td { text-align: left; vertical-align: top; padding: 10px; border-top: 1px solid var(--border); }
  td:first-child { max-width: 400px; overflow-wrap: anywhere; }
  .decisions { justify-content: flex-end; }
  .unavailable { padding: 12px; background: var(--surface); border-radius: var(--radius); overflow-wrap: anywhere; }
  .unavailable p { margin: 4px 0; }
  @media (max-width: 700px) { dl { grid-template-columns: 1fr; } }
</style>
