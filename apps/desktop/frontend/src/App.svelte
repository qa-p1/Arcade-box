<script lang="ts">
  import { copyText, watchIslandShown, watchIslandHiding, watchIslandHidden, watchOpenSettings, watchLinkOpen, arcadeAppName, islandReady, setIslandInputRegion, type LinkOpenRequest } from './lib/arcade';
  import { onMount, tick } from 'svelte';
  import { fade } from 'svelte/transition';
  import Icon from './lib/Icon.svelte';
  import DiffInput from './lib/DiffInput.svelte';
  import ImageResult from './lib/ImageResult.svelte';
  import ToolOutputView from './lib/ToolOutputView.svelte';
  import { inputPlaceholder, runLabel } from './lib/tool-presentation';
  import StandardToolForm from './lib/StandardToolForm.svelte';
  import VideoAssist from './lib/VideoAssist.svelte';
  import AudioAssist from './lib/AudioAssist.svelte';
  import SelectedFilesInput from './lib/SelectedFilesInput.svelte';
  import SelectedFolderInput from './lib/SelectedFolderInput.svelte';
  import ScreenColorSampler from './lib/ScreenColorSampler.svelte';
  import ScreenRuler from './lib/ScreenRuler.svelte';
  import ClipboardHistoryView from './lib/ClipboardHistoryView.svelte';
  import TimerView from './lib/TimerView.svelte';
  import PipelinesView from './lib/PipelinesView.svelte';
  import PluginManager from './lib/PluginManager.svelte';
  import SettingsView from './lib/SettingsView.svelte';
  import type { IconName } from './lib/icon-names';
