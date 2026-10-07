/** Public frontend view of the Rust command contract. Keep these types aligned with arcade-contract. */
export type PrivacyClass = 'LOCAL' | 'NETWORK' | 'CLOUD' | (string & {});

export type UiInputKind = 'none' | 'text' | 'url' | 'file' | 'files' | 'folder';

export interface UiChoice {
  value: string;
  label: string;
  /** Provider capabilities the choice needs, such as `encoder:libx265`. */
  requires?: string[];
}

export interface UiCondition {
  key: string;
  equals?: unknown;
  oneOf?: unknown[];
}

export interface UiControl {
  key: string;
  label: string;
  type: 'text' | 'password' | 'number' | 'select' | 'toggle' | 'directory';
  default?: unknown;
  choices?: UiChoice[];
  minimum?: number;
  maximum?: number;
  step?: number;
  placeholder?: string;
  help?: string;
  advanced?: boolean;
  showWhen?: UiCondition;
}

export interface StandardToolUi {
  version: number;
  input: {
    kind: UiInputKind;
    label: string;
    minItems?: number;
    maxItems?: number;
    sortable?: boolean;
  };
  controls: UiControl[];
}

export interface ToolSummary {
  id: string;
  name: string;
  description: string;
  category: string;
  aliases: string[];
  privacyClass: PrivacyClass;
  inputs: string[];
  outputs: string[];
  status: string;
  ui?: StandardToolUi | null;
  presets?: { id: string; name: string; options: Record<string, unknown> }[];
}

export interface ToolInput {
  kind: 'text' | 'url' | 'file' | 'artifact';
  value: string;
  mime: string;
}

export interface ToolRequest {
  toolId: string;
  inputs: ToolInput[];
  options?: Record<string, unknown>;
}

export type PipelineInputSource =
  | { kind: 'external'; index: number }
  | { kind: 'node'; nodeId: string; outputIndex: number };

export interface PipelineLinkNode { app: string; action: string; version: number }

export interface PipelineNode {
  id: string;
  toolId?: string;
  link?: PipelineLinkNode;
  inputs: PipelineInputSource[];
  options: Record<string, unknown>;
}

export interface Pipeline {
  id: string;
  name: string;
  version: number;
  nodes: PipelineNode[];
  outputNodes: string[];
}

export type PluginPermissionGrant = 'read-user-selected';

export interface PluginToolManifest extends ToolSummary {
  version: string;
  apiVersion: string;
  permissions: {
    filesystem: { read: string; write: string };
    network: { mode: string; domains: string[] };
  };
}

export interface PluginManifest {
  schemaVersion: number;
  toolManifest: PluginToolManifest;
  package: { author: string; source: string; license: string; componentSha256: string };
}

export interface PluginSummary {
  manifest: PluginManifest;
  grantedPermissions: PluginPermissionGrant[];
}

export interface PluginPreview {
  manifest: PluginManifest;
  installedVersion: string | null;
  requestedPermissions: PluginPermissionGrant[];
  additionalPermissions: PluginPermissionGrant[];
}

export interface SelectedFile {
  token: string;
  name: string;
  size: number;
  mime: string;
}

export interface SelectedDirectory {
  token: string;
  name: string;
}

export interface ScreenCaptureStatus {
  platform: string;
  captureAvailable: boolean;
  selectionMode: string;
  message: string;
  recordingAvailable: boolean;
  recordingMessage: string;
}

export interface ScreenImagePreview {
  dataUrl: string;
  width: number;
  height: number;
  sourceWidth: number;
  sourceHeight: number;
}

export interface SampledScreenPixel {
  x: number;
  y: number;
  rgba: [number, number, number, number];
  hex: string;
  rgb: [number, number, number];
  hsl: [number, number, number];
  magnifierDataUrl: string;
}

export interface ScreenMeasurement {
  start: [number, number];
  end: [number, number];
  sourceWidth: number;
  sourceHeight: number;
  widthPixels: number;
  heightPixels: number;
  horizontalDistance: number;
  verticalDistance: number;
  diagonalDistance: number;
}

export interface PinWindowStatus {
  label: string;
  message: string;
  width: number;
  height: number;
}

export interface ClipboardHistoryItem {
  id: string;
  kind: 'text' | 'image' | 'file' | string;
  text: string | null;
  imageRgbaBase64: string | null;
  imageWidth: number | null;
  imageHeight: number | null;
  preview: string;
  capturedAt: number;
  pinned: boolean;
  sourceApplication: string | null;
}

