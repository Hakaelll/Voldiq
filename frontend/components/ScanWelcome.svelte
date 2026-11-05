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
    onscan,
    onchoose,
  }: {
    volume: Volume | null | undefined;
    busy: boolean;
    native: boolean;
    scanning: boolean;
    recent: string[];
    onscan: (path: string) => void;
    onchoose: () => void;
  } = $props();
  const space = $derived(volumeMeasurement(volume));
  const callerSpace = $derived(volumeCaller(volume));
  const percent = $derived(
    space && Number(space.total) > 0 ? (Number(space.used) / Number(space.total)) * 100 : null,
  );
</script>

<div class="scan-welcome">
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
          style:background={`conic-gradient(var(--accent-fill) ${percent}%, var(--paper) 0)`}
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
  <section class="welcome-start">
    <img src="/symbol.png" alt="" width="112" height="112" />
    <span class="welcome-eyebrow">Storage overview</span>
    <h2>{scanning ? 'Scanning…' : native ? 'Understand your disk space' : 'Desktop preview'}</h2>
    <p class="welcome-description">
      {scanning
        ? 'Your storage map will appear here when the scan finishes.'
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
      ><button onclick={onchoose} disabled={busy || !native}
        ><Icon name="folder" />Choose a folder</button
      >
    </div>
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
</div>

<style>
  .scan-welcome {
    display: grid;
    grid-template-columns: minmax(260px, 360px) minmax(0, 1fr);
    max-width: 1100px;
    margin: 12px auto;
    gap: 20px;
    min-height: 500px;
  }
  .scan-welcome:not(:has(.welcome-drive)) {
    grid-template-columns: minmax(0, 1fr);
  }
  .welcome-drive {
    padding: 28px;
    border: 1px solid var(--border);
    border-radius: 8px;
    background: var(--panel);
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
    width: 204px;
    height: 204px;
    margin: 32px auto;
    border-radius: 50%;
    padding: 16px;
  }
  .drive-ring > div {
    height: 100%;
    border-radius: 50%;
    background: var(--panel);
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
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    padding: 40px 28px;
    min-width: 0;
    border: 1px solid var(--border);
    border-radius: 8px;
    background: var(--panel);
    text-align: center;
  }
  .welcome-start > img {
    object-fit: contain;
    margin-bottom: 24px;
  }
  .welcome-start > h2 {
    font-size: 28px;
    letter-spacing: -0.6px;
    margin: 8px 0 12px;
    max-width: 360px;
  }
  .welcome-eyebrow {
    color: var(--secondary);
    font-size: 11px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.08em;
  }
  .welcome-description {
    max-width: 340px;
    margin-bottom: 28px;
    font-size: 13px;
  }
  .welcome-drive > p {
    margin-top: 14px;
    font-size: 11px;
  }
  .welcome-actions {
    display: flex;
    flex-wrap: wrap;
    justify-content: center;
    gap: 12px;
  }
  .welcome-actions button {
    min-height: 42px;
    padding: 10px 16px;
  }
  .welcome-recent {
    width: 100%;
    max-width: 400px;
    margin-top: 40px;
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
  @media (max-width: 1000px) {
    .scan-welcome {
      grid-template-columns: minmax(0, 1fr);
    }
    .welcome-start {
      padding: 32px 20px;
    }
  }
  @media (max-width: 700px) {
    .scan-welcome {
      grid-template-columns: minmax(0, 1fr);
      margin-top: 16px;
    }
    .welcome-drive {
      padding: 24px;
    }
    .drive-ring {
      width: 170px;
      height: 170px;
    }
  }
</style>
