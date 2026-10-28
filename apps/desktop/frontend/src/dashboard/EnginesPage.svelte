<script lang="ts">
  import Icon from '../lib/Icon.svelte';
  import { listProviders } from '../lib/arcade';
  import type { ProviderInfo } from '../lib/contracts';
  import { useBox } from '../box.svelte';

  const box = useBox();
  // The engines Box's own tools need, shown found or not, as in Arcade Tools.
  const engines = [
    { capability: 'media.ffmpeg', name: 'FFmpeg', usedFor: 'Video and audio tools' },
    { capability: 'image.vips', name: 'libvips', usedFor: 'Image tools' },
    { capability: 'pdf.merge', name: 'qpdf', usedFor: 'PDF tools' },
    { capability: 'ocr.tesseract', name: 'Tesseract', usedFor: 'Text recognition (OCR)' },
  ];
  const names: Record<string, string> = {
    'image.model.background-removal': 'Background removal model',
    'image.model.upscale': 'Upscaling model',
    'ocr.lens': 'Arcade Lens',
    'screen.select.lens': 'Arcade Lens screen selection',
  };
  let checking = $state(false);
  const extra = $derived(box.providers.filter((provider) => !engines.some((engine) => engine.capability === provider.capability)));
  const foundCount = $derived(engines.filter((engine) => found(engine.capability)).length);

  function found(capability: string): ProviderInfo | undefined {
    return box.providers.find((provider) => provider.capability === capability);
  }
  const source = (provider: ProviderInfo) => provider.source === 'system' ? 'System installation' : provider.source === 'arcade-app' ? 'Arcade app' : provider.source === 'arcade' ? 'Downloaded by Arcade' : provider.source;

  async function checkAgain(): Promise<void> {
    checking = true;
    box.providerError = '';
    try {
      box.providers = await listProviders(true);
      box.providersLoaded = true;
    } catch (error) {
      box.providerError = typeof error === 'string' ? error : error instanceof Error ? error.message : String(error);
    } finally {
      checking = false;
    }
  }
</script>

<header class="page-head">
  <div>
    <h1>Engines</h1>
    <p class="sub">{box.providersLoaded ? `${foundCount} of ${engines.length} found · ` : ''}Programs Box uses for media, images, PDFs and text recognition.</p>
  </div>
  <div class="page-actions">
    <button type="button" class="btn" class:busy={checking} disabled={checking} onclick={() => void checkAgain()}><Icon name="refresh" size={14} />Check again</button>
  </div>
</header>

{#if box.providerError}
  <div class="notice danger" role="alert"><Icon name="alert" /><span class="row-main">Couldn't check the engines. {box.providerError}</span><button type="button" class="btn sm" onclick={() => void checkAgain()}>Retry</button></div>
{/if}

{#if !box.providersLoaded}
  <div class="list" aria-busy="true" aria-label="Checking engines">
    {#each engines as engine (engine.capability)}<div class="skeleton-row"><div class="row-main"><div class="skeleton line skel-a"></div><div class="skeleton line skel-b"></div></div><div class="skeleton line skel-c"></div></div>{/each}
  </div>
{:else}
  <div class="list">
    {#each engines as engine (engine.capability)}
      {@const provider = found(engine.capability)}
      <section class="engine" aria-labelledby="engine-{engine.name}">
        <div class="row">
          <div class="row-main">
            <h2 class="row-title" id="engine-{engine.name}">{engine.name}</h2>
            <div class="row-sub">{engine.usedFor}{#if provider} · {provider.version} · {source(provider)}{/if}</div>
            {#if provider}<div class="path row-path">{provider.executablePath}</div>{/if}
          </div>
          {#if provider}
            <span class="badge {provider.compatible ? 'ok' : 'warn'}">{#if provider.compatible}<span class="dot ok"></span>Found{:else}Needs attention{/if}</span>
          {:else}
            <span class="badge warn">Not found</span>
          {/if}
        </div>
        {#if provider?.warning}<div class="row-extra"><p class="status-warning">{provider.warning}</p></div>{/if}
        {#if engine.capability === 'ocr.tesseract' && !provider}
          <div class="row-extra">
            <p class="row-sub">{box.tesseractMessage || (box.tesseractOffer ? 'Arcade apps can download a private copy they all share.' : 'Install Tesseract with its English language data from your package manager.')}</p>
            {#if box.tesseractOffer}<button type="button" class="btn sm primary" disabled={box.tesseractBusy} onclick={() => void box.getTesseract()}>{box.tesseractBusy ? 'Downloading…' : `Download with Arcade (${box.tesseractOffer} MB)`}</button>{/if}
          </div>
        {/if}
      </section>
    {/each}
  </div>
  {#if extra.length}
    <section class="section">
      <div class="section-head"><h2>Also found</h2></div>
      <div class="list">
        {#each extra as provider (`${provider.capability}-${provider.executablePath}`)}
          <div class="row">
            <div class="row-main">
              <div class="row-title">{names[provider.capability] ?? provider.capability}</div>
              <div class="row-sub">{provider.version} · {source(provider)}</div>
              {#if provider.executablePath}<div class="path row-path">{provider.executablePath}</div>{/if}
            </div>
            <span class="badge {provider.compatible ? 'ok' : 'warn'}">{provider.compatible ? 'Found' : 'Needs attention'}</span>
          </div>
        {/each}
      </div>
    </section>
  {/if}
  <p class="section-note">Box shows what it finds but never installs system packages itself.</p>
{/if}

<style>
  .engine + .engine { border-top: 1px solid var(--line); }
  h2.row-title { font-size: var(--fs-body); font-weight: var(--fw-semibold); }
  .row-path { margin-top: var(--sp-2); color: var(--faint); }
  .row-extra { display: flex; flex-wrap: wrap; align-items: center; gap: var(--sp-8); padding-left: var(--sp-14); }
  .row-extra .row-sub { flex: 1 1 240px; margin: 0; }
  .skel-a { width: 30%; }
  .skel-b { width: 55%; margin-top: var(--sp-8); }
  .skel-c { width: 70px; }
</style>
