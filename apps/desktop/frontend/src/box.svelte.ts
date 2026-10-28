// Arcade Box UI state and actions, shared by the Island and the dashboard.
// Created once per window by App.svelte and handed to the surfaces through
// context (useBox). Moved verbatim from App.svelte; behaviour is unchanged.
import { announceTheme, copyText, currentSurface, dashboardReady, openDashboard, openIsland, openToolInIsland, watchDashboardPage, watchIslandShown, watchIslandHiding, watchIslandHidden, watchLinkOpen, watchOpenTool, watchTheme, islandReady, setIslandInputRegion, type LinkOpenRequest, type OpenToolRequest } from './lib/arcade';
import { onMount, tick } from 'svelte';
import { cancelResultLinkAction, overlapState, pickPeerClipboard, watchLinkChanged, type OverlapState } from './lib/arcade';
import type { IconName } from './lib/icon-names';
import { cancelJob, cancelScreenRecording, detectContext, downloadTesseract, getHistory, getPreference, hideIsland, listFavorites, listJobs, listProviders, listTools, openArtifact, openReviewedUrl, pastePlainStatus, pastePlainText, pinScreenCapture, revokeInputFolder, revokeOutputDirectory, revealArtifact, runContextAction, runScreenTool, saveArtifactAs, screenCaptureStatus, screenRecordingStatus, searchTools, selectFiles, setAlias, setFavorite, setPreference, setShortcut, setSurfaceMode, setWindowPin, shortcutStatus, startJob, startScreenRecording, stopScreenRecording, tesseractDownloadSize, terminateProcess, watchIslandFocus, watchJobUpdates, watchShortcutStatus, windowPinStatus } from './lib/arcade';
import type { ContextSuggestion, HistoryEntry, JobSnapshot, PastePlainStatus, ProcessInfo, ProcessList, ProviderInfo, ScreenCaptureStatus, ScreenRecordingSnapshot, SelectedDirectory, SelectedFile, ShortcutStatus, SystemWindowPinStatus, ToolInput, ToolResult, ToolSummary } from './lib/contracts';
import { acceptsSelectedFile, acceptsTextInput, fileInputMime, inputMime, isRunnable, privacyHint, usesFileInput } from './lib/tool-utils';
import { defaultUiValues, serializeStandardUiOptions, standardUiOptionsProblem } from './lib/standard-ui';
import { getContext, setContext } from 'svelte';

/** The dashboard's pages, as the tray, CLI and Link name them (dashboard.rs PAGES). */
const DASHBOARD_PAGES = ['all', 'favorites', 'recent', 'jobs', 'pipelines', 'plugins', 'engines', 'settings'] as const;

