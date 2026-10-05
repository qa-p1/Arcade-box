import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { getCurrentWindow } from '@tauri-apps/api/window';
import type { AudioPreview, ClipboardHistoryItem, ClipboardHistoryStatus, ContextSuggestion, HistoryEntry, ImageResultPreview, JobSnapshot, LoudnessReport, PastePlainStatus, Pipeline, PinWindowStatus, PluginPermissionGrant, PluginPreview, PluginSummary, ProviderInfo, ProcessInfo, SampledScreenPixel, ScreenCaptureStatus, ScreenImagePreview, ScreenRecordingSnapshot, SelectedDirectory, SelectedFile, ShortcutStatus, SystemWindowPinStatus, ToolInput, ToolRequest, ToolResult, ToolSummary, ToolOutput, VideoEstimate, VideoPreview } from './contracts';

function isDesktopRuntime(): boolean {
  return typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
}

function requireDesktopRuntime(): void {
  if (!isDesktopRuntime()) {
    throw new Error('Open Arcade Box as a desktop app to connect to its local tool runtime.');
  }
}

export async function copyText(text: string): Promise<void> {
  requireDesktopRuntime();
  await invoke<void>('copy_text', { text });
}

export async function imageResultPreview(token: string): Promise<ImageResultPreview> {
  requireDesktopRuntime();
  return invoke<ImageResultPreview>('image_result_preview', { token });
}

/** Thumbnails across a granted video, plus one larger frame when `frameAt` is set. */
export async function videoPreview(token: string, thumbnails: number, frameAt?: number): Promise<VideoPreview> {
  requireDesktopRuntime();
  return invoke<VideoPreview>('video_preview', { token, thumbnails, frameAt: frameAt ?? null });
}

/** Predict the compressed size using the same options the tool will run with. */
export async function audioPreview(token: string, columns: number, silence?: { thresholdDb: number; minimumSeconds: number }): Promise<AudioPreview> {
  requireDesktopRuntime();
  return invoke<AudioPreview>('audio_preview', {
    token,
    columns,
    silenceThresholdDb: silence?.thresholdDb ?? null,
    silenceMinimumSeconds: silence?.minimumSeconds ?? null,
  });
}

export async function measureAudioLoudness(token: string): Promise<LoudnessReport> {
  requireDesktopRuntime();
  return invoke<LoudnessReport>('measure_audio_loudness', { token });
}

export async function estimateVideoOutput(token: string, options: Record<string, unknown>): Promise<VideoEstimate> {
  requireDesktopRuntime();
  return invoke<VideoEstimate>('estimate_video_output', { token, options });
}

export async function copyImageResult(token: string): Promise<void> {
  requireDesktopRuntime();
  await invoke<void>('copy_image_result', { token });
}

export async function listTools(): Promise<ToolSummary[]> {
  requireDesktopRuntime();
  return invoke<ToolSummary[]>('list_tools');
}

export async function listProviders(): Promise<ProviderInfo[]> {
  requireDesktopRuntime();
  return invoke<ProviderInfo[]>('list_providers');
}

export async function getHistory(): Promise<HistoryEntry[]> {
  requireDesktopRuntime();
  return invoke<HistoryEntry[]>('get_history');
}

export async function listFavorites(): Promise<string[]> {
  requireDesktopRuntime();
  return invoke<string[]>('list_favorites');
}

export async function setFavorite(toolId: string, favorite: boolean): Promise<void> {
  requireDesktopRuntime();
  await invoke<void>('set_favorite', { toolId, favorite });
}

export async function setAlias(alias: string, toolId: string): Promise<void> {
  requireDesktopRuntime();
  await invoke<void>('set_alias', { alias, toolId });
}

export async function startJob(request: ToolRequest): Promise<JobSnapshot> {
  requireDesktopRuntime();
  return invoke<JobSnapshot>('start_job', { request });
}

export async function cancelJob(jobId: string): Promise<JobSnapshot> {
  requireDesktopRuntime();
  return invoke<JobSnapshot>('cancel_job', { jobId });
}

export async function listJobs(): Promise<JobSnapshot[]> {
  requireDesktopRuntime();
  return invoke<JobSnapshot[]>('list_jobs');
}

export async function watchJobUpdates(handler: (job: JobSnapshot) => void): Promise<() => void> {
  if (!isDesktopRuntime()) return () => {};
  return listen<JobSnapshot>('arcade://job-update', ({ payload }) => handler(payload));
}

export async function getPreference(key: 'onboarding_complete' | 'theme'): Promise<string | null> {
  requireDesktopRuntime();
  return invoke<string | null>('get_preference', { key });
}

