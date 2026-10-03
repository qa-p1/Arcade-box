import type { SelectedFile, ToolSummary } from './contracts';

export function isRunnable(tool: ToolSummary): boolean {
  return tool.status === 'implemented' || tool.status === 'partial';
}

export function acceptsTextInput(tool: ToolSummary): boolean {
  if (tool.ui?.version === 1 && ['none', 'file', 'files', 'folder'].includes(tool.ui.input.kind)) return false;
  if (tool.ui?.input.kind === 'text' || tool.ui?.input.kind === 'url') return true;
  return tool.inputs.length === 0 || tool.inputs.some((input) => {
    const normalized = input.toLowerCase();
    return normalized.startsWith('text/')
      || normalized.startsWith('structured/')
      || normalized.startsWith('clipboard/text')
      || normalized.includes('/url')
      || normalized === 'text';
  });
}

export function inputMime(tool: ToolSummary): string {
  if (tool.ui?.input.kind === 'url') return tool.inputs.find((input) => input === 'network/url' || input === 'text/url') || 'text/url';
  if (tool.ui?.input.kind === 'text') return tool.inputs.find((input) => input.startsWith('text/') || input.startsWith('structured/')) || 'text/plain';
  return tool.inputs.find((input) => input.startsWith('text/') || input.startsWith('structured/')) || 'text/plain';
}

export function privacyHint(value: string): string {
  switch (value.toUpperCase()) {
    case 'LOCAL': return 'Input stays on this device';
    case 'NETWORK': return 'This action contacts a network service';
    case 'CLOUD': return 'Input may be sent to a remote provider';
    default: return 'Review the privacy details before running';
  }
}

export function acceptsSelectedFile(tool: ToolSummary, file: SelectedFile): boolean {
  const acceptedTypes = tool.inputs.map((input) => input.replace(/\[\]$/, ''));
  if (acceptedTypes.includes('file/any')) return true;
  if (acceptedTypes.some((mime) => mime === file.mime || (mime.endsWith('/*') && file.mime.startsWith(mime.slice(0, -1))))) return true;
  return acceptedTypes.includes('file/media') && (file.mime === 'file/video' || file.mime === 'file/audio');
}

export function fileInputMime(file: SelectedFile): string {
  return file.mime;
}

export function usesFileInput(tool: ToolSummary): boolean {
  if (tool.ui?.version === 1) return tool.ui.input.kind === 'file' || tool.ui.input.kind === 'files';
  return tool.inputs.some((input) => input.replace(/\[\]$/, '').startsWith('file/'));
}

export function privacyLabel(value: string): string {
  switch (value.toUpperCase()) {
    case 'LOCAL': return 'LOCAL';
    case 'NETWORK': return 'NETWORK';
    case 'CLOUD': return 'CLOUD';
    default: return value.toUpperCase() || 'LOCAL';
  }
}

export function initials(value: string): string {
  return value
    .split(/[\s/&-]+/)
    .filter(Boolean)
    .slice(0, 2)
    .map((part) => part[0]?.toUpperCase() ?? '')
    .join('');
}