import { cancelJob, detectContext, getHistory, getPreference, hideIsland, listFavorites, listJobs, listProviders, listTools, openArtifact, openReviewedUrl, pastePlainStatus, pastePlainText, pinScreenCapture, revokeInputFolder, revokeOutputDirectory, revealArtifact, runContextAction, runScreenTool, saveArtifactAs, screenCaptureStatus, screenRecordingStatus, searchTools, selectFiles, setAlias, setFavorite, setPreference, setShortcut, setSurfaceMode, setWindowPin, shortcutStatus, startJob, startScreenRecording, stopScreenRecording, terminateProcess, watchIslandFocus, watchJobUpdates, watchShortcutStatus, windowPinStatus } from './lib/arcade';
import type { ContextSuggestion, HistoryEntry, JobSnapshot, PastePlainStatus, ProcessInfo, ProcessList, ProviderInfo, ScreenCaptureStatus, ScreenRecordingSnapshot, SelectedDirectory, SelectedFile, ShortcutStatus, SystemWindowPinStatus, ToolInput, ToolResult, ToolSummary } from './lib/contracts';
  import { acceptsSelectedFile, acceptsTextInput, fileInputMime, inputMime, isRunnable, privacyHint, privacyLabel, usesFileInput } from './lib/tool-utils';
  import { defaultUiValues, serializeStandardUiOptions, standardUiOptionsProblem } from './lib/standard-ui';

  type Surface = 'island' | 'dashboard';
  const commandKey = /macintosh|mac os/i.test(navigator.userAgent) ? '⌘' : 'Ctrl';

  interface MediaFormat {
    format_name?: string;
    format_long_name?: string;
    duration?: string;
    size?: string;
    bit_rate?: string;
    tags?: Record<string, string>;
  }

  interface MediaStream {
    index?: number;
    codec_type?: string;
    codec_name?: string;
    codec_long_name?: string;
    profile?: string;
    width?: number;
    height?: number;
    avg_frame_rate?: string;
    r_frame_rate?: string;
    sample_rate?: string;
    channels?: number;
    channel_layout?: string;
    bit_rate?: string;
    pix_fmt?: string;
    color_transfer?: string;
    color_primaries?: string;
    bits_per_raw_sample?: string;
    disposition?: Record<string, number>;
    tags?: Record<string, string>;
  }

  interface MediaProbe {
    format?: MediaFormat;
    streams?: MediaStream[];
    chapters?: Array<Record<string, unknown>>;
  }

  interface BarcodeEntry {
    format?: string;
    contentType?: string;
    text?: string;
    position?: {
      topLeft?: { x: number; y: number };
      bottomRight?: { x: number; y: number };
    };
  }

  interface BarcodeScan {
    count: number;
    results: BarcodeEntry[];
    message?: string | null;
  }

  interface SystemInformation {
    operatingSystem?: { name?: string | null; version?: string | null; longVersion?: string | null; kernel?: string | null; family?: string; architecture?: string; hostName?: string | null; uptimeSeconds?: number };
    processor?: { model?: string | null; logicalCores?: number; physicalCores?: number | null; reportedFrequencyMhz?: number | null };
    memory?: { totalBytes?: number; usedBytes?: number; availableBytes?: number; swapTotalBytes?: number; swapUsedBytes?: number };
    storage?: Array<{ name?: string; mountPoint?: string; fileSystem?: string; totalBytes?: number; availableBytes?: number }>;
    networkInterfaces?: Array<{ name?: string; receivedBytes?: number; transmittedBytes?: number }>;
    limitations?: string[];
  }

  let processTerminationRequests = $state<Record<string, string>>({});

  let surface = $state<Surface>('island');
  let dismissing = $state(false);
  let islandContent: HTMLDivElement | undefined = $state();
  let islandHeight = $state<number | undefined>();
  let stopWatchingShown = () => {};
  let query = $state('');
  let searchResults = $state<ToolSummary[]>([]);
  let tools = $state<ToolSummary[]>([]);
  let contextSuggestions = $state<ContextSuggestion[]>([]);
  let history = $state<HistoryEntry[]>([]);
  let favorites = $state<string[]>([]);
  let providers = $state<ProviderInfo[]>([]);
  let providersLoaded = $state(false);
  // Capabilities from compatible providers; null until detection finishes so
  // choices are never marked missing before Arcade Box has looked.
  const providerCapabilities = $derived(providersLoaded
    ? new Set(providers.filter((provider) => provider.compatible).flatMap((provider) => provider.capabilities ?? []))
    : null);
  let backgroundJobs = $state<JobSnapshot[]>([]);
  let screenStatus = $state<ScreenCaptureStatus | null>(null);
  let screenRecording = $state<ScreenRecordingSnapshot | null>(null);
  let plainPasteStatus = $state<PastePlainStatus | null>(null);
  let activeWindowPinCapability = $state<SystemWindowPinStatus | null>(null);
  let activeJobId = $state('');
  let cancellingJobId = $state('');
  let jobError = $state('');
  const notedJobResults = new Set<string>();
  const jobDirectoryTokens = new Map<string, string>();
  const jobInputFolderTokens = new Map<string, string>();
  const pendingDirectoryTokens = new Set<string>();
  const pendingInputFolderTokens = new Set<string>();
  let providerError = $state('');
  let favoriteError = $state('');
  let aliasValue = $state('');
  let aliasMessage = $state('');
  let aliasSaving = $state(false);
  let onboardingReady = $state(false);
  let onboardingVisible = $state(false);
  let onboardingComplete = $state(false);
  let shortcutInfo = $state<ShortcutStatus | null>(null);
  let shortcutInput = $state('');
  let shortcutError = $state('');
  let shortcutSaving = $state(false);
  let themeMode = $state<'system' | 'light' | 'dark'>('system');
  let finishAfterShortcut = $state(false);
  let selectedTool = $state<ToolSummary | null>(null);
  let activeResult = $state<ToolResult | null>(null);
  let outputPage = $state(0);
  const fileOutputs = $derived(activeResult?.outputs.filter((output) => output.kind === 'artifact' || output.kind === 'file') ?? []);
  $effect(() => { activeResult; outputPage = 0; });
  let toolOptions = $state<Record<string, string>>({});
  let selectedFiles = $state<SelectedFile[]>([]);
  let selectedInputFolder = $state<SelectedDirectory | null>(null);
  let textInputMode = $state<'text' | 'file'>('text');
  let fileSelectionError = $state('');
  let screenCaptureBusy = $state(false);
  let artifactActionError = $state('');
  let artifactActionMessage = $state('');
  let artifactActionBusy = $state(false);
  let inputText = $state('');
  let diffRight = $state('');
  let diffBase = $state('');
  let loadingCatalog = $state(true);
  let loadingSearch = $state(false);
  let running = $state(false);
  let viewGeneration = $state(0);
  let focused = $state(false);
  let selectedIndex = $state(0);
  let searchAnnouncement = $state('');
  let searchError = $state('');
  let runError = $state('');
  let catalogError = $state('');
  let copyState = $state<'idle' | 'copied' | 'error'>('idle');
  let categoryFilter = $state('All tools');
  let catalogMode = $state<'all' | 'recent' | 'favorites' | 'engines' | 'jobs' | 'pipelines' | 'plugins' | 'settings'>('all');
  let dashboardQuery = $state('');
  let returnToDashboard = $state(false);
  let islandShell: HTMLElement | undefined = $state();
  let searchInput: HTMLInputElement | undefined = $state();
  let catalogSearchInput: HTMLInputElement | undefined = $state();
  let dashboardCloseButton: HTMLButtonElement | undefined = $state();
  let selectedToolHeading: HTMLHeadingElement | undefined = $state();
  let shortcutField: HTMLInputElement | undefined = $state();
  let onboardingDialog: HTMLDialogElement | undefined = $state();
  let copyTimer: number | undefined;
  let stopWatchingFocus = () => {};
  let stopWatchingJobs = () => {};
  let stopWatchingShortcut = () => {};
  let appliedSurfaceMode = '';
  let searchRequestId = 0;
  let contextRequestId = 0;

  const toolById = $derived(new Map(tools.map((tool) => [tool.id, tool])));
  const catalogSearchText = $derived(new Map(tools.map((tool) => [tool.id, [tool.name, tool.description, tool.category, ...tool.aliases].join(' ').toLowerCase()])));
  const runnableSearchResults = $derived(searchResults.filter(isRunnable));
  const plannedSearchResults = $derived(searchResults.filter((tool) => !isRunnable(tool)));
  const runnableContextSuggestions = $derived(contextSuggestions.filter((suggestion) => Boolean(toolById.get(suggestion.toolId) && isRunnable(toolById.get(suggestion.toolId)!))));
  // Categories keep the catalog's order, which groups the most-used tools first.
  const categories = $derived([...new Set(tools.map((tool) => tool.category))]);
  const implementedTools = $derived(tools.filter((tool) => tool.status === 'implemented'));
  const partialTools = $derived(tools.filter((tool) => tool.status === 'partial'));
  const plannedTools = $derived(tools.filter((tool) => !isRunnable(tool)));
  const resultAnnouncement = $derived(activeResult
    ? `${selectedTool?.name || 'Action'} ${activeResult.status === 'success' ? 'finished successfully' : 'finished with an error'}. ${activeResult.outputs.length} ${activeResult.outputs.length === 1 ? 'output' : 'outputs'}.`
    : '');
  const recentToolIds = $derived([...new Set(history.map((entry) => entry.toolId))].slice(0, 8));
  const activeJob = $derived(backgroundJobs.find((job) => job.id === activeJobId));
  const isCompact = $derived(surface === 'island' && !selectedTool && !query.trim() && !runnableContextSuggestions.length);
  const mediaProbe = $derived.by(() => selectedTool?.id === 'arcade.video.inspect' ? parseMediaProbe(activeResult) : null);
  const barcodeScan = $derived.by(() => selectedTool?.id === 'arcade.barcode.decode' || selectedTool?.id === 'arcade.screen.qr' ? parseStructuredOutput<BarcodeScan>(activeResult, 'structured/barcode') : null);
  const systemInformation = $derived.by(() => selectedTool?.id === 'arcade.system.system-info' ? parseStructuredOutput<SystemInformation>(activeResult, 'structured/system-info') : null);
  const processList = $derived.by(() => selectedTool?.id === 'arcade.system.process' ? parseStructuredOutput<ProcessList>(activeResult, 'structured/process-list') : null);
  const visibleDashboardTools = $derived.by(() => {
    if (catalogMode === 'engines' || catalogMode === 'jobs' || catalogMode === 'pipelines' || catalogMode === 'plugins') return [];
    let result = catalogMode === 'recent'
      ? recentToolIds.map((id) => toolById.get(id)).filter((tool): tool is ToolSummary => Boolean(tool))
      : catalogMode === 'favorites'
        ? favorites.map((id) => toolById.get(id)).filter((tool): tool is ToolSummary => Boolean(tool))
        : tools;
    if (categoryFilter !== 'All tools') result = result.filter((tool) => tool.category === categoryFilter);
    const term = dashboardQuery.trim().toLowerCase();
    if (term) {
      result = result.filter((tool) => catalogSearchText.get(tool.id)?.includes(term));
    }
    return result;
  });
  const dashboardSearchAnnouncement = $derived(dashboardQuery.trim()
    ? `${visibleDashboardTools.length} ${visibleDashboardTools.length === 1 ? 'tool' : 'tools'} shown in the catalog.`
    : '');

  $effect(() => {
    const mode = surface === 'dashboard' ? 'dashboard' : selectedTool || onboardingVisible ? 'tool' : isCompact ? 'compact' : 'search';
    if (mode !== appliedSurfaceMode) {
      appliedSurfaceMode = mode;
      void setSurfaceMode(mode).catch((error) => (catalogError = messageOf(error)));
    }
  });

  // Poll only while a native screen recording is active; idle workspaces do not wake.
  $effect(() => {
    if (!screenRecording?.recording) return;
    const timer = window.setInterval(() => {
      void screenRecordingStatus().then((status) => (screenRecording = status)).catch(() => {});
    }, 1000);
    return () => window.clearInterval(timer);
  });

  $effect(() => {
    if (onboardingVisible) void tick().then(() => {
      if (onboardingDialog && !onboardingDialog.open) onboardingDialog.showModal();
      shortcutField?.focus();
    });
  });

  $effect(() => {
    if (!islandContent) return;
    const observer = new ResizeObserver(([entry]) => {
      const nextHeight = Math.ceil(entry.borderBoxSize?.[0]?.blockSize ?? entry.contentRect.height);
      if (nextHeight > 0) islandHeight = nextHeight;
    });
    observer.observe(islandContent);
    return () => observer.disconnect();
  });

  let surfaceVisible = $state(!hasDesktopBridge());
  let lastFocusedElement: HTMLElement | null = null;
  let stopWatchingHiding = () => {};
  let stopWatchingHidden = () => {};
  let stopWatchingOpenSettings = () => {};
  let stopWatchingLinkOpen = () => {};
  /** Input from another Arcade app waiting for the user to choose a tool. */
  let pendingLink = $state<LinkOpenRequest | null>(null);

  // The pointer region follows the Island's target geometry, not each frame of
  // its height animation, so native input-shape updates happen once per change.
  let appliedInputRegion = '';
  let inputRegionFrame = 0;
  function queueInputRegion(): void {
    if (inputRegionFrame) return;
    inputRegionFrame = requestAnimationFrame(() => {
      inputRegionFrame = 0;
      let region: [number, number, number, number] | null = null;
      if (onboardingVisible && onboardingDialog?.open) {
        const rect = onboardingDialog.getBoundingClientRect();
        region = [rect.x, rect.y, rect.width, rect.height];
      } else if (surface === 'dashboard') {
        region = [0, 0, window.innerWidth, window.innerHeight];
      } else if (islandShell) {
        region = [islandShell.offsetLeft, islandShell.offsetTop, islandShell.offsetWidth, islandHeight ?? islandShell.offsetHeight];
      }
      if (!region) return;
      const key = region.map(Math.round).join(',');
      if (key === appliedInputRegion) return;
      appliedInputRegion = key;
      void setIslandInputRegion(...region);
    });
  }

  $effect(() => {
    void [surface, onboardingVisible, islandShell, onboardingDialog, islandHeight];
    queueInputRegion();
  });

  $effect(() => {
    const dialog = onboardingDialog;
    window.addEventListener('resize', queueInputRegion);
    const observer = dialog ? new ResizeObserver(queueInputRegion) : null;
    if (dialog) observer?.observe(dialog);
    return () => { observer?.disconnect(); window.removeEventListener('resize', queueInputRegion); };
  });

  function animateInvocation(): void {
    if (!islandShell) return;
    islandShell.getAnimations().forEach((animation) => animation.cancel());
    if (window.matchMedia('(prefers-reduced-motion: reduce)').matches) return;
    islandShell.animate([
      { opacity: 0, transform: 'translateY(-12px) scale(.97)' },
      { opacity: 1, transform: 'none' },
    ], { duration: 210, easing: 'cubic-bezier(.16,1,.3,1)' });
  }

  async function revealIsland(): Promise<void> {
    dismissing = false;
    surfaceVisible = true;
    await tick();
    animateInvocation();
    if (onboardingVisible) shortcutField?.focus();
    else if (lastFocusedElement?.isConnected) lastFocusedElement.focus({ preventScroll: true });
    else if (surface === 'dashboard') focusDashboardEntry();
    else if (!selectedTool) searchInput?.focus();
    else islandShell?.querySelector<HTMLElement>('textarea, input, select, button')?.focus();
  }

  onMount(() => {
    void loadInitialData();
    void tick().then(() => searchInput?.focus());
    const systemTheme = window.matchMedia('(prefers-color-scheme: light)');
    const onSystemThemeChange = () => {
      if (themeMode === 'system') document.documentElement.dataset.theme = systemTheme.matches ? 'light' : 'dark';
    };
    onSystemThemeChange();
    systemTheme.addEventListener('change', onSystemThemeChange);
    let disposed = false;
    void Promise.all([
      watchIslandShown(() => { void revealIsland(); }),
      watchIslandHiding(() => {
        lastFocusedElement = document.activeElement instanceof HTMLElement ? document.activeElement : null;
        islandShell?.getAnimations().forEach((animation) => animation.cancel());
        dismissing = true;
      }),
      watchIslandHidden(() => { surfaceVisible = false; dismissing = false; }),
      watchOpenSettings(() => openSettings()),
      watchLinkOpen((request) => openFromLink(request)),
    ]).then(async ([shown, hiding, hidden, settings, linkOpen]) => {
      if (disposed) { shown(); hiding(); hidden(); settings(); linkOpen(); return; }
      stopWatchingShown = shown; stopWatchingHiding = hiding; stopWatchingHidden = hidden; stopWatchingOpenSettings = settings; stopWatchingLinkOpen = linkOpen;
      await islandReady();
    }).catch((error) => (catalogError = messageOf(error)));
    void watchIslandFocus((isFocused) => {
      if (isFocused) {
        void refreshContext();
        if (surface === 'island' && !selectedTool) void tick().then(() => searchInput?.focus());
      }
    }).then((unlisten) => { if (disposed) unlisten(); else stopWatchingFocus = unlisten; });
    void watchJobUpdates(acceptJobSnapshot).then((unlisten) => { if (disposed) unlisten(); else stopWatchingJobs = unlisten; });
    void watchShortcutStatus(acceptShortcutStatus).then((unlisten) => { if (disposed) unlisten(); else stopWatchingShortcut = unlisten; });
    return () => {
      stopWatchingFocus();
      disposed = true;
      stopWatchingShown();
      stopWatchingHiding();
      stopWatchingHidden();
      stopWatchingOpenSettings();
      stopWatchingLinkOpen();
      stopWatchingJobs();
      stopWatchingShortcut();
      systemTheme.removeEventListener('change', onSystemThemeChange);
      if (inputRegionFrame) cancelAnimationFrame(inputRegionFrame);
      if (copyTimer !== undefined) window.clearTimeout(copyTimer);
    };
  });

  $effect(() => {
    const term = query.trim();
    const requestId = ++searchRequestId;
    if (!term) {
      searchResults = [];
      searchError = '';
      loadingSearch = false;
      selectedIndex = 0;
      searchAnnouncement = runnableContextSuggestions.length
        ? `${runnableContextSuggestions.length} clipboard ${runnableContextSuggestions.length === 1 ? 'action is' : 'actions are'} available. Use the arrow keys to choose one, then press Enter.`
        : '';
      return;
    }
    // Local search is fast; keep the previous list on screen until the new
    // one arrives so the Island does not collapse and regrow on each key.
    loadingSearch = true;
    void performSearch(term, requestId);
  });

  async function loadInitialData(): Promise<void> {
    loadingCatalog = true;
    try {
      const [catalog, historyList, favoriteList, jobs, onboarding, shortcut, theme] = await Promise.all([
        listTools(),
        getHistory().catch(() => []),
        listFavorites().catch(() => []),
        listJobs().catch(() => []),
        getPreference('onboarding_complete').catch(() => null),
        shortcutStatus().catch((error) => { shortcutError = messageOf(error); return null; }),
        getPreference('theme').catch(() => null),
      ]);
      tools = catalog;
      history = historyList;
      favorites = favoriteList;
      mergeBackgroundJobs(jobs);
      onboardingComplete = onboarding === 'true';
      shortcutInfo = shortcut;
      themeMode = theme === 'light' || theme === 'dark' ? theme : 'system';
      applyTheme(themeMode);
      shortcutInput = shortcutDescription(shortcut) || (shortcut?.backend.includes('portal') ? 'CTRL+ALT+space' : defaultShortcutForPlatform());
      onboardingReady = true;
      onboardingVisible = hasDesktopBridge() && !onboardingComplete;
      catalogError = '';
      // Optional providers and desktop portals must not hold up search or onboarding.
      void listProviders().then((items) => { providers = items; providersLoaded = true; providerError = ''; })
        .catch((error) => { providerError = messageOf(error); });
      void screenCaptureStatus().then((status) => { screenStatus = status; }).catch(() => {});
      void screenRecordingStatus().then((status) => { screenRecording = status; }).catch(() => {});
      void pastePlainStatus().then((status) => { plainPasteStatus = status; }).catch(() => {});
      void windowPinStatus().then((status) => { activeWindowPinCapability = status; }).catch(() => {});
      // Context detection is invocation-scoped and local. Unsupported platform sources may return no suggestions.
      void refreshContext();
    } catch (error) {
      catalogError = messageOf(error);
    } finally {
      loadingCatalog = false;
    }
  }

  function hasDesktopBridge(): boolean {
    return typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
  }

  function applyTheme(value: 'system' | 'light' | 'dark'): void {
    document.documentElement.dataset.theme = value === 'system'
      ? (window.matchMedia('(prefers-color-scheme: light)').matches ? 'light' : 'dark')
      : value;
  }

  async function changeTheme(value: 'system' | 'light' | 'dark'): Promise<void> {
    await setPreference('theme', value);
    themeMode = value;
    applyTheme(value);
  }

  function shortcutDescription(status: ShortcutStatus | null): string {
    return status?.triggerDescription || status?.trigger_description || '';
  }

  function defaultShortcutForPlatform(): string {
    if (typeof navigator !== 'undefined' && /macintosh|mac os/i.test(navigator.userAgent)) return 'Super+Shift+Space';
    return 'Ctrl+Alt+Space';
  }

  function acceptShortcutStatus(status: ShortcutStatus): void {
    shortcutInfo = status;
    const state = status.state.toLowerCase();
    if (state === 'registered') {
      shortcutError = '';
      if (finishAfterShortcut) void finishOnboarding();
    } else if (['unsupported', 'unavailable', 'error', 'not_bound', 'registration_rejected', 'user_declined', 'invalid_trigger', 'hyprland_binding_unavailable'].includes(state)) {
      finishAfterShortcut = false;
      shortcutError = status.message;
    }
  }

  async function applyShortcutAndContinue(): Promise<void> {
    if (!shortcutInput.trim()) {
      shortcutError = 'Enter a shortcut such as Ctrl+Alt+Space.';
      return;
    }
    shortcutSaving = true;
    shortcutError = '';
    finishAfterShortcut = true;
    try {
      const status = await setShortcut(shortcutInput.trim());
      acceptShortcutStatus(status);
      if (status.state.toLowerCase() === 'updating' || status.state.toLowerCase() === 'checking') {
        if (onboardingVisible) shortcutError = 'Waiting for the desktop to confirm this shortcut. You can also continue and adjust it later in settings.';
      } else if (status.state.toLowerCase() !== 'registered') {
        finishAfterShortcut = false;
        shortcutError = status.message || 'The shortcut could not be registered.';
      }
    } catch (error) {
      finishAfterShortcut = false;
      shortcutError = messageOf(error);
    } finally {
      shortcutSaving = false;
    }
  }

  async function finishOnboarding(): Promise<void> {
    finishAfterShortcut = false;
    try {
      await setPreference('onboarding_complete', 'true');
      onboardingComplete = true;
      onboardingVisible = false;
      finishAfterShortcut = false;
      shortcutError = '';
      void tick().then(() => searchInput?.focus());
    } catch (error) {
      shortcutError = messageOf(error);
    }
  }

  async function refreshContext(): Promise<void> {
    const requestId = ++contextRequestId;
    const suggestions = await detectContext().catch(() => []);
    if (requestId !== contextRequestId) return;
    contextSuggestions = suggestions;
    if (!query.trim()) {
      selectedIndex = 0;
      searchAnnouncement = contextSuggestions.length
        ? `${contextSuggestions.length} clipboard ${contextSuggestions.length === 1 ? 'action is' : 'actions are'} available. Use the arrow keys to choose one, then press Enter.`
        : '';
    }
  }

  async function performSearch(term: string, requestId: number): Promise<void> {
    try {
      const results = await searchTools(term);
      if (requestId !== searchRequestId || term !== query.trim()) return;
      searchResults = results;
      searchError = '';
      selectedIndex = 0;
      const available = results.filter(isRunnable).length;
      const planned = results.length - available;
      searchAnnouncement = available === 0
        ? planned ? `No available actions match. ${planned} planned ${planned === 1 ? 'tool' : 'tools'} found.` : 'No matching actions found.'
        : `${available} ${available === 1 ? 'available action' : 'available actions'}${planned ? ` and ${planned} planned ${planned === 1 ? 'tool' : 'tools'}` : ''}. Use the arrow keys to select an available action, then press Enter.`;
    } catch (error) {
      if (requestId !== searchRequestId || term !== query.trim()) return;
      searchResults = [];
      searchError = messageOf(error);
      searchAnnouncement = `Search failed. ${searchError}`;
    } finally {
      if (requestId === searchRequestId) loadingSearch = false;
    }
  }

  function messageOf(error: unknown): string {
    if (typeof error === 'string') return error;
    if (error instanceof Error) return error.message;
    return 'The local tool runtime could not complete that request.';
  }

  function openTool(tool: ToolSummary, fromDashboard = false): void {
    releaseCurrentDirectoryGrant();
    selectedTool = tool;
    activeJobId = '';
    activeResult = null;
    inputText = '';
    diffRight = '';
    diffBase = '';
    toolOptions = defaultOptionsFor(tool);
    selectedFiles = [];
    selectedInputFolder = null;
    textInputMode = 'text';
    fileSelectionError = '';
    screenCaptureBusy = false;
    artifactActionError = '';
    artifactActionMessage = '';
    artifactActionBusy = false;
    aliasValue = '';
    aliasMessage = '';
    runError = '';
    copyState = 'idle';
    returnToDashboard = fromDashboard;
    surface = 'island';
    focused = false;
    if (pendingLink) {
      applyLinkInput(pendingLink);
      pendingLink = null;
    }
    void tick().then(() => {
      const entry = Array.from(islandShell?.querySelectorAll<HTMLElement>('#tool-input, .granted-file-picker, .input-folder-picker button, .standard-tool-form input:not([type=checkbox]), .standard-tool-form select, .run-button') ?? []).find((element) => element.getClientRects().length && !element.hasAttribute('disabled'));
      (entry ?? selectedToolHeading)?.focus({ preventScroll: true });
      islandContent?.scrollTo({ top: 0 });
    });
  }

  /** "More in Arcade Box…" from another app: open the named tool with the
   * input attached, or keep the input until the user chooses a tool. */
  function openFromLink(request: LinkOpenRequest): void {
    const tool = request.tool ? tools.find((candidate) => candidate.id === request.tool) : undefined;
    pendingLink = request;
    if (tool) {
      openTool(tool);
      return;
    }
    releaseCurrentDirectoryGrant();
    selectedTool = null;
    activeResult = null;
    surface = 'island';
    query = '';
    void tick().then(() => searchInput?.focus());
  }

  function applyLinkInput(request: LinkOpenRequest): void {
    if (request.files.length) {
      selectedFiles = request.files;
      textInputMode = 'file';
    }
    if (request.text) {
      inputText = request.text;
      textInputMode = 'text';
    }
    for (const [key, value] of Object.entries(request.options)) toolOptions[key] = String(value);
  }

  function defaultOptionsFor(tool: ToolSummary): Record<string, string> {
    return tool.ui?.version === 1 ? defaultUiValues(tool.ui) : {};
  }

  function openRunnableResult(index = selectedIndex): void {
    const tool = runnableSearchResults[index];
    if (!tool) return;
    openTool(tool);
  }

  function backFromTool(): void {
    releaseCurrentDirectoryGrant();
    selectedTool = null;
    activeJobId = '';
    activeResult = null;
    runError = '';
    if (returnToDashboard) {
      surface = 'dashboard';
      returnToDashboard = false;
      focusDashboardEntry();
    } else {
      surface = 'island';
      void tick().then(() => searchInput?.focus());
    }
  }

  function focusDashboardEntry(): void {
    void tick().then(() => (catalogSearchInput ?? dashboardCloseButton)?.focus());
  }

  function acceptToolCatalog(catalog: ToolSummary[]): void {
    tools = catalog;
    if (query.trim()) {
      const requestId = ++searchRequestId;
      loadingSearch = true;
      void performSearch(query.trim(), requestId);
    }
  }

  function announceSearchSelection(): void {
    void tick().then(() => islandShell?.querySelector('.search-result.active, .context-suggestion.active')?.scrollIntoView({ block: 'nearest' }));
    if (query.trim()) {
      const tool = runnableSearchResults[selectedIndex];
      if (tool) searchAnnouncement = `${tool.name}, ${statusText(tool)}. Press Enter to open.`;
      return;
    }
    const suggestion = runnableContextSuggestions[selectedIndex];
    const tool = suggestion && tools.find((item) => item.id === suggestion.toolId);
    if (tool && suggestion) searchAnnouncement = `${tool.name}. ${suggestion.reason}. Press Enter to run this action.`;
  }

  function handleSearchKeydown(event: KeyboardEvent): void {
    if (event.key === 'Enter' && loadingSearch) {
      event.preventDefault();
      return;
    }
    if (event.key === 'ArrowDown' && !loadingSearch && runnableSearchResults.length) {
      event.preventDefault();
      selectedIndex = (selectedIndex + 1) % runnableSearchResults.length;
      announceSearchSelection();
    } else if (event.key === 'ArrowUp' && !loadingSearch && runnableSearchResults.length) {
      event.preventDefault();
      selectedIndex = (selectedIndex - 1 + runnableSearchResults.length) % runnableSearchResults.length;
      announceSearchSelection();
    } else if (event.key === 'ArrowDown' && !query.trim() && runnableContextSuggestions.length) {
      event.preventDefault();
      selectedIndex = (selectedIndex + 1) % runnableContextSuggestions.length;
      announceSearchSelection();
    } else if (event.key === 'ArrowUp' && !query.trim() && runnableContextSuggestions.length) {
      event.preventDefault();
      selectedIndex = (selectedIndex - 1 + runnableContextSuggestions.length) % runnableContextSuggestions.length;
      announceSearchSelection();
    } else if (event.key === 'Enter' && runnableSearchResults.length) {
      event.preventDefault();
      openRunnableResult();
    } else if (event.key === 'Enter' && !query.trim() && runnableContextSuggestions.length) {
      event.preventDefault();
      void executeContextAction(runnableContextSuggestions[selectedIndex] ?? runnableContextSuggestions[0]);
    } else if (event.key === 'Escape') {
      event.preventDefault();
      event.stopPropagation();
      if (query.trim()) {
        query = '';
        focused = true;
        searchInput?.focus();
      } else {
        void dismissIsland();
      }
    }
  }

  function handleWindowKeydown(event: KeyboardEvent): void {
    if (event.defaultPrevented || event.isComposing) return;
    if (onboardingVisible) {
      if (event.key === 'Escape') event.preventDefault();
      return;
    }
    const command = event.metaKey || event.ctrlKey;
    if (selectedTool && command && event.key === 'Enter') {
      event.preventDefault();
      void submitTool();
      return;
    }
    if (selectedTool && command && event.shiftKey && event.key.toLowerCase() === 'c') {
      event.preventDefault();
      const imageCopy = islandShell?.querySelector<HTMLButtonElement>('[data-copy-image]');
      const text = activeResult?.outputs.find((output) => output.kind === 'text' || output.kind === 'url');
      if (imageCopy) imageCopy.click();
      else if (text) void copyOutput(text.value);
      return;
    }
    if (selectedTool && command && event.key.toLowerCase() === 'o' && fileWorkflowActive(selectedTool)) {
      event.preventDefault();
      if (!running && !(activeJob && jobIsActive(activeJob))) void chooseInputFile();
      return;
    }
    if (selectedTool && event.altKey && event.key === 'ArrowLeft') {
      event.preventDefault();
      backFromTool();
      return;
    }
    if (event.key === 'Escape' && event.target instanceof HTMLSelectElement) return;
    if (event.key === 'Escape') {
      event.preventDefault();
      void dismissIsland();
    } else if (command && event.key.toLowerCase() === 'k') {
      event.preventDefault();
      if (surface === 'dashboard') catalogSearchInput?.focus();
      else {
        if (selectedTool) { returnToDashboard = false; backFromTool(); }
        void tick().then(() => { searchInput?.focus(); searchInput?.select(); });
      }
    } else if (event.key === '/' && !isEditableTarget(event.target)) {
      event.preventDefault();
      releaseCurrentDirectoryGrant();
      surface = 'island';
      selectedTool = null;
      activeJobId = '';
      activeResult = null;
      void tick().then(() => searchInput?.focus());
    }
  }

  function isEditableTarget(target: EventTarget | null): boolean {
    return target instanceof HTMLElement && (
      target.isContentEditable || ['INPUT', 'TEXTAREA', 'SELECT'].includes(target.tagName)
    );
  }

  async function dismissIsland(): Promise<void> {
    // Native owns the exit timer and cancels it if a new invocation arrives.
    // Keep tool input/results intact when the user temporarily puts us away.
    try {
      await hideIsland();
    } catch (error) {
      catalogError = messageOf(error);
    }
  }

  async function submitTool(): Promise<void> {
    if (!selectedTool || !isRunnable(selectedTool) || !canSubmitSelectedTool() || running || (activeJob && jobIsActive(activeJob))) return;
    const tool = selectedTool;
    const generation = viewGeneration;
    running = true;
    runError = '';
    activeResult = null;
    if (tool.id === 'arcade.system.process') processTerminationRequests = {};
    const directoryToken = typeof toolOptions.destinationGrant === 'string' && toolOptions.destinationGrant
      ? toolOptions.destinationGrant
      : '';
    const inputFolderToken = tool.ui?.input.kind === 'folder'
      ? selectedInputFolder?.token || ''
      : '';
    if (directoryToken) pendingDirectoryTokens.add(directoryToken);
    if (inputFolderToken) pendingInputFolderTokens.add(inputFolderToken);
    try {
      if (isScreenRecorderTool(tool.id)) {
        const status = await startScreenRecording();
        if (status) {
          screenRecording = status;
          if (generation === viewGeneration && selectedTool?.id === tool.id) activeJobId = status.jobId || '';
          mergeBackgroundJobs(await listJobs().catch(() => []));
        }
        return;
      }
      if (tool.id === 'arcade.system.paste-plain') {
        await pastePlainText();
        return;
      }
      if (tool.id === 'arcade.system.window-pin') {
        const status = await windowPinStatus();
        activeWindowPinCapability = status;
        if (generation !== viewGeneration) return;
        activeResult = {
          toolId: tool.id,
          status: 'success',
          outputs: [{ kind: 'text', value: JSON.stringify(status, null, 2), mime: 'structured/window-pin' }],
          message: status.message,
          warnings: [],
          metadata: {},
        };
        recordHistory(tool.id, 'success');
        return;
      }
      if (isScreenTool(tool.id)) {
        const result = await runScreenTool(tool.id);
        if (result && generation === viewGeneration) {
          if (tool.id === 'arcade.screen.pin' && result.status === 'success') {
            const image = result.outputs.find((output) => output.kind === 'artifact' || output.kind === 'file');
            if (!image) throw new Error('The screen capture did not return an image to pin.');
            const pinned = await pinScreenCapture(image.value);
            if (generation !== viewGeneration || selectedTool?.id !== tool.id) return;
            result.message = pinned.message;
            result.metadata = { ...result.metadata, pinWindow: pinned.label, width: pinned.width, height: pinned.height };
          }
          activeResult = result;
          if (result.status === 'error') runError = result.message || 'The tool reported an error.';
          recordHistory(tool.id, result.status);
        }
        return;
      }
      const uiInputKind = tool.ui?.version === 1 ? tool.ui.input.kind : undefined;
      const fileMode = fileWorkflowActive(tool);
      const inputs: ToolInput[] = tool.id === 'arcade.text.diff'
        && !fileMode
        ? (toolOptions.operation === 'merge' ? [diffBase, inputText, diffRight] : [inputText, diffRight]).map((value) => ({ kind: 'text' as const, value, mime: 'text/plain' }))
        : uiInputKind === 'folder'
        ? selectedInputFolder
          ? [{ kind: 'artifact', value: selectedInputFolder.token, mime: 'folder/reference' }]
          : []
        : fileMode
          ? selectedFiles.map((file) => ({ kind: 'artifact', value: file.token, mime: fileInputMime(file) }))
          : uiInputKind === 'none'
            ? []
            : [{ kind: uiInputKind === 'url' ? 'url' : 'text', value: inputText, mime: inputMime(tool) }];
      const options: Record<string, unknown> = tool.ui?.version === 1
        ? serializeStandardUiOptions(tool.ui, toolOptions)
        : { ...toolOptions };
      const request = {
        toolId: tool.id,
        inputs,
        options,
      };
      {
        const job = await startJob(request);
        if (directoryToken && jobIsActive(job)) jobDirectoryTokens.set(job.id, directoryToken);
        if (inputFolderToken && jobIsActive(job)) jobInputFolderTokens.set(job.id, inputFolderToken);
        if (generation === viewGeneration) activeJobId = job.id;
        acceptJobSnapshot(job);
        if (job.result && generation === viewGeneration) activeResult = job.result;
        const latest = await listJobs().catch(() => []);
        if (latest.length) {
          mergeBackgroundJobs(latest);
          const refreshed = latest.find((entry) => entry.id === job.id);
          if (refreshed) acceptJobSnapshot(refreshed);
        }
      }
    } catch (error) {
      if (generation === viewGeneration) runError = messageOf(error);
    } finally {
      if (generation === viewGeneration) running = false;
      if (directoryToken) {
        pendingDirectoryTokens.delete(directoryToken);
        releaseDirectoryGrantIfUnused(directoryToken, !selectedTool || toolOptions.destinationGrant !== directoryToken);
      }
      if (inputFolderToken) {
        pendingInputFolderTokens.delete(inputFolderToken);
        releaseInputFolderGrantIfUnused(inputFolderToken, !selectedTool || selectedInputFolder?.token !== inputFolderToken);
      }
    }
  }

  async function requestProcessTermination(process: ProcessInfo): Promise<void> {
    const identity = `${process.pid}:${process.startTime}`;
    if (process.protected || processTerminationRequests[identity] === 'working') return;
    const confirmed = window.confirm(
      `Send a termination request to “${process.name}” (PID ${process.pid})? Unsaved work in that application may be lost.`,
    );
    if (!confirmed) return;
    processTerminationRequests = { ...processTerminationRequests, [identity]: 'working' };
    try {
      await terminateProcess(process);
      processTerminationRequests = { ...processTerminationRequests, [identity]: 'requested' };
    } catch (error) {
      processTerminationRequests = { ...processTerminationRequests, [identity]: messageOf(error) };
    }
  }

  async function changeForegroundWindowPin(pinned: boolean): Promise<void> {
    if (!activeWindowPinCapability?.available) return;
    try {
      activeWindowPinCapability = await setWindowPin(pinned);
    } catch (error) {
      activeWindowPinCapability = { ...activeWindowPinCapability, message: messageOf(error) };
      runError = messageOf(error);
    }
  }

  async function finishScreenRecording(): Promise<void> {
    if (!screenRecording?.recording || screenRecording.finalizing) return;
    const generation = viewGeneration;
    const recordingToolId = selectedTool?.id;
    running = true;
    runError = '';
    activeResult = null;
    try {
      const result = await stopScreenRecording();
      if (generation === viewGeneration && selectedTool?.id === recordingToolId) activeResult = result;
      screenRecording = await screenRecordingStatus().catch(() => ({
        ...screenRecording!, recording: false, finalizing: false, elapsedSeconds: null,
        jobId: screenRecording?.jobId || null, message: 'Recording finished.',
      }));
      mergeBackgroundJobs(await listJobs().catch(() => []));
    } catch (error) {
      if (generation === viewGeneration && selectedTool?.id === recordingToolId) runError = messageOf(error);
      screenRecording = await screenRecordingStatus().catch(() => screenRecording);
    } finally {
      if (generation === viewGeneration) running = false;
    }
  }

  async function discardScreenRecording(): Promise<void> {
    const job = backgroundJobs.find((entry) => entry.id === screenRecording?.jobId);
    if (job && jobIsActive(job)) await cancelBackgroundJob(job);
  }

  async function copyOutput(value: string): Promise<void> {
    try {
      await copyText(value);
      copyState = 'copied';
      if (copyTimer !== undefined) window.clearTimeout(copyTimer);
      copyTimer = window.setTimeout(() => (copyState = 'idle'), 1800);
    } catch {
      copyState = 'error';
    }
  }

  function toggleDashboard(): void {
    releaseCurrentDirectoryGrant();
    selectedTool = null;
    activeResult = null;
    surface = surface === 'dashboard' ? 'island' : 'dashboard';
    dashboardQuery = '';
    categoryFilter = 'All tools';
    catalogMode = 'all';
    if (surface === 'island') void tick().then(() => searchInput?.focus());
    else focusDashboardEntry();
  }

  const categoryIcons: Record<string, IconName> = {
    PDF: 'document',
    Images: 'image',
    Video: 'video',
    'Audio & Speech': 'audio',
    Downloads: 'download',
    'Web & Network': 'network',
    'Text & Data': 'type',
    'Converters & Calculators': 'calculator',
    'Everyday Utilities': 'spark',
    'QR & Barcodes': 'qr',
    Developer: 'code',
    Files: 'folder',
    'Screen & Capture': 'monitor',
    System: 'command',
  };

  function iconForCategory(category: string): IconName {
    return categoryIcons[category] ?? 'grid';
  }

  function statusText(tool: ToolSummary): string {
    if (tool.status === 'implemented') return 'Ready';
    if (tool.status === 'partial') return 'In progress';
    return 'Planned';
  }

  function selectContextSuggestion(suggestion: ContextSuggestion): void {
    void executeContextAction(suggestion);
  }

  async function executeContextAction(suggestion: ContextSuggestion): Promise<void> {
    const tool = tools.find((item) => item.id === suggestion.toolId);
    if (!tool || !isRunnable(tool) || running) return;
    openTool(tool);
    const generation = viewGeneration;
    running = true;
    runError = '';
    try {
      const result = await runContextAction(suggestion.toolId);
      if (generation !== viewGeneration) return;
      activeResult = result;
      if (activeResult.status === 'error') runError = activeResult.message || 'The context action reported an error.';
      recordHistory(tool.id, activeResult.status);
    } catch (error) {
      if (generation === viewGeneration) runError = messageOf(error);
    } finally {
      if (generation === viewGeneration) running = false;
    }
  }

  function recordHistory(toolId: string, status: string): void {
    history = [{ toolId, status, createdAt: new Date().toISOString() }, ...history.filter((entry) => entry.toolId !== toolId)].slice(0, 50);
    void getHistory().then((entries) => (history = entries)).catch(() => {});
  }

  function jobIsActive(job: JobSnapshot): boolean {
    return job.status === 'queued' || job.status === 'running' || job.status === 'cancelling';
  }

  const activeJobCount = $derived(backgroundJobs.filter(jobIsActive).length);

  function acceptJobSnapshot(snapshot: JobSnapshot): void {
    const current = backgroundJobs.find((job) => job.id === snapshot.id);
    if (current) snapshot = newerJobSnapshot(current, snapshot);
    if (current === snapshot) return;
    backgroundJobs = [snapshot, ...backgroundJobs.filter((job) => job.id !== snapshot.id)].slice(0, 40);
    if (snapshot.toolId === 'arcade.screen.recorder' && screenRecording?.jobId === snapshot.id) {
      if (snapshot.status === 'cancelling') {
        screenRecording = { ...screenRecording, finalizing: true, message: 'Discarding the partial recording…' };
      } else if (!jobIsActive(snapshot)) {
        screenRecording = {
          ...screenRecording,
          recording: false,
          starting: false,
          finalizing: false,
          elapsedSeconds: null,
          jobId: null,
          message: snapshot.status === 'succeeded'
            ? 'Recording saved.'
            : snapshot.message || (snapshot.status === 'cancelled' ? 'Recording discarded.' : 'Recording ended.'),
        };
      }
    }
    if (!jobIsActive(snapshot)) {
      const directoryToken = jobDirectoryTokens.get(snapshot.id);
      if (directoryToken) {
        jobDirectoryTokens.delete(snapshot.id);
        releaseDirectoryGrantIfUnused(directoryToken, !selectedTool || toolOptions.destinationGrant !== directoryToken);
      }
      const inputFolderToken = jobInputFolderTokens.get(snapshot.id);
      if (inputFolderToken) {
        jobInputFolderTokens.delete(snapshot.id);
        if (selectedInputFolder?.token === inputFolderToken) selectedInputFolder = null;
        releaseInputFolderGrantIfUnused(inputFolderToken, true);
      }
    }
    if (snapshot.id !== activeJobId || !selectedTool || snapshot.toolId !== selectedTool.id) return;
    if (snapshot.result) activeResult = snapshot.result;
    if (snapshot.status === 'failed' || snapshot.status === 'interrupted') {
      runError = snapshot.message || snapshot.result?.message || 'The background job did not finish.';
    } else if (snapshot.status === 'succeeded') {
      runError = '';
      if (!notedJobResults.has(snapshot.id)) {
        notedJobResults.add(snapshot.id);
        recordHistory(snapshot.toolId, 'success');
      }
    }
  }

  function mergeBackgroundJobs(snapshots: JobSnapshot[]): void {
    if (!snapshots.length) return;
    const currentById = new Map(backgroundJobs.map((job) => [job.id, job]));
    const incomingIds = new Set(snapshots.map((job) => job.id));
    const merged = snapshots.map((incoming) => {
      const current = currentById.get(incoming.id);
      return current ? newerJobSnapshot(current, incoming) : incoming;
    });
    backgroundJobs = [...merged, ...backgroundJobs.filter((job) => !incomingIds.has(job.id))].slice(0, 40);
  }

  function newerJobSnapshot(current: JobSnapshot, incoming: JobSnapshot): JobSnapshot {
    if (!jobIsActive(current) && jobIsActive(incoming)) return current;
    if (current.status === 'cancelling' && (incoming.status === 'queued' || incoming.status === 'running')) return current;
    if (current.result && !incoming.result) return current;
    const currentUpdated = Date.parse(current.updatedAt || current.createdAt || '');
    const incomingUpdated = Date.parse(incoming.updatedAt || incoming.createdAt || '');
    return Number.isFinite(currentUpdated) && (!Number.isFinite(incomingUpdated) || currentUpdated > incomingUpdated)
      ? current
      : incoming;
  }

  function releaseCurrentDirectoryGrant(): void {
    viewGeneration += 1;
    running = false;
    screenCaptureBusy = false;
    artifactActionBusy = false;
    artifactActionError = '';
    artifactActionMessage = '';
    copyState = 'idle';
    if (copyTimer !== undefined) {
      window.clearTimeout(copyTimer);
      copyTimer = undefined;
    }
    const inputFolderToken = selectedInputFolder?.token;
    selectedInputFolder = null;
    if (inputFolderToken) releaseInputFolderGrantIfUnused(inputFolderToken, true);
    const token = toolOptions.destinationGrant;
    if (typeof token !== 'string' || !token) return;
    toolOptions = { ...toolOptions, destinationGrant: '' };
    releaseDirectoryGrantIfUnused(token, true);
  }

  function inputFolderSelectionHandler(generation: number, toolId: string): (folder: SelectedDirectory) => void {
    return (folder) => {
      if (generation !== viewGeneration || selectedTool?.id !== toolId || selectedTool.ui?.input.kind !== 'folder') {
        releaseInputFolderGrantIfUnused(folder.token, true);
        return;
      }
      const previous = selectedInputFolder?.token;
      selectedInputFolder = folder;
      if (previous && previous !== folder.token) releaseInputFolderGrantIfUnused(previous, true);
    };
  }

  function releaseDirectoryGrantIfUnused(token: string, noLongerSelected: boolean): void {
    if (!noLongerSelected || pendingDirectoryTokens.has(token)) return;
    if ([...jobDirectoryTokens.values()].includes(token)) return;
    void revokeOutputDirectory(token).catch(() => {});
  }

  function releaseInputFolderGrantIfUnused(token: string, noLongerSelected: boolean): void {
    if (!noLongerSelected || pendingInputFolderTokens.has(token)) return;
    if ([...jobInputFolderTokens.values()].includes(token)) return;
    void revokeInputFolder(token).catch(() => {});
  }

  async function cancelBackgroundJob(job: JobSnapshot): Promise<void> {
    cancellingJobId = job.id;
    jobError = '';
    try {
      acceptJobSnapshot(await cancelJob(job.id));
    } catch (error) {
      jobError = messageOf(error);
    } finally {
      cancellingJobId = '';
    }
  }

  function jobStatusText(status: JobSnapshot['status']): string {
    switch (status) {
      case 'queued': return 'Queued';
      case 'running': return 'Running';
      case 'cancelling': return 'Cancelling';
      case 'succeeded': return 'Complete';
      case 'failed': return 'Failed';
      case 'cancelled': return 'Cancelled';
      case 'interrupted': return 'Interrupted';
    }
  }

  function viewJobResult(job: JobSnapshot): void {
    const tool = tools.find((item) => item.id === job.toolId);
    if (!tool || !job.result) return;
    openTool(tool, true);
    activeJobId = job.id;
    activeResult = job.result;
  }

  function isFavorite(toolId: string): boolean {
    return favorites.includes(toolId);
  }

  async function toggleFavorite(tool: ToolSummary, event?: Event): Promise<void> {
    event?.stopPropagation();
    const previous = favorites;
    const next = previous.includes(tool.id) ? previous.filter((id) => id !== tool.id) : [tool.id, ...previous];
    favorites = next;
    favoriteError = '';
    try {
      await setFavorite(tool.id, next.includes(tool.id));
    } catch (error) {
      favorites = previous;
      favoriteError = messageOf(error);
    }
  }

  async function saveAlias(): Promise<void> {
    if (!selectedTool || !aliasValue.trim()) return;
    aliasSaving = true;
    aliasMessage = '';
    try {
      await setAlias(aliasValue.trim(), selectedTool.id);
      aliasMessage = `“${aliasValue.trim()}” now opens ${selectedTool.name}.`;
      aliasValue = '';
    } catch (error) {
      aliasMessage = messageOf(error);
    } finally {
      aliasSaving = false;
    }
  }

  function privacyHintFor(tool: ToolSummary): string {
    return privacyHint(tool.privacyClass);
  }

  function providerSupports(capability: string): boolean {
    return providers.some((provider) => provider.compatible && provider.capabilities?.includes(capability));
  }

  function requiredPdfCapability(tool: ToolSummary): string | null {
    switch (tool.id) {
      case 'arcade.pdf.merge': return 'pdf:merge';
      case 'arcade.pdf.split': return 'pdf:split';
      case 'arcade.pdf.compress': return 'pdf:structural';
      case 'arcade.pdf.watermark': return 'pdf:structural';
      default: return null;
    }
  }

  function openSettings(): void {
    releaseCurrentDirectoryGrant();
    selectedTool = null;
    activeJobId = '';
    activeResult = null;
    surface = 'dashboard';
    catalogMode = 'settings';
    categoryFilter = 'All tools';
    dashboardQuery = '';
  }

  function openEngines(): void {
    releaseCurrentDirectoryGrant();
    selectedTool = null;
    activeJobId = '';
    activeResult = null;
    screenCaptureBusy = false;
    artifactActionBusy = false;
    surface = 'dashboard';
    catalogMode = 'engines';
    categoryFilter = 'All tools';
    dashboardQuery = '';
    focusDashboardEntry();
  }

  function providerIcon(capability: string): IconName {
    if (capability.startsWith('image.')) return 'image';
    if (capability.startsWith('pdf.')) return 'document';
    if (capability.startsWith('media.')) return 'video';
    return 'network';
  }

  /** Text tools that also accept a granted file show a Text / File switch. */
  function hasTextFileSwitch(tool: ToolSummary): boolean {
    return tool.ui?.input.kind === 'text' && tool.inputs.some((input) => input.startsWith('file/'));
  }

  function fileWorkflowActive(tool: ToolSummary): boolean {
    return hasTextFileSwitch(tool) ? textInputMode === 'file' : usesFileInput(tool);
  }

  function fileInputLabel(tool: ToolSummary): string {
    if (tool.id === 'arcade.text.diff') return toolOptions.operation === 'merge' ? 'Original, your version, then other version' : 'Original and changed text files';
    if (tool.id === 'arcade.pdf.watermark') {
      if (toolOptions.mode === 'image') return 'Source PDF, then watermark image';
      if (toolOptions.mode === 'pdf-overlay') return 'Source PDF, then overlay PDF';
      return 'Source PDF';
    }
    if (tool.ui?.version === 1 && tool.ui.input.kind !== 'none') return tool.ui.input.label;
    return 'Input file';
  }

  function canSubmitSelectedTool(): boolean {
    if (!selectedTool || !isRunnable(selectedTool)) return false;
    if (isScreenRecorderTool(selectedTool.id)) {
      return screenStatus?.recordingAvailable !== false
        && !screenRecording?.recording
        && !screenRecording?.starting
        && !screenRecording?.finalizing;
    }
    if (selectedTool.id === 'arcade.system.paste-plain') return plainPasteStatus?.available === true;
    if (isScreenTool(selectedTool.id) && screenStatus?.captureAvailable === false) return false;
    if (isScreenTool(selectedTool.id)) return true;
    const standardProblem = standardUiOptionsProblem(selectedTool.ui, toolOptions);
    if (standardProblem) return false;
    if (selectedTool.id === 'arcade.pdf.watermark') {
      if (selectedFiles[0]?.mime !== 'file/pdf') return false;
      if (toolOptions.mode === 'text') return selectedFiles.length === 1;
      if (toolOptions.mode === 'image') return selectedFiles.length === 2 && selectedFiles[1].mime.startsWith('file/image');
      if (toolOptions.mode === 'pdf-overlay') return selectedFiles.length === 2 && selectedFiles[1].mime === 'file/pdf';
      return false;
    }
    if (selectedTool.id === 'arcade.text.diff' && textInputMode === 'file') return selectedFiles.length === (toolOptions.operation === 'merge' ? 3 : 2);
    if (hasTextFileSwitch(selectedTool)) return textInputMode === 'text' || selectedFiles.length === 1;
    if (selectedTool.ui?.version === 1) {
      const { kind, minItems = kind === 'files' || kind === 'file' ? 1 : 0, maxItems = kind === 'file' ? 1 : undefined } = selectedTool.ui.input;
      if (kind === 'none') return true;
      if (kind === 'file' || kind === 'files') {
        if (selectedFiles.length < minItems || (maxItems !== undefined && selectedFiles.length > maxItems)) return false;
        return selectedFiles.every((file) => acceptsSelectedFile(selectedTool!, file));
      }
      if (kind === 'folder') return selectedInputFolder !== null;
      return acceptsTextInput(selectedTool);
    }
    if (fileWorkflowActive(selectedTool)) return selectedFiles.length === 1;
    return acceptsTextInput(selectedTool);
  }

  function isVideoEditorTool(toolId: string): boolean {
    return toolId.startsWith('arcade.video.') && toolId !== 'arcade.video.inspect';
  }

  function isScreenTool(toolId: string): boolean {
    return ['arcade.screen.screenshot', 'arcade.screen.qr', 'arcade.screen.ocr', 'arcade.screen.color', 'arcade.screen.ruler', 'arcade.screen.pin', 'arcade.screen.recorder'].includes(toolId);
  }

  function isScreenRecorderTool(toolId: string): boolean {
    return toolId === 'arcade.screen.recorder';
  }

  async function chooseInputFile(): Promise<void> {
    const tool = selectedTool;
    if (!tool) return;
    const generation = viewGeneration;
    fileSelectionError = '';
    try {
      const chosen = await selectFiles();
      if (generation !== viewGeneration || selectedTool?.id !== tool.id) return;
      if (chosen.length === 0) return;
      if (tool.id === 'arcade.pdf.watermark') {
        const candidates = [...selectedFiles, ...chosen.filter((file) => !selectedFiles.some((existing) => existing.token === file.token))];
        const mode = toolOptions.mode || 'text';
        if (candidates.length > 2 || candidates[0]?.mime !== 'file/pdf') {
          fileSelectionError = 'Select the source PDF first. Add at most one watermark file after it.';
          return;
        }
        if (mode === 'text' && candidates.length > 1) {
          fileSelectionError = 'Text mode uses only the source PDF. Change the watermark type before adding a second file.';
          return;
        }
        if (mode === 'image' && candidates.length > 1 && !candidates[1].mime.startsWith('file/image')) {
          fileSelectionError = 'Image mode needs a source PDF first, then one image file.';
          return;
        }
        if (mode === 'pdf-overlay' && candidates.length > 1 && candidates[1].mime !== 'file/pdf') {
          fileSelectionError = 'PDF overlay mode needs a source PDF first, then one overlay PDF.';
          return;
        }
        selectedFiles = candidates;
        fileSelectionError = '';
        return;
      }
      const inputSpec = tool.ui?.version === 1 ? tool.ui.input : null;
      const isMultiple = inputSpec?.kind === 'files' || tool.id === 'arcade.pdf.merge' || tool.id === 'arcade.text.diff';
      if (isMultiple) {
        const invalid = chosen.filter((file) => !acceptsSelectedFile(tool, file));
        if (invalid.length > 0) {
          fileSelectionError = `${invalid.map((file) => file.name).join(', ')} ${invalid.length === 1 ? 'is' : 'are'} not compatible with this tool. Choose a compatible file.`;
          return;
        }
        const combined = [...selectedFiles, ...chosen.filter((file) => !selectedFiles.some((existing) => existing.token === file.token))];
        const maximum = tool.id === 'arcade.text.diff' ? (toolOptions.operation === 'merge' ? 3 : 2) : inputSpec?.maxItems ?? 128;
        if (combined.length > maximum) {
          fileSelectionError = `Select no more than ${maximum} files.`;
          return;
        }
        selectedFiles = combined;
        fileSelectionError = '';
        return;
      }
      if (chosen.length !== 1) {
        selectedFiles = [];
        fileSelectionError = 'This action accepts one file at a time. Choose a single file to continue.';
        return;
      }
      if (!acceptsSelectedFile(tool, chosen[0])) {
        selectedFiles = [];
        fileSelectionError = `This action cannot use ${chosen[0].mime}. Choose a compatible file.`;
        return;
      }
      selectedFiles = chosen;
    } catch (error) {
      if (generation === viewGeneration && selectedTool?.id === tool.id) fileSelectionError = messageOf(error);
    }
  }

  function clearInputFolder(): void {
    const token = selectedInputFolder?.token;
    selectedInputFolder = null;
    if (token) releaseInputFolderGrantIfUnused(token, true);
  }

  async function chooseScreenArea(): Promise<void> {
    const tool = selectedTool;
    if (!tool || screenCaptureBusy) return;
    const generation = viewGeneration;
    screenCaptureBusy = true;
    fileSelectionError = '';
    try {
      const capture = await runScreenTool('arcade.screen.screenshot');
      if (generation !== viewGeneration || selectedTool?.id !== tool.id) return;
      if (!capture) return;
      if (capture.status === 'error') throw new Error(capture.message || 'The screen capture could not be completed.');
      const output = capture.outputs.find((item) => item.kind === 'artifact' || item.kind === 'file');
      if (!output) throw new Error('The screen capture did not return an image file.');
      const selected: SelectedFile = {
        token: output.value,
        name: typeof capture.metadata?.outputName === 'string' ? capture.metadata.outputName : 'screen-selection.png',
        size: Number(capture.metadata?.outputBytes) || 0,
        mime: output.mime,
      };
      if (!acceptsSelectedFile(tool, selected)) {
        fileSelectionError = 'This tool does not accept a screen image. Choose another tool or select a compatible file.';
        return;
      }
      const inputSpec = tool.ui?.version === 1 ? tool.ui.input : null;
      if (inputSpec?.kind === 'files') {
        const maxItems = inputSpec.maxItems ?? 128;
        if (selectedFiles.length >= maxItems) {
          fileSelectionError = `Select no more than ${maxItems} files.`;
          return;
        }
        selectedFiles = [...selectedFiles, selected];
      } else {
        selectedFiles = [selected];
      }
    } catch (error) {
      if (generation === viewGeneration && selectedTool?.id === tool.id) fileSelectionError = messageOf(error);
    } finally {
      if (generation === viewGeneration) screenCaptureBusy = false;
    }
  }

  function removeSelectedFileToken(token: string): void {
    selectedFiles = selectedFiles.filter((file) => file.token !== token);
    fileSelectionError = '';
  }

  function reorderSelectedFiles(fromToken: string, toIndex: number): void {
    const fromIndex = selectedFiles.findIndex((file) => file.token === fromToken);
    if (fromIndex < 0 || toIndex < 0 || toIndex >= selectedFiles.length || fromIndex === toIndex) return;
    const ordered = [...selectedFiles];
    const [file] = ordered.splice(fromIndex, 1);
    ordered.splice(toIndex, 0, file);
    selectedFiles = ordered;
  }

  function parseMediaProbe(result: ToolResult | null): MediaProbe | null {
    if (!result || result.status !== 'success') return null;
    const output = result.outputs.find((item) => item.mime === 'structured/json' || item.mime.includes('json'));
    if (!output) return null;
    try {
      const value: unknown = JSON.parse(output.value);
      return value && typeof value === 'object' ? value as MediaProbe : null;
    } catch {
      return null;
    }
  }

  function parseStructuredOutput<T>(result: ToolResult | null, mime: string): T | null {
    if (!result || result.status !== 'success') return null;
    const output = result.outputs.find((item) => item.mime === mime);
    if (!output) return null;
    try {
      const parsed: unknown = JSON.parse(output.value);
      return parsed && typeof parsed === 'object' ? parsed as T : null;
    } catch {
      return null;
    }
  }

  function formatBytes(value: number | string | undefined): string {
    const bytes = typeof value === 'string' ? Number(value) : value;
    if (!Number.isFinite(bytes) || bytes === undefined || bytes < 0) return 'Size unavailable';
    if (bytes < 1024) return `${bytes} B`;
    const units = ['KB', 'MB', 'GB', 'TB'];
    let amount = bytes;
    let unit = 0;
    do { amount /= 1024; unit += 1; } while (amount >= 1024 && unit < units.length);
    return `${amount.toFixed(amount >= 10 ? 1 : 2)} ${units[unit - 1]}`;
  }

  function formatDuration(value: string | undefined): string {
    if (!value) return 'Unknown';
    const seconds = Number(value);
    if (!Number.isFinite(seconds) || seconds < 0) return value;
    const whole = Math.floor(seconds);
    const hours = Math.floor(whole / 3600);
    const minutes = Math.floor((whole % 3600) / 60);
    const remainder = whole % 60;
    return hours > 0 ? `${hours}:${String(minutes).padStart(2, '0')}:${String(remainder).padStart(2, '0')}` : `${minutes}:${String(remainder).padStart(2, '0')}`;
  }

  function frameRate(value: string | undefined): string {
    if (!value || value === '0/0') return '';
    const [numerator, denominator] = value.split('/').map(Number);
    if (denominator && Number.isFinite(numerator) && Number.isFinite(denominator)) return `${(numerator / denominator).toFixed(2)} fps`;
    return value.includes('fps') ? value : `${value} fps`;
  }

  function dynamicRange(stream: MediaStream): string {
    if (stream.color_transfer === 'smpte2084') return 'HDR10 (PQ)';
    if (stream.color_transfer === 'arib-std-b67') return 'HLG';
    return '';
  }

  function bitDepth(stream: MediaStream): string {
    const raw = Number(stream.bits_per_raw_sample);
    if (Number.isFinite(raw) && raw > 8) return `${raw}-bit`;
    const match = stream.pix_fmt?.match(/p(10|12|16)(le|be)?$/);
    return match ? `${match[1]}-bit` : '';
  }

  function streamLabel(stream: MediaStream): string {
    const language = stream.tags?.language && stream.tags.language !== 'und' ? stream.tags.language : '';
    const flags = stream.disposition?.default ? 'default' : stream.disposition?.forced ? 'forced' : '';
    return [stream.tags?.title, language, flags].filter(Boolean).join(' · ');
  }

  function streamDetail(stream: MediaStream): string {
    if (stream.codec_type === 'video') {
      if (stream.disposition?.attached_pic) return 'Cover art';
      const dimensions = stream.width && stream.height ? `${stream.width} × ${stream.height}` : '';
      const rate = frameRate(stream.avg_frame_rate || stream.r_frame_rate);
      return [dimensions, rate, bitDepth(stream), dynamicRange(stream), stream.pix_fmt].filter(Boolean).join(' · ') || 'Video stream';
    }
    if (stream.codec_type === 'audio') {
      const rate = stream.sample_rate ? `${Number(stream.sample_rate) / 1000} kHz` : '';
      const channels = stream.channel_layout || (stream.channels ? `${stream.channels} channels` : '');
      const bitrate = stream.bit_rate ? `${Math.round(Number(stream.bit_rate) / 1000)} kb/s` : '';
      return [rate, channels, bitrate, streamLabel(stream)].filter(Boolean).join(' · ') || 'Audio stream';
    }
    if (stream.codec_type === 'subtitle') {
      return [stream.codec_long_name || stream.codec_name, streamLabel(stream)].filter(Boolean).join(' · ') || 'Subtitle track';
    }
    return stream.codec_long_name || stream.codec_name || 'Media stream';
  }

  function streamCount(probe: MediaProbe, kind: string): number {
    return (probe.streams ?? []).filter((stream) => stream.codec_type === kind && !stream.disposition?.attached_pic).length;
  }

  function resultMetadataText(key: string): string {
    const value = activeResult?.metadata?.[key];
    return typeof value === 'string' ? value : '';
  }

  function artifactOutputs(): ToolResult['outputs'] {
    return fileOutputs;
  }

  function artifactOutputName(index: number): string {
    const names = activeResult?.metadata?.outputNames;
    if (Array.isArray(names) && typeof names[index] === 'string') return names[index];
    if (index === 0) return resultMetadataText('outputName') || (artifactOutputs().length > 1 ? `Output ${index + 1}` : 'Output file');
    return `Output ${index + 1}`;
  }

  function artifactOutputSize(index: number): string {
    const value = activeResult?.metadata?.outputBytes;
    if (Array.isArray(value)) return formatBytes(typeof value[index] === 'number' || typeof value[index] === 'string' ? value[index] : undefined);
    return artifactOutputs().length === 1 ? formatBytes(typeof value === 'number' || typeof value === 'string' ? value : undefined) : '';
  }

  function outputMetadataSize(index: number): number {
    const value = activeResult?.metadata?.outputBytes;
    if (Array.isArray(value)) return Number(value[index]) || 0;
    return artifactOutputs().length === 1 ? Number(value) || 0 : 0;
  }

  function outputAcceptsMime(tool: ToolSummary, mime: string): boolean {
    return tool.inputs.some((accepted) => {
      const normalized = accepted.replace(/\[\]$/, '');
      return normalized === mime
        || normalized === 'file/any'
        || (normalized.endsWith('/*') && mime.startsWith(normalized.slice(0, -1)))
        || (normalized === 'file/media' && (mime === 'file/video' || mime === 'file/audio'))
        || ((normalized === 'network/url' || normalized === 'text/url') && (mime === 'network/url' || mime === 'text/url'))
        || (normalized === 'text' && mime.startsWith('text/'));
    });
  }

  function toolsForOutput(output: ToolResult['outputs'][number]): ToolSummary[] {
    if (!activeResult) return [];
    const textual = output.kind === 'text' || output.kind === 'url' || output.mime.startsWith('text/') || output.mime.startsWith('structured/');
    return tools.filter((tool) => isRunnable(tool)
      && tool.id !== activeResult?.toolId
      && tool.ui?.input.kind !== 'none'
      && (textual ? acceptsTextInput(tool) && outputAcceptsMime(tool, output.mime) : usesFileInput(tool) && outputAcceptsMime(tool, output.mime)));
  }

  function continueWithOutput(tool: ToolSummary, output: ToolResult['outputs'][number], index: number): void {
    const textual = output.kind === 'text' || output.kind === 'url' || output.mime.startsWith('text/') || output.mime.startsWith('structured/');
    const name = artifactOutputName(index);
    const size = outputMetadataSize(index);
    openTool(tool);
    if (textual) {
      inputText = output.value;
      if (output.kind === 'url' && tool.ui?.input.kind !== 'url') toolOptions = defaultOptionsFor(tool);
      return;
    }
    selectedFiles = [{
      token: output.value,
      name,
      size,
      mime: output.mime,
    }];
  }

  function artifactIcon(mime: string): IconName {
    if (mime.startsWith('file/image')) return 'image';
    if (mime.startsWith('file/pdf')) return 'document';
    if (mime.startsWith('file/audio')) return 'audio';
    if (mime.startsWith('file/video')) return 'video';
    return 'file';
  }

  async function showArtifact(token: string, reveal: boolean): Promise<void> {
    const generation = viewGeneration;
    artifactActionError = '';
    artifactActionMessage = '';
    artifactActionBusy = true;
    try {
      if (reveal) await revealArtifact(token);
      else await openArtifact(token);
    } catch (error) {
      if (generation === viewGeneration) artifactActionError = messageOf(error);
    } finally {
      if (generation === viewGeneration) artifactActionBusy = false;
    }
  }

  async function saveArtifact(token: string): Promise<void> {
    const generation = viewGeneration;
    artifactActionError = '';
    artifactActionMessage = '';
    artifactActionBusy = true;
    try {
      const saved = await saveArtifactAs(token);
      if (generation === viewGeneration && saved) artifactActionMessage = `Saved ${saved.name}.`;
    } catch (error) {
      if (generation === viewGeneration) artifactActionError = messageOf(error);
    } finally {
      if (generation === viewGeneration) artifactActionBusy = false;
    }
  }

  async function openScannedLink(value: string): Promise<void> {
    const generation = viewGeneration;
    artifactActionError = '';
    artifactActionMessage = '';
    artifactActionBusy = true;
    try {
      await openReviewedUrl(value);
      if (generation === viewGeneration) artifactActionMessage = 'Opened the reviewed web link.';
    } catch (error) {
      if (generation === viewGeneration) artifactActionError = messageOf(error);
    } finally {
      if (generation === viewGeneration) artifactActionBusy = false;
    }
  }

  function reviewedHttpUrl(value: string): boolean {
    if (!value || value.length > 8192 || /[\u0000-\u001f]/.test(value)) return false;
    try {
      const parsed = new URL(value);
      return (parsed.protocol === 'https:' || parsed.protocol === 'http:')
        && Boolean(parsed.hostname)
        && !parsed.username
        && !parsed.password;
    } catch {
      return false;
    }
  }

  function chooseCategory(category: string): void {
    catalogMode = 'all';
    categoryFilter = category;
  }