export async function setPreference(key: 'onboarding_complete' | 'theme', value: string): Promise<void> {
  requireDesktopRuntime();
  await invoke<void>('set_preference', { key, value });
}

export async function shortcutStatus(): Promise<ShortcutStatus> {
  requireDesktopRuntime();
  return invoke<ShortcutStatus>('shortcut_status');
}

export async function setShortcut(trigger: string): Promise<ShortcutStatus> {
  requireDesktopRuntime();
  return invoke<ShortcutStatus>('set_shortcut', { trigger });
}

export async function watchShortcutStatus(handler: (status: ShortcutStatus) => void): Promise<() => void> {
  if (!isDesktopRuntime()) return () => {};
  return listen<ShortcutStatus>('arcade://shortcut-status', ({ payload }) => handler(payload));
}

export async function searchTools(query: string): Promise<ToolSummary[]> {
  requireDesktopRuntime();
  return invoke<ToolSummary[]>('search_tools', { query });
}

export async function runTool(request: ToolRequest): Promise<ToolResult> {
  requireDesktopRuntime();
  return invoke<ToolResult>('run_tool', { request });
}

export async function runContextAction(toolId: string): Promise<ToolResult> {
  requireDesktopRuntime();
  return invoke<ToolResult>('run_context_action', { toolId });
}

export async function selectFiles(): Promise<SelectedFile[]> {
  requireDesktopRuntime();
  return invoke<SelectedFile[]>('select_files');
}

export async function selectInputFolder(): Promise<SelectedDirectory | null> {
  requireDesktopRuntime();
  return invoke<SelectedDirectory | null>('select_input_folder');
}

export async function revokeInputFolder(token: string): Promise<void> {
  requireDesktopRuntime();
  await invoke<void>('revoke_input_folder', { token });
}

export async function screenCaptureStatus(): Promise<ScreenCaptureStatus> {
  requireDesktopRuntime();
  return invoke<ScreenCaptureStatus>('screen_capture_status');
}

export async function runScreenTool(toolId: string): Promise<ToolResult | null> {
  requireDesktopRuntime();
  return invoke<ToolResult | null>('run_screen_tool', { toolId });
}

export async function screenImagePreview(token: string, maxEdge = 512): Promise<ScreenImagePreview> {
  requireDesktopRuntime();
  return invoke<ScreenImagePreview>('screen_image_preview', { token, maxEdge });
}

export async function sampleScreenImagePixel(token: string, x: number, y: number): Promise<SampledScreenPixel> {
  requireDesktopRuntime();
  return invoke<SampledScreenPixel>('sample_screen_image_pixel', { token, x, y });
}

export async function measureScreenArea(token: string, start: [number, number], end: [number, number]): Promise<ToolResult> {
  requireDesktopRuntime();
  return invoke<ToolResult>('measure_screen_area', { token, start, end });
}

export async function pinScreenCapture(token: string): Promise<PinWindowStatus> {
  requireDesktopRuntime();
  return invoke<PinWindowStatus>('pin_screen_capture', { token });
}

export async function clipboardHistoryStatus(): Promise<ClipboardHistoryStatus> {
  requireDesktopRuntime();
  return invoke<ClipboardHistoryStatus>('clipboard_history_status');
}

export async function listClipboardHistory(query = ''): Promise<ClipboardHistoryItem[]> {
  requireDesktopRuntime();
  return invoke<ClipboardHistoryItem[]>('list_clipboard_history', { query });
}

export async function setClipboardHistoryEnabled(enabled: boolean): Promise<ClipboardHistoryStatus> {
  requireDesktopRuntime();
  return invoke<ClipboardHistoryStatus>('set_clipboard_history_enabled', { enabled });
}

export async function setClipboardHistoryRetention(days: number): Promise<ClipboardHistoryStatus> {
  requireDesktopRuntime();
  return invoke<ClipboardHistoryStatus>('set_clipboard_history_retention', { days });
}

export async function setClipboardHistoryExclusions(applications: string[]): Promise<ClipboardHistoryStatus> {
  requireDesktopRuntime();
  return invoke<ClipboardHistoryStatus>('set_clipboard_history_exclusions', { applications });
}

export async function pinClipboardHistoryItem(id: string, pinned: boolean): Promise<void> {
  requireDesktopRuntime();
  await invoke<void>('pin_clipboard_history_item', { id, pinned });
}

export async function deleteClipboardHistoryItem(id: string): Promise<void> {
  requireDesktopRuntime();
  await invoke<void>('delete_clipboard_history_item', { id });
}

export async function clearClipboardHistory(): Promise<void> {
  requireDesktopRuntime();
  await invoke<void>('clear_clipboard_history');
}

