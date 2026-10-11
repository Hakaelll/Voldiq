<script lang="ts">
  import { onMount } from 'svelte';
  import { Channel, invoke, isTauri } from '@tauri-apps/api/core';
  import Icon from './components/Icon.svelte';
  import Treemap from './components/Treemap.svelte';
  import FileTable from './components/FileTable.svelte';
  import Duplicates from './components/Duplicates.svelte';
  import History from './components/History.svelte';
  import {
    bytes,
    date,
    type Entry,
    type Details,
    type FileType,
    type Issue,
    type Page,
    type Progress,
    type Query,
    type Volume,
    type Snapshot,
  } from './lib/types';
  type View = 'overview' | 'explorer' | 'large' | 'duplicates' | 'history' | 'settings';
  const navigation: { id: View; label: string; icon: string }[] = [
    { id: 'overview', label: 'Overview', icon: 'overview' },
    { id: 'explorer', label: 'Explorer', icon: 'folder' },
    { id: 'large', label: 'Large files', icon: 'large' },
    { id: 'duplicates', label: 'Duplicates', icon: 'duplicate' },
    { id: 'history', label: 'History', icon: 'history' },
  ];
  let view = $state<View>('overview');
  let expanded = $state(true);
  let volumes = $state<Volume[]>([]),
    volumePath = $state(''),
    scanPath = $state('');
  let operation = $state<Progress | null>(null),
    scanId = $state<number | null>(null);
  let ready = $state(false),
    revision = $state(0),
    scope = $state(0);
  let directory = $state<Details | null>(null),
    details = $state<Details | null>(null);
  let children = $state<Page>({ items: [], total: 0 }),
    types = $state<FileType[]>([]);
  let text = $state(''),
    extension = $state(''),
    minSize = $state(100),
    before = $state(''),
    sort = $state('size');
  let error = $state(''),
    issues = $state<Issue[]>([]),
    showIssues = $state(false);
  let excluded = $state(''),
    theme = $state('light'),
    sizeMode = $state('logical'),
    reduceMotion = $state(false);
  let search: HTMLInputElement;
  let refreshToken = 0;
  let interactionToken = 0;
  let analysisBusy = $state(false);
  let historical = $state<Snapshot | null>(null);
  let volume = $derived(volumes.find((v) => v.path === volumePath));
  let busy = $derived(operation?.phase === 'scanning' || analysisBusy);
  let query = $derived<Query>({
    ...(view === 'large' || text ? { scope } : { parent: scope }),
    text,
    extension,
    min_size: view === 'large' ? Math.max(0, minSize) * 1024 * 1024 : 0,
    ...(before ? { modified_before: new Date(before).getTime() / 1000 } : {}),
    files_only: view === 'large',
    sort,
  });
  let nodes = $derived.by(() => {
    const entries = children.items.map((entry) => ({
      id: entry.id,
      name: entry.name,
      value: sizeMode === 'allocated' ? entry.allocated : entry.logical,
      directory: entry.kind === 'directory',
    }));
    const total = directory
      ? sizeMode === 'allocated'
        ? directory.entry.allocated
        : directory.entry.logical
      : 0;
    const remainder = Math.max(0, total - entries.reduce((s, n) => s + n.value, 0));
    if (remainder > 0)
      entries.push({
        id: -1,
        name: `${Math.max(0, children.total - children.items.length)} smaller items`,
        value: remainder,
        directory: true,
      });
    return entries;
  });
  function report(problem: unknown): void {
    error = problem instanceof Error ? problem.message : String(problem);
  }
  async function refresh(id = scope): Promise<void> {
    const token = ++refreshToken;
    try {
      const result = await Promise.all([
        invoke<Details>('get_details', { id }),
        invoke<Page>('query_files', { query: { parent: id, limit: 250, sort: 'size' } }),
        invoke<FileType[]>('get_file_types', { id }),
      ]);
      if (token !== refreshToken) return;
      [directory, children, types] = result;
      scope = id;
      revision += 1;
    } catch (problem) {
      if (token === refreshToken) report(problem);
    }
  }
  async function select(id: number): Promise<void> {
    const token = ++interactionToken;
    if (id === -1) {
      view = 'explorer';
      return;
    }
    try {
      const item = await invoke<Details>('get_details', { id });
      if (token === interactionToken && ready) details = item;
    } catch (problem) {
      if (token === interactionToken) report(problem);
    }
  }
  async function open(id: number): Promise<void> {
    const token = ++interactionToken;
    refreshToken += 1;
    if (id === -1) {
      view = 'explorer';
      return;
    }
    try {
      const item = await invoke<Details>('get_details', { id });
      if (token !== interactionToken || !ready) return;
      if (item.entry.kind === 'directory') {
        text = '';
        details = null;
        await refresh(id);
      } else {
        details = item;
      }
    } catch (problem) {
      if (token === interactionToken) report(problem);
    }
  }
  async function start(path: string): Promise<void> {
    if (!path || busy) return;
    refreshToken += 1;
    interactionToken += 1;
    ready = false;
    historical = null;
    directory = null;
    details = null;
    children = { items: [], total: 0 };
    types = [];
    error = '';
    scanPath = path;
    scope = 0;
    operation = {
      phase: 'scanning',
      entries: 0,
      files: 0,
      logical: 0,
      elapsed_ms: 0,
      errors: 0,
      method: 'Directory enumeration',
    };
    const channel = new Channel<Progress>();
    let lastRefresh = 0;
    channel.onmessage = (update) => {
      operation = update;
      if (update.phase === 'scanning') {
        ready = true;
        if (update.elapsed_ms === 0 || update.elapsed_ms - lastRefresh >= 1000) {
          lastRefresh = update.elapsed_ms;
          void refresh(scope);
        }
      }
      if (update.phase === 'failed') {
        error = update.method;
        scanId = null;
      } else if (update.phase === 'complete' || update.phase === 'cancelled') {
        ready = true;
        scanId = null;
        details = null;
        void refresh(scope);
        void invoke<Volume[]>('get_volumes')
          .then((value) => {
            volumes = value;
          })
          .catch(report);
      }
    };
    try {
      const id = await invoke<number>('start_scan', {
        path,
        options: {
          exclusions: excluded
            .split('\n')
            .map((line) => line.trim())
            .filter(Boolean),
        },
        progress: channel,
      });
      if (operation?.phase === 'scanning') scanId = id;
    } catch (problem) {
      operation = null;
      report(problem);
    }
  }
  async function choose(): Promise<void> {
    try {
      const folder = await invoke<string | null>('choose_folder');
      if (folder) await start(folder);
    } catch (problem) {
      report(problem);
    }
  }
  async function loadSnapshot(snapshot: Snapshot): Promise<void> {
    refreshToken += 1;
    interactionToken += 1;
    details = null;
    operation = null;
    historical = snapshot;
    scanPath = snapshot.root;
    scope = 0;
    text = '';
    ready = true;
    await refresh(0);
    view = 'overview';
  }
  async function cancel(): Promise<void> {
    if (scanId !== null)
      try {
        await invoke('cancel_operation', { id: scanId });
      } catch (problem) {
        report(problem);
      }
  }
  async function reveal(): Promise<void> {
    if (details)
      try {
        await invoke('reveal_item', { id: details.entry.id });
      } catch (problem) {
        report(problem);
      }
  }
  async function copy(): Promise<void> {
    if (details?.path_exact)
      try {
        await navigator.clipboard.writeText(details.path);
      } catch (problem) {
        report(problem);
      }
  }
  async function inspectIssues(): Promise<void> {
    try {
      issues = await invoke<Issue[]>('get_issues');
      showIssues = true;
    } catch (problem) {
      report(problem);
    }
  }
  onMount(() => {
    try {
      const value: unknown = JSON.parse(localStorage.getItem('voldiq-preferences') ?? '{}');
      if (value && typeof value === 'object') {
        if ('expanded' in value && typeof value.expanded === 'boolean') expanded = value.expanded;
        if ('excluded' in value && typeof value.excluded === 'string') excluded = value.excluded;
        if ('theme' in value && (value.theme === 'light' || value.theme === 'dark'))
          theme = value.theme;
        if ('sizeMode' in value && (value.sizeMode === 'logical' || value.sizeMode === 'allocated'))
          sizeMode = value.sizeMode;
        if ('reduceMotion' in value && typeof value.reduceMotion === 'boolean')
          reduceMotion = value.reduceMotion;
        if ('defaultDrive' in value && typeof value.defaultDrive === 'string')
          volumePath = value.defaultDrive;
      }
    } catch {
      error = 'Saved preferences could not be read. Defaults restored.';
    }
    if (isTauri())
      void invoke<Volume[]>('get_volumes')
        .then((value) => {
          volumes = value;
          if (!volumes.some((v) => v.path === volumePath))
            volumePath = value.find((v) => v.accessible)?.path ?? '';
        })
        .catch(report);
    const keyboard = (event: KeyboardEvent): void => {
      if (event.ctrlKey && event.key.toLowerCase() === 'k') {
        event.preventDefault();
        search.focus();
        search.select();
      }
      if (event.key === 'Escape') {
        showIssues = false;
        details = null;
      }
    };
    window.addEventListener('keydown', keyboard);
    return () => window.removeEventListener('keydown', keyboard);
  });
  $effect(() => {
    document.documentElement.dataset.theme = theme;
    document.documentElement.dataset.motion = reduceMotion ? 'reduced' : 'normal';
  });
  function savePreferences(): void {
    try {
      localStorage.setItem(
        'voldiq-preferences',
        JSON.stringify({
          expanded,
          excluded,
          theme,
          sizeMode,
          reduceMotion,
          defaultDrive: volumePath,
        }),
      );
    } catch (problem) {
      report(problem);
    }
  }
