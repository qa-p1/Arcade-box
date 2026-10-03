// Keep inspection bounded even when a tool returns a large report.
export const RESULT_PAGE_SIZE = 40;
export const TEXT_PREVIEW_LIMIT = 24_000;

export function resultLabel(key: string): string {
  return key.replace(/([a-z\d])([A-Z])/g, (_, before: string, after: string) => `${before} ${after.toLowerCase()}`).replace(/[_-]/g, ' ').replace(/^./, (letter) => letter.toUpperCase());
}

export function parseResultData(value: string, mime: string, toolId: string): unknown {
  // Formatting/conversion tools must show the exact output, including whitespace.
  if (toolId === 'arcade.text.structured') return undefined;
  if (!mime.startsWith('structured/') || value.length > 2_000_000) return undefined;
  try {
    const data: unknown = JSON.parse(value);
    return data !== null && typeof data === 'object' ? data : undefined;
  } catch { return undefined; }
}

export function displayValue(value: unknown): string {
  if (value === null || value === undefined) return '—';
  if (typeof value === 'boolean') return value ? 'Yes' : 'No';
  if (typeof value === 'object') return JSON.stringify(value);
  return String(value);
}

export function tableColumns(rows: unknown[]): string[] {
  if (!rows.length || !rows.slice(0, RESULT_PAGE_SIZE).every((row) => row !== null && typeof row === 'object' && !Array.isArray(row))) return [];
  const keys = [...new Set(rows.slice(0, RESULT_PAGE_SIZE).flatMap((row) => Object.keys(row as object)))];
  return keys.length <= 8 ? keys : [];
}