export async function copyClipboardHistoryItem(id: string): Promise<void> {
  requireDesktopRuntime();
  await invoke<void>('copy_clipboard_history_item', { id });
}

export async function pastePlainStatus(): Promise<PastePlainStatus> {
  requireDesktopRuntime();
  return invoke<PastePlainStatus>('paste_plain_status');
}

export async function pastePlainText(): Promise<void> {
  requireDesktopRuntime();
  await invoke<void>('paste_plain_text');
}

export async function terminateProcess(process: ProcessInfo): Promise<void> {
  requireDesktopRuntime();
  await invoke<void>('terminate_process', {
    pid: process.pid,
    processName: process.name,
    executable: process.executable,
    startTime: process.startTime,
  });
}

export async function windowPinStatus(): Promise<SystemWindowPinStatus> {
  requireDesktopRuntime();
  return invoke<SystemWindowPinStatus>('window_pin_status');
}

export async function setWindowPin(pinned: boolean): Promise<SystemWindowPinStatus> {
  requireDesktopRuntime();
  return invoke<SystemWindowPinStatus>('set_window_pin', { pinned });
}

export async function screenRecordingStatus(): Promise<ScreenRecordingSnapshot> {
  requireDesktopRuntime();
  return invoke<ScreenRecordingSnapshot>('screen_recording_status');
}

export async function startScreenRecording(): Promise<ScreenRecordingSnapshot | null> {
  requireDesktopRuntime();
  return invoke<ScreenRecordingSnapshot | null>('start_screen_recording');
}

export async function stopScreenRecording(): Promise<ToolResult> {
  requireDesktopRuntime();
  return invoke<ToolResult>('stop_screen_recording');
}

export async function cancelScreenRecording(): Promise<ScreenRecordingSnapshot> {
  requireDesktopRuntime();
  return invoke<ScreenRecordingSnapshot>('cancel_screen_recording');
}

export async function chooseOutputDirectory(): Promise<SelectedDirectory | null> {
  requireDesktopRuntime();
  return invoke<SelectedDirectory | null>('choose_output_directory');
}

export async function revokeOutputDirectory(token: string): Promise<void> {
  requireDesktopRuntime();
  await invoke<void>('revoke_output_directory', { token });
}

export async function revealArtifact(token: string): Promise<void> {
  requireDesktopRuntime();
  await invoke<void>('reveal_artifact', { token });
}

export async function openArtifact(token: string): Promise<void> {
  requireDesktopRuntime();
  await invoke<void>('open_artifact', { token });
}

export async function openReviewedUrl(url: string): Promise<void> {
  requireDesktopRuntime();
  await invoke<void>('open_reviewed_url', { url });
}

export async function detectContext(): Promise<ContextSuggestion[]> {
  requireDesktopRuntime();
  return invoke<ContextSuggestion[]>('detect_context');
}

export async function listPipelines(): Promise<Pipeline[]> {
  requireDesktopRuntime();
  return invoke<Pipeline[]>('list_pipelines');
}

export async function choosePluginPackage(): Promise<string | null> {
  requireDesktopRuntime();
  return invoke<string | null>('choose_plugin_package');
}

export async function listPlugins(): Promise<PluginSummary[]> {
  requireDesktopRuntime();
  return invoke<PluginSummary[]>('list_plugins');
}

export async function previewPlugin(sourceDir: string): Promise<PluginPreview> {
  requireDesktopRuntime();
  return invoke<PluginPreview>('preview_plugin', { sourceDir });
}

export async function installPlugin(sourceDir: string, grants: PluginPermissionGrant[], acknowledgeEscalation: boolean): Promise<PluginSummary> {
  requireDesktopRuntime();
  return invoke<PluginSummary>('install_plugin', { sourceDir, grants, acknowledgeEscalation });
}

export async function uninstallPlugin(pluginId: string): Promise<void> {
  requireDesktopRuntime();
  await invoke<void>('uninstall_plugin', { pluginId });
}

export async function savePipeline(pipeline: Pipeline): Promise<Pipeline> {
  requireDesktopRuntime();
  return invoke<Pipeline>('save_pipeline', { pipeline });
}

export async function saveArtifactAs(token: string): Promise<SelectedFile | null> {
  requireDesktopRuntime();
  return invoke<SelectedFile | null>('save_artifact_as', { token });
}

export async function deletePipeline(id: string): Promise<void> {
  requireDesktopRuntime();
  await invoke<void>('delete_pipeline', { id });
}

