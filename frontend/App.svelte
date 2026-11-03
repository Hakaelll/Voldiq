<script lang="ts">
  import { invokeIndexed, setIndexContext, type IndexId } from './lib/index-context';
  import { onMount, tick } from 'svelte';
  import { Channel, invoke, isTauri } from '@tauri-apps/api/core';
  import { listen } from '@tauri-apps/api/event';
  import Icon from './components/Icon.svelte';
  import Overview from './components/Overview.svelte';
  import StorageMap from './components/StorageMap.svelte';
  import ScanWelcome from './components/ScanWelcome.svelte';
  import FileTable from './components/FileTable.svelte';
  import AdvancedSearch from './components/AdvancedSearch.svelte';
  import SavedWorkspaces from './components/SavedWorkspaces.svelte';
  import CommandPalette from './components/CommandPalette.svelte';
  import {
    detachedQuery,
    restoreWorkspace,
    workspaceContextIsCurrent,
    type CatalogState,
    type ViewSpec,
    type Workspaces,
    type RecordRef,
    type SavedView,
    type Destination,
  } from './lib/workbench';
  import { paletteCommands, type PaletteCommand } from './lib/command-palette';
  import type { SearchDraftRequest } from './lib/search-editor';
  import CloudFilterSelect from './components/CloudFilterSelect.svelte';
  import FolderComposition from './components/FolderComposition.svelte';
  import Duplicates from './components/Duplicates.svelte';
  import History from './components/History.svelte';
  import DeveloperStorage from './components/DeveloperStorage.svelte';
  import CleanupPreviewDialog from './components/CleanupPreview.svelte';
  import CleanupRules from './components/CleanupRules.svelte';
  import RecycleBin from './components/RecycleBin.svelte';
  import CleanupActivity from './components/CleanupActivity.svelte';
  import RestorationDialog from './components/RestorationDialog.svelte';
  import AgentConnections from './components/AgentConnections.svelte';
  import AgentPlanReview from './components/AgentPlanReview.svelte';
  import FolderNotes from './components/FolderNotes.svelte';
  import FolderBudget from './components/FolderBudget.svelte';
  import LiveControl from './components/LiveControl.svelte';
  import { volumeMeasurement, volumeCaller } from './lib/volume-space';
  import type { LiveObservation } from './lib/live-session';
  import ProtectedFolders from './components/ProtectedFolders.svelte';
  import { readRecentPaths, rememberPath } from './lib/recent-paths';
  import { overviewQueries, detailQueries } from './lib/queries';
  import { createNavigationRefresh } from './lib/navigation-refresh';
  import { scopedFileQuery } from './lib/file-query';
  import { storageNodes } from './lib/storage-nodes';
  import { cloudLabel, entryKindLabel } from './lib/cloud';
  import { fileSizeLabel } from './lib/file-size';
  import type { MapCommand } from './lib/map-menu';
  import {
    emptyExploration,
    explorationIds,
    updateExploration,
    type Exploration,
    type SelectionOperation,
  } from './lib/exploration';
  import { categoryPaint, categoryLabel, visualPaint, type ColorMode } from './lib/storage-map';
  import {
    bytes,
    date,
    type Entry,
    type Details,
    type SelectedDetails,
    type FileTypes,
    type Overview as OverviewData,
    type Issue,
    type Page,
    type Progress,
    type Query,
    type CloudFilter,
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
    { id: 'developer', label: 'Developer storage', icon: 'code' },
    { id: 'history', label: 'History', icon: 'history' },
  ];
  let view = $state<View>('overview');
  const indexedView = $derived(view === 'overview' || view === 'explorer' || view === 'large');
  $effect(() => {
    if (!indexedView) advancedOpen = false;
  });
  let navigationHovered = $state(false);
  let navigationKeyboardFocus = $state(false);
  let expanded = $derived(navigationHovered || navigationKeyboardFocus);
  let volumes = $state<Volume[]>([]),
    volumePath = $state(''),
    scanPath = $state('');
  let defaultDrive = $state('');
  let scanMode = $state<'auto' | 'directory'>('auto');
  let scanConcurrency = $state<number | 'auto'>('auto');
  let recentPaths = $state<string[]>([]);
  let operation = $state<Progress | null>(null),
    scanId = $state<number | null>(null);
  let indexId = $state<IndexId | null>(setIndexContext(null));
  let ready = $state(false),
    revision = $state(0),
    scope = $state(0);
  let directory = $state<Details | null>(null),
    details = $state<SelectedDetails | null>(null);
  let fileWorkspace = $state<HTMLDivElement>();
  let detailsPanel = $state<HTMLElement>();
  let children = $state<Page>({ items: [], total: 0 }),
    types = $state<FileTypes>({ items: [], total_bytes: 0, total_files: 0 });
  let branches = $state<OverviewData['branches']>([]);
  let text = $state(''),
    extension = $state(''),
    withoutExtension = $state(false),
    minSize = $state(100),
    largeKind = $state<'file' | 'directory'>('file'),
    before = $state(''),
    sort = $state<Exclude<NonNullable<Query['sort']>, 'allocated'>>('size');
  let cloudFilter = $state<CloudFilter | null>(null);
  let searchExpression = $state('');
  let advancedOpen = $state(false);
  let advancedButton: HTMLButtonElement;
  let savedButton: HTMLButtonElement;
  let savedOpen = $state(false),
    paletteOpen = $state(false),
    savedLoading = $state(false);
  let catalog = $state.raw<CatalogState>({ kind: 'loading' });
  let catalogEpoch = 0,
    workspaceLoadToken = 0;
  let paletteOpener: HTMLElement | null = null;
  let appliedSearchKey = $state<string | null>(null);
  let searchProblem = $state<{ key: string; message: string } | null>(null);
  let pendingSearchKey: string | null = null;
  let mapCloud = $state<CloudFilter | null>(null);
  let cloudProblem = $state('');
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
  let scanRequest = 0;
  let mapNavigation = $state(false);
  let mapMode = $state<import('./lib/storage-map').StorageView>('treemap');
  let mapDepth = $state<import('./lib/storage-map').StorageDepth>(6);
  let mapColorMode = $state<ColorMode>('type');
  let explorerMap = $state(false);
  let fileRepresentation = $state<'table' | 'grid'>('table');
  let mapMetrics = $state<OverviewData['map_metrics']>([]);
  let showMapList = $state(false);
  const navigationRefresh = createNavigationRefresh();
  let analysisBusy = $state(false);
  let liveBusy = $state(false);
  let liveObservation = $state<{ entries: number; measured_at: number | null } | null>(null);
  const entryCount = $derived(liveObservation?.entries ?? operation?.entries ?? null);
  let agentOperation = $state<number | null>(null);
  let agentNotice = $state('');
  let historical = $state<Snapshot | null>(null);
  let exploration = $state.raw<Exploration>(emptyExploration(null));
  const explored = $derived(explorationIds(exploration, indexId));
  let selection = $state(new Set<number>());
  let cleanupPreview = $state<CleanupPreview | null>(null);
  let cleanupNeedsRefresh = false;
  let restoreReceipt = $state<number | null>(null);
  let restoreNeedsRefresh = false;
  async function loadWorkspaces(): Promise<void> {
    const owner = ++catalogEpoch;
    catalog = { kind: 'loading' };
    if (!isTauri()) {
      catalog = { kind: 'failed', message: 'Saved workspaces are available in the desktop app.' };
      return;
    }
    try {
      const value = await invoke<Workspaces>('list_workspaces');
      if (owner === catalogEpoch) catalog = { kind: 'ready', value };
    } catch (problem) {
      if (owner === catalogEpoch) catalog = { kind: 'failed', message: String(problem) };
    }
  }
  function adoptWorkspaces(value: Workspaces): void {
    catalogEpoch += 1;
    catalog = { kind: 'ready', value };
  }
  function workspaceContext() {
    return {
      indexId,
      scope,
      query,
      owner: interactionToken,
      revision: catalog.kind === 'ready' ? catalog.value.revision : null,
    };
  }
  async function loadWorkspace(reference: RecordRef): Promise<void> {
    if (
      !ready ||
      indexId === null ||
      analysisBusy ||
      savedLoading ||
      catalog.kind !== 'ready' ||
      reference.revision !== catalog.value.revision
    )
      return;
    const request = workspaceContext(),
      token = ++workspaceLoadToken;
    savedLoading = true;
    try {
      const item = await invoke<SavedView>('load_workspace', { reference });
      if (
        token !== workspaceLoadToken ||
        !ready ||
        analysisBusy ||
        !workspaceContextIsCurrent(request, workspaceContext())
      )
        return;
      const next = restoreWorkspace(item.spec, scope);
      invalidateRefresh();
      const owner = invalidateInteraction();
      view = next.view;
      scope = next.scope;
      text = next.text;
      searchExpression = next.expression;
      extension = next.extension;
      withoutExtension = next.withoutExtension;
      cloudFilter = next.cloud;
      minSize = next.minSize;
      largeKind = next.largeKind;
      before = next.before;
      sort = next.sort;
      sizeMode = next.metric;
      mapDepth = next.depth;
      mapColorMode = next.color;
      if (next.mapMode !== null) mapMode = next.mapMode;
      explorerMap = next.explorerMap;
      fileRepresentation = next.fileRepresentation;
      advancedOpen = false;
      savedOpen = false;
      details = null;
      showMapList = false;
      void refresh(scope);
      await tick();
      if (
        owner === interactionToken &&
        request.indexId === indexId &&
        document.activeElement === document.body
      )
        fileWorkspace?.focus({ preventScroll: true });
    } catch (problem) {
      if (token === workspaceLoadToken && workspaceContextIsCurrent(request, workspaceContext())) {
        report(problem);
        void loadWorkspaces();
      }
    } finally {
      if (token === workspaceLoadToken) savedLoading = false;
    }
  }
  function openSaved(): void {
    savedOpen = !savedOpen;
    if (savedOpen) {
      advancedOpen = false;
      void loadWorkspaces();
    }
  }
  function closeSaved(): void {
    savedOpen = false;
    const owner = interactionToken;
    void tick().then(() => {
      if (owner === interactionToken && document.activeElement === document.body)
        savedButton?.focus({ preventScroll: true });
    });
  }
  function closePalette(restore = true): void {
    paletteOpen = false;
    const owner = interactionToken,
      opener = paletteOpener;
    if (restore)
      void tick().then(() => {
        if (
          owner === interactionToken &&
          opener?.isConnected &&
          document.activeElement === document.body
        )
          opener.focus({ preventScroll: true });
      });
  }
  function openPalette(): void {
    if (analysisBusy || cleanupPreview || restoreReceipt !== null || showIssues) return;
    if (paletteOpen) {
      closePalette();
      return;
    }
    if (document.querySelector('dialog[open], [role="dialog"][aria-modal="true"]')) return;
    paletteOpener = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    paletteOpen = true;
    void loadWorkspaces();
  }
  function navigate(next: Destination): void {
    if (analysisBusy) return;
    invalidateInteraction();
    invalidateRefresh();
    view = next;
    text = '';
    if (ready && (next === 'overview' || next === 'explorer' || next === 'large'))
      void refresh(scope);
  }
  async function focusFileSearch(): Promise<void> {
    if (!ready || indexId === null || analysisBusy) return;
    invalidateRefresh();
    const owner = invalidateInteraction(),
      expected = indexId;
    scope = 0;
    view = 'explorer';
    details = null;
    void refresh(0);
    await tick();
    if (owner === interactionToken && expected === indexId) {
      search.focus();
      search.select();
    }
  }
  function runCommand(command: PaletteCommand): void {
    if (!paletteOpen || !commands.includes(command) || command.disabled || analysisBusy) return;
    closePalette(false);
    const action = command.action;
    switch (action.kind) {
      case 'navigate':
        navigate(action.view);
        return;
      case 'search':
        void focusFileSearch();
        return;
      case 'choose':
        void choose();
        return;
      case 'saved':
        savedOpen = true;
        advancedOpen = false;
        return;
      case 'load':
        void loadWorkspace(action.reference);
        return;
      case 'scan':
        savedOpen = false;
        void start(action.path, action.reference);
        return;
      default: {
        const unreachable: never = action;
        return unreachable;
      }
    }
  }
  function explore(ids: readonly number[], operation: SelectionOperation): boolean {
    const result = updateExploration(exploration, indexId, ids, operation);
    if (result.kind !== 'accepted') {
      error =
        result.kind === 'limit'
          ? 'Select up to 500 items for inspection.'
          : 'This item is unavailable for inspection. Refresh the analysis.';
      return false;
    }
    exploration = result.state;
    return true;
  }
  function clearExploration(): void {
    invalidateInteraction();
    exploration = emptyExploration(indexId);
    details = null;
  }
  function selectMany(
    ids: readonly number[],
    operation: SelectionOperation,
    expectedIndex: IndexId | null,
  ): boolean {
    if (expectedIndex !== indexId) return false;
    invalidateInteraction();
    if (!explore(ids, operation)) return false;
    details = null;
    return true;
  }
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
      cleanupPreview = await invokeIndexed<CleanupPreview>('preview_cleanup', {
        selection: [...selection],
      });
    } catch (problem) {
      report(problem);
    } finally {
      analysisBusy = false;
    }
  }
  let volume = $derived(volumes.find((v) => v.path === volumePath));
  const volumeSpace = $derived(volumeMeasurement(volume));
  const volumeAvailability = $derived(volumeCaller(volume));
  let busy = $derived(
    operation?.phase === 'scanning' ||
      liveBusy ||
      analysisBusy ||
      agentOperation !== null ||
      restoreReceipt !== null,
  );
  let query = $derived<Query>(
    scopedFileQuery({
      scope,
      ranked: view === 'large',
      text,
      expression: searchExpression,
      ...(cloudFilter ? { cloud: cloudFilter } : {}),
      extension: withoutExtension ? '' : extension,
      without_extension: withoutExtension,
      min_size: view === 'large' ? Math.max(0, minSize) * 1024 * 1024 : 0,
      ...(before ? { modified_before: new Date(before).getTime() / 1000 } : {}),
      files_only: view === 'large' && largeKind === 'file',
      ...(view === 'large' ? { kind: largeKind } : {}),
      sort: sort === 'size' && view === 'explorer' && sizeMode === 'allocated' ? 'allocated' : sort,
    }),
  );
  const workspaceCapture = $derived<ViewSpec | null>(
    ready && indexedView
      ? {
          view: view === 'overview' ? 'overview' : view === 'large' ? 'large' : 'explorer',
          query: detachedQuery(query),
          scope: 'current_folder',
          representation:
            view === 'overview'
              ? mapMode
              : view === 'large'
                ? 'table'
                : explorerMap
                  ? mapMode
                  : fileRepresentation,
          metric: sizeMode === 'allocated' ? 'allocated' : 'logical',
          depth: mapDepth,
          color: mapColorMode,
        }
      : null,
  );
  const commands = $derived(
    paletteCommands(
      catalog.kind === 'ready' ? catalog.value : null,
      ready && !savedLoading,
      busy || !isTauri(),
    ),
  );
  const mapSearch = $derived<Query | null>(
    indexedView && (text || extension || withoutExtension || searchExpression.trim())
      ? {
          text,
          expression: searchExpression,
          extension: withoutExtension ? '' : extension,
          without_extension: withoutExtension,
          ...(cloudFilter ? { cloud: cloudFilter } : {}),
        }
      : null,
  );
  const mapSearchKey = $derived(JSON.stringify(mapSearch));
  const matchProblem = $derived(searchProblem?.key === mapSearchKey ? searchProblem.message : '');
  const searchState = $derived<'inactive' | 'loading' | 'ready' | 'failed'>(
    !mapSearch
      ? 'inactive'
      : matchProblem
        ? 'failed'
        : appliedSearchKey !== mapSearchKey
          ? 'loading'
          : !mapMetrics.length || mapMetrics.some((metric) => !metric.matches)
            ? 'failed'
            : 'ready',
  );
  const searchTotal = $derived.by(() => {
    const root =
      directory && searchState === 'ready'
        ? mapMetrics.find((metric) => metric.id === directory?.entry.id)?.matches
        : null;
    return root ? Number(root.own) + root.descendants : null;
  });
  $effect(() => {
    const key = mapSearchKey;
    const active = ready && indexedView;
    const analysis = indexId;
    const timer = setTimeout(() => {
      if (
        active &&
        ready &&
        analysis === indexId &&
        key === mapSearchKey &&
        appliedSearchKey !== key &&
        pendingSearchKey !== key &&
        navigationRefresh.request()
      )
        void refresh(scope);
    }, 150);
    return () => clearTimeout(timer);
  });
  const nodeProjection = $derived(
    storageNodes({
      directory: directory?.entry ?? null,
      children,
      branches,
      metrics: mapMetrics,
      allocated: sizeMode === 'allocated',
      filtered: cloudFilter !== null,
    }),
  );
  const mapQueryState = $derived(
    mapCloud !== cloudFilter
      ? cloudProblem
        ? 'failed'
        : 'loading'
      : nodeProjection === null
        ? 'failed'
        : 'ready',
  );
  const nodes = $derived(mapQueryState === 'ready' ? (nodeProjection ?? []) : []);
  function report(problem: unknown): void {
    error = problem instanceof Error ? problem.message : String(problem);
  }
  function entryPaint(entry: Entry): { fill: string; ink: string } {
    const metric = mapMetrics.find((m) => m.id === entry.id);
    return visualPaint(
      {
        id: entry.id,
        name: entry.name,
        value: sizeMode === 'allocated' ? entry.allocated : entry.logical,
        directory: entry.kind === 'directory',
        modified: entry.kind === 'directory' ? (metric?.latest_modified ?? null) : entry.modified,
      },
      mapColorMode,
      Math.max(1, ...nodes.map((node) => node.value)),
      Date.now() / 1000,
    );
  }
  function invalidateRefresh(): void {
    refreshToken += 1;
    pendingSearchKey = null;
    overviewQueries.cancel();
  }
  function invalidateInteraction(): number {
    mapNavigation = false;
    navigationRefresh.reset();
    detailQueries.cancel();
    return ++interactionToken;
  }
  function adoptIndex(value: number | null): void {
    const next = setIndexContext(value);
    if (next === indexId) return;
    indexId = next;
    liveObservation = null;
    invalidateInteraction();
    invalidateRefresh();
    scope = 0;
    exploration = emptyExploration(next);
    selection = new Set();
    directory = null;
    details = null;
    children = { items: [], total: 0 };
    types = { items: [], total_bytes: 0, total_files: 0 };
    branches = [];
    advancedOpen = false;
    savedOpen = false;
    paletteOpen = false;
    appliedSearchKey = null;
    pendingSearchKey = null;
    searchProblem = null;
    mapCloud = null;
    cloudProblem = '';
    cleanupPreview = null;
    cleanupNeedsRefresh = false;
    issues = [];
    showIssues = false;
    error = '';
    revision += 1;
  }
  async function refresh(id = scope, navigationOwner: number | null = null): Promise<void> {
    const token = ++refreshToken;
    const requestedCloud = cloudFilter;
    const requestedSearchKey = mapSearchKey;
    const requestedSearch = mapSearch;
    pendingSearchKey = requestedSearchKey;
    if (searchProblem?.key === requestedSearchKey) searchProblem = null;
    cloudProblem = '';
    const overviewRequest = overviewQueries.request({
      id,
      indexId,
      cloud: requestedCloud,
      search: requestedSearch,
    });
    try {
      const result = await overviewRequest.result;
      if (
        token !== refreshToken ||
        (navigationOwner !== null && navigationOwner !== interactionToken)
      )
        return;
      if (result === null || result.cloud !== requestedCloud) {
        if (mapCloud !== cloudFilter) cloudProblem = 'Map query was cancelled or superseded.';
        if (requestedSearch && requestedSearchKey === mapSearchKey)
          searchProblem = { key: requestedSearchKey, message: 'Search matches were cancelled.' };
        return;
      }
      directory = result.directory;
      children = result.children;
      branches = result.branches ?? [];
      mapMetrics = result.map_metrics ?? [];
      types = result.types;
      mapCloud = result.cloud;
      appliedSearchKey = requestedSearchKey;
      scope = id;
      revision += 1;
    } catch (problem) {
      overviewRequest.cancel();
      if (
        token === refreshToken &&
        (navigationOwner === null || navigationOwner === interactionToken)
      ) {
        if (mapCloud !== cloudFilter) cloudProblem = 'Map query failed.';
        if (requestedSearchKey === mapSearchKey) {
          if (requestedSearch)
            searchProblem = { key: requestedSearchKey, message: String(problem) };
          report(problem);
        }
      }
    } finally {
      if (token === refreshToken) pendingSearchKey = null;
    }
  }
  async function observeLive(event: LiveObservation): Promise<void> {
    if (indexId !== event.before_index) throw new Error('Live analysis changed. Scan again.');
    const restoreFocus =
      fileWorkspace?.contains(document.activeElement) ||
      detailsPanel?.contains(document.activeElement);
    const previousScope = scope;
    scanRequest += 1;
    adoptIndex(event.index_id);
    const owner = interactionToken;
    operation = null;
    scanId = null;
    historical = null;
    ready = true;
    liveObservation = { entries: event.entries, measured_at: event.report.after.measured_at };
    let nextScope = 0;
    if (previousScope !== 0) {
      try {
        nextScope = await invoke<number>('live_directory', {
          id: event.id,
          beforeIndex: event.before_index,
          indexId: event.index_id,
          scope: previousScope,
        });
      } catch {
        /* A stopped session no longer retains its previous folder context. */
      }
    }
    if (indexId !== event.index_id || !ready) return;
    await refresh(owner === interactionToken ? nextScope : scope);
    await tick();
    if (
      restoreFocus &&
      indexId === event.index_id &&
      owner === interactionToken &&
      document.activeElement === document.body
    )
      fileWorkspace?.focus({ preventScroll: true });
  }
  function closeAdvanced(restore: boolean): void {
    const owner = interactionToken;
    advancedOpen = false;
    if (restore)
      void tick().then(() => {
        if (owner === interactionToken && document.activeElement === document.body)
          advancedButton?.focus({ preventScroll: true });
      });
  }
  function applyAdvanced(request: SearchDraftRequest): void {
    if (
      !ready ||
      !advancedOpen ||
      !indexedView ||
      indexId === null ||
      request.indexId !== indexId ||
      request.query !== query
    )
      return;
    invalidateRefresh();
    const owner = invalidateInteraction();
    searchExpression = request.expression;
    advancedOpen = false;
    void refresh(scope);
    void tick().then(() => {
      if (
        owner === interactionToken &&
        request.indexId === indexId &&
        indexedView &&
        document.activeElement === document.body
      )
        fileWorkspace?.focus({ preventScroll: true });
    });
  }
  function changeCloudFilter(next: CloudFilter | null): void {
    if (next === cloudFilter) return;
    cloudFilter = next;
    invalidateInteraction();
    invalidateRefresh();
    cloudProblem = '';
    if (ready) void refresh(scope);
  }
  async function select(id: number, operation: SelectionOperation = 'replace'): Promise<void> {
    const token = invalidateInteraction();
    if (id === -1) {
      view = 'explorer';
      return;
    }
    if (!explore([id], operation)) return;
    if (!explored.has(id)) {
      if (details?.entry.id === id) details = null;
      return;
    }
    if (operation === 'replace' && details?.entry.id !== id) details = null;
    try {
      const item = await detailQueries.request({ id, indexId }).result;
      if (item && token === interactionToken && ready) details = item;
    } catch (problem) {
      if (token === interactionToken) report(problem);
    }
  }
  async function activateChild(child: Entry, button: HTMLButtonElement): Promise<void> {
    let restore = document.activeElement === button;
    const token = interactionToken + 1;
    const moved = (event: FocusEvent): void => {
      if (event.target !== button && event.target !== document.body) restore = false;
    };
    document.addEventListener('focusin', moved);
    try {
      if (child.kind === 'directory') await open(child.id);
      else await select(child.id);
      await tick();
      if (
        restore &&
        token === interactionToken &&
        !button.isConnected &&
        document.activeElement === document.body
      ) {
        if (details) detailsPanel?.focus();
        else fileWorkspace?.focus();
      }
    } finally {
      document.removeEventListener('focusin', moved);
    }
  }
  async function open(id: number, pointerNavigation = false): Promise<void> {
    const token = invalidateInteraction();
    invalidateRefresh();
    if (id === -1) {
      view = 'explorer';
      return;
    }
    navigationRefresh.begin(token);
    mapNavigation = pointerNavigation;
    try {
      const item = await detailQueries.request({ id, indexId }).result;
      if (!item || token !== interactionToken || !ready) return;
      if (item.entry.kind === 'directory') {
        text = '';
        details = null;
        await refresh(id, token);
      } else {
        if (!explored.has(id)) explore([id], 'replace');
        details = item;
      }
    } catch (problem) {
      if (token === interactionToken) report(problem);
    } finally {
      if (navigationRefresh.finish(token) && ready) void refresh(scope);
    }
  }
  async function start(path: string, favorite?: RecordRef): Promise<void> {
    if (!path || busy) return;
    const previous = { ready, indexId, scope, historical, selection, operation, scanPath, details };
    const owner = ++scanRequest;
    scanId = null;
    selection = new Set();
    cleanupPreview = null;
    invalidateRefresh();
    invalidateInteraction();
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
      index_id: indexId,
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
      if (owner !== scanRequest) return;
      adoptIndex(update.index_id);
      operation = update;
      if (update.phase === 'scanning') {
        ready = true;
        if (update.elapsed_ms === 0 || update.elapsed_ms - lastRefresh >= 1000) {
          lastRefresh = update.elapsed_ms;
          if (navigationRefresh.request()) void refresh(scope);
        }
      }
      if (update.phase === 'failed') {
        invalidateInteraction();
        details = null;
        error = update.method;
        scanId = null;
      } else if (update.phase === 'complete' || update.phase === 'cancelled') {
        if (update.phase === 'complete' && update.entries > 0 && !update.previous) {
          recentPaths = rememberPath(recentPaths, path);
          savePreferences();
        }
        ready = update.entries > 0;
        scanId = null;
        invalidateInteraction();
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
      const id = await invoke<number>(favorite ? 'start_favorite_scan' : 'start_scan', {
        ...(favorite ? { reference: favorite } : { path }),
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
      if (owner === scanRequest && operation?.phase === 'scanning') scanId = id;
    } catch (problem) {
      if (owner !== scanRequest) return;
      operation = null;
      if (indexId === previous.indexId) {
        ready = previous.ready;
        scope = previous.scope;
        historical = previous.historical;
        selection = previous.selection;
        scanPath = previous.scanPath;
        details = previous.details;
        operation = previous.operation;
        if (ready) void refresh(scope);
      }
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
    invalidateInteraction();
    try {
      await invoke('clear_current_analysis');
      scanRequest += 1;
      adoptIndex(null);
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
  async function loadSnapshot(snapshot: Snapshot, loadedIndex: number): Promise<void> {
    scanRequest += 1;
    adoptIndex(loadedIndex);
    selection = new Set();
    cleanupPreview = null;
    invalidateRefresh();
    invalidateInteraction();
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
  async function reveal(id: number): Promise<void> {
    try {
      await invokeIndexed('reveal_item', { id });
    } catch (problem) {
      report(problem);
    }
  }
  async function copy(id: number): Promise<void> {
    const expectedIndex = indexId,
      token = invalidateInteraction();
    try {
      const item =
        details?.entry.id === id ? details : await detailQueries.request({ id, indexId }).result;
      if (!item || token !== interactionToken || expectedIndex !== indexId || !ready) return;
      if (!item.path_exact) {
        error = 'This path contains non-Unicode characters and cannot be copied as text.';
        return;
      }
      await navigator.clipboard.writeText(item.path);
    } catch (problem) {
      if (token === interactionToken && expectedIndex === indexId) report(problem);
    }
  }
  async function showFileList(): Promise<void> {
    const token = invalidateInteraction();
    explorerMap = false;
    fileRepresentation = 'table';
    view = 'explorer';
    await tick();
    if (
      token === interactionToken &&
      view === 'explorer' &&
      !explorerMap &&
      fileRepresentation === 'table'
    )
      fileWorkspace?.focus();
  }
  async function mapCommand(command: MapCommand, expectedIndex: IndexId): Promise<void> {
    if (expectedIndex !== indexId || !ready) return;
    switch (command.kind) {
      case 'inspect':
        await select(command.id);
        return;
      case 'open':
        await open(command.id, true);
        return;
      case 'reveal':
        await reveal(command.id);
        return;
      case 'copy':
        await copy(command.id);
        return;
      case 'list':
        await showFileList();
        return;
      default: {
        const unreachable: never = command;
        return unreachable;
      }
    }
  }
  async function inspectIssues(): Promise<void> {
    try {
      issues = await invokeIndexed<Issue[]>('get_issues');
      showIssues = true;
    } catch (problem) {
      report(problem);
    }
  }
  onMount(() => {
    let mounted = true;
    void loadWorkspaces();
    const agentEvents = isTauri()
      ? listen<unknown>('agent-operation', ({ payload }) => {
          if (
            !mounted ||
            !payload ||
            typeof payload !== 'object' ||
            !('id' in payload) ||
            typeof payload.id !== 'number' ||
            !Number.isSafeInteger(payload.id) ||
            payload.id <= 0 ||
            !('phase' in payload)
          )
            return;
          if (payload.phase === 'started') {
            agentOperation = payload.id;
            agentNotice =
              'An approved agent operation is running. Review its plan for actual outcomes.';
            if ('index_id' in payload && payload.index_id === indexId && indexId !== null) {
              scanRequest += 1;
              adoptIndex(null);
              ready = false;
              historical = null;
              operation = null;
              scanId = null;
            }
          } else if (payload.phase === 'finished' && agentOperation === payload.id) {
            agentOperation = null;
            agentNotice =
              'Agent operation finished running. Review its result and scan again before using file measurements.';
          }
        }).catch(report)
      : null;
    try {
      const value: unknown = JSON.parse(localStorage.getItem('voldiq-preferences') ?? '{}');
      if (value && typeof value === 'object') {
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
      if (
        !event.defaultPrevented &&
        !event.repeat &&
        event.ctrlKey &&
        event.key.toLowerCase() === 'k'
      ) {
        event.preventDefault();
        openPalette();
      }
      if (event.key === 'Escape') {
        if (savedOpen) {
          event.preventDefault();
          closeSaved();
          return;
        }
        invalidateInteraction();
        showIssues = false;
        details = null;
        if (view === 'overview' || view === 'explorer' || view === 'large')
          exploration = emptyExploration(indexId);
      }
    };
    window.addEventListener('keydown', keyboard);
    return () => {
      mounted = false;
      catalogEpoch += 1;
      workspaceLoadToken += 1;
      if (agentEvents) void agentEvents.then((unlisten) => unlisten?.());
      invalidateInteraction();
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
  <aside
    class="navigation"
    onpointerenter={(event) => {
      if (event.pointerType !== 'touch') navigationHovered = true;
    }}
    onpointerleave={(event) => {
      navigationHovered = false;
      const active = document.activeElement;
      navigationKeyboardFocus =
        active instanceof HTMLElement &&
        event.currentTarget.contains(active) &&
        active.matches(':focus-visible');
    }}
    onfocusin={(event) => {
      navigationKeyboardFocus =
        event.target instanceof HTMLElement && event.target.matches(':focus-visible');
    }}
    onfocusout={(event) => {
      if (
        !(event.relatedTarget instanceof Node) ||
        !event.currentTarget.contains(event.relatedTarget)
      )
        navigationKeyboardFocus = false;
    }}
  >
    <div class="brand">
      <img src="/symbol.png" alt="" width="32" height="32" />{#if expanded}<strong>Voldiq</strong
        >{/if}
    </div>
    <nav aria-label="Main navigation">
      {#each navigation as item}<button
          onpointerdown={() => {
            navigationKeyboardFocus = false;
          }}
          onkeydown={() => {
            navigationKeyboardFocus = true;
          }}
          class:active={view === item.id}
          title={item.label}
          disabled={analysisBusy}
          aria-label={item.label}
          aria-current={view === item.id ? 'page' : undefined}
          onclick={() => {
            navigate(item.id);
          }}
          ><Icon name={item.icon} size={20} />{#if expanded}<span>{item.label}</span>{/if}</button
        >{/each}
    </nav>
    <div class="nav-bottom">
      {#if expanded && volume?.accessible}<div class="drive-summary">
          <span><Icon name="disk" />{volume.name || volume.path}</span
          >{#if volumeSpace && Number(volumeSpace.total) > 0}<progress
              aria-label="Drive space used"
              value={Number(volumeSpace.used)}
              max={Number(volumeSpace.total)}
            ></progress><small
              >{bytes(Number(volumeSpace.used))} of {bytes(Number(volumeSpace.total))} used</small
            >
            <small>{bytes(Number(volumeSpace.free))} free</small>{:else}<small
              >Volume usage unavailable</small
            >{/if}
          {#if volumeAvailability}<small
              >{bytes(Number(volumeAvailability.available))} available to you</small
            >{/if}
        </div>{/if}<button
        class:active={view === 'settings'}
        onpointerdown={() => {
          navigationKeyboardFocus = false;
        }}
        onkeydown={() => {
          navigationKeyboardFocus = true;
        }}
        title="Settings"
        disabled={analysisBusy}
        aria-label="Settings"
        onclick={() => {
          navigate('settings');
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
          placeholder="Search files and folders"
          aria-label="Search files and folders"
          disabled={!ready ||
            view === 'settings' ||
            view === 'duplicates' ||
            view === 'cleanup' ||
            view === 'history' ||
            view === 'developer'}
        />
      </div>
      <button
        class="advanced-toggle"
        bind:this={advancedButton}
        disabled={!ready || indexId === null || !['overview', 'explorer', 'large'].includes(view)}
        aria-expanded={advancedOpen}
        aria-controls="advanced-query-editor"
        aria-pressed={Boolean(searchExpression.trim())}
        onclick={() => {
          advancedOpen = !advancedOpen;
          if (advancedOpen) savedOpen = false;
        }}
        >Advanced{#if searchExpression.trim()}
          · On{/if}</button
      >
      <button
        bind:this={savedButton}
        aria-expanded={savedOpen}
        aria-controls="saved-workspaces-panel"
        onclick={openSaved}>Saved</button
      >
      <button
        class="command-trigger"
        onclick={openPalette}
        aria-haspopup="dialog"
        aria-label="Open command palette"><Icon name="search" />Commands <kbd>Ctrl K</kbd></button
      >
    </header>
    {#if savedOpen}<div id="saved-workspaces-panel">
        <SavedWorkspaces
          {catalog}
          capture={workspaceCapture}
          {indexId}
          folder={scope}
          path={directory?.path ?? ''}
          disabled={busy}
          canLoad={ready && !analysisBusy && !savedLoading}
          onadopt={adoptWorkspaces}
          onreload={() => void loadWorkspaces()}
          onload={(reference) => void loadWorkspace(reference)}
          onscan={(reference, path) => {
            savedOpen = false;
            void start(path, reference);
          }}
          onclose={closeSaved}
        />
      </div>{/if}
    {#if advancedOpen}<div id="advanced-query-editor">
        <AdvancedSearch
          value={searchExpression}
          {query}
          {indexId}
          disabled={!ready || indexId === null}
          onapply={applyAdvanced}
          onclose={closeAdvanced}
        />
      </div>
    {:else if searchExpression.trim() && ['overview', 'explorer', 'large'].includes(view)}<div
        class="applied-query-bar"
      >
        <code title={searchExpression}>Expression: {searchExpression}</code><button
          disabled={!ready || indexId === null}
          onclick={() => {
            invalidateInteraction();
            searchExpression = '';
          }}>Clear expression</button
        >
      </div>{/if}
    <main
      class:file-view={ready && (view === 'explorer' || view === 'large')}
      class:visual-explorer={ready && view === 'explorer' && explorerMap}
    >
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
      {#if agentNotice}<div class="scan-status" role="status">
          <span>{agentNotice}</span>
          {#if agentOperation !== null}<button
              onclick={() => {
                if (agentOperation !== null)
                  void invoke('cancel_operation', { id: agentOperation }).catch(report);
              }}>Request stop</button
            >
          {:else}<button
              onclick={() => {
                agentNotice = '';
              }}>Dismiss</button
            >{/if}
        </div>{/if}
      {#if ready && historical === null && indexId !== null}
        <LiveControl
          {indexId}
          path={scanPath}
          disabled={busy}
          concurrency={scanConcurrency}
          onbusy={(value) => {
            liveBusy = value;
          }}
          onobserved={observeLive}
        />
      {/if}
      {#if operation && !liveBusy && !liveObservation && view !== 'settings' && view !== 'duplicates' && view !== 'history'}<div
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
      {#if indexedView && ready && indexId !== null && directory}
        <FolderBudget
          {indexId}
          id={details?.entry.kind === 'directory' ? details.entry.id : scope}
          path={details?.entry.kind === 'directory' ? details.path : directory.path}
          sourceKey={`${operation?.phase ?? 'saved'}:${operation?.analyzed_at ?? liveObservation?.measured_at ?? historical?.captured ?? 'undated'}`}
          disabled={busy || historical !== null}
        />
      {/if}
      {#if view === 'settings'}
        <section class="settings-panel">
          <ProtectedFolders />
          <section aria-labelledby="appearance-heading">
            <h2 id="appearance-heading">Appearance</h2>
            <div class="settings-row">
              <span id="theme-label">Theme</span>
              <div class="theme-choices" role="group" aria-labelledby="theme-label">
                <label
                  ><input
                    type="radio"
                    name="theme"
                    value="light"
                    checked={theme === 'light'}
                    onchange={() => {
                      theme = 'light';
                      savePreferences();
                    }}
                  /><span>Light</span></label
                >
                <label
                  ><input
                    type="radio"
                    name="theme"
                    value="dark"
                    checked={theme === 'dark'}
                    onchange={() => {
                      theme = 'dark';
                      savePreferences();
                    }}
                  /><span>Dark</span></label
                >
              </div>
            </div>
            <label
              >Treemap size metric<select bind:value={sizeMode} onchange={savePreferences}
                ><option value="logical">Logical size</option><option value="allocated"
                  >Unique physical allocation</option
                ></select
              ></label
            ><label class="check"
              ><span>Reduce motion</span><input
                type="checkbox"
                bind:checked={reduceMotion}
                onchange={savePreferences}
              /></label
            >
          </section>
          <section aria-labelledby="scanning-heading">
            <h2 id="scanning-heading">Scanning</h2>
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
              ><span>Excluded paths<small>One per line</small></span><textarea
                bind:value={excluded}
                onchange={savePreferences}
                rows="5"
                placeholder="C:\\folder"></textarea></label
            >
            {#if recentPaths.length}<div class="settings-action">
                <button
                  onclick={() => {
                    recentPaths = [];
                    savePreferences();
                  }}>Clear recent paths</button
                >
              </div>{/if}
          </section>
          <section aria-labelledby="index-heading">
            <h2 id="index-heading">Index</h2>
            <div class="settings-action">
              <button onclick={clearAnalysis} disabled={busy || !isTauri() || !scanPath}
                >Clear current analysis</button
              >
            </div>
          </section>
          <AgentPlanReview />
          <AgentConnections />
        </section>
      {:else if view === 'developer'}
        {#key indexId}<DeveloperStorage
            enabled={ready && !busy}
            {revision}
            onopen={(id) => {
              view = 'explorer';
              void open(id);
            }}
            onerror={report}
          />
        {/key}
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
        {#key indexId}
          {#if !ready && view !== 'duplicates' && view !== 'cleanup'}<ScanWelcome
              {volume}
              {busy}
              native={isTauri()}
              scanning={operation?.phase === 'scanning'}
              recent={recentPaths}
              onscan={(path) => {
                void start(path);
              }}
              onchoose={() => {
                void choose();
              }}
            />
          {:else}
            {#if ready}<div class="breadcrumbs">
                <button title="Scan root" onclick={(event) => open(0, event.detail > 0)}
                  ><Icon name="disk" />Scan root</button
                >{#each directory?.ancestors.filter((entry) => entry.id !== 0) ?? [] as ancestor}<Icon
                    name="expand"
                    size={12}
                  /><button onclick={(event) => open(ancestor.id, event.detail > 0)}
                    >{ancestor.name}</button
                  >{/each}{#if directory && scope !== 0}<Icon name="expand" size={12} /><span
                    >{directory.entry.name}</span
                  >{/if}{#if directory?.entry.parent !== null && directory?.entry.parent !== undefined}<button
                    class="back-button"
                    onclick={(event) => open(directory?.entry.parent ?? 0, event.detail > 0)}
                    ><Icon name="back" />Parent folder</button
                  >{/if}
              </div>{/if}
            {#if directory?.entry.incomplete}<div class="notice">Incomplete coverage</div>{/if}
            {#if view === 'overview' || view === 'explorer' || view === 'large'}
              <div class="exploration-bar" role="group" aria-label="Inspection selection">
                <span
                  role="status"
                  title="Up to 500 items across the current analysis, including items outside this view."
                  >{explored.size} selected for inspection</span
                >
                <span class="exploration-hint"
                  >Ctrl+click toggles · Shift+click adds · Ctrl+Space toggles a focused item</span
                >
                <button disabled={!explored.size && !details} onclick={clearExploration}
                  >Clear inspection</button
                >
              </div>
            {/if}
            <div
              bind:this={fileWorkspace}
              tabindex="-1"
              class:file-workspace={view === 'explorer' || view === 'large'}
              class:map-workspace={view === 'explorer' && explorerMap}
              class:has-details={details !== null}
            >
              {#if view === 'overview'}
                <Overview
                  {indexId}
                  paused={busy}
                  {volume}
                  {directory}
                  {children}
                  {nodes}
                  {mapMode}
                  bind:depth={mapDepth}
                  bind:colorMode={mapColorMode}
                  mapIdentity={`${indexId}:${scope}:${cloudFilter ?? 'all'}`}
                  {cloudFilter}
                  queryState={mapQueryState}
                  oncloudfilter={changeCloudFilter}
                  onretry={() => {
                    void refresh(scope);
                  }}
                  {searchState}
                  searchIdentity={mapSearchKey}
                  {searchTotal}
                  onmode={(mode) => {
                    mapMode = mode;
                  }}
                  onback={() => {
                    if (details) details = null;
                    else if (directory?.entry.parent != null) void open(directory.entry.parent);
                  }}
                  {types}
                  selected={details}
                  {explored}
                  onclearExplore={clearExploration}
                  onselectMany={selectMany}
                  oncommand={mapCommand}
                  {sizeMode}
                  {reduceMotion}
                  animateNavigation={mapNavigation}
                  onmetric={(metric) => {
                    sizeMode = metric;
                    savePreferences();
                  }}
                  onselect={(id, operation) => {
                    void select(id, operation);
                  }}
                  onopen={(id, pointer = false) => {
                    void open(id, pointer);
                  }}
                  onlist={() => {
                    void showFileList();
                  }}
                  ongrid={() => {
                    explorerMap = false;
                    fileRepresentation = 'grid';
                    view = 'explorer';
                  }}
                  ontype={(item) => {
                    withoutExtension = item.extension === null;
                    extension = item.extension ?? '';
                    view = 'explorer';
                  }}
                />
              {:else if view === 'cleanup'}
                <button
                  disabled={busy}
                  onclick={async () => {
                    try {
                      await invoke('open_windows_storage');
                    } catch (problem) {
                      report(problem);
                    }
                  }}>Windows storage</button
                >
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
                <CleanupActivity
                  {revision}
                  {busy}
                  onrestore={(receipt) => {
                    if (!busy && !cleanupPreview && restoreReceipt === null)
                      restoreReceipt = receipt;
                  }}
                />
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
                  {#if view === 'explorer' && !explorerMap}<div
                      class="workspace-view-switch map-switch"
                      role="group"
                      aria-label="Storage representation"
                    >
                      <button
                        onclick={() => {
                          explorerMap = true;
                          mapMode = 'treemap';
                        }}>Treemap</button
                      >
                      <button
                        onclick={() => {
                          explorerMap = true;
                          mapMode = 'city';
                        }}>Skyline</button
                      >
                      <button
                        onclick={() => {
                          explorerMap = true;
                          mapMode = 'sunburst';
                        }}>Sunburst</button
                      >
                      <button
                        onclick={() => {
                          explorerMap = true;
                          mapMode = 'circles';
                        }}>Circles</button
                      >
                      <button
                        onclick={() => {
                          explorerMap = true;
                          mapMode = 'voronoi';
                        }}>Voronoi</button
                      >
                      <button
                        aria-pressed={fileRepresentation === 'table'}
                        onclick={() => (fileRepresentation = 'table')}>List</button
                      >
                      <button
                        aria-pressed={fileRepresentation === 'grid'}
                        onclick={() => (fileRepresentation = 'grid')}>Grid</button
                      >
                    </div>{/if}
                  {#if view === 'explorer' && explorerMap}
                    <StorageMap
                      {nodes}
                      identity={`${indexId}:${scope}:${cloudFilter ?? 'all'}`}
                      {cloudFilter}
                      queryState={mapQueryState}
                      oncloudfilter={changeCloudFilter}
                      onretry={() => {
                        void refresh(scope);
                      }}
                      mode={mapMode}
                      bind:depth={mapDepth}
                      bind:colorMode={mapColorMode}
                      selected={details?.entry.id ?? null}
                      {explored}
                      scopeAncestors={directory
                        ? [directory.entry.id, ...directory.ancestors.map((entry) => entry.id)]
                        : []}
                      onclearExplore={clearExploration}
                      onselectMany={selectMany}
                      oncommand={mapCommand}
                      {searchState}
                      searchIdentity={mapSearchKey}
                      {searchTotal}
                      {sizeMode}
                      onmetric={(metric) => {
                        sizeMode = metric;
                        savePreferences();
                      }}
                      onmode={(mode) => {
                        mapMode = mode;
                      }}
                      onselect={(id, operation) => {
                        void select(id, operation);
                      }}
                      onopen={(id) => {
                        void open(id, true);
                      }}
                      onlist={() => {
                        void showFileList();
                      }}
                      ongrid={() => {
                        explorerMap = false;
                        fileRepresentation = 'grid';
                      }}
                      onback={() => {
                        if (details) details = null;
                        else if (directory?.entry.parent != null) void open(directory.entry.parent);
                      }}
                      oninspect={() => {
                        if (details) details = null;
                        else if (nodes.find((n) => n.id >= 0))
                          void select(nodes.find((n) => n.id >= 0)?.id ?? 0);
                      }}
                    />
                    <button
                      class="map-list-toggle"
                      aria-expanded={showMapList}
                      onclick={() => {
                        showMapList = !showMapList;
                      }}
                      >Items · keyboard accessible list<Icon
                        name={showMapList ? 'chevron-up' : 'chevron-down'}
                        size={14}
                      /></button
                    >
                  {/if}
                  <div
                    class:map-list-hidden={view === 'explorer' && explorerMap && !showMapList}
                    class:map-list-visible={view === 'explorer' && explorerMap && showMapList}
                  >
                    <div class="filter-bar">
                      <CloudFilterSelect value={cloudFilter} onchange={changeCloudFilter} />
                      {#if view === 'explorer'}<label
                          >Size<select bind:value={sizeMode} onchange={savePreferences}>
                            <option value="logical">Logical bytes</option><option value="allocated"
                              >Unique allocation</option
                            >
                          </select></label
                        >{/if}
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
                            ><option value="file">Files</option><option value="directory"
                              >Folders</option
                            ></select
                          ></label
                        ><label
                          >Minimum MiB<input type="number" min="0" bind:value={minSize} /></label
                        ><label>Modified before<input type="date" bind:value={before} /></label
                        >{/if}<label
                        >Sort<select bind:value={sort}
                          ><option value="size">Largest first</option><option value="name"
                            >Name</option
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
                          searchExpression = '';
                          changeCloudFilter(null);
                        }}>Reset filters</button
                      >
                    </div>
                    <FileTable
                      representation={view === 'explorer' && !explorerMap
                        ? fileRepresentation
                        : 'table'}
                      allocated={view === 'explorer' && sizeMode === 'allocated'}
                      paint={entryPaint}
                      onsort={(next) => {
                        sort = next;
                      }}
                      ranked={view === 'large'}
                      {query}
                      {revision}
                      selected={details?.entry.id ?? null}
                      {explored}
                      onclearExplore={clearExploration}
                      checked={selection}
                      manageable={!busy && historical === null}
                      oncheck={checkFile}
                      onselect={(entry, operation) => {
                        void select(entry.id, operation);
                      }}
                      onopen={(entry) => {
                        void open(entry.id);
                      }}
                      onerror={report}
                    />
                  </div>
                  {#if selection.size > 0}<div class="selection-bar">
                      <span>{selection.size} in cleanup review</span><button
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
              {#if details}<section class="details-panel" tabindex="-1" bind:this={detailsPanel}>
                  <div>
                    <Icon name={details.entry.kind === 'directory' ? 'folder' : 'file'} />
                    <h2>{details.entry.name}</h2>
                    <button
                      class="dismiss"
                      aria-label="Close details"
                      title="Close details"
                      onclick={() => {
                        invalidateInteraction();
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
                        {fileSizeLabel(details.entry, true)}
                      </dd>
                    </div>
                    <div>
                      <dt>Modified</dt>
                      <dd>{date(details.entry.modified)}</dd>
                    </div>
                    <div>
                      <dt>Type</dt>
                      <dd>
                        {entryKindLabel(details.entry)}
                      </dd>
                    </div>
                    <div>
                      <dt>Cloud state</dt>
                      <dd
                        title="Observed metadata only. No recall flag does not prove complete local content."
                      >
                        {cloudLabel(details.entry.cloud)}
                      </dd>
                    </div>
                    <div>
                      <dt>Map category</dt>
                      <dd>
                        <span
                          class="item-swatch"
                          style:background={categoryPaint({
                            id: details.entry.id,
                            name: details.entry.name,
                            value: details.entry.logical,
                            directory: details.entry.kind === 'directory',
                          }).fill}
                        ></span>{categoryLabel({
                          id: details.entry.id,
                          name: details.entry.name,
                          value: details.entry.logical,
                          directory: details.entry.kind === 'directory',
                        })}
                      </dd>
                    </div>
                    <div>
                      <dt>Share of folder</dt>
                      <dd>
                        {directory?.entry.logical
                          ? ((details.entry.logical / directory.entry.logical) * 100).toFixed(1)
                          : '0'}% logical
                      </dd>
                    </div>
                    {#if details.entry.kind === 'directory'}<div>
                        <dt>Descendant modification</dt>
                        <dd>
                          {date(
                            mapMetrics.find((m) => m.id === details?.entry.id)?.latest_modified ??
                              null,
                          )} · {mapMetrics.find((m) => m.id === details?.entry.id)
                            ?.modified_known ?? 0} / {mapMetrics.find(
                            (m) => m.id === details?.entry.id,
                          )?.modified_total ?? details.entry.files} dates known
                        </dd>
                      </div>{/if}
                    {#if details.entry.kind === 'directory'}<div>
                        <dt>Files</dt>
                        <dd>{details.entry.files.toLocaleString()}</dd>
                      </div>{/if}
                  </dl>
                  <div class="detail-actions">
                    <button
                      disabled={busy ||
                        historical !== null ||
                        details.entry.kind !== 'file' ||
                        details.entry.incomplete}
                      title={details.entry.kind === 'directory'
                        ? 'Select concrete files in the list before reviewing cleanup'
                        : 'Open the native cleanup preview; nothing is removed now'}
                      onclick={() => {
                        if (details?.entry.kind === 'file') {
                          selection = new Set([details.entry.id]);
                          void previewSelection();
                        }
                      }}><Icon name="trash" />Review cleanup</button
                    >
                    <button
                      onclick={() => {
                        if (details) void reveal(details.entry.id);
                      }}><Icon name="external" />Show in Explorer</button
                    ><button
                      onclick={() => {
                        if (details) void copy(details.entry.id);
                      }}
                      disabled={!details.path_exact}
                      title={details.path_exact
                        ? 'Copy path'
                        : 'Path contains non-Unicode characters'}
                      ><Icon name="copy" />Copy path</button
                    >{#if details.entry.kind === 'directory'}<button
                        onclick={() => {
                          void open(details?.entry.id ?? 0);
                        }}
                        disabled={analysisBusy}><Icon name="folder" />Explore folder</button
                      >{/if}
                  </div>
                  {#if details.entry.kind === 'directory' && details.path_exact}
                    {#key details.path}<FolderNotes path={details.path} />{/key}
                  {/if}
                  {#if details.children}<div class="detail-children">
                      <h3>Children ({details.children.total.toLocaleString()})</h3>
                      <FolderComposition
                        children={details.children.items}
                        logical={details.entry.logical}
                      />
                      {#each details.children.items as child, position (child.id)}
                        <button
                          disabled={analysisBusy}
                          onclick={(event) => {
                            void activateChild(child, event.currentTarget);
                          }}
                        >
                          <span
                            class="child-icon"
                            style:color={entryPaint(child).fill}
                            aria-hidden="true"
                            ><Icon name={child.kind === 'directory' ? 'folder' : 'file'} /></span
                          >
                          <span title={child.name}>{child.name}</span><small
                            >{bytes(child.logical)}{#if child.incomplete}
                              · Partial{/if}</small
                          >
                        </button>
                      {/each}
                    </div>{/if}
                </section>{/if}
            </div>
          {/if}
        {/key}
      {/if}
    </main>
    <footer class="app-footer">
      <span>{isTauri() ? 'Voldiq 0.2.0' : 'Browser preview'}</span><span
        >{ready && entryCount !== null ? `${entryCount.toLocaleString()} entries` : ''}</span
      >
    </footer>
  </div>
</div>

{#if paletteOpen}<CommandPalette
    {commands}
    {catalog}
    oncommand={runCommand}
    onclose={() => closePalette()}
    onreload={() => void loadWorkspaces()}
  />{/if}
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
        scanRequest += 1;
        adoptIndex(null);
        cleanupNeedsRefresh = true;
        invalidateRefresh();
        invalidateInteraction();
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

{#if restoreReceipt !== null}
  <RestorationDialog
    receipt={restoreReceipt}
    onbusy={(value) => {
      analysisBusy = value;
    }}
    onchanged={() => {
      scanRequest += 1;
      adoptIndex(null);
      restoreNeedsRefresh = true;
      invalidateRefresh();
      invalidateInteraction();
      ready = false;
      operation = null;
      selection = new Set();
      details = null;
      directory = null;
      children = { items: [], total: 0 };
      types = { items: [], total_bytes: 0, total_files: 0 };
      revision += 1;
    }}
    onclose={() => {
      restoreReceipt = null;
      if (restoreNeedsRefresh) {
        restoreNeedsRefresh = false;
        historical = null;
        if (scanPath) void start(scanPath);
      }
    }}
    onerror={report}
  />
{/if}
