<script lang="ts">
  import { currentStorageUnits } from '../lib/storage-units';

  let {
    bytes,
    onchange,
    disabled = false,
  }: {
    bytes: number;
    onchange: (bytes: number) => void;
    disabled?: boolean;
  } = $props();
  const binary = $derived(currentStorageUnits() === 'binary');
  const scale = $derived(binary ? 1_048_576 : 1_000_000);

  function update(value: number): void {
    onchange(
      Number.isFinite(value)
        ? Math.max(0, Math.min(Number.MAX_SAFE_INTEGER, Math.round(value * scale)))
        : 0,
    );
  }
</script>

<label>
  Minimum {binary ? 'MiB' : 'MB'}
  <input
    type="number"
    min="0"
    step="any"
    value={bytes / scale}
    {disabled}
    onchange={(event) => update(event.currentTarget.valueAsNumber)}
  />
</label>
