<script lang="ts">
  import { onMount } from 'svelte';
  import ArcadeBadge from './ArcadeBadge.svelte';
  import type { ToolOutput, ToolSummary } from './contracts';
  import { cancelResultLinkAction, invokeResultLinkAction, resultLinkActions, watchLinkChanged, type ResultLinkAction } from './arcade';
  let { outputs = [], toolId, presets = [] }: { outputs?: ToolOutput[]; toolId: string; presets?: NonNullable<ToolSummary['presets']> } = $props();
  let offers = $state<ResultLinkAction[]>([]);
  let preset = $state('');
  let revision = $state(0);
  let pendingSend = $state<ResultLinkAction | null>(null);
  let busy = $state('');
  let error = $state('');
  let message = $state('');
  let requestId = '';
  let disposed = false;
  $effect(() => {
    const currentOutputs = outputs; const currentTool = toolId; const currentPreset = preset || null; revision;
    let current = true;
    void resultLinkActions(currentOutputs, currentTool, currentPreset).then((value) => { if (current) offers = value; }).catch((reason) => { if (current) { offers = []; error = errorText(reason); } });
    return () => { current = false; };
  });
  onMount(() => {
    let stop = () => {};
    void watchLinkChanged(() => revision++).then((unlisten) => { if (disposed) unlisten(); else stop = unlisten; });
    return () => { disposed = true; stop(); if (requestId) void cancelResultLinkAction(requestId); };
  });
  function errorText(reason: unknown): string { return typeof reason === 'string' ? reason : reason instanceof Error ? reason.message : 'The action could not finish.'; }
  async function run(offer: ResultLinkAction): Promise<void> {
    busy = offer.key; error = ''; message = ''; pendingSend = null;
    requestId = crypto.randomUUID();
    try { const result = await invokeResultLinkAction(requestId, offer.key, outputs, toolId, preset || null); if (!disposed) message = result.message || 'Done.'; }
    catch (reason) { error = errorText(reason); revision++; }
    finally { busy = ''; requestId = ''; }
  }
</script>

{#if offers.length}
  <div class="arcade-result-actions" aria-label="Connected app actions">
    <div class="actions-row">{#each offers as offer (offer.key)}<button type="button" class="quiet-button" disabled={!offer.enabled || !!busy} title={offer.reason || offer.preview} onclick={() => { if (offer.key === 'send') pendingSend = offer; else void run(offer); }}><ArcadeBadge app={offer.app} />{busy === offer.key ? 'Working…' : offer.title}</button>{/each}{#if busy}<button type="button" class="quiet-button" onclick={() => void cancelResultLinkAction(requestId)}>Cancel action</button>{/if}</div>
    {#if presets.length && offers.some((offer) => offer.key === 'wheel')}<label class="wheel-preset">Wheel action <select bind:value={preset} disabled={!!busy}><option value="">This tool</option>{#each presets as choice}<option value={choice.id}>{choice.name}</option>{/each}</select></label>{/if}
    {#each offers.filter((offer) => offer.reason) as offer}<p class="action-reason">{offer.reason}</p>{/each}
    {#if pendingSend}<div class="send-preview"><strong>Send to my devices ↗</strong><p>{pendingSend.preview}</p><div class="actions-row"><button type="button" class="quiet-button" onclick={() => pendingSend = null}>Cancel</button><button type="button" class="quiet-button" onclick={() => void run(pendingSend!)}>Send to my devices</button></div></div>{/if}
    {#if message}<p role="status">{message}</p>{/if}
    {#if error}<p class="field-error" role="alert">{error}</p>{/if}
  </div>
{/if}

<style>
  .arcade-result-actions { padding: 12px 14px; border-top: 1px solid var(--line); font-size: 12px; }
  .actions-row { display: flex; gap: 8px; flex-wrap: wrap; }
  .actions-row button { display: flex; align-items: center; gap: 7px; }
  .wheel-preset { display: flex; align-items: center; gap: 8px; margin-top: 10px; color: var(--muted); }
  select { max-width: 240px; font-size: 12px; }
  .action-reason { color: var(--muted); margin: 8px 0 0; }
  .send-preview { border: 1px solid var(--line); padding: 12px; border-radius: 12px; margin-top: 10px; }
  .send-preview p { white-space: pre-wrap; overflow-wrap: anywhere; max-height: 120px; overflow: auto; }
</style>
