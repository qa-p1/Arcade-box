<script lang="ts">
  // Readable "Is this site down?" report: verdict first, then each check.
  type Record_ = Record<string, unknown>;
  let { value }: { value: unknown; mime?: string; copy?: (text: string) => void } = $props();

  const report = $derived((value && typeof value === 'object' ? value : {}) as Record_);
  const http = $derived((report.http ?? {}) as Record_);
  const dns = $derived((report.dns ?? {}) as Record_);
  const ping = $derived((report.ping ?? {}) as Record_);
  const tls = $derived((report.certificate ?? {}) as Record_);
  const cert = $derived((tls.certificate ?? {}) as Record_);
  const records = $derived(Array.isArray(dns.records) ? (dns.records as Record_[]) : []);
  const redirects = $derived(Array.isArray(http.redirects) ? (http.redirects as Record_[]).filter((hop) => typeof hop.location === 'string') : []);
  const verdict = $derived(typeof report.verdict === 'string' ? report.verdict : 'problem');

  const text = (item: unknown) => (typeof item === 'string' || typeof item === 'number' ? String(item) : '');
  const issuer = $derived(text(cert.issuer).match(/O=([^,]+)/)?.[1] ?? text(cert.issuer));
</script>

<div class="site-check">
  <p class="verdict {verdict}"><span class="dot"></span>{text(report.summary) || verdict}</p>

  <div class="media-facts">
    <div><span>HTTP</span><strong>{http.error ? 'Failed' : `${text(http.status)}${typeof http.elapsedSeconds === 'number' ? ` · ${http.elapsedSeconds.toFixed(2)} s` : ''}`}</strong></div>
    <div><span>Ping</span><strong>{typeof ping.averageMs === 'number' ? `${Math.round(ping.averageMs)} ms${typeof ping.packetLossPercent === 'number' && ping.packetLossPercent > 0 ? ` · ${ping.packetLossPercent}% lost` : ''}` : 'No reply'}</strong></div>
    <div><span>DNS</span><strong>{records.length ? `${records.length} record${records.length === 1 ? '' : 's'}` : 'Not found'}</strong></div>
    <div><span>Certificate</span><strong>{typeof tls.daysLeft === 'number' ? (tls.valid === false ? 'Invalid' : `${tls.daysLeft} days left`) : tls.error ? 'Unavailable' : '—'}</strong></div>
  </div>

  <dl>
    {#if http.effectiveUrl}<dt>Final address</dt><dd>{text(http.effectiveUrl)}</dd>{/if}
    {#if http.remoteAddress}<dt>Server IP</dt><dd>{text(http.remoteAddress)}</dd>{/if}
    {#if http.error}<dt>HTTP error</dt><dd class="bad">{text(http.error)}</dd>{/if}
    {#if redirects.length}<dt>Redirects</dt><dd>{#each redirects as hop}<div>{text(hop.status)} → {text(hop.location)}</div>{/each}</dd>{/if}
    {#if records.length}<dt>DNS records</dt><dd>{#each records as record}<div><code>{text(record.type)}</code> {text(record.value)}</div>{/each}</dd>{/if}
    {#if dns.error}<dt>DNS</dt><dd class="bad">{text(dns.error)}</dd>{/if}
    {#if cert.subject}<dt>Certificate</dt><dd>{text(cert.subject).replace(/^CN=/, '')}{issuer ? ` · issued by ${issuer}` : ''}{cert.notafter ? ` · expires ${text(cert.notafter)}` : ''}</dd>{/if}
    {#if tls.error && !http.error}<dt>Certificate</dt><dd class="bad">{text(tls.error).split('\n')[0]}</dd>{/if}
  </dl>
  {#if report.note}<p class="note">{text(report.note)}</p>{/if}
</div>

<style>
  .site-check { display: grid; gap: 12px; }
  .verdict { display: flex; align-items: center; gap: 9px; margin: 0; font-size: 15px; font-weight: 600; color: var(--ink); }
  .dot { width: 10px; height: 10px; border-radius: 50%; background: var(--amber); box-shadow: 0 0 0 4px var(--amber-wash); }
  .verdict.up .dot { background: var(--mint); box-shadow: 0 0 0 4px var(--mint-wash); }
  .verdict.down .dot { background: var(--red); box-shadow: 0 0 0 4px var(--red-wash); }
  dl { display: grid; grid-template-columns: minmax(96px, max-content) 1fr; gap: 6px 14px; margin: 0; font-size: 12.5px; }
  dt { color: var(--ink-3); }
  dd { margin: 0; min-width: 0; overflow-wrap: anywhere; color: var(--ink); }
  dd div + div { margin-top: 2px; }
  code { font-size: 11.5px; color: var(--ink-2); }
  .bad { color: var(--red); }
  .note { margin: 0; font-size: 12px; color: var(--ink-3); }
</style>
