// Small formatters shared by the media editors.

/** `1:02.50`-style time, with hours when needed. */
export function clock(seconds: number): string {
  const safe = Math.max(0, seconds);
  const minutes = Math.floor(safe / 60);
  const rest = safe - minutes * 60;
  const hours = Math.floor(minutes / 60);
  const body = `${String(minutes % 60).padStart(hours ? 2 : 1, '0')}:${rest.toFixed(2).padStart(5, '0')}`;
  return hours ? `${hours}:${body}` : body;
}

export function bytes(value: number | null | undefined): string {
  if (!value || value <= 0) return '—';
  const units = ['B', 'KB', 'MB', 'GB'];
  let amount = value;
  let unit = 0;
  while (amount >= 1024 && unit < units.length - 1) { amount /= 1024; unit += 1; }
  return `${amount.toFixed(amount >= 100 || unit === 0 ? 0 : 1)} ${units[unit]}`;
}
