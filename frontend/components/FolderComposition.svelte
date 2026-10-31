<script lang="ts">
  import { bytes, type Entry } from '../lib/types';
  import { categoryPaint } from '../lib/storage-map';
  let { children, logical }: { children: Entry[]; logical: number } = $props();
  const childBytes = $derived(children.reduce((total, child) => total + child.logical, 0));
  const remaining = $derived(logical - childBytes);
  const description = $derived(
    [
      ...children.map((child) => `${child.name}: ${bytes(child.logical)}`),
      ...(remaining > 0 ? [`Other: ${bytes(remaining)}`] : []),
    ].join(', '),
  );
</script>

{#if logical > 0 && remaining >= 0}
  <div class="folder-composition" role="img" aria-label={`Indexed folder contents. ${description}`}>
    {#each children as child, position}
      {#if child.logical > 0}
        <span
          style:flex-grow={child.logical}
          style:background={categoryPaint({
            id: child.id,
            name: child.name,
            value: child.logical,
            directory: child.kind === 'directory',
          }).fill}
          style:color={categoryPaint({
            id: child.id,
            name: child.name,
            value: child.logical,
            directory: child.kind === 'directory',
          }).ink}
          title={`${child.name}: ${bytes(child.logical)}`}
          >{#if child.logical / logical >= 0.2}{child.name}{/if}</span
        >
      {/if}
    {/each}
    {#if remaining > 0}<span
        class="remaining"
        style:flex-grow={remaining}
        title={`Other: ${bytes(remaining)}`}
        >{#if remaining / logical >= 0.2}Other{/if}</span
      >{/if}
  </div>
{/if}

<style>
  .folder-composition {
    display: flex;
    height: 48px;
    gap: 2px;
    overflow: hidden;
    margin: 0 0 16px;
  }
  .folder-composition > span {
    flex-basis: 0;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    line-height: 48px;
    text-indent: 8px;
    font-size: 11px;
    font-weight: 600;
  }
  .remaining {
    background: var(--paper);
    color: var(--secondary);
  }
</style>