export interface ClipboardHistoryStatus {
  enabled: boolean;
  retentionDays: number;
  itemCount: number;
  pinnedCount: number;
  sourceApplicationAvailable: boolean;
  excludedApplications: string[];
  message: string;
}

export interface PastePlainStatus {
  platform: string;
  available: boolean;
  shortcut: string;
  message: string;
}

export interface ProcessInfo {
  pid: number;
  name: string;
  executable: string | null;
  startTime: number;
  cpuPercent: number;
  memoryBytes: number;
  protected: boolean;
}

export interface ProcessList {
  query: string | null;
  totalMatches: number;
  listedCount: number;
  truncated: boolean;
  processes: ProcessInfo[];
  limitations: string[];
}

export interface SystemWindowPinStatus {
  platform: string;
  available: boolean;
  message: string;
}

export interface ScreenRecordingSnapshot {
  platform: string;
  available: boolean;
  starting: boolean;
  recording: boolean;
  finalizing: boolean;
  elapsedSeconds: number | null;
  jobId: string | null;
  message: string;
}

export interface ProviderInfo {
  capability: string;
  source: string;
  executablePath: string;
  version: string;
  compatible: boolean;
  capabilities?: string[];
  warning?: string | null;
}

export interface HistoryEntry {
  toolId: string;
  status: string;
  createdAt: string;
}

export type JobStatus = 'queued' | 'running' | 'cancelling' | 'succeeded' | 'failed' | 'cancelled' | 'interrupted';

export interface JobSnapshot {
  id: string;
  toolId: string;
  status: JobStatus;
  progress: number | null;
  message: string | null;
  result: ToolResult | null;
  createdAt?: string;
  updatedAt?: string;
}

export interface ShortcutStatus {
  backend: string;
  state: string;
  message: string;
  triggerDescription?: string | null;
  trigger_description?: string | null;
}

export interface ToolOutput {
  kind: string;
  value: string;
  mime: string;
}

export interface ImageResultPreview {
  dataUrl: string;
  width: number;
  height: number;
}

export interface ToolResult {
  toolId: string;
  status: 'success' | 'error';
  outputs: ToolOutput[];
  message?: string;
  warnings?: string[];
  metadata?: Record<string, unknown>;
}

export interface ContextSuggestion {
  toolId: string;
  reason: string;
}

export type ToolStatus = 'implemented' | 'planned' | (string & {});

export interface MediaStreamSummary {
  /** Position among streams of the same kind, matching the tool's track options. */
  index: number;
  kind: 'audio' | 'subtitle';
  codec: string;
  language?: string | null;
  title?: string | null;
  detail: string;
  /** Image-based subtitles (PGS, VobSub) can be burned in but not saved as text. */
  bitmap: boolean;
}

export interface VideoPreviewFrame {
  timeSeconds: number;
  dataUrl: string;
}

export interface VideoPreview {
  durationSeconds: number | null;
  width: number;
  height: number;
  frameRate: number | null;
  videoCodec: string | null;
  sourceBytes: number | null;
  audio: MediaStreamSummary[];
  subtitles: MediaStreamSummary[];
  thumbnails: VideoPreviewFrame[];
  frame: VideoPreviewFrame | null;
}

export interface AudioPreview {
  durationSeconds: number | null;
  codec: string | null;
  sampleRate: number | null;
  channels: number | null;
  channelLayout: string | null;
  bitRate: number | null;
  sourceBytes: number | null;
  container: string;
  tracks: number;
  lossless: boolean;
  /** Cutting or joining with "Same as source" can copy without re-encoding. */
  copyable: boolean;
  language: string | null;
  tags: Record<string, string>;
  cover: string | null;
  /** Peak amplitude per waveform column, 0–1. */
  peaks: number[];
  /** Silent stretches as [start, end] seconds, when detection was requested. */
  silences: Array<[number, number]> | null;
}

export interface LoudnessReport {
  /** Integrated loudness in LUFS; null when the file is silent. */
  integrated: number | null;
  truePeak: number | null;
  range: number | null;
  threshold: number | null;
}

export interface VideoEstimate {
  sourceBytes: number | null;
  durationSeconds: number;
  estimatedBytes: number;
  method: 'target' | 'sample';
  sampledSeconds: number;
  warnings: string[];
}
