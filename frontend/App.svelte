<script lang="ts">
  import { onMount, tick } from 'svelte';
  import { Channel, invoke, isTauri } from '@tauri-apps/api/core';
  import Icon from './components/Icon.svelte';
  import Treemap from './components/Treemap.svelte';
  import FileTable from './components/FileTable.svelte';
  import Duplicates from './components/Duplicates.svelte';
  import History from './components/History.svelte';
  import DeveloperStorage from './components/DeveloperStorage.svelte';
  import DockerStorage from './components/DockerStorage.svelte';
  import CleanupPreviewDialog from './components/CleanupPreview.svelte';
  import CleanupRules from './components/CleanupRules.svelte';
  import RecycleBin from './components/RecycleBin.svelte';
  import CleanupActivity from './components/CleanupActivity.svelte';
  import { readRecentPaths, rememberPath } from './lib/recent-paths';
  import { overviewQueries } from './lib/queries';
  import { color } from './lib/treemap';
  import { scopedFileQuery } from './lib/file-query';
  import { visibleFileTypes } from './lib/file-types';
  import {
    bytes,
    date,
    type Entry,
    type Details,
    type FileTypes,
    type Issue,
    type Page,
    type Progress,
    type Query,
    type Volume,
    type Snapshot,
    type CleanupPreview,
  } from './lib/types';
  type View =
    | 'overview'
    | 'explorer'
    | 'large'
    | 'duplicates'
    | 'cleanup'
    | 'developer'
    | 'history'
    | 'settings';
  const navigation: { id: View; label: string; icon: string }[] = [
    { id: 'overview', label: 'Overview', icon: 'overview' },
    { id: 'explorer', label: 'Explorer', icon: 'folder' },
    { id: 'large', label: 'Large files', icon: 'large' },
    { id: 'duplicates', label: 'Duplicates', icon: 'duplicate' },
    { id: 'cleanup', label: 'Cleanup', icon: 'trash' },
    { id: 'developer', label: 'Developer storage', icon: 'folder' },
    { id: 'history', label: 'History', icon: 'history' },
  ];
  let view = $state<View>('overview');
  let expanded = $state(true);
  let volumes = $state<Volume[]>([]),
    volumePath = $state(''),
    scanPath = $state('');
  let defaultDrive = $state('');
  let scanMode = $state<'auto' | 'directory'>('auto');
  let scanConcurrency = $state<number | 'auto'>('auto');
  let recentPaths = $state<string[]>([]);
  let operation = $state<Progress | null>(null),
    scanId = $state<number | null>(null);
  let ready = $state(false),
    revision = $state(0),
    scope = $state(0);
  let directory = $state<Details | null>(null),
    details = $state<Details | null>(null);
  let children = $state<Page>({ items: [], total: 0 }),
    types = $state<FileTypes>({ items: [], total_bytes: 0, total_files: 0 });
  const typeDistribution = $derived(visibleFileTypes(types));
  let text = $state(''),
    extension = $state(''),
    withoutExtension = $state(false),
    minSize = $state(100),
    largeKind = $state<'file' | 'directory'>('file'),
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
  let selection = $state(new Set<number>());
  let cleanupPreview = $state<CleanupPreview | null>(null);
  let cleanupNeedsRefresh = false;
  function checkFile(id: number, checked: boolean): void {
    const next = new Set(selection);
    if (checked) {
      if (next.size >= 500) {
        error = 'Select up to 500 files.';
        return;
      }
      next.add(id);
    } else next.delete(id);
    selection = next;
  }
  async function previewSelection(): Promise<void> {
    if (busy || historical || selection.size === 0) return;
    analysisBusy = true;
    try {
      cleanupPreview = await invoke<CleanupPreview>('preview_cleanup', {
        selection: [...selection],
      });
    } catch (problem) {
      report(problem);
    } finally {
      analysisBusy = false;
    }
  }
  let volume = $derived(volumes.find((v) => v.path === volumePath));
  let busy = $derived(operation?.phase === 'scanning' || analysisBusy);
  let query = $derived<Query>(
    scopedFileQuery({
      scope,
      ranked: view === 'large',
      text,
      extension: withoutExtension ? '' : extension,
      without_extension: withoutExtension,
      min_size: view === 'large' ? Math.max(0, minSize) * 1024 * 1024 : 0,
      ...(before ? { modified_before: new Date(before).getTime() / 1000 } : {}),
      files_only: view === 'large' && largeKind === 'file',
      ...(view === 'large' ? { kind: largeKind } : {}),
      sort,
    }),
  );
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
  function invalidateRefresh(): void {
    refreshToken += 1;
    overviewQueries.cancel();
  }
  async function refresh(id = scope): Promise<void> {
    const token = ++refreshToken;
    const overviewRequest = overviewQueries.request({ id });
    try {
      const result = await overviewRequest.result;
      if (token !== refreshToken || result === null) return;
      directory = result.directory;
      children = result.children;
      types = result.types;
      scope = id;
      revision += 1;
    } catch (problem) {
      overviewRequest.cancel();
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
    invalidateRefresh();
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
    selection = new Set();
    cleanupPreview = null;
    invalidateRefresh();
    interactionToken += 1;
    ready = false;
    historical = null;
    directory = null;
    details = null;
    children = { items: [], total: 0 };
    types = { items: [], total_bytes: 0, total_files: 0 };
    error = '';
    scanPath = path;
    scope = 0;
    operation = {
      analyzed_at: null,
      previous: false,
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
        if (update.phase === 'complete' && update.entries > 0 && !update.previous) {
          recentPaths = rememberPath(recentPaths, path);
          savePreferences();
        }
        ready = update.entries > 0;
        scanId = null;
        details = null;
        if (ready) void refresh(scope);
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
          mode: scanMode,
          concurrency: scanConcurrency,
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
  async function clearAnalysis(): Promise<void> {
    if (busy) return;
    analysisBusy = true;
    invalidateRefresh();
    interactionToken += 1;
    try {
      await invoke('clear_current_analysis');
      ready = false;
      historical = null;
      operation = null;
      scanId = null;
      scanPath = '';
      scope = 0;
      directory = details = null;
      children = { items: [], total: 0 };
      types = { items: [], total_bytes: 0, total_files: 0 };
      selection = new Set();
      cleanupPreview = null;
      cleanupNeedsRefresh = false;
      issues = [];
      showIssues = false;
      error = '';
      revision += 1;
    } catch (problem) {
      report(problem);
    } finally {
      analysisBusy = false;
    }
  }
  async function loadSnapshot(snapshot: Snapshot): Promise<void> {
    selection = new Set();
    cleanupPreview = null;
    invalidateRefresh();
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
          defaultDrive = volumePath = value.defaultDrive;
        if ('recentPaths' in value) recentPaths = readRecentPaths(value.recentPaths);
        if ('scanMode' in value && (value.scanMode === 'auto' || value.scanMode === 'directory'))
          scanMode = value.scanMode;
        if (
          'scanConcurrency' in value &&
          (value.scanConcurrency === 'auto' ||
            (typeof value.scanConcurrency === 'number' &&
              Number.isInteger(value.scanConcurrency) &&
              value.scanConcurrency >= 1 &&
              value.scanConcurrency <= 8))
        )
          scanConcurrency = value.scanConcurrency;
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
        if (ready && !analysisBusy && !cleanupPreview && !showIssues) {
          interactionToken += 1;
          invalidateRefresh();
          scope = 0;
          view = 'explorer';
          extension = '';
          withoutExtension = false;
          before = '';
          details = null;
          selection = new Set();
          void refresh(0);
          void tick().then(() => {
            search.focus();
            search.select();
          });
        }
      }
      if (event.key === 'Escape') {
        showIssues = false;
        details = null;
      }
    };
    window.addEventListener('keydown', keyboard);
    return () => {
      invalidateRefresh();
      window.removeEventListener('keydown', keyboard);
    };
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
          defaultDrive,
          scanMode,
          scanConcurrency,
          recentPaths,
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
      {#if recentPaths.length}
        <select
          class="recent-select"
          aria-label="Recently analyzed paths"
          disabled={busy || !isTauri()}
          onchange={(event) => {
            const path = event.currentTarget.value;
            event.currentTarget.value = '';
            void start(path);
          }}
        >
          <option value="">Recent</option>
          {#each recentPaths as path}<option value={path}>{path}</option>{/each}
        </select>
      {/if}
      <div class="search">
        <Icon name="search" /><input
          bind:this={search}
          bind:value={text}
          oninput={(event) => {
            if (view === 'overview' && event.currentTarget.value.trim()) view = 'explorer';
          }}
          placeholder="Search files and folders"
          aria-label="Search files and folders"
          disabled={!ready ||
            view === 'settings' ||
            view === 'duplicates' ||
            view === 'cleanup' ||
            view === 'history' ||
            view === 'developer'}
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
                    : view === 'cleanup'
                      ? 'Cleanup'
                      : view === 'developer'
                        ? 'Developer storage'
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
          <span class:working={operation.phase === 'scanning'} class="status-dot"></span><span
            >{operation.phase === 'scanning'
              ? operation.previous
                ? 'Refreshing · previous results'
                : 'Scanning'
              : operation.phase === 'complete'
                ? 'Scan complete'
                : operation.phase === 'cancelled'
                  ? operation.entries === 0
                    ? 'Cancelled'
                    : operation.previous
                      ? 'Cancelled · previous results'
                      : 'Cancelled · partial results'
                  : 'Scan failed'}</span
          ><span
            >{operation.files.toLocaleString()} files · {bytes(operation.logical)} logical · {(
              operation.elapsed_ms / 1000
            ).toFixed(1)} s</span
          >{#if operation.errors > 0}<button onclick={inspectIssues}
              >{operation.errors} access issues</button
            >{/if}{#if operation.phase === 'scanning'}<button
              onclick={cancel}
              disabled={scanId === null}>Cancel scan</button
            >{/if}{#if operation.analyzed_at !== null}<time
              datetime={new Date(operation.analyzed_at * 1000).toISOString()}
              title="Analysis date">{date(operation.analyzed_at)}</time
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
            >Scan mode<select bind:value={scanMode} onchange={savePreferences} disabled={busy}>
              <option value="auto">Automatic</option><option value="directory"
                >Directory enumeration</option
              >
            </select></label
          >
          <label
            >Concurrency limit<select
              bind:value={scanConcurrency}
              onchange={savePreferences}
              disabled={busy}
            >
              <option value="auto">Automatic</option>
              {#each [1, 2, 3, 4, 5, 6, 7, 8] as workers}<option value={workers}>{workers}</option
                >{/each}
            </select></label
          >
          <label
            >Default drive<select bind:value={defaultDrive} onchange={savePreferences}
              ><option value="">First available</option>{#each volumes as drive}<option
                  value={drive.path}
                  disabled={!drive.accessible}>{drive.path} {drive.name}</option
                >{/each}</select
            ></label
          ><label
            >Excluded paths · one per line<textarea
              bind:value={excluded}
              onchange={savePreferences}
              rows="5"
              placeholder="C:\\folder"></textarea></label
          >
          {#if recentPaths.length}<button
              onclick={() => {
                recentPaths = [];
                savePreferences();
              }}>Clear recent paths</button
            >{/if}
          <h2>Index</h2>
          <button onclick={clearAnalysis} disabled={busy || !isTauri() || !scanPath}
            >Clear current analysis</button
          >
        </section>
      {:else if view === 'developer'}
        <DeveloperStorage enabled={ready && !busy} {revision} onselect={select} onerror={report} />
        <DockerStorage />
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
        {#if !ready && view !== 'duplicates' && view !== 'cleanup'}<section class="welcome">
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
                {#each typeDistribution.items as item}<span
                    style:flex-grow={item.bytes}
                    style:background={color(item.extension ?? '<no-extension>')}
                    title={`${item.extension === null ? 'No extension' : `.${item.extension}`}: ${bytes(item.bytes)}`}
                  ></span>{/each}
                {#if typeDistribution.otherBytes > 0}<span
                    style:flex-grow={typeDistribution.otherBytes}
                    style:background="var(--muted)"
                    title={`Other: ${bytes(typeDistribution.otherBytes)}`}
                  ></span>{/if}
              </div>
              <div class="type-legend">
                {#each typeDistribution.items as item}<button
                    onclick={() => {
                      withoutExtension = item.extension === null;
                      extension = item.extension ?? '';
                      view = 'explorer';
                    }}
                    ><strong
                      >{item.extension === null ? 'No extension' : `.${item.extension}`}</strong
                    ><span>{bytes(item.bytes)}</span><small
                      >{item.count.toLocaleString()} files</small
                    ></button
                  >{/each}
                {#if typeDistribution.otherFiles > 0}<div class="type-other">
                    <strong>Other</strong><span>{bytes(typeDistribution.otherBytes)}</span>
                    <small>{typeDistribution.otherFiles.toLocaleString()} files</small>
                  </div>{/if}
              </div>
            </section>
          {:else if view === 'cleanup'}
            <button disabled={busy} onclick={async () => {
              try { await invoke('open_windows_storage'); }
              catch (problem) { report(problem); }
            }}>Windows storage</button>
            <RecycleBin drive={volumePath} />
            <CleanupRules
              scanning={operation?.phase === 'scanning'}
              {revision}
              {busy}
              manageable={ready && !historical}
              onscan={start}
              onbusy={(value) => {
                analysisBusy = value;
              }}
              onpreview={(plan) => {
                cleanupPreview = plan;
              }}
              onerror={report}
            />
            <CleanupActivity {revision} {busy} />
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
              onpreview={(plan) => {
                cleanupPreview = plan;
              }}
            />
          {:else}<section class="panel explorer-panel">
              <div class="filter-bar">
                <label
                  >Extension<input
                    bind:value={extension}
                    placeholder="All extensions"
                    disabled={withoutExtension}
                  /></label
                >
                <label class="extensionless-filter"
                  ><input type="checkbox" bind:checked={withoutExtension} />No extension</label
                >{#if view === 'large'}<label
                    >Items<select bind:value={largeKind}
                      ><option value="file">Files</option><option value="directory">Folders</option
                      ></select
                    ></label
                  ><label>Minimum MiB<input type="number" min="0" bind:value={minSize} /></label
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
                    withoutExtension = false;
                    before = '';
                    minSize = 100;
                    largeKind = 'file';
                  }}>Reset filters</button
                >
              </div>
              <FileTable
                {query}
                {revision}
                selected={details?.entry.id ?? null}
                checked={selection}
                manageable={!busy && historical === null}
                oncheck={checkFile}
                onselect={(entry) => {
                  void select(entry.id);
                }}
                onopen={(entry) => {
                  void open(entry.id);
                }}
                onerror={report}
              />
              {#if selection.size > 0}<div class="selection-bar">
                  <span>{selection.size} selected</span><button
                    disabled={busy || historical !== null}
                    onclick={previewSelection}>Review selection</button
                  ><button
                    disabled={busy}
                    onclick={() => {
                      selection = new Set();
                    }}>Clear</button
                  >
                </div>{/if}
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

{#if cleanupPreview}{#key cleanupPreview.id}<CleanupPreviewDialog
      plan={cleanupPreview}
      onclose={() => {
        cleanupPreview = null;
        if (cleanupNeedsRefresh) {
          cleanupNeedsRefresh = false;
          void start(scanPath);
        }
      }}
      onbusy={(value) => {
        analysisBusy = value;
      }}
      onchanged={() => {
        cleanupNeedsRefresh = true;
        invalidateRefresh();
        interactionToken += 1;
        ready = false;
        operation = null;
        selection = new Set();
        details = null;
        directory = null;
        children = { items: [], total: 0 };
        types = { items: [], total_bytes: 0, total_files: 0 };
      }}
      onerror={report}
    />{/key}{/if}