export async function runPipeline(id: string, inputs: ToolInput[]): Promise<Record<string, ToolOutput[]>> {
  requireDesktopRuntime();
  return invoke<Record<string, ToolOutput[]>>('run_pipeline', { id, inputs });
}

export async function hideIsland(): Promise<void> {
  if (!isDesktopRuntime()) return;
  await invoke<void>('hide_island');
}

export type SurfaceMode = 'compact' | 'search' | 'tool' | 'dashboard';

export async function setSurfaceMode(mode: SurfaceMode): Promise<void> {
  if (!isDesktopRuntime()) return;
  await invoke<void>('set_surface_mode', { mode });
}

export async function watchIslandFocus(handler: (focused: boolean) => void): Promise<() => void> {
  if (!isDesktopRuntime()) return () => {};
  return getCurrentWindow().onFocusChanged(({ payload }) => handler(payload));
}

export async function watchIslandShown(handler: () => void): Promise<() => void> {
  if (!isDesktopRuntime()) return () => {};
  return listen('arcade://island-shown', handler);
}

/** Another Arcade app opened the Island with its input ("More in Arcade Box…"). */
export interface LinkOpenRequest {
  source: string;
  tool: string | null;
  files: SelectedFile[];
  text: string | null;
  options: Record<string, unknown>;
}

export async function watchLinkOpen(handler: (request: LinkOpenRequest) => void): Promise<() => void> {
  if (!isDesktopRuntime()) return () => {};
  return listen<LinkOpenRequest>('arcade://link-open', ({ payload }) => handler(payload));
}

/** Display names of the Arcade apps, for "From Arcade Look" notes. */
export function arcadeAppName(id: string): string {
  return ({ 'arcade.box': 'Arcade Box', 'arcade.lens': 'Arcade Lens', 'arcade.look': 'Arcade Look', 'arcade.wheel': 'Arcade Wheel', 'arcade.clipboard': 'Arcade Clipboard' } as Record<string, string>)[id] ?? 'another Arcade app';
}

/** `arcade-desktop --settings` (or another app) asked Box to open Settings. */
export async function watchOpenSettings(handler: () => void): Promise<() => void> {
  if (!isDesktopRuntime()) return () => {};
  return listen('arcade://open-settings', handler);
}

export interface LinkSettings { enabled: boolean; disabledPeers: string[] }
export interface ConnectedApp { id: string; name: string; state: string; version: string | null; enabled: boolean; pitch: string; endpoint: string }
export interface ConnectedAppsState { settings: LinkSettings; apps: ConnectedApp[]; registryPath: string; endpointState: string; lastError: string | null }
export interface ResultLinkAction { key: string; app: string; title: string; enabled: boolean; reason: string | null; preview: string }

export async function connectedApps(): Promise<ConnectedAppsState | null> {
  if (!isDesktopRuntime()) return null;
  return invoke<ConnectedAppsState>('connected_apps');
}
export async function setLinkSettings(settings: LinkSettings): Promise<void> {
  requireDesktopRuntime();
  return invoke<void>('set_link_settings', { settings });
}
export async function getConnectedApp(id: string): Promise<void> {
  requireDesktopRuntime();
  return invoke<void>('get_connected_app', { id });
}
export async function resultLinkActions(outputs: ToolOutput[], toolId: string, preset: string | null): Promise<ResultLinkAction[]> {
  if (!isDesktopRuntime()) return [];
  return invoke<ResultLinkAction[]>('result_link_actions', { outputs, toolId, preset });
}
export async function invokeResultLinkAction(key: string, outputs: ToolOutput[], toolId: string, preset: string | null): Promise<{ message?: string }> {
  requireDesktopRuntime();
  return invoke<{ message?: string }>('invoke_result_link_action', { key, outputs, toolId, preset });
}
export async function watchLinkChanged(handler: () => void): Promise<() => void> {
  if (!isDesktopRuntime()) return () => {};
  return listen('arcade://link-changed', handler);
}

// Register lifecycle listeners before acknowledging readiness, so an early
// shortcut cannot be lost while the WebView is still loading.
export async function islandReady(): Promise<void> {
  if (isDesktopRuntime()) await invoke<void>('island_ready');
}

export async function watchIslandHiding(handler: () => void): Promise<() => void> {
  if (!isDesktopRuntime()) return () => {};
  return listen('arcade://island-hiding', handler);
}

export async function watchIslandHidden(handler: () => void): Promise<() => void> {
  if (!isDesktopRuntime()) return () => {};
  return listen('arcade://island-hidden', handler);
}

export async function setIslandInputRegion(x: number, y: number, width: number, height: number): Promise<void> {
  if (isDesktopRuntime()) await invoke<void>('set_island_input_region', { x, y, width, height });
}
