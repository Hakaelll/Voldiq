<script lang="ts">
  import { onMount, untrack } from 'svelte';
  import { menuKeyIndex, menuPosition, type MapMenuModel, type MapCommand } from '../lib/map-menu';
  import type { Point } from '../lib/disk-city';
  let {
    model,
    anchor,
    onchoose,
    ondismiss,
  }: {
    model: MapMenuModel;
    anchor: Point;
    onchoose: (owner: MapMenuModel, command: MapCommand) => void;
    ondismiss: (owner: MapMenuModel, restoreFocus: boolean) => void;
  } = $props();
  const owner = untrack(() => model);
  let menu: HTMLDivElement;
  let position = $state<Point>({ x: 8, y: 8 });
  let active = $state(0);
  function keyboard(event: KeyboardEvent): void {
    const next = menuKeyIndex(event.key, active, owner.items.length);
    if (next !== null) {
      event.preventDefault();
      event.stopPropagation();
      menu.querySelectorAll<HTMLButtonElement>('button[role="menuitem"]')[next]?.focus();
    } else if (event.key === 'Escape' || event.key === 'Tab') {
      if (event.key === 'Escape') event.preventDefault();
      event.stopPropagation();
      ondismiss(owner, true);
    }
  }
  onMount(() => {
    const place = () => {
      position = menuPosition(anchor, menu.getBoundingClientRect(), {
        width: document.documentElement.clientWidth,
        height: document.documentElement.clientHeight,
      });
    };
    const outside = (event: Event) => {
      if (!(event.target instanceof Node) || !menu.contains(event.target)) ondismiss(owner, false);
    };
    const blur = () => ondismiss(owner, false);
    const resize = new ResizeObserver(place);
    place();
    menu
      .querySelector<HTMLButtonElement>('button[role="menuitem"]')
      ?.focus({ preventScroll: true });
    resize.observe(menu);
    document.addEventListener('pointerdown', outside, true);
    document.addEventListener('focusin', outside);
    window.addEventListener('scroll', outside, true);
    window.addEventListener('resize', place);
    window.addEventListener('blur', blur);
    return () => {
      resize.disconnect();
      document.removeEventListener('pointerdown', outside, true);
      document.removeEventListener('focusin', outside);
      window.removeEventListener('scroll', outside, true);
      window.removeEventListener('resize', place);
      window.removeEventListener('blur', blur);
    };
  });
</script>

<div
  bind:this={menu}
  class="map-context-menu"
  role="menu"
  tabindex="-1"
  aria-label={`Actions for ${owner.name}`}
  style:left={`${position.x}px`}
  style:top={`${position.y}px`}
  onkeydown={keyboard}
  oncontextmenu={(event) => event.preventDefault()}
>
  <div class="menu-title" role="presentation" aria-hidden="true" title={owner.name}>
    {owner.name}
  </div>
  {#each owner.items as item, i}
    <button
      role="menuitem"
      tabindex={i === active ? 0 : -1}
      onfocus={() => {
        active = i;
      }}
      onclick={() => onchoose(owner, item.command)}>{item.label}</button
    >
  {/each}
</div>

<style>
  .map-context-menu {
    position: fixed;
    z-index: 9;
    width: 230px;
    max-width: calc(100vw - 16px);
    max-height: calc(100vh - 16px);
    overflow: auto;
    padding: 4px;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    background: var(--canvas);
  }
  .menu-title {
    padding: 6px 8px;
    font-size: 11px;
    color: var(--secondary);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    border-bottom: 1px solid var(--border);
    margin-bottom: 4px;
  }
  button {
    width: 100%;
    min-height: 32px;
    justify-content: flex-start;
    background: transparent;
    border-color: transparent;
    font-size: 12px;
  }
  button:hover,
  button:focus-visible {
    background: var(--selected);
  }
  button:focus-visible {
    outline-offset: -2px;
  }
</style>