export function createBox() {

  type Surface = 'island' | 'dashboard';
  type CatalogMode = (typeof DASHBOARD_PAGES)[number];
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

  // Each window runs its own copy: the Island, or the dashboard (dashboard.html).
  const surface: Surface = currentSurface();
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
  // Tesseract is never shipped: Engines offers a download when it's missing.
  let tesseractOffer = $state<number | null>(null);
  let tesseractBusy = $state(false);
  let tesseractMessage = $state('');
  const tesseractMissing = $derived(!providers.some((provider) => provider.capability === 'ocr.tesseract' && provider.compatible));
  let overlaps = $state<OverlapState>({ lensCapture: false, lensActions: false, clipboardPick: false });
  let peerRequestId = $state('');
  let providersLoaded = $state(false);
  // Capabilities from compatible providers; null until detection finishes so
  // choices are never marked missing before Arcade Box has looked.
  const providerCapabilities = $derived(providersLoaded
    ? new Set(providers.filter((provider) => provider.compatible).flatMap((provider) => provider.capabilities ?? []))
    : null);
  const screenOcrAvailable = $derived(providerCapabilities?.has('ocr:language:eng') ?? false);
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
  const jobInputFolderTokens = new Map<string, string[]>();
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
  // Opening Engines is when the user looks at their providers: re-check them all.
  $effect(() => {
    if (catalogMode !== 'engines') return;
    void listProviders(true).then((items) => { providers = items; providersLoaded = true; }).catch(() => {});
    void tesseractDownloadSize().then((size) => { tesseractOffer = size; }).catch(() => {});
  });
  let toolOptions = $state<Record<string, string>>({});
  let selectedFiles = $state<SelectedFile[]>([]);
  let selectedInputFolders = $state<SelectedDirectory[]>([]);
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
  let catalogMode = $state<CatalogMode>('all');
  let dashboardQuery = $state('');
  let returnToDashboard = $state(false);
  let islandShell: HTMLElement | undefined = $state();
  let searchInput: HTMLInputElement | undefined = $state();
  let catalogSearchInput: HTMLInputElement | undefined = $state();
  let selectedToolHeading: HTMLHeadingElement | undefined = $state();
  let shortcutField: HTMLInputElement | undefined = $state();
  let onboardingDialog: HTMLDialogElement | undefined = $state();
  let copyTimer: number | undefined;
  let stopWatchingFocus = () => {};
  let stopWatchingJobs = () => {};
  let stopWatchingShortcut = () => {};
  let stopWatchingTheme = () => {};
  let initialLoad: Promise<void> = Promise.resolve();
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
  // The dashboard lists only what can be used today (STANDARD §8: no "planned" counters).
  const runnableTools = $derived(implementedTools);
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
    result = result.filter((tool) => tool.status === 'implemented');
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
    if (surface === 'dashboard') return;
    const mode = selectedTool || onboardingVisible ? 'tool' : isCompact ? 'compact' : 'search';
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

  // The dashboard is a normal window, shown by the OS; the Island fades in.
  let surfaceVisible = $state(!hasDesktopBridge() || surface === 'dashboard');
  let lastFocusedElement: HTMLElement | null = null;
  let stopWatchingHiding = () => {};
  let stopWatchingHidden = () => {};
  let stopWatchingLinkOpen = () => {};
  let stopWatchingWindow = () => {};
  /** Input from another Arcade app waiting for the user to choose a tool. */
  let pendingLink = $state<LinkOpenRequest | null>(null);

  // The pointer region follows the Island's target geometry, not each frame of
  // its height animation, so native input-shape updates happen once per change.
  let appliedInputRegion = '';
  let inputRegionFrame = 0;
  function queueInputRegion(): void {
    if (surface === 'dashboard' || inputRegionFrame) return;
    inputRegionFrame = requestAnimationFrame(() => {
      inputRegionFrame = 0;
      let region: [number, number, number, number] | null = null;
      if (onboardingVisible && onboardingDialog?.open) {
        const rect = onboardingDialog.getBoundingClientRect();
        region = [rect.x, rect.y, rect.width, rect.height];
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
    // Settings › General › "Show again" clears the flag while the Island lives.
    if (!onboardingVisible && hasDesktopBridge() && (await getPreference('onboarding_complete').catch(() => 'true')) !== 'true') {
      onboardingVisible = true;
    }
    surfaceVisible = true;
    await tick();
    animateInvocation();
    if (onboardingVisible) shortcutField?.focus();
    else if (lastFocusedElement?.isConnected) lastFocusedElement.focus({ preventScroll: true });
    else if (!selectedTool) searchInput?.focus();
    else islandShell?.querySelector<HTMLElement>('textarea, input, select, button')?.focus();
  }

  onMount(() => {
    initialLoad = loadInitialData();
    void tick().then(() => (surface === 'dashboard' ? catalogSearchInput : searchInput)?.focus());
    const systemTheme = window.matchMedia('(prefers-color-scheme: light)');
    const onSystemThemeChange = () => {
      if (themeMode === 'system') document.documentElement.dataset.theme = systemTheme.matches ? 'light' : 'dark';
    };
    onSystemThemeChange();
    systemTheme.addEventListener('change', onSystemThemeChange);
    let disposed = false;
    void watchTheme((theme) => {
      themeMode = theme === 'light' || theme === 'dark' ? theme : 'system';
      applyTheme(themeMode);
    }).then((unlisten) => { if (disposed) unlisten(); else stopWatchingTheme = unlisten; });
    if (surface === 'dashboard') {
      // Register for pages first, so a page asked for while loading is kept.
      void watchDashboardPage((page) => showPage(page)).then(async (unlisten) => {
        if (disposed) { unlisten(); return; }
        stopWatchingWindow = unlisten;
        await dashboardReady();
      }).catch((error) => (catalogError = messageOf(error)));
    } else {
      void Promise.all([
        watchIslandShown(() => { void revealIsland(); }),
        watchIslandHiding(() => {
          lastFocusedElement = document.activeElement instanceof HTMLElement ? document.activeElement : null;
          islandShell?.getAnimations().forEach((animation) => animation.cancel());
          dismissing = true;
        }),
        watchIslandHidden(() => { surfaceVisible = false; dismissing = false; }),
        watchOpenTool((request) => void openFromDashboard(request)),
        watchLinkOpen((request) => openFromLink(request)),
      ]).then(async ([shown, hiding, hidden, openTool, linkOpen]) => {
        if (disposed) { shown(); hiding(); hidden(); openTool(); linkOpen(); return; }
        stopWatchingShown = shown; stopWatchingHiding = hiding; stopWatchingHidden = hidden; stopWatchingWindow = openTool; stopWatchingLinkOpen = linkOpen;
        await islandReady();
      }).catch((error) => (catalogError = messageOf(error)));
    }
    void watchIslandFocus((isFocused) => {
      if (!isFocused) return;
      // Favourites, recents and jobs change in the other window too.
      void refreshSharedState();
      if (surface === 'dashboard') return;
      void refreshContext();
      if (!selectedTool) void tick().then(() => searchInput?.focus());
    }).then((unlisten) => { if (disposed) unlisten(); else stopWatchingFocus = unlisten; });
    void watchJobUpdates(acceptJobSnapshot).then((unlisten) => { if (disposed) unlisten(); else stopWatchingJobs = unlisten; });
    let stopPeers = () => {};
    void watchLinkChanged(() => void refreshPeerState()).then((stop) => { if (disposed) stop(); else stopPeers = stop; });
    void watchShortcutStatus(acceptShortcutStatus).then((unlisten) => { if (disposed) unlisten(); else stopWatchingShortcut = unlisten; });
    return () => {
      stopWatchingFocus();
      stopPeers();
      if (peerRequestId) void cancelResultLinkAction(peerRequestId);
      disposed = true;
      stopWatchingShown();
      stopWatchingHiding();
      stopWatchingHidden();
      stopWatchingWindow();
      stopWatchingTheme();
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
      onboardingVisible = hasDesktopBridge() && !onboardingComplete && surface === 'island';
      catalogError = '';
      // Optional providers and desktop portals must not hold up search or onboarding.
      void listProviders().then((items) => { providers = items; providersLoaded = true; providerError = ''; })
        .catch((error) => { providerError = messageOf(error); });
      void refreshPeerState();
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

  async function refreshPeerState(): Promise<void> {
    const [state, status] = await Promise.all([overlapState(), screenCaptureStatus().catch(() => null)]);
    overlaps = state;
    if (status) screenStatus = status;
    void listProviders().then((items) => { providers = items; providersLoaded = true; }).catch(() => {});
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
    void announceTheme(value).catch(() => {});
  }

  /** Reloads what the other window may have changed since this one last looked. */
  async function refreshSharedState(): Promise<void> {
    const [favoriteList, historyList, jobs] = await Promise.all([
      listFavorites().catch(() => null),
      getHistory().catch(() => null),
      listJobs().catch(() => null),
    ]);
    if (favoriteList) favorites = favoriteList;
    if (historyList) history = historyList;
    if (jobs) mergeBackgroundJobs(jobs);
  }

  function shortcutDescription(status: ShortcutStatus | null): string {
    return status?.triggerDescription || status?.trigger_description || '';
  }

  // STANDARD §10: the same chord on every OS (⌃⌥Space on macOS).
  function defaultShortcutForPlatform(): string {
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

  /** Settings › General: run the first-run welcome again in the Island. */
  async function showWelcomeAgain(): Promise<void> {
    await setPreference('onboarding_complete', 'false');
    await openIsland();
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
    if (surface === 'dashboard') {
      void openToolInIsland(tool.id).catch((error) => (catalogError = messageOf(error)));
      return;
    }
    if (peerRequestId) { void cancelResultLinkAction(peerRequestId); peerRequestId = ''; }
    running = false;
    releaseCurrentDirectoryGrant();
    selectedTool = tool;
    activeJobId = '';
    activeResult = null;
    inputText = '';
    diffRight = '';
    diffBase = '';
    toolOptions = defaultOptionsFor(tool);
    selectedFiles = [];
    selectedInputFolders = [];
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
    focused = false;
    if (pendingLink) {
      applyLinkInput(pendingLink);
      pendingLink = null;
    }
    void tick().then(() => {
      const entry = Array.from(islandShell?.querySelectorAll<HTMLElement>('#tool-input, .granted-file-picker, .input-folder-picker button, .standard-tool-form input:not([type=checkbox]), .standard-tool-form select, .run-button') ?? []).find((element) => element.getClientRects().length && !element.hasAttribute('disabled'));
      (entry ?? selectedToolHeading)?.focus({ preventScroll: true });
      islandContent?.scrollTo({ top: 0 });
      if (tool.id === 'arcade.system.clipboard-history' && overlaps.clipboardPick) void submitTool();
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
    query = '';
    void tick().then(() => searchInput?.focus());
  }

  /** The dashboard opened a tool, or a job's result, in this Island. */
  async function openFromDashboard(request: OpenToolRequest): Promise<void> {
    await initialLoad;
    const tool = toolById.get(request.tool);
    if (!tool) return;
    const job = request.job ? (await listJobs().catch(() => [])).find((entry) => entry.id === request.job) : undefined;
    openTool(tool, true);
    if (job?.result) {
      acceptJobSnapshot(job);
      activeJobId = job.id;
      activeResult = job.result;
    }
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
    // A tool opened from the dashboard goes back there.
    if (returnToDashboard) {
      returnToDashboard = false;
      void openDashboard();
    }
    void tick().then(() => searchInput?.focus());
  }

  function focusDashboardEntry(): void {
    void tick().then(() => catalogSearchInput?.focus());
  }

  /** The dashboard's page, asked for by the tray, `--settings`, Link or the Island. */
  function showPage(page: string): void {
    if (!DASHBOARD_PAGES.includes(page as CatalogMode)) return;
    catalogMode = page as CatalogMode;
    categoryFilter = 'All tools';
    dashboardQuery = '';
    focusDashboardEntry();
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
    const command = event.metaKey || event.ctrlKey;
    if (surface === 'dashboard') {
      // A normal window: Esc doesn't close it; ⌘K and / go to the catalog search.
      if ((command && event.key.toLowerCase() === 'k') || (event.key === '/' && !isEditableTarget(event.target))) {
        event.preventDefault();
        catalogSearchInput?.focus();
        catalogSearchInput?.select();
      }
      return;
    }
    if (onboardingVisible) {
      // Esc closes the Island as everywhere else; the first-run card comes
      // back next time until it is finished.
      if (event.key === 'Escape') {
        event.preventDefault();
        void dismissIsland();
      }
      return;
    }
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
      if (selectedTool) { returnToDashboard = false; backFromTool(); }
      void tick().then(() => { searchInput?.focus(); searchInput?.select(); });
    } else if (event.key === '/' && !isEditableTarget(event.target)) {
      event.preventDefault();
      releaseCurrentDirectoryGrant();
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
    const inputFolderTokens = tool.ui?.input.kind === 'folder'
      ? selectedInputFolders.map((folder) => folder.token)
      : [];
    if (directoryToken) pendingDirectoryTokens.add(directoryToken);
    for (const token of inputFolderTokens) pendingInputFolderTokens.add(token);
    try {
      if (tool.id === 'arcade.system.clipboard-history' && overlaps.clipboardPick) {
        peerRequestId = crypto.randomUUID();
        const result = await pickPeerClipboard(peerRequestId);
        if (generation === viewGeneration) activeResult = result;
        return;
      }
      if (isScreenRecorderTool(tool.id)) {
        screenRecording = {
          platform: screenRecording?.platform || screenStatus?.platform || 'unknown',
          available: screenRecording?.available ?? (screenStatus?.recordingAvailable === true),
          starting: true,
          recording: false,
          finalizing: false,
          elapsedSeconds: null,
          jobId: null,
          message: 'Waiting for screen selection…',
        };
        try {
          const status = await startScreenRecording();
          screenRecording = status || await screenRecordingStatus();
          if (status) {
            if (generation === viewGeneration && selectedTool?.id === tool.id) activeJobId = status.jobId || '';
            mergeBackgroundJobs(await listJobs().catch(() => []));
          }
        } catch (error) {
          screenRecording = await screenRecordingStatus().catch(() => ({
            ...screenRecording!, starting: false, message: messageOf(error),
          }));
          throw error;
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
        peerRequestId = crypto.randomUUID();
        const result = await runScreenTool(peerRequestId, tool.id);
        if (result && generation === viewGeneration) {
          if (tool.id === 'arcade.screen.pin' && result.status === 'success' && !result.metadata?.handedOver) {
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
        ? inputFolderTokens.map((token) => ({ kind: 'artifact', value: token, mime: 'folder/reference' }))
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
        if (inputFolderTokens.length && jobIsActive(job)) jobInputFolderTokens.set(job.id, inputFolderTokens);
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
      if (generation === viewGeneration) { running = false; peerRequestId = ''; }
      if (directoryToken) {
        pendingDirectoryTokens.delete(directoryToken);
        releaseDirectoryGrantIfUnused(directoryToken, !selectedTool || toolOptions.destinationGrant !== directoryToken);
      }
      for (const token of inputFolderTokens) {
        pendingInputFolderTokens.delete(token);
        releaseInputFolderGrantIfUnused(token, !selectedTool || !selectedInputFolders.some((folder) => folder.token === token));
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
    screenRecording = { ...screenRecording, finalizing: true, message: 'Finalizing the recording and saving the video…' };
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
      if (generation === viewGeneration) { running = false; peerRequestId = ''; }
    }
  }

  async function discardScreenRecording(): Promise<void> {
    if (!screenRecording?.recording || screenRecording.finalizing) return;
    screenRecording = { ...screenRecording, finalizing: true, message: 'Discarding the partial recording…' };
    runError = '';
    try {
      screenRecording = { ...(await cancelScreenRecording()), message: 'Recording discarded.' };
      mergeBackgroundJobs(await listJobs().catch(() => []));
    } catch (error) {
      runError = messageOf(error);
      screenRecording = await screenRecordingStatus().catch(() => ({
        ...screenRecording!, recording: true, finalizing: false, message: messageOf(error),
      }));
    }
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

  /** "Browse all tools": the Island opens the dashboard on Tools; in the
   * dashboard it shows all tools. */
  function toggleDashboard(): void {
    if (surface === 'island') {
      void openDashboard('all').catch((error) => (catalogError = messageOf(error)));
      return;
    }
    showPage('all');
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
      if (generation === viewGeneration) { running = false; peerRequestId = ''; }
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
      const inputFolderTokens = jobInputFolderTokens.get(snapshot.id);
      if (inputFolderTokens) {
        jobInputFolderTokens.delete(snapshot.id);
        selectedInputFolders = selectedInputFolders.filter((folder) => !inputFolderTokens.includes(folder.token));
        for (const token of inputFolderTokens) releaseInputFolderGrantIfUnused(token, true);
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
    const inputFolders = selectedInputFolders;
    selectedInputFolders = [];
    for (const folder of inputFolders) releaseInputFolderGrantIfUnused(folder.token, true);
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
      const limit = selectedTool.ui.input.maxItems ?? 1;
      if (selectedInputFolders.some((selected) => selected.token === folder.token)) return;
      // A single-folder tool replaces its folder; a multi-folder one adds.
      const dropped = limit <= 1 ? selectedInputFolders : [];
      if (limit > 1 && selectedInputFolders.length >= limit) {
        releaseInputFolderGrantIfUnused(folder.token, true);
        fileSelectionError = `Choose at most ${limit} folders.`;
        return;
      }
      fileSelectionError = '';
      selectedInputFolders = [...(limit <= 1 ? [] : selectedInputFolders), folder];
      for (const previous of dropped) releaseInputFolderGrantIfUnused(previous.token, true);
    };
  }

  function releaseDirectoryGrantIfUnused(token: string, noLongerSelected: boolean): void {
    if (!noLongerSelected || pendingDirectoryTokens.has(token)) return;
    if ([...jobDirectoryTokens.values()].includes(token)) return;
    void revokeOutputDirectory(token).catch(() => {});
  }

  function releaseInputFolderGrantIfUnused(token: string, noLongerSelected: boolean): void {
    if (!noLongerSelected || pendingInputFolderTokens.has(token)) return;
    if ([...jobInputFolderTokens.values()].some((tokens) => tokens.includes(token))) return;
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
    if (surface === 'dashboard') {
      void openToolInIsland(tool.id, job.id).catch((error) => (jobError = messageOf(error)));
      return;
    }
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

  /** Opens a dashboard page: in this window, or in the dashboard window from the Island. */
  function openPage(page: CatalogMode): void {
    if (surface === 'island') void openDashboard(page).catch((error) => (catalogError = messageOf(error)));
    else showPage(page);
  }

  function openSettings(): void {
    openPage('settings');
  }

  function openPipelines(): void {
    openPage('pipelines');
  }

  async function getTesseract(): Promise<void> {
    tesseractBusy = true;
    tesseractMessage = '';
    try {
      tesseractMessage = await downloadTesseract();
      providers = await listProviders(true);
    } catch (error) {
      tesseractMessage = String(error);
    } finally {
      tesseractBusy = false;
    }
  }

  function openEngines(): void {
    openPage('engines');
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
    if (selectedTool.id === 'arcade.system.clipboard-history') return overlaps.clipboardPick;
    if (selectedTool.id === 'arcade.system.paste-plain') return plainPasteStatus?.available === true;
    if (selectedTool.id === 'arcade.screen.ocr' && !screenOcrAvailable) return false;
    if (isScreenTool(selectedTool.id) && screenStatus?.captureAvailable === false) return false;
    if (isScreenTool(selectedTool.id)) return true;
    if (selectedTool.id === 'arcade.pdf.convert' && !providerSupports('document:render:pdf')) return false;
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
      if (kind === 'folder') return selectedInputFolders.length >= Math.max(minItems, 1) && (maxItems === undefined || selectedInputFolders.length <= maxItems);
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

  function removeInputFolder(token: string): void {
    selectedInputFolders = selectedInputFolders.filter((folder) => folder.token !== token);
    fileSelectionError = '';
    releaseInputFolderGrantIfUnused(token, true);
  }

  async function chooseScreenArea(): Promise<void> {
    const tool = selectedTool;
    if (!tool || screenCaptureBusy) return;
    const generation = viewGeneration;
    screenCaptureBusy = true;
    fileSelectionError = '';
    try {
      const capture = await runScreenTool(crypto.randomUUID(), 'arcade.screen.screenshot');
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
  return {
    get commandKey() { return commandKey; },
    get processTerminationRequests() { return processTerminationRequests; },
    set processTerminationRequests(value) { processTerminationRequests = value; },
    get surface() { return surface; },
    get dismissing() { return dismissing; },
    set dismissing(value) { dismissing = value; },
    get islandContent() { return islandContent; },
    set islandContent(value) { islandContent = value; },
    get islandHeight() { return islandHeight; },
    set islandHeight(value) { islandHeight = value; },
    get stopWatchingShown() { return stopWatchingShown; },
    set stopWatchingShown(value) { stopWatchingShown = value; },
    get query() { return query; },
    set query(value) { query = value; },
    get searchResults() { return searchResults; },
    set searchResults(value) { searchResults = value; },
    get tools() { return tools; },
    set tools(value) { tools = value; },
    get contextSuggestions() { return contextSuggestions; },
    set contextSuggestions(value) { contextSuggestions = value; },
    get history() { return history; },
    set history(value) { history = value; },
    get favorites() { return favorites; },
    set favorites(value) { favorites = value; },
    get providers() { return providers; },
    set providers(value) { providers = value; },
    get tesseractOffer() { return tesseractOffer; },
    set tesseractOffer(value) { tesseractOffer = value; },
    get tesseractBusy() { return tesseractBusy; },
    set tesseractBusy(value) { tesseractBusy = value; },
    get tesseractMessage() { return tesseractMessage; },
    set tesseractMessage(value) { tesseractMessage = value; },
    get tesseractMissing() { return tesseractMissing; },
    get overlaps() { return overlaps; },
    set overlaps(value) { overlaps = value; },
    get peerRequestId() { return peerRequestId; },
    set peerRequestId(value) { peerRequestId = value; },
    get providersLoaded() { return providersLoaded; },
    set providersLoaded(value) { providersLoaded = value; },
    get providerCapabilities() { return providerCapabilities; },
    get screenOcrAvailable() { return screenOcrAvailable; },
    get backgroundJobs() { return backgroundJobs; },
    set backgroundJobs(value) { backgroundJobs = value; },
    get screenStatus() { return screenStatus; },
    set screenStatus(value) { screenStatus = value; },
    get screenRecording() { return screenRecording; },
    set screenRecording(value) { screenRecording = value; },
    get plainPasteStatus() { return plainPasteStatus; },
    set plainPasteStatus(value) { plainPasteStatus = value; },
    get activeWindowPinCapability() { return activeWindowPinCapability; },
    set activeWindowPinCapability(value) { activeWindowPinCapability = value; },
    get activeJobId() { return activeJobId; },
    set activeJobId(value) { activeJobId = value; },
    get cancellingJobId() { return cancellingJobId; },
    set cancellingJobId(value) { cancellingJobId = value; },
    get jobError() { return jobError; },
    set jobError(value) { jobError = value; },
    get notedJobResults() { return notedJobResults; },
    get jobDirectoryTokens() { return jobDirectoryTokens; },
    get jobInputFolderTokens() { return jobInputFolderTokens; },
    get pendingDirectoryTokens() { return pendingDirectoryTokens; },
    get pendingInputFolderTokens() { return pendingInputFolderTokens; },
    get providerError() { return providerError; },
    set providerError(value) { providerError = value; },
    get favoriteError() { return favoriteError; },
    set favoriteError(value) { favoriteError = value; },
    get aliasValue() { return aliasValue; },
    set aliasValue(value) { aliasValue = value; },
    get aliasMessage() { return aliasMessage; },
    set aliasMessage(value) { aliasMessage = value; },
    get aliasSaving() { return aliasSaving; },
    set aliasSaving(value) { aliasSaving = value; },
    get onboardingReady() { return onboardingReady; },
    set onboardingReady(value) { onboardingReady = value; },
    get onboardingVisible() { return onboardingVisible; },
    set onboardingVisible(value) { onboardingVisible = value; },
    get onboardingComplete() { return onboardingComplete; },
    set onboardingComplete(value) { onboardingComplete = value; },
    get shortcutInfo() { return shortcutInfo; },
    set shortcutInfo(value) { shortcutInfo = value; },
    get shortcutInput() { return shortcutInput; },
    set shortcutInput(value) { shortcutInput = value; },
    get shortcutError() { return shortcutError; },
    set shortcutError(value) { shortcutError = value; },
    get shortcutSaving() { return shortcutSaving; },
    set shortcutSaving(value) { shortcutSaving = value; },
    get themeMode() { return themeMode; },
    set themeMode(value) { themeMode = value; },
    get finishAfterShortcut() { return finishAfterShortcut; },
    set finishAfterShortcut(value) { finishAfterShortcut = value; },
    get selectedTool() { return selectedTool; },
    set selectedTool(value) { selectedTool = value; },
    get activeResult() { return activeResult; },
    set activeResult(value) { activeResult = value; },
    get outputPage() { return outputPage; },
    set outputPage(value) { outputPage = value; },
    get fileOutputs() { return fileOutputs; },
    get toolOptions() { return toolOptions; },
    set toolOptions(value) { toolOptions = value; },
    get selectedFiles() { return selectedFiles; },
    set selectedFiles(value) { selectedFiles = value; },
    get selectedInputFolders() { return selectedInputFolders; },
    set selectedInputFolders(value) { selectedInputFolders = value; },
    get textInputMode() { return textInputMode; },
    set textInputMode(value) { textInputMode = value; },
    get fileSelectionError() { return fileSelectionError; },
    set fileSelectionError(value) { fileSelectionError = value; },
    get screenCaptureBusy() { return screenCaptureBusy; },
    set screenCaptureBusy(value) { screenCaptureBusy = value; },
    get artifactActionError() { return artifactActionError; },
    set artifactActionError(value) { artifactActionError = value; },
    get artifactActionMessage() { return artifactActionMessage; },
    set artifactActionMessage(value) { artifactActionMessage = value; },
    get artifactActionBusy() { return artifactActionBusy; },
    set artifactActionBusy(value) { artifactActionBusy = value; },
    get inputText() { return inputText; },
    set inputText(value) { inputText = value; },
    get diffRight() { return diffRight; },
    set diffRight(value) { diffRight = value; },
    get diffBase() { return diffBase; },
    set diffBase(value) { diffBase = value; },
    get loadingCatalog() { return loadingCatalog; },
    set loadingCatalog(value) { loadingCatalog = value; },
    get loadingSearch() { return loadingSearch; },
    set loadingSearch(value) { loadingSearch = value; },
    get running() { return running; },
    set running(value) { running = value; },
    get viewGeneration() { return viewGeneration; },
    set viewGeneration(value) { viewGeneration = value; },
    get focused() { return focused; },
    set focused(value) { focused = value; },
    get selectedIndex() { return selectedIndex; },
    set selectedIndex(value) { selectedIndex = value; },
    get searchAnnouncement() { return searchAnnouncement; },
    set searchAnnouncement(value) { searchAnnouncement = value; },
    get searchError() { return searchError; },
    set searchError(value) { searchError = value; },
    get runError() { return runError; },
    set runError(value) { runError = value; },
    get catalogError() { return catalogError; },
    set catalogError(value) { catalogError = value; },
    get copyState() { return copyState; },
    set copyState(value) { copyState = value; },
    get categoryFilter() { return categoryFilter; },
    set categoryFilter(value) { categoryFilter = value; },
    get catalogMode() { return catalogMode; },
    set catalogMode(value) { catalogMode = value; },
    get dashboardQuery() { return dashboardQuery; },
    set dashboardQuery(value) { dashboardQuery = value; },
    get returnToDashboard() { return returnToDashboard; },
    set returnToDashboard(value) { returnToDashboard = value; },
    get islandShell() { return islandShell; },
    set islandShell(value) { islandShell = value; },
    get searchInput() { return searchInput; },
    set searchInput(value) { searchInput = value; },
    get catalogSearchInput() { return catalogSearchInput; },
    set catalogSearchInput(value) { catalogSearchInput = value; },
    get selectedToolHeading() { return selectedToolHeading; },
    set selectedToolHeading(value) { selectedToolHeading = value; },
    get shortcutField() { return shortcutField; },
    set shortcutField(value) { shortcutField = value; },
    get onboardingDialog() { return onboardingDialog; },
    set onboardingDialog(value) { onboardingDialog = value; },
    get copyTimer() { return copyTimer; },
    set copyTimer(value) { copyTimer = value; },
    get stopWatchingFocus() { return stopWatchingFocus; },
    set stopWatchingFocus(value) { stopWatchingFocus = value; },
    get stopWatchingJobs() { return stopWatchingJobs; },
    set stopWatchingJobs(value) { stopWatchingJobs = value; },
    get stopWatchingShortcut() { return stopWatchingShortcut; },
    set stopWatchingShortcut(value) { stopWatchingShortcut = value; },
    get appliedSurfaceMode() { return appliedSurfaceMode; },
    set appliedSurfaceMode(value) { appliedSurfaceMode = value; },
    get searchRequestId() { return searchRequestId; },
    set searchRequestId(value) { searchRequestId = value; },
    get contextRequestId() { return contextRequestId; },
    set contextRequestId(value) { contextRequestId = value; },
    get toolById() { return toolById; },
    get catalogSearchText() { return catalogSearchText; },
    get runnableSearchResults() { return runnableSearchResults; },
    get plannedSearchResults() { return plannedSearchResults; },
    get runnableContextSuggestions() { return runnableContextSuggestions; },
    get categories() { return categories; },
    get implementedTools() { return implementedTools; },
    get runnableTools() { return runnableTools; },
    get version() { return __BOX_VERSION__; },
    get partialTools() { return partialTools; },
    get plannedTools() { return plannedTools; },
    get resultAnnouncement() { return resultAnnouncement; },
    get recentToolIds() { return recentToolIds; },
    get activeJob() { return activeJob; },
    get isCompact() { return isCompact; },
    get mediaProbe() { return mediaProbe; },
    get barcodeScan() { return barcodeScan; },
    get systemInformation() { return systemInformation; },
    get processList() { return processList; },
    get visibleDashboardTools() { return visibleDashboardTools; },
    get dashboardSearchAnnouncement() { return dashboardSearchAnnouncement; },
    get surfaceVisible() { return surfaceVisible; },
    set surfaceVisible(value) { surfaceVisible = value; },
    get lastFocusedElement() { return lastFocusedElement; },
    set lastFocusedElement(value) { lastFocusedElement = value; },
    get stopWatchingHiding() { return stopWatchingHiding; },
    set stopWatchingHiding(value) { stopWatchingHiding = value; },
    get stopWatchingHidden() { return stopWatchingHidden; },
    set stopWatchingHidden(value) { stopWatchingHidden = value; },
    get stopWatchingLinkOpen() { return stopWatchingLinkOpen; },
    set stopWatchingLinkOpen(value) { stopWatchingLinkOpen = value; },
    get pendingLink() { return pendingLink; },
    set pendingLink(value) { pendingLink = value; },
    get appliedInputRegion() { return appliedInputRegion; },
    set appliedInputRegion(value) { appliedInputRegion = value; },
    get inputRegionFrame() { return inputRegionFrame; },
    set inputRegionFrame(value) { inputRegionFrame = value; },
    queueInputRegion,
    animateInvocation,
    revealIsland,
    loadInitialData,
    refreshPeerState,
    hasDesktopBridge,
    applyTheme,
    changeTheme,
    shortcutDescription,
    defaultShortcutForPlatform,
    showWelcomeAgain,
    acceptShortcutStatus,
    applyShortcutAndContinue,
    finishOnboarding,
    refreshContext,
    performSearch,
    messageOf,
    openTool,
    openFromLink,
    applyLinkInput,
    defaultOptionsFor,
    openRunnableResult,
    backFromTool,
    focusDashboardEntry,
    acceptToolCatalog,
    announceSearchSelection,
    handleSearchKeydown,
    handleWindowKeydown,
    isEditableTarget,
    dismissIsland,
    submitTool,
    requestProcessTermination,
    changeForegroundWindowPin,
    finishScreenRecording,
    discardScreenRecording,
    copyOutput,
    toggleDashboard,
    get categoryIcons() { return categoryIcons; },
    iconForCategory,
    statusText,
    selectContextSuggestion,
    executeContextAction,
    recordHistory,
    jobIsActive,
    get activeJobCount() { return activeJobCount; },
    acceptJobSnapshot,
    mergeBackgroundJobs,
    newerJobSnapshot,
    releaseCurrentDirectoryGrant,
    inputFolderSelectionHandler,
    releaseDirectoryGrantIfUnused,
    releaseInputFolderGrantIfUnused,
    cancelBackgroundJob,
    jobStatusText,
    viewJobResult,
    isFavorite,
    toggleFavorite,
    saveAlias,
    privacyHintFor,
    providerSupports,
    requiredPdfCapability,
    openSettings,
    openPipelines,
    getTesseract,
    openEngines,
    providerIcon,
    hasTextFileSwitch,
    fileWorkflowActive,
    fileInputLabel,
    canSubmitSelectedTool,
    isVideoEditorTool,
    isScreenTool,
    isScreenRecorderTool,
    chooseInputFile,
    removeInputFolder,
    chooseScreenArea,
    removeSelectedFileToken,
    reorderSelectedFiles,
    parseMediaProbe,
    parseStructuredOutput,
    formatBytes,
    formatDuration,
    frameRate,
    dynamicRange,
    bitDepth,
    streamLabel,
    streamDetail,
    streamCount,
    resultMetadataText,
    artifactOutputs,
    artifactOutputName,
    artifactOutputSize,
    outputMetadataSize,
    outputAcceptsMime,
    toolsForOutput,
    continueWithOutput,
    artifactIcon,
    showArtifact,
    saveArtifact,
    openScannedLink,
    reviewedHttpUrl,
    chooseCategory,
  };
}

export type Box = ReturnType<typeof createBox>;

const KEY = Symbol('arcade-box');
export function provideBox(box: Box): Box {
  return setContext(KEY, box);
}
export function useBox(): Box {
  return getContext<Box>(KEY);
}
