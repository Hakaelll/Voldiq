<script lang="ts">
  import type { CloudFilter } from '../lib/types';
  import { CLOUD_FILTERS, parseCloudFilter } from '../lib/cloud';
  let {
    value,
    onchange,
  }: { value: CloudFilter | null; onchange: (value: CloudFilter | null) => void } = $props();
</script>

<label title="Observed metadata. No recall flag does not prove complete local content.">
  Cloud<select
    value={value ?? ''}
    onchange={(event) => {
      const next = parseCloudFilter(event.currentTarget.value);
      if (next !== undefined) onchange(next);
    }}
  >
    <option value="">All observed states</option>
    {#each CLOUD_FILTERS as filter}<option value={filter.value}>{filter.label}</option>{/each}
  </select>
</label>
