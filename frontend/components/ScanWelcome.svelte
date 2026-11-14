<script lang="ts">
  import Icon from './Icon.svelte';
  import { bytes, type Volume } from '../lib/types';
  import { volumeMeasurement, volumeCaller, volumeObservationLabel } from '../lib/volume-space';
  let {
    volume,
    busy,
    native,
    scanning,
    recent,
    showGuide,
    onscan,
    onchoose,
    onhelp,
    onskip,
  }: {
    volume: Volume | null | undefined;
    busy: boolean;
    native: boolean;
    scanning: boolean;
    recent: string[];
    showGuide: boolean;
    onscan: (path: string) => void;
    onchoose: () => void;
    onhelp: () => void;
    onskip: () => void;
  } = $props();
  let heading: HTMLHeadingElement;
  const space = $derived(volumeMeasurement(volume));
  const callerSpace = $derived(volumeCaller(volume));
  const percent = $derived(
    space && Number(space.total) > 0 ? (Number(space.used) / Number(space.total)) * 100 : null,
  );
</script>

<div class="scan-welcome">
  <section class="welcome-start">
    <h2 bind:this={heading} tabindex="-1">
      {scanning ? 'Scanning…' : native ? 'Scan your storage' : 'Desktop preview'}
    </h2>
    <p class="welcome-description">
      {scanning
        ? 'Your storage map will appear here when the scan finishes.'
        : native
          ? 'Scan a drive, choose a folder or drop a folder here to see what takes up space.'
          : 'Scan a drive or folder to see what takes up space and explore your files.'}
    </p>
    <div class="welcome-actions">
      <button
        class="primary"
        onclick={() => {
          if (volume) onscan(volume.path);
        }}
        disabled={busy || !native || !volume?.accessible}
        ><Icon name="scan" />Scan selected drive</button
      ><button class="quiet-action" onclick={onchoose} disabled={busy || !native}
        ><Icon name="folder" />Choose a folder</button
      >
    </div>
    {#if showGuide && native && !busy && !scanning}
      <section class="welcome-guide" aria-label="Getting started">
        <p>
          Start with a folder you know. After the scan, open Explorer to see what takes up space.
        </p>
        <div>
          <button
            class="quiet-action"
            onclick={onhelp}
            aria-haspopup="dialog"
            aria-keyshortcuts="F1">Help & shortcuts</button
          >
          <button
            class="quiet-action"
            onclick={() => {
              onskip();
              heading.focus({ preventScroll: true });
            }}>Skip tips</button
          >
        </div>
      </section>
    {/if}
    {#if recent.length}<div class="welcome-recent">
        <h3>Recent</h3>
        {#each recent.slice(0, 3) as path}<button
            title={path}
            disabled={busy || !native}
            onclick={() => onscan(path)}
            ><Icon name="folder" /><span>{path}</span><Icon name="expand" size={14} /></button
          >{/each}
      </div>{/if}
  </section>
  {#if volume?.accessible}<section class="welcome-drive" aria-label="Selected drive">
      <header>
        <Icon name="disk" size={24} />
        <div>
          <h2>{volume.name || volume.path}</h2>
          <span>{volume.path} · {volume.filesystem}</span>
        </div>
      </header>
      {#if percent !== null && space}<div
          class="drive-ring"
          style:background={`conic-gradient(from -90deg, var(--chart-blue) 0%, var(--chart-violet) ${percent}%, var(--paper) ${percent}% 100%)`}
          aria-hidden="true"
        >
          <div><strong>{percent.toFixed(1)}<small>%</small></strong><span>Used</span></div>
        </div>
        <progress
          class="sr-only"
          aria-label="Selected drive space used"
          value={Number(space.used)}
          max={Number(space.total)}
        ></progress>{:else}<p>Volume usage unavailable</p>{/if}
      <dl>
        <div>
          <dt>Used</dt>
          <dd title={space ? `${space.used} bytes` : undefined}>
            {space ? bytes(Number(space.used)) : 'Unavailable'}
          </dd>
        </div>
        <div>
          <dt>Free</dt>
          <dd title={space ? `${space.free} bytes` : undefined}>
            {space ? bytes(Number(space.free)) : 'Unavailable'}
          </dd>
        </div>
        <div>
          <dt>Capacity</dt>
          <dd title={space ? `${space.total} bytes` : undefined}>
            {space ? bytes(Number(space.total)) : 'Unavailable'}
          </dd>
        </div>
      </dl>
      {#if callerSpace}<p title={`${callerSpace.available} bytes available to the caller`}>
          Available to you · {bytes(Number(callerSpace.available))}
        </p>{/if}
      <p>{volumeObservationLabel(volume)}</p>
    </section>{/if}
</div>

<style>
  .scan-welcome {
    display: grid;
    grid-template-columns: minmax(0, 1fr) minmax(230px, 320px);
    align-items: start;
    max-width: 1040px;
    gap: clamp(32px, 6vw, 88px);
    padding-block: 32px;
    border-top: 1px solid var(--border);
  }
  .scan-welcome:not(:has(.welcome-drive)) {
    grid-template-columns: minmax(0, 1fr);
  }
  .welcome-drive {
    min-width: 0;
    border-inline-start: 1px solid var(--border);
    padding-inline-start: 28px;
  }
  header {
    display: flex;
    align-items: center;
    gap: 14px;
  }
  header h2 {
    font-size: 21px;
  }
  header > div {
    min-width: 0;
    overflow-wrap: anywhere;
  }
  header span {
    color: var(--secondary);
    font-size: 12px;
    display: block;
    margin-top: 4px;
  }
  .drive-ring {
    width: 164px;
    height: 164px;
    margin: 24px 0;
    border-radius: 50%;
    padding: 12px;
  }
  .drive-ring > div {
    height: 100%;
    border-radius: 50%;
    background: var(--workspace);
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 4px;
  }
  .drive-ring strong {
    font-size: 38px;
    letter-spacing: -1px;
    font-weight: 600;
    font-variant-numeric: tabular-nums;
  }
  .drive-ring strong small {
    font-size: 20px;
    color: var(--text);
  }
  .drive-ring span {
    color: var(--secondary);
    font-size: 12px;
  }
  dl {
    margin: 0;
    display: grid;
    gap: 15px;
  }
  dl > div {
    display: flex;
    justify-content: space-between;
    gap: 12px;
  }
  dt {
    color: var(--secondary);
  }
  dd {
    margin: 0;
    font-weight: 600;
    font-variant-numeric: tabular-nums;
  }
  .welcome-start {
    min-width: 0;
  }
  .welcome-start > h2 {
    font-size: 23px;
    letter-spacing: -0.02em;
    margin-bottom: 8px;
  }
  .welcome-description {
    max-width: 46ch;
    margin-bottom: 20px;
  }
  .welcome-drive > p {
    margin-top: 14px;
    font-size: 12px;
  }
  .welcome-actions {
    display: flex;
    flex-wrap: wrap;
    gap: 12px;
  }
  .welcome-actions button {
    min-height: 42px;
    padding: 10px 16px;
  }
  .welcome-guide {
    margin-top: 20px;
    padding-top: 12px;
    max-width: 46ch;
    border-top: 1px solid var(--border);
  }
  .welcome-guide p {
    color: var(--secondary);
    margin-bottom: 4px;
  }
  .welcome-guide > div {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
  }
  .welcome-recent {
    width: 100%;
    max-width: 400px;
    margin-top: 30px;
  }
  h3 {
    margin: 0 0 10px;
    font-size: 12px;
    color: var(--secondary);
    font-weight: 500;
  }
  .welcome-recent button {
    width: 100%;
    justify-content: flex-start;
    border: 0;
    border-radius: 0;
    border-top: 1px solid var(--border);
    padding: 13px 0;
    background: transparent;
  }
  .welcome-recent button span {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    text-align: left;
  }
  @media (max-width: 850px) {
    .scan-welcome {
      grid-template-columns: minmax(0, 1fr);
    }
    .welcome-drive {
      border-inline-start: 0;
      padding: 24px 0 0;
      border-top: 1px solid var(--border);
    }
  }
</style>