</script>

<svelte:window onkeydown={handleWindowKeydown} />

<main class:surface-hidden={!surfaceVisible} class:dismissing={dismissing} class:dashboard-mode={surface === 'dashboard'} class:theme-light={themeMode === 'light'} class:theme-dark={themeMode === 'dark'} class:theme-system={themeMode === 'system'} class="app-frame">
  <div class="sr-only" role="status" aria-live="polite" aria-atomic="true">{searchAnnouncement}</div>
  <div class="sr-only" role="status" aria-live="polite" aria-atomic="true">{resultAnnouncement}</div>
  <span class="sr-only" id="search-keyboard-help">Use the up and down arrow keys to move through available actions, Enter to open or run one, and Escape to clear the search or close Arcade Box.</span>
  {#if surface === 'island'}
    <section bind:this={islandShell} class:expanded={!isCompact} class:compact={isCompact} class:tool-open={Boolean(selectedTool)} class:dismissing={dismissing} style:height={islandHeight ? `${islandHeight}px` : undefined} class="island-shell" aria-label="Arcade Box action surface">
      <div class="island-content" bind:this={islandContent}>
      {#if selectedTool}
        <div class="tool-view" in:fade={{ duration: 140 }}>
          <header class="tool-header">
            <button class="icon-button tool-back" onclick={backFromTool} aria-label="Back to search" title="Back · Alt+←"><Icon name="back" size={16} /></button>
            <div class="tool-symbol" class:planned={!isRunnable(selectedTool)}>
              <Icon name={iconForCategory(selectedTool.category)} size={18} />
            </div>
            <div class="tool-title-block">
              <div class="tool-title-line">
                <h1 bind:this={selectedToolHeading} tabindex="-1">{selectedTool.name}</h1>
                <span class="privacy-tag" class:network={privacyLabel(selectedTool.privacyClass) === 'NETWORK'} class:cloud={privacyLabel(selectedTool.privacyClass) === 'CLOUD'}>{privacyLabel(selectedTool.privacyClass)}</span>
                {#if selectedTool.status !== 'implemented'}<span class="status-tag" class:partial={selectedTool.status === 'partial'}>{statusText(selectedTool)}</span>{/if}
              </div>
              <p>{selectedTool.description}</p>
            </div>
            <button class="icon-button dashboard-toggle" aria-label="Browse all tools" title="Browse all tools" onclick={toggleDashboard}><Icon name="grid" size={16} /></button>
          </header>

          {#if !isRunnable(selectedTool)}
            <div class="notice-card planned-notice">
              <span class="notice-icon"><Icon name="clock" size={18} /></span>
              <div>
                <strong>This tool is planned</strong>
                <p>It is listed for discovery and search, but the local runtime does not expose it yet.</p>
              </div>
            </div>
          {:else if !isScreenTool(selectedTool.id) && !acceptsTextInput(selectedTool) && !usesFileInput(selectedTool) && selectedTool.ui?.input.kind !== 'none' && selectedTool.ui?.input.kind !== 'folder'}
            <div class="notice-card integration-notice">
              <span class="notice-icon"><Icon name="file" size={18} /></span>
              <div>
                <strong>This input is not available in the current view</strong>
                <p>This tool accepts {selectedTool.inputs.join(', ') || 'a non-text input'}.</p>
              </div>
            </div>
          {:else}
            <div class="input-panel">
              {#if selectedTool.id === 'arcade.system.clipboard-history'}
                <ClipboardHistoryView />
              {:else if selectedTool.id === 'arcade.utility.timer'}
                <TimerView />
              {:else}
              {#if isScreenTool(selectedTool.id)}
                <section class="screen-action-status" aria-label="Screen capture availability" aria-live="polite">
                  <div class="screen-status-heading"><span class="runtime-dot" class:offline={(isScreenRecorderTool(selectedTool.id) ? screenStatus?.recordingAvailable : screenStatus?.captureAvailable) === false}></span><strong>{isScreenRecorderTool(selectedTool.id) ? (screenStatus?.recordingAvailable ? 'Screen recording is available' : screenStatus ? 'Screen recording unavailable' : 'Checking screen recording') : (screenStatus?.captureAvailable ? 'Screen selection is available' : screenStatus ? 'Screen selection unavailable' : 'Checking screen capture')}</strong><span>{isScreenRecorderTool(selectedTool.id) ? screenRecording?.platform || screenStatus?.platform || 'checking' : screenStatus?.selectionMode || 'checking'}</span></div>
                  <p>{isScreenRecorderTool(selectedTool.id) ? screenRecording?.message || screenStatus?.recordingMessage || 'Arcade Box is checking screen recording support.' : screenStatus?.message || 'Arcade Box is checking your desktop’s native capture support.'}</p>
                  {#if selectedTool.id === 'arcade.screen.qr'}<small>Decoded destinations are shown first. Arcade Box will not open them automatically.</small>{/if}
                  {#if selectedTool.id === 'arcade.screen.ocr'}<small>Capture and OCR run locally. The selected image is passed through a scoped file grant.</small>{/if}
                  {#if selectedTool.id === 'arcade.screen.color'}<small>Choose a screen area, then click a pixel or use arrow keys to inspect its exact color locally.</small>{/if}
                  {#if isScreenRecorderTool(selectedTool.id) && screenRecording?.recording}
                    <div class="screen-recording-controls">
                      <span><strong>Recording active</strong><small>{Math.floor((screenRecording.elapsedSeconds || 0) / 60).toString().padStart(2, '0')}:{((screenRecording.elapsedSeconds || 0) % 60).toString().padStart(2, '0')} elapsed · closing the Island keeps it running</small></span>
                      <button type="button" class="quiet-button" disabled={screenRecording.finalizing || running} onclick={() => void finishScreenRecording()}><Icon name="check" size={14} /><span>{screenRecording.finalizing ? 'Saving…' : 'Stop & save'}</span></button>
                      <button type="button" class="quiet-button" disabled={screenRecording.finalizing} onclick={() => void discardScreenRecording()}><Icon name="close" size={14} /><span>Discard</span></button>
                    </div>
                  {/if}
                </section>
              {/if}
              {#if selectedTool.id === 'arcade.system.paste-plain'}
                <section class="screen-action-status" aria-label="Plain-text paste availability" aria-live="polite">
                  <div class="screen-status-heading"><span class="runtime-dot" class:offline={plainPasteStatus?.available === false}></span><strong>{plainPasteStatus?.available ? `Ready · ${plainPasteStatus.shortcut}` : plainPasteStatus ? 'Unavailable on this desktop' : 'Checking paste support'}</strong><span>{plainPasteStatus?.platform || 'checking'}</span></div>
                  <p>{plainPasteStatus?.message || 'Arcade Box is checking whether this desktop can send the plain-text paste shortcut.'}</p>
                </section>
              {/if}
              {#if hasTextFileSwitch(selectedTool)}
                <div class="input-mode-switch" role="group" aria-label="Input type">
                  <button class:mode-active={textInputMode === 'text'} aria-pressed={textInputMode === 'text'} onclick={() => { textInputMode = 'text'; selectedFiles = []; fileSelectionError = ''; }}><Icon name="type" size={14} /> Text</button>
                  <button class:mode-active={textInputMode === 'file'} aria-pressed={textInputMode === 'file'} onclick={() => { textInputMode = 'file'; inputText = ''; fileSelectionError = ''; }}><Icon name="file" size={14} /> File</button>
                </div>
              {/if}
              {#if selectedTool.id === 'arcade.text.diff' && !fileWorkflowActive(selectedTool)}
                <DiffInput bind:left={inputText} bind:right={diffRight} bind:base={diffBase} merge={toolOptions.operation === 'merge'} />
              {:else if selectedTool.ui?.input.kind === 'folder'}
                <div class="field-label-row"><span class="field-caption">{selectedTool.ui.input.label}</span><span class="input-type"><Icon name="lock" size={13} /> Selected folder</span></div>
                <SelectedFolderInput
                  value={selectedInputFolder}
                  selectionGeneration={viewGeneration}
                  disabled={running || Boolean(activeJob && jobIsActive(activeJob))}
                  onSelect={inputFolderSelectionHandler(viewGeneration, selectedTool.id)}
                  onClear={clearInputFolder}
                />
                {#if fileSelectionError}<div class="field-error" role="alert">{fileSelectionError}</div>{/if}
              {:else if fileWorkflowActive(selectedTool)}
                <div class="field-label-row"><span class="field-caption">{fileInputLabel(selectedTool)}</span><span class="input-type"><Icon name="lock" size={13} /> Selected files</span></div>
                <SelectedFilesInput
                  files={selectedFiles}
                  disabled={running || Boolean(activeJob && jobIsActive(activeJob))}
                  label={fileInputLabel(selectedTool)}
                  multiple={selectedTool.id === 'arcade.text.diff' || selectedTool.ui?.input.kind === 'files'}
                  sortable={selectedTool.id === 'arcade.text.diff' || (selectedTool.ui?.input.sortable ?? false)}
                  maxItems={selectedTool.id === 'arcade.text.diff' ? (toolOptions.operation === 'merge' ? 3 : 2) : selectedTool.ui?.input.maxItems}
                  onAdd={() => void chooseInputFile()}
                  onRemove={removeSelectedFileToken}
                  onReorder={reorderSelectedFiles}
                  formatSize={formatBytes}
                />
                {#if selectedTool.inputs.includes('file/image') && !selectedTool.id.startsWith('arcade.audio.') && (selectedTool.id !== 'arcade.pdf.watermark' || toolOptions.mode === 'image')}
                  <button type="button" class="screen-capture-button" disabled={screenCaptureBusy || running || Boolean(activeJob && jobIsActive(activeJob))} onclick={() => void chooseScreenArea()}>
                    <Icon name="image" size={14} />
                    <span>{screenCaptureBusy ? 'Waiting for screen selection…' : 'Capture screen content'}</span>
                    <small>Uses your desktop’s capture picker</small>
                  </button>
                {/if}
                {#if selectedTool.id === 'arcade.pdf.watermark'}
                  <p class="field-note">Text mode uses one PDF. Image and PDF overlay modes use the source PDF first and one mark file second. Text marks support {'{page}'} and {'{pages}'} page-number tokens.</p>
                {:else if selectedTool.ui?.version === 1 && selectedTool.ui.input.kind === 'files' && selectedFiles.length < (selectedTool.ui.input.minItems ?? 1)}<p class="field-note">Select at least {selectedTool.ui.input.minItems ?? 1} {(selectedTool.ui.input.minItems ?? 1) === 1 ? 'file' : 'files'}{selectedTool.ui.input.sortable ? '. Their order here controls the result.' : '.'}</p>{/if}
                {@const pdfCapability = requiredPdfCapability(selectedTool)}
                {#if pdfCapability && !loadingCatalog && !providerError && !providerSupports(pdfCapability)}
                  <div class="provider-inline-missing" role="status"><Icon name="document" size={14} /><span>No compatible qpdf provider is currently available. Install qpdf or review provider detection in Engines &amp; Dependencies.</span><button type="button" onclick={openEngines}>View engines</button></div>
                {/if}
                {#if selectedTool.id === 'arcade.pdf.compress'}<p class="field-note">This performs lossless structural optimization. It does not reduce image quality.</p>{/if}
              {:else}
                {#if isScreenTool(selectedTool.id) || selectedTool.ui?.input.kind === 'none'}
                  <p class="field-note no-input-note">{selectedTool.id === 'arcade.system.paste-plain' ? 'Choose Paste as plain text to hide Arcade Box and send the desktop’s common plain-text paste shortcut to the previously focused app. The clipboard itself stays unchanged.' : isScreenRecorderTool(selectedTool.id) ? 'Start a local recording from a screen or window chosen in the system capture dialog. You can close Arcade Island and stop it later.' : isScreenTool(selectedTool.id) ? 'Run this action to open your desktop’s capture picker.' : ''}</p>
                {:else}
                  <div class="field-label-row">
                    <label for="tool-input">{selectedTool.ui?.version === 1 ? selectedTool.ui.input.label : 'Input'}</label>
                    <span class="input-type"><Icon name={selectedTool.ui?.input.kind === 'url' ? 'globe' : 'type'} size={13} /> {selectedTool.ui?.input.kind === 'url' ? 'URL' : 'Text'}</span>
                  </div>
                  {#if selectedTool.ui?.input.kind === 'url'}
                    <input id="tool-input" class="tool-url-input" type="url" bind:value={inputText} placeholder={inputPlaceholder(selectedTool, toolOptions)} spellcheck="false" aria-keyshortcuts="Control+Enter Meta+Enter" onkeydown={(event) => { if ((event.metaKey || event.ctrlKey) && event.key === 'Enter') { event.preventDefault(); void submitTool(); } }} />
                  {:else}
                    <textarea
                      id="tool-input"
                      bind:value={inputText}
                      placeholder={inputPlaceholder(selectedTool, toolOptions)}
                      spellcheck="false"
                      aria-keyshortcuts="Control+Enter Meta+Enter"
                      onkeydown={(event) => {
                        if ((event.metaKey || event.ctrlKey) && event.key === 'Enter') {
                          event.preventDefault();
                          void submitTool();
                        }
                      }}
                    ></textarea>
                  {/if}
                {/if}
              {/if}
              {#if fileSelectionError}<div class="field-error" role="alert">{fileSelectionError}</div>{/if}
              {#if isVideoEditorTool(selectedTool.id) && selectedFiles.length}
                <VideoAssist tool={selectedTool} files={selectedFiles} values={toolOptions} disabled={running || Boolean(activeJob && jobIsActive(activeJob))} onValueChange={(key, value) => { toolOptions[key] = value; }} />
              {:else if selectedTool.id.startsWith('arcade.audio.') && selectedFiles.length}
                <AudioAssist tool={selectedTool} files={selectedFiles} values={toolOptions} disabled={running || Boolean(activeJob && jobIsActive(activeJob))} onValueChange={(key, value) => { toolOptions[key] = value; }} />
              {/if}
              {#if selectedTool.ui?.version === 1}
                <StandardToolForm ui={selectedTool.ui} values={toolOptions} capabilities={providerCapabilities} idPrefix={`tool-${selectedTool.id}`} disabled={running || Boolean(activeJob && jobIsActive(activeJob))} onValueChange={(key, value) => {
                  toolOptions[key] = value;
                  if (selectedTool?.id === 'arcade.pdf.watermark' && key === 'mode' && selectedFiles.length > 1) {
                    const second = selectedFiles[1];
                    const keepSecond = value === 'image'
                      ? second.mime.startsWith('file/image')
                      : value === 'pdf-overlay' && second.mime === 'file/pdf';
                    if (!keepSecond) selectedFiles = selectedFiles.slice(0, 1);
                    fileSelectionError = '';
                  }
                }} />
                {#if standardUiOptionsProblem(selectedTool.ui, toolOptions)}<div class="field-error standard-option-error" role="alert">{standardUiOptionsProblem(selectedTool.ui, toolOptions)}</div>{/if}
              {/if}
              <div class="input-footer">
                <span class="privacy-hint"><Icon name={privacyLabel(selectedTool.privacyClass) === 'LOCAL' ? 'lock' : 'globe'} size={13} /> {privacyLabel(selectedTool.privacyClass)} · {privacyHintFor(selectedTool)}</span>
                <button class="run-button" onclick={() => void submitTool()} disabled={running || !canSubmitSelectedTool() || Boolean(activeJob && jobIsActive(activeJob))}>
                  {#if running}<span class="spinner"></span><span>Starting</span>{:else if activeJob && jobIsActive(activeJob)}<span class="spinner"></span><span>In background</span>{:else}<Icon name={isScreenTool(selectedTool.id) ? 'image' : 'play'} size={15} /><span>{runLabel(selectedTool, toolOptions)}</span>{/if}
                  <kbd>{commandKey} ↵</kbd>
                </button>
              </div>
              {/if}
            </div>

            {#if activeJob && jobIsActive(activeJob)}
              <div class="active-job-card" role="region" aria-label="{selectedTool.name} background job">
                <span class="sr-only" role="status" aria-live="polite" aria-atomic="true">{jobStatusText(activeJob.status)} in the background.</span>
                <span class="active-job-symbol"><span class="spinner"></span></span>
                <div class="active-job-copy"><strong>{jobStatusText(activeJob.status)} in the background</strong><span>{activeJob.message || 'You can close Arcade Island; this work will continue.'}</span><div class="job-progress-track" role="progressbar" aria-label="{selectedTool.name} progress" aria-valuemin="0" aria-valuemax="100" aria-valuenow={activeJob.progress === null ? undefined : Math.round(Math.max(0, Math.min(1, activeJob.progress)) * 100)} aria-valuetext={activeJob.progress === null ? 'Progress is not available' : `${Math.round(Math.max(0, Math.min(1, activeJob.progress)) * 100)} percent`}><span style={`width:${Math.max(0, Math.min(100, (activeJob.progress ?? 0) * 100))}%`}></span></div></div>
                <button class="quiet-button cancel-job-button" disabled={cancellingJobId === activeJob.id || activeJob.status === 'cancelling'} onclick={() => void cancelBackgroundJob(activeJob)}><Icon name="close" size={14} /><span>Cancel</span></button>
              </div>
            {/if}

            {#if runError}
              <div class="notice-card error-notice" role="alert">
                <span class="notice-icon"><Icon name="close" size={18} /></span>
                <div><strong>Couldn’t finish</strong><p>{runError}</p></div>
              </div>
            {/if}

            {#if activeResult && (activeResult.outputs.length > 0 || activeResult.message)}
              <section class="result-panel" class:result-failed={activeResult.status !== 'success'} aria-labelledby="result-heading" aria-live="off">
                <div class="result-heading" id="result-heading">
                  <div class="result-label"><span class="result-check"><Icon name={activeResult.status === 'success' ? 'check' : 'close'} size={14} /></span><div><strong>{activeResult.status === 'success' ? 'Done' : 'Couldn’t finish'}</strong><span>{activeResult.outputs.length} {activeResult.outputs.length === 1 ? 'result' : 'results'}</span></div></div>
                  <span class="result-mime" title="Copy result" role="status">{copyState === 'copied' ? 'Copied' : copyState === 'error' ? 'Copy failed — try again' : `${commandKey} Shift C`}</span>
                </div>
                {#if selectedTool.id === 'arcade.screen.color' || selectedTool.id === 'arcade.screen.ruler'}
                  {@const screenImage = activeResult.outputs.find((output) => output.kind === 'artifact' || output.kind === 'file')}
                  {#if screenImage}
                    {#if selectedTool.id === 'arcade.screen.color'}<ScreenColorSampler token={screenImage.value} />{:else}<ScreenRuler token={screenImage.value} />{/if}
                    <div class="screen-color-source-actions"><span><Icon name="lock" size={13} /> Captured image stays local</span><button type="button" class="quiet-button" disabled={artifactActionBusy} onclick={() => void showArtifact(screenImage.value, false)}><Icon name="external" size={13} /><span>Open capture</span></button><button type="button" class="quiet-button" disabled={artifactActionBusy} onclick={() => void saveArtifact(screenImage.value)}><Icon name="folder" size={13} /><span>Save as</span></button></div>
                  {:else}<div class="field-error" role="alert">The screen capture did not return an image for sampling.</div>{/if}
                {:else if selectedTool.id === 'arcade.video.inspect' && mediaProbe}
                  <div class="media-summary">
                    <div class="media-facts">
                      <div><span>Container</span><strong>{mediaProbe.format?.format_long_name || mediaProbe.format?.format_name || 'Unknown'}</strong></div>
                      <div><span>Duration</span><strong>{formatDuration(mediaProbe.format?.duration)}</strong></div>
                      <div><span>File size</span><strong>{formatBytes(mediaProbe.format?.size)}</strong></div>
                      <div><span>Bitrate</span><strong>{mediaProbe.format?.bit_rate ? `${Math.round(Number(mediaProbe.format.bit_rate) / 1000)} kb/s` : 'Unknown'}</strong></div>
                      <div><span>Tracks</span><strong>{[`${streamCount(mediaProbe, 'video')} video`, `${streamCount(mediaProbe, 'audio')} audio`, streamCount(mediaProbe, 'subtitle') ? `${streamCount(mediaProbe, 'subtitle')} subtitle` : ''].filter(Boolean).join(' · ')}</strong></div>
                      {#if mediaProbe.chapters?.length}<div><span>Chapters</span><strong>{mediaProbe.chapters.length}</strong></div>{/if}
                    </div>
                    <div class="media-stream-list">
                      {#each mediaProbe.streams || [] as stream, index (`${stream.index ?? index}-${stream.codec_type ?? 'stream'}`)}
                        <div class="media-stream-row"><span class="media-stream-icon"><Icon name={stream.codec_type === 'video' ? 'video' : stream.codec_type === 'audio' ? 'audio' : 'file'} size={16} /></span><span class="media-stream-copy"><strong>{stream.codec_type || 'Stream'}{stream.codec_name ? ` · ${stream.codec_name}` : ''}{stream.profile ? ` · ${stream.profile}` : ''}</strong><small>{streamDetail(stream)}</small></span></div>
                      {/each}
                      {#if !mediaProbe.streams?.length}<p class="field-note">No stream details were returned by the media provider.</p>{/if}
                    </div>
                    <details class="technical-details"><summary>Show raw probe data</summary><pre class="output-text">{activeResult.outputs[0]?.value || ''}</pre></details>
                    {#if resultMetadataText('providerPath')}<div class="provider-footnote">Inspected by <code>{resultMetadataText('providerPath')}</code></div>{/if}
                  </div>
                {:else if (selectedTool.id === 'arcade.barcode.decode' || selectedTool.id === 'arcade.screen.qr') && barcodeScan}
                  <div class="barcode-scan-results" aria-label="Decoded barcode results">
                    {#if barcodeScan.results.length === 0}
                      <div class="inspector-empty"><Icon name="image" size={17} /><span>{barcodeScan.message || 'No readable barcode was found in this image.'}</span></div>
                    {:else}
                      {#each barcodeScan.results as barcode, index (`${barcode.format || 'code'}-${index}`)}
                        <article class="barcode-scan-card">
                          <div class="barcode-scan-heading"><span class="barcode-format-tag">{barcode.format || 'Barcode'}</span><span>{barcode.contentType || 'Decoded locally'}</span></div>
                          <pre class="barcode-content">{barcode.text || 'No readable text content'}</pre>
                          <div class="barcode-scan-actions">
                            {#if barcode.text}<button class="quiet-button" onclick={() => void copyOutput(barcode.text || '')}><Icon name={copyState === 'copied' ? 'check' : 'copy'} size={14} /><span>{copyState === 'copied' ? 'Copied' : 'Copy content'}</span></button>{/if}
                            {#if barcode.text && reviewedHttpUrl(barcode.text)}<button class="quiet-button barcode-open-link" disabled={artifactActionBusy} onclick={() => void openScannedLink(barcode.text || '')}><Icon name="external" size={14} /><span>Open reviewed link</span></button>{/if}
                          </div>
                        </article>
                      {/each}
                      <p class="barcode-safety-note"><Icon name="shield" size={13} /> Links stay unopened until you choose Open reviewed link.</p>
                    {/if}
                    {#if artifactActionError}<div class="field-error" role="alert">{artifactActionError}</div>{/if}
                    {#if artifactActionMessage}<div class="pipeline-message" role="status">{artifactActionMessage}</div>{/if}
                  </div>
                {:else if selectedTool.id === 'arcade.system.window-pin' && activeWindowPinCapability}
                  <div class="window-pin-results">
                    <div class="window-pin-state"><span class="window-pin-icon"><Icon name="pin" size={17} /></span><div><strong>{activeWindowPinCapability.available ? 'Ready to pin the active window' : 'Window pinning unavailable'}</strong><p>{activeWindowPinCapability.message}</p></div></div>
                    {#if activeWindowPinCapability.available}
                      <div class="window-pin-actions"><button class="quiet-button window-pin-primary" onclick={() => void changeForegroundWindowPin(true)}><Icon name="pin" size={14} /><span>Keep active window on top</span></button><button class="quiet-button" onclick={() => void changeForegroundWindowPin(false)}><span>Remove always-on-top</span></button></div>
                      <p class="inspector-limitation">Arcade Box hides itself before changing the window that was active, so that window regains focus. The desktop window manager may still refuse the change.</p>
                    {/if}
                  </div>
                {:else if selectedTool.id === 'arcade.system.process' && processList}
                  <div class="process-results">
                    <div class="process-results-heading"><div><strong>{processList.totalMatches} process{processList.totalMatches === 1 ? '' : 'es'}</strong><span>{processList.query ? `Matching “${processList.query}”` : 'Sorted by memory use'}</span></div><button class="quiet-button" disabled={running} onclick={() => void submitTool()}><Icon name="refresh" size={13} /><span>Refresh</span></button></div>
                    {#if processList.processes.length}
                      <div class="process-list" aria-label="Running processes">
                        {#each processList.processes as process (`${process.pid}-${process.startTime}`)}
                          {@const identity = `${process.pid}:${process.startTime}`}
                          <article class="process-row">
                            <div class="process-row-main"><strong>{process.name || 'Unnamed process'}</strong><span>PID {process.pid} · {formatBytes(process.memoryBytes)} memory · {process.cpuPercent.toFixed(1)}% CPU</span>{#if process.executable}<code title={process.executable}>{process.executable}</code>{/if}</div>
                            <div class="process-row-action">
                              {#if processTerminationRequests[identity] === 'requested'}<span class="process-action-status" role="status">Termination requested</span>
                              {:else if process.protected}<span class="process-action-status">Protected</span>
                              {:else}<button class="quiet-button process-terminate-button" disabled={processTerminationRequests[identity] === 'working'} aria-label={`End ${process.name}, PID ${process.pid}`} onclick={() => void requestProcessTermination(process)}><Icon name="close" size={13} /><span>{processTerminationRequests[identity] === 'working' ? 'Sending…' : 'End process'}</span></button>{/if}
                              {#if processTerminationRequests[identity] && processTerminationRequests[identity] !== 'working' && processTerminationRequests[identity] !== 'requested'}<span class="process-action-error" role="alert">{processTerminationRequests[identity]}</span>{/if}
                            </div>
                          </article>
                        {/each}
                      </div>
                    {:else}<div class="inspector-empty"><Icon name="search" size={16} /><span>No process matched this search.</span></div>{/if}
                    {#if processList.truncated}<p class="field-note">Showing the first {processList.listedCount} matches. Add a process name or PID to narrow the list.</p>{/if}
                    {#if processList.limitations?.length}<p class="inspector-limitation">{processList.limitations.join(' ')}</p>{/if}
                    <p class="inspector-limitation">Ending a process can lose unsaved work. Protected core processes cannot be ended here; permissions and platform policy can also block the request.</p>
                  </div>
                {:else if selectedTool.id === 'arcade.system.system-info' && systemInformation}
                  <div class="system-info-results">
                    <div class="system-fact-grid">
                      <div><span>Operating system</span><strong>{[systemInformation.operatingSystem?.name, systemInformation.operatingSystem?.version].filter(Boolean).join(' ') || 'Unknown'}</strong><small>{systemInformation.operatingSystem?.kernel || systemInformation.operatingSystem?.longVersion || systemInformation.operatingSystem?.family || ''}</small></div>
                      <div><span>Processor</span><strong>{systemInformation.processor?.model || 'Processor details unavailable'}</strong><small>{[systemInformation.processor?.physicalCores ? `${systemInformation.processor.physicalCores} physical` : '', systemInformation.processor?.logicalCores ? `${systemInformation.processor.logicalCores} logical cores` : ''].filter(Boolean).join(' · ')}</small></div>
                      <div><span>Memory</span><strong>{formatBytes(systemInformation.memory?.totalBytes)}</strong><small>{systemInformation.memory?.availableBytes === undefined ? 'Available memory unavailable' : `${formatBytes(systemInformation.memory.availableBytes)} available`}</small></div>
                      <div><span>Architecture</span><strong>{systemInformation.operatingSystem?.architecture || 'Unknown'}</strong><small>{systemInformation.operatingSystem?.hostName ? `Device: ${systemInformation.operatingSystem.hostName}` : ''}</small></div>
                    </div>
                    {#if systemInformation.storage?.length}
                      <details class="inspector-section"><summary>Storage volumes <span>{systemInformation.storage.length}</span></summary><div class="inspector-row-list">{#each systemInformation.storage as volume, index (`${volume.mountPoint}-${index}`)}<div><strong>{volume.mountPoint || volume.name || `Volume ${index + 1}`}</strong><span>{formatBytes(volume.availableBytes)} free of {formatBytes(volume.totalBytes)}{volume.fileSystem ? ` · ${volume.fileSystem}` : ''}</span></div>{/each}</div></details>
                    {/if}
                    {#if systemInformation.networkInterfaces?.length}
                      <details class="inspector-section"><summary>Network interfaces <span>{systemInformation.networkInterfaces.length}</span></summary><div class="inspector-row-list">{#each systemInformation.networkInterfaces as network, index (`${network.name}-${index}`)}<div><strong>{network.name || `Interface ${index + 1}`}</strong><span>Received {formatBytes(network.receivedBytes)} · Sent {formatBytes(network.transmittedBytes)}</span></div>{/each}</div></details>
                    {/if}
                    {#if systemInformation.limitations?.length}<p class="inspector-limitation">{systemInformation.limitations.join(' ')}</p>{/if}
                    <button class="copy-button" onclick={() => void copyOutput(activeResult?.outputs[0]?.value || '')}><Icon name={copyState === 'copied' ? 'check' : 'copy'} size={14} />{copyState === 'copied' ? 'Diagnostics copied' : 'Copy diagnostics'}</button>
                  </div>
                {:else if artifactOutputs().length}
                  <div class="artifact-result-list" aria-label="Generated files">
                    {#each artifactOutputs().slice(outputPage * 20, (outputPage + 1) * 20) as artifact, pageIndex (`${artifact.value}-${pageIndex}`)}
                      {@const index = outputPage * 20 + pageIndex}
                      {@const size = artifactOutputSize(index)}
                      {@const nextTools = toolsForOutput(artifact)}
                      <div class="artifact-output-entry">
                        {#if artifact.mime.startsWith('file/image') || artifact.mime === 'file/svg'}
                          <ImageResult output={artifact} name={artifactOutputName(index)} {size} busy={artifactActionBusy} onSave={() => saveArtifact(artifact.value)} onOpen={() => showArtifact(artifact.value, false)} onReveal={() => showArtifact(artifact.value, true)} />
                        {:else}
                        <div class="artifact-result-card">
                          <span class="artifact-file-icon"><Icon name={artifactIcon(artifact.mime)} size={20} /></span>
                          <div class="artifact-result-copy"><strong>{artifactOutputName(index)}</strong><span>{#if size}{size} <span aria-hidden="true">·</span> {/if}{artifact.mime}</span></div>
                          <div class="artifact-actions"><button class="quiet-button" aria-label={`Open ${artifactOutputName(index)}`} disabled={artifactActionBusy} onclick={() => void showArtifact(artifact.value, false)}><Icon name="external" size={14} /><span>Open</span></button><button class="quiet-button" aria-label={`Save ${artifactOutputName(index)} as`} disabled={artifactActionBusy} onclick={() => void saveArtifact(artifact.value)}><Icon name="folder" size={14} /><span>Save as</span></button><button class="quiet-button" aria-label={`Reveal ${artifactOutputName(index)} in folder`} disabled={artifactActionBusy} onclick={() => void showArtifact(artifact.value, true)}><Icon name="folder" size={14} /><span>Reveal</span></button></div>
                        </div>
                        {/if}
                        {#if nextTools.length}<details class="result-next-actions"><summary>Continue in another tool <span>{nextTools.length}</span></summary><div>{#each nextTools as nextTool (nextTool.id)}<button onclick={() => continueWithOutput(nextTool, artifact, index)}><Icon name={iconForCategory(nextTool.category)} size={13} /><span>{nextTool.name}</span></button>{/each}</div></details>{/if}
                      </div>
                    {/each}
                  </div>
                  {#if fileOutputs.length > 20}
                    <div class="artifact-pagination"><button class="quiet-button" disabled={outputPage === 0} onclick={() => outputPage--}>Previous files</button><span>{outputPage * 20 + 1}–{Math.min((outputPage + 1) * 20, fileOutputs.length)} of {fileOutputs.length}</span><button class="quiet-button" disabled={(outputPage + 1) * 20 >= fileOutputs.length} onclick={() => outputPage++}>Next files</button></div>
                  {/if}
                  {#each activeResult.outputs.filter((output) => output.kind === 'text' || output.kind === 'url') as output}
                    {#if output.mime === 'structured/audio-loudness' || output.mime === 'structured/audio-metadata'}
                      <ToolOutputView {output} toolId={selectedTool.id} />
                    {:else}
                      <details class="technical-details"><summary>Result details</summary><ToolOutputView {output} toolId={selectedTool.id} /></details>
                    {/if}
                  {/each}
                  {#if resultMetadataText('providerPath')}<details class="technical-details"><summary>Provider details</summary><div class="provider-footnote"><code>{resultMetadataText('providerPath')}</code></div></details>{/if}
                  {#if artifactActionError}<div class="field-error" role="alert">{artifactActionError}</div>{/if}
                  {#if artifactActionMessage}<div class="pipeline-message" role="status">{artifactActionMessage}</div>{/if}
                {:else}
                  {#each activeResult.outputs as output, index}
                    <ToolOutputView {output} toolId={selectedTool.id} />
                    {@const nextTools = toolsForOutput(output)}
                    {#if nextTools.length}<details class="result-next-actions"><summary>Continue in another tool <span>{nextTools.length}</span></summary><div>{#each nextTools as nextTool (nextTool.id)}<button onclick={() => continueWithOutput(nextTool, output, index)}><Icon name={iconForCategory(nextTool.category)} size={13} /><span>{nextTool.name}</span></button>{/each}</div></details>{/if}
                  {/each}
                {/if}
                {#if activeResult.message}<p class="result-message">{activeResult.message}</p>{/if}
                {#if activeResult.warnings?.length}
                  <div class="result-warnings">{#each activeResult.warnings as warning}<span><Icon name="dots" size={13} />{warning}</span>{/each}</div>
                {/if}
              </section>
            {/if}
          {/if}
          {#if isRunnable(selectedTool)}
            <details class="alias-settings">
              <summary><Icon name="command" size={14} /><span>Set a search alias</span><Icon name="chevron" size={13} /></summary>
              <form class="alias-form" onsubmit={(event) => { event.preventDefault(); void saveAlias(); }}>
                <label for="tool-alias">Type a short phrase to open this tool</label>
                <div><input id="tool-alias" bind:value={aliasValue} maxlength="80" placeholder="e.g. clean text" /><button type="submit" disabled={aliasSaving || !aliasValue.trim()}>{aliasSaving ? 'Saving…' : 'Save alias'}</button></div>
                {#if aliasMessage}<span class="alias-message" role="status">{aliasMessage}</span>{/if}
              </form>
            </details>
          {/if}

        </div>
      {:else}
        {#if pendingLink}
          <div class="link-pending" role="status">
            <span>From {arcadeAppName(pendingLink.source)}: {pendingLink.files.length ? `${pendingLink.files.length === 1 ? pendingLink.files[0].name : `${pendingLink.files.length} files`}` : 'text'}. Choose a tool to use it.</span>
            <button class="clear-search" aria-label="Discard the input from the other app" onclick={() => (pendingLink = null)}><Icon name="close" size={14} /></button>
          </div>
        {/if}
        <div class="search-row" class:has-query={query.trim().length > 0}>
          <span class="search-leading"><Icon name="search" size={20} /></span>
          <input
            bind:this={searchInput}
            bind:value={query}
            onfocus={() => (focused = true)}
            onblur={(event) => {
              const nextFocus = event.relatedTarget;
              focused = nextFocus instanceof Node && Boolean(islandShell?.contains(nextFocus));
            }}
            onkeydown={handleSearchKeydown}
            aria-label="Search tools and actions"
            aria-describedby="search-keyboard-help"
            aria-keyshortcuts="ArrowDown ArrowUp Enter Escape"
            autocomplete="off"
            autocapitalize="off"
            spellcheck="false"
            placeholder="What do you need done?"
          />
          {#if query}
            <button class="clear-search" aria-label="Clear search" onclick={() => { query = ''; searchInput?.focus(); }}><Icon name="close" size={16} /></button>
          {:else}
            <button class="icon-button search-browse" aria-label="Browse all tools" title="Browse all tools" onclick={toggleDashboard}><Icon name="grid" size={17} /></button>
          {/if}
        </div>

        {#if query.trim()}
          <div class="search-panel" id="search-list" role="region" aria-label="Tool search results">
            {#if searchError}
              <div class="search-state error-state"><span class="state-symbol"><Icon name="network" size={18} /></span><div><strong>Local runtime unavailable</strong><span>{searchError}</span></div></div>
            {:else if searchResults.length === 0 && loadingSearch}
              <div class="search-state search-pending"><span class="spinner"></span><span>Finding the right action…</span></div>
            {:else if searchResults.length === 0}
              <div class="search-state empty-state"><span class="state-symbol"><Icon name="search" size={17} /></span><div><strong>No matching tools</strong><span>Try another phrase or browse the catalog.</span></div></div>
            {:else}
              {#if runnableSearchResults.length > 0}
                <div class="result-group-label"><span>Actions</span><span>{runnableSearchResults.length}</span></div>
                <div class="search-results">
                  {#each runnableSearchResults as tool, index (tool.id)}
                    <div class="search-result-row">
                      <button
                        id={`tool-${tool.id}`}
                        class="search-result"
                        class:active={index === selectedIndex}
                        onclick={() => openTool(tool)}
                        onmouseenter={() => (selectedIndex = index)}
                      >
                        <span class="result-tool-icon"><Icon name={iconForCategory(tool.category)} size={18} /></span>
                        <span class="result-tool-copy"><strong>{tool.name}</strong><span>{tool.description}</span></span>
                        <span class="result-meta">{#if tool.status === 'partial'}<span class="availability-label">In progress</span>{/if}<span class="privacy-mini" class:network={privacyLabel(tool.privacyClass) === 'NETWORK'} class:cloud={privacyLabel(tool.privacyClass) === 'CLOUD'}>{privacyLabel(tool.privacyClass)}</span><Icon name="chevron" size={15} /></span>
                      </button>
                      <button class="favorite-toggle search-favorite" class:favorite-on={isFavorite(tool.id)} aria-label={isFavorite(tool.id) ? `Remove ${tool.name} from favorites` : `Add ${tool.name} to favorites`} aria-pressed={isFavorite(tool.id)} title={isFavorite(tool.id) ? 'Remove from favorites' : 'Add to favorites'} onclick={(event) => void toggleFavorite(tool, event)}><Icon name="star" size={15} /></button>
                    </div>
                  {/each}
                </div>
              {/if}
              {#if plannedSearchResults.length > 0}
                <div class="result-group-label discovery-label"><span>Planned</span><span>{plannedSearchResults.length}</span></div>
                <div class="search-results planned-results">
                  {#each plannedSearchResults as tool (tool.id)}
                    <div class="search-result-row">
                      <button class="search-result planned-result" onclick={() => openTool(tool)}>
                        <span class="result-tool-icon planned-icon"><Icon name={iconForCategory(tool.category)} size={18} /></span>
                        <span class="result-tool-copy"><strong>{tool.name}</strong><span>{tool.description}</span></span>
                        <span class="planned-label">Planned</span>
                      </button>
                      <button class="favorite-toggle search-favorite" class:favorite-on={isFavorite(tool.id)} aria-label={isFavorite(tool.id) ? `Remove ${tool.name} from favorites` : `Add ${tool.name} to favorites`} aria-pressed={isFavorite(tool.id)} title={isFavorite(tool.id) ? 'Remove from favorites' : 'Add to favorites'} onclick={(event) => void toggleFavorite(tool, event)}><Icon name="star" size={15} /></button>
                    </div>
                  {/each}
                </div>
              {/if}
            {/if}
          </div>
        {:else if focused}
          {#if runnableContextSuggestions.length > 0}
            <div class="context-panel" id="context-list" role="region" aria-label="Actions for the current clipboard">
              <div class="context-label"><Icon name="spark" size={14} /><span>From your clipboard</span><span class="local-chip">Local</span></div>
              {#each runnableContextSuggestions as suggestion, index}
                {@const suggestedTool = tools.find((tool) => tool.id === suggestion.toolId)}
                {#if suggestedTool}
                  <button id={`context-${suggestion.toolId}`} class="context-suggestion" class:active={index === selectedIndex} onclick={() => selectContextSuggestion(suggestion)} onmouseenter={() => (selectedIndex = index)}>
                    <span class="result-tool-icon"><Icon name={iconForCategory(suggestedTool.category)} size={17} /></span>
                    <span class="result-tool-copy"><strong>{suggestedTool.name}</strong><span>{suggestion.reason}</span></span>
                    <Icon name="chevron" size={15} />
                  </button>
                {/if}
              {/each}
            </div>
          {:else}
            <div class="welcome-prompt">
              <div class="welcome-orbit"><Icon name="spark" size={21} /></div>
              <div><strong>Ready when you are.</strong><span>Search for a task, or browse your tools.</span></div>
            </div>
          {/if}
        {/if}

        {#if catalogError && !query.trim()}
          <div class="runtime-notice" role="status"><span class="runtime-notice-dot"></span><span>{catalogError}</span></div>
        {/if}
      {/if}

      <footer class="island-footer">
        <div class="footer-left">{#if selectedTool}<span class="key-hint"><kbd>Alt</kbd><kbd>←</kbd><span>back</span></span>{:else}<span class="key-hint"><kbd>↑</kbd><kbd>↓</kbd><span>navigate</span></span><span class="key-hint"><kbd>↵</kbd><span>open</span></span>{/if}</div>
        <div class="footer-right"><span class="privacy-footer"><span></span>Private by default</span><button class="dismiss-button" onclick={() => void dismissIsland()} aria-label="Close Arcade Box"><kbd>Esc</kbd><span>close</span></button></div>
      </footer>
      </div>
    </section>
  {:else}
    <section class="dashboard-shell" aria-label="Arcade Box dashboard">
      <aside class="sidebar">
        <div class="sidebar-brand"><span class="brand-mark"><Icon name="command" size={20} /></span><span class="brand-name">Arcade <span>Box</span></span></div>
        <div class="sidebar-section-label">Workspace</div>
        <nav class="main-nav" aria-label="Workspace">
          <button class:nav-active={catalogMode === 'all' && categoryFilter === 'All tools'} aria-current={catalogMode === 'all' && categoryFilter === 'All tools' ? 'page' : undefined} onclick={() => { catalogMode = 'all'; categoryFilter = 'All tools'; }}><Icon name="grid" size={17} /><span>All tools</span><span class="nav-count">{tools.length}</span></button>
          <button class:nav-active={catalogMode === 'favorites'} aria-current={catalogMode === 'favorites' ? 'page' : undefined} onclick={() => { catalogMode = 'favorites'; categoryFilter = 'All tools'; }}><Icon name="star" size={17} /><span>Favorites</span>{#if favorites.length}<span class="nav-count">{favorites.length}</span>{/if}</button>
          <button class:nav-active={catalogMode === 'recent'} aria-current={catalogMode === 'recent' ? 'page' : undefined} onclick={() => { catalogMode = 'recent'; categoryFilter = 'All tools'; }}><Icon name="clock" size={17} /><span>Recently used</span>{#if recentToolIds.length}<span class="nav-count">{recentToolIds.length}</span>{/if}</button>
          <button class:nav-active={catalogMode === 'jobs'} aria-current={catalogMode === 'jobs' ? 'page' : undefined} onclick={() => { catalogMode = 'jobs'; categoryFilter = 'All tools'; }}><Icon name="play" size={17} /><span>Background jobs</span>{#if activeJobCount}<span class="nav-count">{activeJobCount}</span>{/if}</button>
          <button class:nav-active={catalogMode === 'pipelines'} aria-current={catalogMode === 'pipelines' ? 'page' : undefined} onclick={() => { catalogMode = 'pipelines'; categoryFilter = 'All tools'; }}><Icon name="spark" size={17} /><span>Pipelines</span></button>
          <button class:nav-active={catalogMode === 'plugins'} aria-current={catalogMode === 'plugins' ? 'page' : undefined} onclick={() => { catalogMode = 'plugins'; categoryFilter = 'All tools'; }}><Icon name="shield" size={17} /><span>Plugins</span></button>
          <button class:nav-active={catalogMode === 'engines'} aria-current={catalogMode === 'engines' ? 'page' : undefined} onclick={() => { catalogMode = 'engines'; categoryFilter = 'All tools'; }}><Icon name="network" size={17} /><span>Engines &amp; dependencies</span><span class="nav-count">{providers.length}</span></button>
          <button class:nav-active={catalogMode === 'settings'} aria-current={catalogMode === 'settings' ? 'page' : undefined} onclick={() => { catalogMode = 'settings'; categoryFilter = 'All tools'; }}><Icon name="settings" size={17} /><span>Settings</span></button>
        </nav>
        <div class="sidebar-section-row"><span class="sidebar-section-label">Categories</span><button aria-label="Show all categories" onclick={() => { catalogMode = 'all'; categoryFilter = 'All tools'; }}><Icon name="plus" size={14} /></button></div>
        <nav class="category-nav" aria-label="Tool categories">
          {#each categories as category}
            <button class:nav-active={categoryFilter === category} aria-current={categoryFilter === category ? 'page' : undefined} onclick={() => chooseCategory(category)}><Icon name={iconForCategory(category)} size={16} /><span>{category}</span></button>
          {/each}
          {#if categories.length === 0 && !loadingCatalog}
            <span class="sidebar-empty">Categories will appear when the local catalog connects.</span>
          {/if}
        </nav>
        <div class="sidebar-bottom">
          <div class="profile-button"><span class="profile-avatar">A</span><span><strong>Arcade Box</strong><small>Desktop workspace</small></span><Icon name="dots" size={17} /></div>
        </div>
      </aside>

      <div class="dashboard-main">
        <div class="sr-only" role="status" aria-live="polite" aria-atomic="true">{dashboardSearchAnnouncement}</div>
        <header class="dashboard-topbar">
          <div class="breadcrumbs"><span>Arcade Box</span><Icon name="chevron" size={14} /><strong>{catalogMode === 'recent' ? 'Recently used' : catalogMode === 'favorites' ? 'Favorites' : catalogMode === 'jobs' ? 'Background jobs' : catalogMode === 'engines' ? 'Engines & dependencies' : catalogMode === 'pipelines' ? 'Pipelines' : catalogMode === 'plugins' ? 'Plugins' : catalogMode === 'settings' ? 'Settings' : categoryFilter}</strong></div>
          <div class="topbar-actions"><span class="sync-state"><span class="runtime-dot" class:offline={Boolean(catalogError)} aria-hidden="true"></span>{catalogError ? 'Runtime offline' : 'Local runtime'}</span><button bind:this={dashboardCloseButton} class="quiet-button" aria-label="Close Arcade Box dashboard" onclick={toggleDashboard}><Icon name="close" size={16} /><span>Close</span></button></div>
        </header>

        <div class="dashboard-content">
          <section class="dashboard-intro">
            <div class="intro-copy"><h1 aria-live="polite" aria-atomic="true">{catalogMode === 'recent' ? 'Recently used' : catalogMode === 'favorites' ? 'Favorites' : catalogMode === 'jobs' ? 'Background jobs' : catalogMode === 'engines' ? 'Engines' : catalogMode === 'pipelines' ? 'Pipelines' : catalogMode === 'plugins' ? 'Plugins' : catalogMode === 'settings' ? 'Settings' : categoryFilter === 'All tools' ? 'Tools' : categoryFilter}</h1><p>{catalogMode === 'recent' ? 'Your recent actions are saved on this device.' : catalogMode === 'favorites' ? 'The tools you want close at hand.' : catalogMode === 'jobs' ? 'Long-running work continues while the Island is closed.' : catalogMode === 'engines' ? 'Providers detected by the local runtime.' : catalogMode === 'pipelines' ? 'Save compatible actions as one reusable workflow.' : catalogMode === 'plugins' ? 'Review permissions before installing community tools.' : catalogMode === 'settings' ? 'Appearance, shortcuts, and privacy controls for this device.' : 'Find a tool. Get it done. Get back to your day.'}</p></div>
          </section>

          {#if catalogMode !== 'engines' && catalogMode !== 'jobs' && catalogMode !== 'pipelines' && catalogMode !== 'plugins' && catalogMode !== 'settings'}
            <div class="dashboard-toolbar">
              <div class="catalog-search"><Icon name="search" size={18} /><input bind:this={catalogSearchInput} bind:value={dashboardQuery} aria-label="Filter tool catalog" placeholder="Search the catalog…" /><kbd>{commandKey} K</kbd></div>
              <div class="catalog-summary"><span class="summary-dot"></span>{implementedTools.length} ready{#if partialTools.length}<span class="summary-divider"></span>{partialTools.length} in progress{/if}<span class="summary-divider"></span>{plannedTools.length} planned</div>
            </div>
          {/if}

          {#if catalogMode === 'pipelines'}
            <PipelinesView tools={tools} onToolsChanged={acceptToolCatalog} />
          {:else if catalogMode === 'plugins'}
            <PluginManager onToolsChanged={acceptToolCatalog} />
          {:else if catalogMode === 'settings'}
            <SettingsView theme={themeMode} shortcut={shortcutInfo} onThemeChange={changeTheme} onShortcutChange={acceptShortcutStatus} />
          {:else if catalogMode === 'jobs'}
            {#if jobError}<div class="dashboard-alert" role="alert"><span class="alert-icon"><Icon name="network" size={16} /></span><div><strong>Could not update job</strong><span>{jobError}</span></div></div>{/if}
            {#if backgroundJobs.length === 0}
              <div class="provider-empty"><span class="provider-empty-icon"><Icon name="clock" size={20} /></span><div><strong>No background jobs yet</strong><span>Video conversions and other long operations will appear here.</span></div></div>
            {:else}
              <section class="catalog-section provider-section">
                <div class="catalog-section-heading"><div><h2>Recent jobs</h2><span>Jobs remain available after closing Arcade Island</span></div><span class="section-count">{backgroundJobs.length}</span></div>
                <div class="job-list">
                  {#each backgroundJobs as job (job.id)}
                    {@const jobTool = tools.find((tool) => tool.id === job.toolId)}
                    <article class="job-card">
                      <span class="job-icon"><Icon name={jobTool ? iconForCategory(jobTool.category) : 'spark'} size={17} /></span>
                      <div class="job-copy"><div class="job-title-row"><strong>{jobTool?.name || job.toolId}</strong><span class="job-status" class:job-status-done={job.status === 'succeeded'} class:job-status-error={job.status === 'failed' || job.status === 'interrupted' || job.status === 'cancelled'}>{jobStatusText(job.status)}</span></div><span>{job.message || (jobIsActive(job) ? 'Working in the background' : 'Job finished')}</span>{#if jobIsActive(job)}<div class="job-progress-track" role="progressbar" aria-label="{jobTool?.name || 'Background operation'} progress" aria-valuemin="0" aria-valuemax="100" aria-valuenow={job.progress === null ? undefined : Math.round(Math.max(0, Math.min(1, job.progress)) * 100)} aria-valuetext={job.progress === null ? 'Progress is not available' : `${Math.round(Math.max(0, Math.min(1, job.progress)) * 100)} percent`}><span style={`width:${Math.max(0, Math.min(100, (job.progress ?? 0) * 100))}%`}></span></div>{/if}</div>
                      <div class="job-actions">
                        {#if jobIsActive(job)}<button class="quiet-button cancel-job-button" disabled={cancellingJobId === job.id || job.status === 'cancelling'} onclick={() => void cancelBackgroundJob(job)}><Icon name="close" size={14} /><span>{cancellingJobId === job.id || job.status === 'cancelling' ? 'Stopping' : 'Cancel'}</span></button>{/if}
                        {#if job.result && jobTool}<button class="quiet-button" onclick={() => viewJobResult(job)}><Icon name="external" size={14} /><span>View result</span></button>{/if}
                      </div>
                    </article>
                  {/each}
                </div>
              </section>
            {/if}
          {:else if catalogMode === 'engines'}
            {#if providerError}
              <div class="dashboard-alert" role="alert"><span class="alert-icon"><Icon name="network" size={16} /></span><div><strong>Could not inspect providers</strong><span>{providerError}</span></div></div>
            {/if}
            {#if providers.length === 0}
              <div class="provider-empty"><span class="provider-empty-icon"><Icon name="network" size={20} /></span><div><strong>No providers detected</strong><span>Arcade Box checks for supported system tools such as FFmpeg, qpdf, and libvips when the runtime starts.</span></div></div>
            {:else}
              <section class="catalog-section provider-section">
                <div class="catalog-section-heading"><div><h2>Detected providers</h2><span>System installations are shown with their verified paths</span></div><span class="section-count">{providers.length}</span></div>
                <div class="provider-list">
                  {#each providers as provider (`${provider.capability}-${provider.executablePath}`)}
                    <article class="provider-card">
                      <span class="provider-icon"><Icon name={providerIcon(provider.capability)} size={19} /></span>
                      <div class="provider-copy"><div class="provider-title-row"><strong>{provider.capability}</strong><span class="provider-source">{provider.source === 'system' ? 'System installation' : provider.source}</span></div><span class="provider-version">Version {provider.version}</span><code>{provider.executablePath}</code>{#if provider.warning}<span class="provider-warning">{provider.warning}</span>{/if}{#if provider.capabilities?.length}<details class="provider-capabilities"><summary>Capabilities · {provider.capabilities.length}</summary><div>{#each provider.capabilities as capability}<span>{capability}</span>{/each}</div></details>{/if}</div>
                      <span class="provider-status" class:provider-incompatible={!provider.compatible}><span></span>{provider.compatible ? 'Compatible' : 'Needs attention'}</span>
                    </article>
                  {/each}
                </div>
              </section>
            {/if}
          {:else if catalogError}
            <div class="dashboard-alert" role="alert"><span class="alert-icon"><Icon name="network" size={16} /></span><div><strong>Local tool runtime unavailable</strong><span>{catalogError}</span></div></div>
          {/if}

          {#if catalogMode === 'settings'}
            <!-- Settings are rendered from persisted preferences above. -->
          {:else if catalogMode === 'engines' || catalogMode === 'jobs' || catalogMode === 'pipelines' || catalogMode === 'plugins'}
            <!-- Specialized views is rendered above from live provider discovery. -->
          {:else if loadingCatalog}
            <div class="catalog-loading"><span class="spinner"></span><span>Connecting to the local catalog…</span></div>
          {:else if visibleDashboardTools.length === 0}
            <div class="catalog-empty">
              <div class="empty-icon"><Icon name={catalogMode === 'recent' ? 'clock' : catalogMode === 'favorites' ? 'star' : 'search'} size={21} /></div>
              <strong>{catalogMode === 'recent' ? 'Nothing here yet' : catalogMode === 'favorites' ? 'No favorites yet' : catalogError ? 'Catalog is waiting for the desktop core' : 'No tools match this view'}</strong>
              <span>{catalogMode === 'recent' ? 'Run an action from the Island and it will appear here.' : catalogMode === 'favorites' ? 'Use the star beside a tool to keep it close.' : catalogError ? 'The frontend connects to the Rust tool registry when Arcade Box starts as a desktop app.' : 'Try another search or choose a different category.'}</span>
              {#if catalogMode === 'recent'}<button class="text-action" onclick={toggleDashboard}>Go to all tools <Icon name="arrow" size={14} /></button>{/if}
            </div>
          {:else}
            {@const dashboardReady = visibleDashboardTools.filter(isRunnable)}
            {@const dashboardPlanned = visibleDashboardTools.filter((tool) => !isRunnable(tool))}
            {#if dashboardReady.length > 0}
              <section class="catalog-section">
                <div class="catalog-section-heading"><div><h2>{catalogMode === 'recent' ? 'Recent actions' : catalogMode === 'favorites' ? 'Favorite actions' : 'Tools'}</h2><span>Some tools require an installed engine. Open a tool for details.</span></div><span class="section-count">{dashboardReady.length}</span></div>
                <div class="tool-grid">
                  {#each dashboardReady as tool (tool.id)}
                    <div class="catalog-card-wrap">
                      <button class="catalog-card" onclick={() => openTool(tool, true)}>
                        <span class="catalog-card-top"><span class="catalog-icon"><Icon name={iconForCategory(tool.category)} size={19} /></span><span class="privacy-tag" class:network={privacyLabel(tool.privacyClass) === 'NETWORK'} class:cloud={privacyLabel(tool.privacyClass) === 'CLOUD'}>{privacyLabel(tool.privacyClass)}</span></span>
                        <span class="catalog-card-title">{tool.name}</span><span class="catalog-card-description">{tool.description}</span>
                        <span class="catalog-card-bottom"><span>{tool.category}</span>{#if tool.status === 'partial'}<span class="availability-label">In progress</span>{/if}<span class="card-arrow"><Icon name="arrow" size={14} /></span></span>
                      </button>
                      <button class="favorite-toggle card-favorite" class:favorite-on={isFavorite(tool.id)} aria-label={isFavorite(tool.id) ? `Remove ${tool.name} from favorites` : `Add ${tool.name} to favorites`} aria-pressed={isFavorite(tool.id)} title={isFavorite(tool.id) ? 'Remove from favorites' : 'Add to favorites'} onclick={(event) => void toggleFavorite(tool, event)}><Icon name="star" size={15} /></button>
                    </div>
                  {/each}
                </div>
              </section>
            {/if}
            {#if dashboardPlanned.length > 0}
              <section class="catalog-section planned-section">
                <div class="catalog-section-heading"><div><h2>Coming to Arcade Box</h2><span>Visible for discovery, unavailable to run</span></div><span class="section-count muted-count">{dashboardPlanned.length}</span></div>
                <div class="planned-grid">
                  {#each dashboardPlanned as tool (tool.id)}
                    <button class="planned-card" onclick={() => openTool(tool, true)}>
                      <span class="planned-card-icon"><Icon name={iconForCategory(tool.category)} size={17} /></span><span class="planned-card-copy"><strong>{tool.name}</strong><small>{tool.category} · {privacyLabel(tool.privacyClass)}</small></span><span class="planned-pill">Planned</span>
                    </button>
                  {/each}
                </div>
              </section>
            {/if}
          {/if}

          {#if favoriteError}<div class="field-error" role="alert">{favoriteError}</div>{/if}
          <footer class="dashboard-footer"><span><span class="footer-orb"></span> Arcade Box runs on your device</span></footer>
        </div>
      </div>
    </section>
  {/if}
  {#if onboardingReady && onboardingVisible}
    <div class="onboarding-backdrop" role="presentation">
      <dialog bind:this={onboardingDialog} class="onboarding-card" aria-modal="true" aria-labelledby="onboarding-title" aria-describedby="onboarding-description" oncancel={(event) => event.preventDefault()}>
        <div class="onboarding-brand"><span class="brand-mark"><Icon name="command" size={19} /></span><span class="brand-name">Arcade <span>Box</span></span><span class="onboarding-step">First run</span></div>
        <h1 id="onboarding-title">Meet Arcade Box.</h1>
        <p id="onboarding-description">Open the Arcade Island, type what you need, and press Enter. It closes back to the app you were using.</p>
        <div class="onboarding-shortcut-block">
          <label for="onboarding-shortcut">Global shortcut</label>
          <div class="onboarding-shortcut-input"><Icon name="command" size={17} /><input bind:this={shortcutField} bind:value={shortcutInput} id="onboarding-shortcut" autocomplete="off" spellcheck="false" aria-describedby="shortcut-status" /><span class="shortcut-edit-hint">Editable</span></div>
          <div class="shortcut-status" id="shortcut-status" class:shortcut-good={shortcutInfo?.state === 'registered'} class:shortcut-bad={shortcutInfo && shortcutInfo.state !== 'registered' && shortcutInfo.state !== 'starting' && shortcutInfo.state !== 'checking' && shortcutInfo.state !== 'updating'}><span class="runtime-dot" class:offline={shortcutInfo?.state !== 'registered'}></span><span>{shortcutInfo?.message || 'A platform default will be tested when you continue.'}</span></div>
          {#if shortcutInfo?.backend.toLowerCase().includes('portal')}
            <p class="portal-note">Wayland registers shortcuts through the XDG Desktop Portal. Your desktop may remap or decline a shortcut; if the portal is unavailable, Arcade Box will explain the limitation.</p>
            <small class="shortcut-syntax">Portal format example: CTRL+ALT+space</small>
          {:else}
            <small class="shortcut-syntax">Use a modifier and key, for example Ctrl+Alt+Space.</small>
          {/if}
        </div>
        {#if shortcutError}<div class="onboarding-error" role="status">{shortcutError}</div>{/if}
        <div class="onboarding-actions"><button class="onboarding-primary" disabled={shortcutSaving} onclick={() => void applyShortcutAndContinue()}>{shortcutSaving ? 'Checking shortcut…' : 'Save shortcut & continue'}<Icon name="arrow" size={15} /></button><button class="onboarding-secondary" onclick={() => void finishOnboarding()}>Continue without changing it</button></div>
        <div class="onboarding-flow"><span><kbd>⌘</kbd> invoke</span><Icon name="chevron" size={13} /><span>type</span><Icon name="chevron" size={13} /><span><kbd>↵</kbd> perform</span></div>
      </dialog>
    </div>
  {/if}
</main>
