<script lang="ts">
  import { difference, type SnapshotComparison } from '../lib/types';
  import { snapshotChangeBars } from '../lib/snapshot-change';
  let { changes }: { changes: SnapshotComparison['file_changes'] } = $props();
  const rows = $derived(snapshotChangeBars(changes));
</script>

<div class="change-balance" aria-label="Logical file changes">
  <div class="balance-axis" aria-hidden="true"><span>Decrease</span><span>Increase</span></div>
  {#each rows as row}
    <div class="balance-row">
      <strong>{row.label}<small>{row.files.toLocaleString()} files</small></strong>
      <div class="balance-track" aria-hidden="true">
        <span class={row.kind} style:left={`${row.start}%`} style:width={`${row.width}%`}></span>
      </div>
      <span class="balance-value">{difference(row.delta)}</span>
    </div>
  {/each}
</div>

<style>
  .change-balance {
    margin: 18px 0 24px;
  }
  .balance-axis {
    margin: 0 108px 8px 90px;
    display: flex;
    justify-content: space-between;
    font-size: 11px;
    color: var(--secondary);
  }
  .balance-row {
    display: grid;
    grid-template-columns: 78px minmax(0, 1fr) 96px;
    align-items: center;
    gap: 12px;
    min-height: 58px;
  }
  .balance-row > strong {
    font-size: 13px;
    font-weight: 500;
  }
  small {
    display: block;
    margin-top: 4px;
    color: var(--secondary);
    font-size: 11px;
    font-weight: 400;
  }
  .balance-track {
    position: relative;
    height: 28px;
    background: var(--surface);
  }
  .balance-track::after {
    content: '';
    position: absolute;
    left: 50%;
    top: -8px;
    bottom: -8px;
    width: 1px;
    background: var(--strong);
  }
  .balance-track > span {
    position: absolute;
    height: 100%;
  }
  .added {
    background: #087f8c;
  }
  .changed {
    background: #075bdd;
  }
  .removed {
    background: #ed701b;
  }
  .balance-value {
    text-align: right;
    font-size: 13px;
    font-variant-numeric: tabular-nums;
    overflow-wrap: anywhere;
  }
</style>