</script>

<div class="app-shell" class:expanded>
  <aside class="navigation">
    <div class="brand">
      <img src="/symbol.png" alt="" width="32" height="32" />{#if expanded}<strong>Voldiq</strong
        >{/if}
    </div>
    <button
      class="nav-toggle"
      title={expanded ? 'Collapse navigation' : 'Expand navigation'}
      aria-label={expanded ? 'Collapse navigation' : 'Expand navigation'}
      onclick={() => {
        expanded = !expanded;
        savePreferences();
      }}
      ><Icon name={expanded ? 'collapse' : 'expand'} />{#if expanded}<span>Collapse navigation</span
        >{/if}</button
    >
    <nav aria-label="Main navigation">
      {#each navigation as item}<button
          class:active={view === item.id}
          title={item.label}
          disabled={analysisBusy}
          aria-label={item.label}
          aria-current={view === item.id ? 'page' : undefined}
          onclick={() => {
            view = item.id;
            text = '';
          }}
          ><Icon name={item.icon} />{#if expanded}<span>{item.label}</span>{/if}</button
        >{/each}
    </nav>
    <div class="nav-bottom">
      {#if expanded && volume}<div class="drive-summary">
          <span><Icon name="disk" />{volume.name || volume.path}</span><progress
            value={volume.total - volume.free}
            max={volume.total || 1}
          ></progress><small
            >{bytes(volume.total - volume.free)} of {bytes(volume.total)} used</small
          >
        </div>{/if}<button
        class:active={view === 'settings'}
        title="Settings"
        disabled={analysisBusy}
        aria-label="Settings"
        onclick={() => {
          view = 'settings';
        }}
        ><Icon name="settings" />{#if expanded}<span>Settings</span>{/if}</button
      >
    </div>
  </aside>
  <div class="workspace">
    <header class="toolbar">
      <label class="drive-select"
        ><Icon name="disk" /><select
          aria-label="Select drive"
          bind:value={volumePath}
          disabled={busy}
          ><option value="" disabled>Select a drive</option>{#each volumes as drive}<option
              value={drive.path}
              disabled={!drive.accessible}
              >{drive.name || 'Local disk'} ({drive.path}) · {drive.filesystem}</option
            >{/each}</select
        ></label
      ><button onclick={choose} disabled={busy || !isTauri()}
        ><Icon name="folder" />Choose folder</button
      >
      <div class="search">
        <Icon name="search" /><input
          bind:this={search}
          bind:value={text}
          placeholder="Search files and folders"
          aria-label="Search files and folders"
          disabled={!ready || view === 'settings' || view === 'duplicates' || view === 'history'}
        /><kbd>Ctrl K</kbd>
      </div>
    </header>
    <main>
      <div class="page-heading">
        <div>
          <h1>
            {view === 'overview'
              ? 'Storage overview'
              : view === 'explorer'
                ? 'Explorer'
                : view === 'large'
                  ? 'Large files'
                  : view === 'duplicates'
                    ? 'Duplicates'
                    : view === 'history'
                      ? 'History'
                      : 'Settings'}
          </h1>
          {#if view !== 'settings' && view !== 'history' && scanPath}<p>
              {scanPath}{#if historical}
                · Snapshot · {date(historical.captured)}{/if}
            </p>{/if}
        </div>
        {#if view !== 'settings' && view !== 'history'}<button
            class="primary"
            onclick={() => start(scanPath || volumePath)}
            disabled={!isTauri() || busy || !(scanPath || volumePath)}
            ><Icon name="scan" />{ready ? 'Rescan' : 'Scan drive'}</button
          >{/if}
      </div>
      {#if error}<div class="error" role="alert">
          <Icon name="warning" /><span>{error}</span><button
            aria-label="Dismiss error"
            onclick={() => {
              error = '';
            }}><Icon name="close" /></button
          >
        </div>{/if}
      {#if operation && view !== 'settings' && view !== 'duplicates' && view !== 'history'}<div
          class="scan-status"
          aria-live="polite"
        >
          <span class:working={busy} class="status-dot"></span><span
            >{busy
              ? 'Scanning'
              : operation.phase === 'complete'
                ? 'Scan complete'
                : operation.phase === 'cancelled'
                  ? 'Cancelled · partial results'
                  : 'Scan failed'}</span
          ><span
            >{operation.files.toLocaleString()} files · {bytes(operation.logical)} logical · {(
              operation.elapsed_ms / 1000
            ).toFixed(1)} s</span
          >{#if operation.errors > 0}<button onclick={inspectIssues}
              >{operation.errors} access issues</button
            >{/if}{#if busy}<button onclick={cancel} disabled={scanId === null}>Cancel scan</button
            >{/if}
        </div>{/if}
      {#if view === 'settings'}
        <section class="settings-panel">
          <h2>Appearance</h2>
          <label
            >Theme<select bind:value={theme} onchange={savePreferences}
              ><option value="light">Light</option><option value="dark">Dark</option></select
            ></label
          ><label
            >Treemap size metric<select bind:value={sizeMode} onchange={savePreferences}
              ><option value="logical">Logical size</option><option value="allocated"
                >Unique physical allocation</option
              ></select
            ></label
          ><label class="check"
            ><input type="checkbox" bind:checked={reduceMotion} onchange={savePreferences} />Reduce
            motion</label
          >
          <h2>Scanning</h2>
          <label
            >Default drive<select bind:value={volumePath} onchange={savePreferences}
              >{#each volumes as drive}<option value={drive.path}>{drive.path} {drive.name}</option
                >{/each}</select
            ></label
          ><label
            >Excluded paths · one per line<textarea
              bind:value={excluded}
              onchange={savePreferences}
              rows="5"
              placeholder="C:\\folder"></textarea></label
          >
        </section>
      {:else if view === 'history'}
        <History
          enabled={ready && !busy && historical === null}
          onbusy={(value) => {
            analysisBusy = value;
          }}
          onload={loadSnapshot}
          onerror={report}
        />
      {:else}
        {#if volume && view !== 'duplicates'}<section
            class="capacity-strip"
            aria-label="Volume capacity"
          >
            <div><span>Total capacity</span><strong>{bytes(volume.total)}</strong></div>
            <div>
              <span>Used on volume</span><strong>{bytes(volume.total - volume.free)}</strong><small
                >{volume.total
                  ? (((volume.total - volume.free) / volume.total) * 100).toFixed(1)
                  : '0'}%</small
              >
            </div>
            <div><span>Free space</span><strong>{bytes(volume.free)}</strong></div>
            <div>
              <span>Indexed logical size</span><strong
                >{ready && directory ? bytes(directory.entry.logical) : 'Not scanned'}</strong
              ><small
                >{directory?.entry.incomplete
                  ? 'Incomplete coverage'
                  : ready
                    ? 'Current folder'
                    : volume.filesystem}</small
              >
            </div>
          </section>{/if}
        {#if !ready && view !== 'duplicates'}<section class="welcome">
            <div class="welcome-art">
              <img src="/symbol.png" alt="Voldiq logo" width="72" height="72" />
            </div>
            <h2>No scan yet</h2>
            <div>
              <button
                class="primary"
                onclick={() => start(volumePath)}
                disabled={!volumePath || busy || !isTauri()}
                ><Icon name="scan" />Scan selected drive</button
              ><button onclick={choose} disabled={busy || !isTauri()}
                ><Icon name="folder" />Choose a folder</button
              >
            </div>
          </section>
        {:else}
          {#if ready}<div class="breadcrumbs">
              <button title="Scan root" onclick={() => open(0)}
                ><Icon name="disk" />Scan root</button
              >{#each directory?.ancestors.filter((entry) => entry.id !== 0) ?? [] as ancestor}<Icon
                  name="expand"
                  size={12}
                /><button onclick={() => open(ancestor.id)}>{ancestor.name}</button
                >{/each}{#if directory && scope !== 0}<Icon name="expand" size={12} /><span
                  >{directory.entry.name}</span
                >{/if}{#if directory?.entry.parent !== null && directory?.entry.parent !== undefined}<button
                  class="back-button"
                  onclick={() => open(directory?.entry.parent ?? 0)}
                  ><Icon name="back" />Parent folder</button
                >{/if}
            </div>{/if}
          {#if directory?.entry.incomplete}<div class="notice">Incomplete coverage</div>{/if}
          {#if view === 'overview'}<div class="overview-grid">
              <section class="panel map-panel">
                <div class="panel-heading">
                  <div>
                    <h2>Storage map</h2>
                    <p>
                      {sizeMode === 'logical' ? 'Logical size' : 'Unique allocation'}
                    </p>
                  </div>
                  <button
                    title="Open list"
                    onclick={() => {
                      view = 'explorer';
                    }}><Icon name="list" />List</button
                  >
                </div>
                <Treemap
                  {nodes}
                  selected={details?.entry.id ?? null}
                  onselect={(id) => {
                    void select(id);
                  }}
                  onopen={(id) => {
                    void open(id);
                  }}
                />
              </section>
              <section class="panel largest-panel">
                <div class="panel-heading">
                  <h2>Largest items</h2>
                  <button
                    onclick={() => {
                      view = 'explorer';
                    }}>See all</button
                  >
                </div>
                <div class="ranking-head"><span>Name</span><span>Logical size</span></div>
                {#each children.items.slice(0, 10) as entry}<button
                    class="ranking-row"
                    onclick={() => {
                      void select(entry.id);
                    }}
                    ondblclick={() => {
                      void open(entry.id);
                    }}
                    ><Icon name={entry.kind === 'directory' ? 'folder' : 'file'} /><span
                      title={entry.name}>{entry.name}</span
                    ><strong>{bytes(entry.logical)}</strong><span class="size-bar"
                      ><span
                        style:width={`${directory?.entry.logical ? (entry.logical / directory.entry.logical) * 100 : 0}%`}
                      ></span></span
                    ></button
                  >{/each}
              </section>
            </div>
            <section class="panel type-panel">
              <div class="panel-heading">
                <div>
                  <h2>File types</h2>
                </div>
              </div>
              <div class="type-bar">
                {#each types.slice(0, 8) as item}<span
                    style:flex-grow={item.bytes}
                    style:background={nodes.find((n) => n.name.endsWith(`.${item.extension}`))
                      ? '#5f91cb'
                      : `hsl(${(item.extension.length * 37) % 360} 25% 65%)`}
                    title={`${item.extension}: ${bytes(item.bytes)}`}
                  ></span>{/each}
              </div>
              <div class="type-legend">
                {#each types.slice(0, 8) as item}<button
                    onclick={() => {
                      extension = item.extension === 'No extension' ? '' : item.extension;
                      view = 'explorer';
                    }}
                    ><strong>{item.extension}</strong><span>{bytes(item.bytes)}</span><small
                      >{item.count.toLocaleString()} files</small
                    ></button
                  >{/each}
              </div>
            </section>
          {:else if view === 'duplicates'}
            <Duplicates
              {scope}
              enabled={ready && !busy}
              onbusy={(value) => {
                analysisBusy = value;
              }}
              onselect={(id) => {
                void select(id);
              }}
              onissues={(items) => {
                issues = items;
                showIssues = true;
              }}
              onerror={report}
            />
          {:else}<section class="panel explorer-panel">
              <div class="filter-bar">
                <label>Extension<input bind:value={extension} placeholder="All extensions" /></label
                >{#if view === 'large'}<label
                    >Minimum MiB<input type="number" min="0" bind:value={minSize} /></label
                  ><label>Modified before<input type="date" bind:value={before} /></label
                  >{/if}<label
                  >Sort<select bind:value={sort}
                    ><option value="size">Largest first</option><option value="name">Name</option
                    ><option value="modified">Newest first</option></select
                  ></label
                ><button
                  onclick={() => {
                    text = '';
                    extension = '';
                    before = '';
                    minSize = 100;
                  }}>Reset filters</button
                >
              </div>
              <FileTable
                {query}
                {revision}
                selected={details?.entry.id ?? null}
                onselect={(entry) => {
                  void select(entry.id);
                }}
                onopen={(entry) => {
                  void open(entry.id);
                }}
                onerror={report}
              />
            </section>{/if}
          {#if details}<section class="details-panel">
              <div>
                <Icon name={details.entry.kind === 'directory' ? 'folder' : 'file'} />
                <h2>{details.entry.name}</h2>
                <button
                  class="dismiss"
                  title="Close details"
                  onclick={() => {
                    details = null;
                  }}><Icon name="close" /></button
                >
              </div>
              <p class="path">{details.path}</p>
              <dl>
                <div>
                  <dt>Logical size</dt>
                  <dd>{bytes(details.entry.logical)}</dd>
                </div>
                <div>
                  <dt>Unique allocated size</dt>
                  <dd>
                    {details.entry.allocation_known
                      ? bytes(details.entry.allocated)
                      : `${bytes(details.entry.allocated)} · incomplete`}
                  </dd>
                </div>
                <div>
                  <dt>Modified</dt>
                  <dd>{date(details.entry.modified)}</dd>
                </div>
                <div>
                  <dt>Files</dt>
                  <dd>{details.entry.files.toLocaleString()}</dd>
                </div>
              </dl>
              <div class="detail-actions">
                <button onclick={reveal}><Icon name="external" />Show in Explorer</button><button
                  onclick={copy}
                  disabled={!details.path_exact}
                  title={details.path_exact ? 'Copy path' : 'Path contains non-Unicode characters'}
                  ><Icon name="copy" />Copy path</button
                >{#if details.entry.kind === 'directory'}<button
                    onclick={() => {
                      void open(details?.entry.id ?? 0);
                    }}
                    disabled={analysisBusy}><Icon name="folder" />Explore folder</button
                  >{/if}
              </div>
            </section>{/if}
        {/if}
      {/if}
    </main>
    <footer class="app-footer">
      <span>{isTauri() ? 'Voldiq 0.1.0' : 'Browser preview'}</span><span
        >{ready ? `${operation?.entries.toLocaleString()} entries` : ''}</span
      >
    </footer>
  </div>
</div>
{#if showIssues}<div class="dialog-backdrop" role="presentation">
    <div
      role="dialog"
      tabindex="-1"
      aria-modal="true"
      aria-labelledby="issues-title"
      class="issues-dialog"
    >
      <div class="panel-heading">
        <h2 id="issues-title">Scan issues</h2>
        <button
          aria-label="Close scan issues"
          onclick={() => {
            showIssues = false;
          }}><Icon name="close" /></button
        >
      </div>
      {#if issues.length === 1000}<p>First 1,000 issues</p>{/if}
      <div class="issues-list">
        {#each issues as issue}<article>
            <strong>{issue.path}</strong>
            <p>{issue.message}</p>
          </article>{/each}
      </div>
    </div>
  </div>{/if}
