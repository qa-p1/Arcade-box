<script lang="ts">
  import { copyText } from './arcade';
  import { onMount } from 'svelte';
  import Icon from './Icon.svelte';
  import StandardToolForm from './StandardToolForm.svelte';
  import SelectedFilesInput from './SelectedFilesInput.svelte';
  import type { Pipeline, PipelineNode, SelectedFile, ToolInput, ToolOutput, ToolSummary } from './contracts';
  import { deletePipeline, listPipelines, listTools, openArtifact, revealArtifact, runPipeline, saveArtifactAs, savePipeline, selectFiles } from './arcade';
  import { fileInputMime, isRunnable } from './tool-utils';
import { defaultUiValues, serializeStandardUiOptions, standardUiOptionsProblem, type UiValues } from './standard-ui';

  interface DraftStage {
    id: string;
    toolId: string;
    options: UiValues;
  }

  let {
    tools,
    onToolsChanged,
  }: {
    tools: ToolSummary[];
    onToolsChanged: (tools: ToolSummary[]) => void;
  } = $props();

  let pipelines = $state<Pipeline[]>([]);
  let loading = $state(true);
  let loadingError = $state('');
  let saveError = $state('');
  let runError = $state('');
  let editorMessage = $state('');
  let editing = $state(false);
  let saving = $state(false);
  let running = $state(false);
  let deletingId = $state('');
  let currentId = $state('');
  let currentVersion = $state(0);
  let pipelineName = $state('');
  let inputType = $state('');
  let inputText = $state('');
  let inputFiles = $state<SelectedFile[]>([]);
  let fileError = $state('');
  let stages = $state<DraftStage[]>([]);
  let stageChoice = $state('');
  let lastRunOutputs = $state<ToolOutput[]>([]);
  let lastRunPipelineId = $state('');
  let lastRunName = $state('');
  let artifactBusy = $state(false);
  let artifactError = $state('');
  let artifactMessage = $state('');

  const inputTypeChoices = $derived.by(() => {
    const types = new Set<string>();
    for (const tool of tools.filter(isPipelineCandidate)) {
      for (const type of tool.inputs) {
        const normalized = normalizeInputType(type);
        if (normalized && normalized !== 'file/any' && normalized !== 'file/media') types.add(normalized);
      }
    }
    return [...types].sort((left, right) => left.localeCompare(right));
  });
  const firstStageChoices = $derived(tools.filter((tool) => isPipelineCandidate(tool)
    && hasCompatibleInput(tool, inputType)
    && tool.ui?.input.kind !== 'files'));
  const firstStageTool = $derived(stages.length ? tools.find((tool) => tool.id === stages[0].toolId) : undefined);
  const lastStageTool = $derived(stages.length ? tools.find((tool) => tool.id === stages[stages.length - 1].toolId) : undefined);
  const nextStageChoices = $derived.by(() => {
    if (!lastStageTool) return [];
    return tools.filter((tool) => isPipelineCandidate(tool)
      && tool.id !== lastStageTool.id
      && tool.ui?.input.kind !== 'files'
      && (tool.ui?.input.minItems ?? 1) <= 1
      && arePipelineTypesCompatible(lastStageTool.outputs, tool.inputs));
  });
  const pipelineId = $derived(slugify(pipelineName));
  const stablePipelineId = $derived(currentId || pipelineId);
  const formProblem = $derived.by(() => {
    if (!pipelineName.trim()) return 'Give this pipeline a name.';
    if (!stablePipelineId || !/^[a-z0-9]+(?:-[a-z0-9]+)*$/.test(stablePipelineId)) return 'Use a name with letters or numbers.';
    if (!inputType) return 'Choose the type of input this pipeline starts with.';
    if (!stages.length) return 'Add at least one tool.';
    const first = tools.find((tool) => tool.id === stages[0].toolId);
    if (!first || !hasCompatibleInput(first, inputType)) return 'The first tool cannot accept this input type.';
    for (let index = 0; index < stages.length; index += 1) {
      const stage = stages[index];
      const tool = tools.find((item) => item.id === stage.toolId);
      if (!tool || !tool.ui || tool.ui.version !== 1) return 'Every stage needs a supported standard form.';
      if (tool.ui.input.kind === 'folder') return 'This saved workflow needs a scoped folder input. The current editor cannot edit folder-based stages.';
      if (tool.ui.input.kind === 'none' || tool.ui.input.kind === 'files') return 'This editor currently supports one input per stage.';
      const optionsProblem = stageOptionsProblem(tool, stage.options);
      if (optionsProblem) return `Stage ${index + 1}: ${optionsProblem}`;
    }
    for (let index = 1; index < stages.length; index += 1) {
      const previous = tools.find((tool) => tool.id === stages[index - 1].toolId);
      const current = tools.find((tool) => tool.id === stages[index].toolId);
      if (!previous || !current || !arePipelineTypesCompatible(previous.outputs, current.inputs)) return `Stage ${index + 1} cannot accept the output from ${previous?.name || 'the previous stage'}.`;
    }
    if (inputType.startsWith('file/') && inputFiles.some((file) => !hasCompatibleInput(first, file.mime))) return 'The selected file type does not match the first stage.';
    return '';
  });
  const runProblem = $derived(formProblem || (inputType.startsWith('file/') && inputFiles.length !== 1 ? 'Choose one starting file to run this pipeline.' : ''));

  onMount(() => {
    void refreshPipelines();
  });

  async function refreshPipelines(): Promise<void> {
    loading = true;
    loadingError = '';
    try {
      pipelines = await listPipelines();
    } catch (error) {
      loadingError = errorMessage(error);
    } finally {
      loading = false;
    }
  }

  function errorMessage(error: unknown): string {
    if (typeof error === 'string') return error;
    if (error instanceof Error) return error.message;
    return 'The local pipeline service could not complete that request.';
  }

  function isPipelineCandidate(tool: ToolSummary): boolean {
    return isRunnable(tool) && tool.ui?.version === 1 && tool.ui.input.kind !== 'none' && tool.ui.input.kind !== 'folder';
  }

  function stageOptionsProblem(tool: ToolSummary, values: UiValues): string {
    const standardProblem = standardUiOptionsProblem(tool.ui, values);
    if (standardProblem || tool.id !== 'arcade.image.resize') return standardProblem;

    const positiveValue = (value: string | undefined) => Boolean(value?.trim()) && Number(value) > 0;
    switch (values.mode) {
      case 'fit':
        return positiveValue(values.width) || positiveValue(values.height)
          ? ''
          : 'Enter a width or height for the fitted image.';
      case 'fill':
        return positiveValue(values.width) && positiveValue(values.height)
          ? ''
          : 'Fill mode needs positive width and height values.';
      case 'longest':
      case 'shortest':
        return positiveValue(values.edge) ? '' : 'Enter an edge length for this resize mode.';
      case 'percentage': {
        const percentage = Number(values.percentage);
        return Number.isFinite(percentage) && percentage >= 1 && percentage <= 1000
          ? ''
          : 'Scale must be between 1% and 1000%.';
      }
      default:
        return 'Choose a resize mode.';
    }
  }

  function normalizeInputType(value: string): string {
    return value.replace(/\[\]$/, '');
  }

  function hasCompatibleInput(tool: ToolSummary | undefined, type: string): boolean {
    if (!tool || !type) return false;
    return tool.inputs.some((accepted) => {
      const normalized = normalizeInputType(accepted);
      return normalized === type
        || normalized === 'file/any' && type.startsWith('file/')
        || normalized === 'file/media' && (type === 'file/video' || type === 'file/audio')
        || ((normalized === 'network/url' || normalized === 'text/url') && (type === 'network/url' || type === 'text/url'));
    });
  }

  function arePipelineTypesCompatible(outputs: string[], inputs: string[]): boolean {
    return outputs.some((output) => inputs.some((input) => input === output || input.endsWith('[]') && input.slice(0, -2) === output));
  }

  function slugify(value: string): string {
    return value.toLowerCase().trim().replace(/[^a-z0-9]+/g, '-').replace(/^-|-$/g, '');
  }

  function humanize(value: string): string {
    return value.split('-').filter(Boolean).map((part) => part[0].toUpperCase() + part.slice(1)).join(' ');
  }

  function startNewPipeline(): void {
    editing = true;
    currentId = '';
    currentVersion = 0;
    pipelineName = '';
    inputType = inputTypeChoices[0] || '';
    inputText = '';
    inputFiles = [];
    fileError = '';
    stages = [];
    stageChoice = '';
    saveError = '';
    runError = '';
    editorMessage = '';
    lastRunOutputs = [];
    lastRunPipelineId = '';
    lastRunName = '';
  }

  function stageOptionsFromNode(tool: ToolSummary | undefined, options: Record<string, unknown>): UiValues {
    const values = defaultUiValues(tool?.ui);
    for (const [key, value] of Object.entries(options)) {
      values[key] = typeof value === 'boolean' || typeof value === 'number' || typeof value === 'string' ? String(value) : '';
    }
    return values;
  }

  function editPipeline(pipeline: Pipeline): void {
    const ordered = [...pipeline.nodes];
    const first = ordered[0];
    const firstTool = first && tools.find((tool) => tool.id === first.toolId);
    if (!first || !firstTool) {
      editorMessage = 'This saved pipeline refers to a tool that is no longer available.';
      return;
    }
    if (ordered.some((node) => tools.find((tool) => tool.id === node.toolId)?.ui?.input.kind === 'folder')) {
      editorMessage = 'This saved workflow needs a scoped folder input. Folder-based workflows cannot run from search yet, and this editor cannot change them. You can still delete the saved pipeline.';
      return;
    }
    const supportedLinear = first.inputs.length === 1
      && first.inputs[0].kind === 'external'
      && first.inputs[0].index === 0
      && ordered.slice(1).every((node, index) => node.inputs.length === 1
        && node.inputs[0].kind === 'node'
        && node.inputs[0].nodeId === ordered[index].id
        && node.inputs[0].outputIndex === 0);
    if (!supportedLinear) {
      editorMessage = 'This definition uses a DAG layout that the linear editor cannot edit. You can still run or delete it.';
      return;
    }
    editing = true;
    currentId = pipeline.id;
    currentVersion = pipeline.version;
    pipelineName = pipeline.name || humanize(pipeline.id);
    inputType = firstTool.inputs.map(normalizeInputType).find((item) => item !== 'file/any' && item !== 'file/media') || '';
    inputText = '';
    inputFiles = [];
    fileError = '';
    stages = ordered.map((node) => {
      const tool = tools.find((item) => item.id === node.toolId);
      const options = node.options && typeof node.options === 'object' && !Array.isArray(node.options) ? node.options as Record<string, unknown> : {};
      return { id: node.id, toolId: node.toolId, options: stageOptionsFromNode(tool, options) };
    });
    stageChoice = '';
    saveError = '';
    runError = '';
    editorMessage = '';
    lastRunOutputs = [];
    lastRunPipelineId = '';
    lastRunName = '';
  }

  function updateInputType(value: string): void {
    inputType = value;
    inputFiles = [];
    inputText = '';
    fileError = '';
    if (stages.length && !hasCompatibleInput(tools.find((tool) => tool.id === stages[0].toolId), value)) stages = [];
  }

  function addStage(toolId: string): void {
    if (!toolId || stages.some((stage) => stage.toolId === toolId)) {
      stageChoice = '';
      return;
    }
    const tool = tools.find((item) => item.id === toolId);
    if (!tool?.ui || tool.ui.version !== 1) return;
    stages = [...stages, { id: `stage-${stages.length + 1}`, toolId, options: defaultUiValues(tool.ui) }];
    stageChoice = '';
    runError = '';
  }

  function removeStage(index: number): void {
    stages = stages.filter((_, stageIndex) => stageIndex !== index).map((stage, stageIndex) => ({ ...stage, id: `stage-${stageIndex + 1}` }));
  }

  function updateStageOption(index: number, key: string, value: string): void {
    stages[index].options[key] = value;
  }

  async function chooseStartingFile(): Promise<void> {
    fileError = '';
    try {
      const chosen = await selectFiles();
      if (!chosen.length) return;
      if (chosen.length !== 1) {
        fileError = 'This pipeline editor accepts one starting file.';
        return;
      }
      if (!hasCompatibleInput(firstStageTool, chosen[0].mime)) {
        fileError = `The selected ${chosen[0].mime} file is not accepted by the first stage.`;
        return;
      }
      inputFiles = chosen;
    } catch (error) {
      fileError = errorMessage(error);
    }
  }

  function removeStartingFile(token: string): void {
    inputFiles = inputFiles.filter((file) => file.token !== token);
  }

  function reorderStartingFiles(fromToken: string, toIndex: number): void {
    if (fromToken !== inputFiles[0]?.token || toIndex !== 0) return;
  }

  function formatBytes(bytes: number): string {
    if (!Number.isFinite(bytes) || bytes < 0) return 'Unknown size';
    if (bytes < 1024) return `${bytes} B`;
    const units = ['KB', 'MB', 'GB', 'TB'];
    let value = bytes / 1024;
    let unit = 0;
    while (value >= 1024 && unit < units.length - 1) { value /= 1024; unit += 1; }
    return `${value.toFixed(value >= 10 ? 0 : 1)} ${units[unit]}`;
  }

  function buildPipeline(version: number): Pipeline {
    const id = stablePipelineId;
    const nodes: PipelineNode[] = stages.map((stage, index) => {
      const tool = tools.find((item) => item.id === stage.toolId)!;
      const inputs = index === 0
        ? [{ kind: 'external' as const, index: 0 }]
        : [{ kind: 'node' as const, nodeId: stages[index - 1].id, outputIndex: 0 }];
      const options = serializeStandardUiOptions(tool.ui, stage.options);
      for (const control of tool.ui?.controls ?? []) {
        if (control.type === 'directory') delete options[control.key];
      }
      return {
        id: stage.id,
        toolId: stage.toolId,
        inputs,
        options,
      };
    });
    return { id, name: pipelineName.trim(), version, nodes, outputNodes: nodes.length ? [nodes[nodes.length - 1].id] : [] };
  }

  async function saveCurrentPipeline(): Promise<Pipeline | null> {
    if (formProblem) {
      saveError = formProblem;
      return null;
    }
    saving = true;
    saveError = '';
    try {
      const existing = pipelines.find((pipeline) => pipeline.id === (currentId || pipelineId));
      if (existing) {
        const candidate = buildPipeline(existing.version);
        const stored = { ...existing, version: candidate.version };
        if (JSON.stringify(stored) === JSON.stringify(candidate)) {
          currentId = existing.id;
          currentVersion = existing.version;
          return existing;
        }
      }
      const definition = buildPipeline(existing ? Math.max(currentVersion, existing.version) + 1 : 1);
      const saved = await savePipeline(definition);
      pipelines = [...pipelines.filter((pipeline) => pipeline.id !== saved.id), saved].sort((a, b) => a.id.localeCompare(b.id));
      currentId = saved.id;
      currentVersion = saved.version;
      editorMessage = `${saved.name} saved.`;
      onToolsChanged(await listTools());
      return saved;
    } catch (error) {
      saveError = errorMessage(error);
      return null;
    } finally {
      saving = false;
    }
  }

  async function runCurrentPipeline(): Promise<void> {
    if (runProblem || running) {
      runError = runProblem;
      return;
    }
    running = true;
    runError = '';
    artifactError = '';
    lastRunOutputs = [];
    const saved = await saveCurrentPipeline();
    if (!saved) {
      running = false;
      return;
    }
    try {
      const inputs: ToolInput[] = inputType.startsWith('file/')
        ? inputFiles.map((file) => ({ kind: 'artifact', value: file.token, mime: fileInputMime(file) }))
        : [{ kind: inputType === 'network/url' || inputType === 'text/url' ? 'url' : 'text', value: inputText, mime: inputType }];
      const result = await runPipeline(saved.id, inputs);
      const lastNode = saved.outputNodes[saved.outputNodes.length - 1];
      lastRunOutputs = result[lastNode] || [];
      lastRunPipelineId = saved.id;
      lastRunName = saved.name;
    } catch (error) {
      runError = errorMessage(error);
    } finally {
      running = false;
    }
  }

  async function removePipeline(pipeline: Pipeline): Promise<void> {
    deletingId = pipeline.id;
    loadingError = '';
    try {
      await deletePipeline(pipeline.id);
      pipelines = pipelines.filter((item) => item.id !== pipeline.id);
      onToolsChanged(await listTools());
      if (currentId === pipeline.id) cancelEditing();
    } catch (error) {
      loadingError = errorMessage(error);
    } finally {
      deletingId = '';
    }
  }

  function cancelEditing(): void {
    editing = false;
    currentId = '';
    pipelineName = '';
    stages = [];
    inputFiles = [];
    inputText = '';
    saveError = '';
    runError = '';
    editorMessage = '';
    lastRunOutputs = [];
    lastRunPipelineId = '';
    lastRunName = '';
  }

  async function copyOutput(value: string): Promise<void> {
    try {
      await copyText(value);
      artifactError = '';
    } catch {
      artifactError = 'Clipboard access is unavailable.';
    }
  }

  async function showArtifact(token: string, reveal: boolean): Promise<void> {
    artifactError = '';
    artifactMessage = '';
    artifactBusy = true;
    try {
      if (reveal) await revealArtifact(token);
      else await openArtifact(token);
    } catch (error) {
      artifactError = errorMessage(error);
    } finally {
      artifactBusy = false;
    }
  }

  async function saveArtifact(token: string): Promise<void> {
    artifactError = '';
    artifactMessage = '';
    artifactBusy = true;
    try {
      const saved = await saveArtifactAs(token);
      if (saved) artifactMessage = `Saved ${saved.name}.`;
    } catch (error) {
      artifactError = errorMessage(error);
    } finally {
      artifactBusy = false;
    }
  }

  function outputLabel(output: ToolOutput, index: number): string {
    if (output.kind === 'text' || output.mime.startsWith('text/') || output.mime.includes('json') || output.mime.startsWith('structured/')) return `Text result ${index + 1}`;
    return `Generated file ${index + 1}`;
  }
</script>

<section class="pipeline-view" aria-label="Saved pipelines and pipeline editor">
  <div class="pipeline-toolbar"><p>Connect compatible tools into a saved workflow. The editor builds a linear chain; the runtime stores the same versioned DAG shape used by the pipeline engine.</p><button class="pipeline-primary-button" onclick={startNewPipeline}><Icon name="plus" size={15} />New pipeline</button></div>
  {#if loadingError}<div class="dashboard-alert" role="alert"><span class="alert-icon"><Icon name="network" size={16} /></span><div><strong>Pipeline storage unavailable</strong><span>{loadingError}</span></div></div>{/if}
  {#if loading}<div class="catalog-loading"><span class="spinner"></span><span>Loading saved pipelines…</span></div>
  {:else if pipelines.length === 0 && !editing}<div class="pipeline-empty"><span class="pipeline-empty-icon"><Icon name="spark" size={23} /></span><strong>No saved pipelines yet</strong><span>Combine compatible tools, then save the workflow under a name you can find again.</span></div>{/if}

  {#if pipelines.length > 0 && !editing}
    <section class="catalog-section pipeline-saved-section">
      <div class="catalog-section-heading"><div><h2>Saved pipelines</h2><span>Versioned workflows on this device</span></div><span class="section-count">{pipelines.length}</span></div>
      <div class="pipeline-list">
        {#each pipelines as pipeline (pipeline.id)}
          <article class="pipeline-card">
            <span class="pipeline-card-icon"><Icon name="spark" size={18} /></span>
            <div class="pipeline-card-copy"><strong>{pipeline.name || humanize(pipeline.id)}</strong><span>v{pipeline.version} · {pipeline.nodes.length} {pipeline.nodes.length === 1 ? 'stage' : 'stages'}</span><small>{pipeline.nodes.map((node) => tools.find((tool) => tool.id === node.toolId)?.name || node.toolId).join(' → ')}</small></div>
            <div class="pipeline-card-actions"><button class="quiet-button" onclick={() => editPipeline(pipeline)}><Icon name="command" size={14} /><span>Edit</span></button><button class="icon-button remove-file-button" aria-label={`Delete pipeline ${pipeline.name || humanize(pipeline.id)}`} disabled={deletingId === pipeline.id} onclick={() => void removePipeline(pipeline)}><Icon name="close" size={14} /></button></div>
          </article>
        {/each}
      </div>
    </section>
  {/if}

  {#if editorMessage}<div class="pipeline-message" role="status">{editorMessage}</div>{/if}
  {#if editing}
    <section class="pipeline-editor" aria-labelledby="pipeline-editor-title">
      <div class="pipeline-editor-heading"><div><span class="eyebrow">PIPELINE EDITOR</span><h2 id="pipeline-editor-title">{currentId ? `Edit ${pipelineName || humanize(currentId)}` : 'Build a new pipeline'}</h2><p>Tools connect only when their declared output and input types match. This editor accepts one starting input and creates a linear workflow.</p></div><button class="quiet-button" onclick={cancelEditing}><Icon name="close" size={14} /><span>Cancel</span></button></div>
      <div class="pipeline-name-field"><label for="pipeline-name">Pipeline name</label><input id="pipeline-name" bind:value={pipelineName} maxlength="100" placeholder="e.g. Optimize Images" /><small>Search ID: {stablePipelineId || 'enter a name'}</small></div>
      <div class="pipeline-start">
        <div class="pipeline-step-number">01</div><div class="pipeline-step-body">
          <div class="pipeline-step-heading"><strong>Starting input</strong><span>External input · index 0</span></div>
          <label for="pipeline-input-type">Input type</label><select id="pipeline-input-type" value={inputType} onchange={(event) => updateInputType((event.currentTarget as HTMLSelectElement).value)}>{#each inputTypeChoices as choice}<option value={choice}>{choice}</option>{/each}</select>
          {#if inputType.startsWith('file/')}
            <SelectedFilesInput files={inputFiles} label="starting file" onAdd={() => void chooseStartingFile()} onRemove={removeStartingFile} onReorder={reorderStartingFiles} formatSize={formatBytes} />
          {:else}
            <label class="pipeline-input-label" for="pipeline-input-value">{inputType.includes('/url') ? 'URL' : 'Starting text'}</label>{#if inputType.includes('/url')}<input id="pipeline-input-value" class="pipeline-text-input" type="url" bind:value={inputText} placeholder="https://example.com" />{:else}<textarea id="pipeline-input-value" class="pipeline-text-input pipeline-textarea" bind:value={inputText} placeholder="Paste text to start the pipeline" rows="3"></textarea>{/if}
          {/if}
          {#if fileError}<div class="field-error" role="alert">{fileError}</div>{/if}
        </div>
      </div>
      {#each stages as stage, index (stage.id)}
        {@const tool = tools.find((item) => item.id === stage.toolId)}
        {@const previous = index ? tools.find((item) => item.id === stages[index - 1].toolId) : undefined}
        <div class="pipeline-connector" aria-label={index === 0 ? 'External input connection' : `Connection from ${previous?.name || 'previous stage'}`}><span></span><small>{index === 0 ? `${inputType} → ${tool?.inputs.join(', ')}` : `${previous?.outputs.join(', ')} → ${tool?.inputs.join(', ')}`}</small></div>
        <article class="pipeline-stage-card">
          <div class="pipeline-stage-top"><span class="pipeline-step-number">{String(index + 2).padStart(2, '0')}</span><div class="pipeline-stage-title"><strong>{tool?.name || stage.toolId}</strong><span>{stage.id} · {tool?.outputs.join(', ') || 'unknown output'}</span></div><button class="icon-button remove-file-button" aria-label={`Remove stage ${index + 1}`} onclick={() => removeStage(index)}><Icon name="close" size={14} /></button></div>
          {#if tool?.ui}<StandardToolForm ui={tool.ui} values={stage.options} idPrefix={`pipeline-${stage.id}`} allowDirectory={false} onValueChange={(key, value) => updateStageOption(index, key, value)} />{/if}
        </article>
      {/each}
      <div class="pipeline-add-stage">
        <label for="pipeline-add-stage">Add compatible stage</label><select id="pipeline-add-stage" value={stageChoice} disabled={stages.length === 0 ? firstStageChoices.length === 0 : nextStageChoices.length === 0} onchange={(event) => addStage((event.currentTarget as HTMLSelectElement).value)}><option value="">{stages.length ? (nextStageChoices.length ? 'Choose a tool for the next step' : 'No compatible next tools') : (firstStageChoices.length ? 'Choose the first tool' : 'No tool accepts this input')}</option>{#each (stages.length ? nextStageChoices : firstStageChoices) as tool (tool.id)}<option value={tool.id}>{tool.name} · {tool.inputs.join(', ')} → {tool.outputs.join(', ')}</option>{/each}</select>
      </div>
      {#if formProblem}<p class="pipeline-validation" role="status"><Icon name="network" size={14} />{formProblem}</p>{:else if runProblem}<p class="pipeline-validation" role="status"><Icon name="file" size={14} />{runProblem}</p>{/if}
      {#if saveError}<div class="field-error" role="alert">{saveError}</div>{/if}
      {#if runError}<div class="field-error" role="alert">{runError}</div>{/if}
      <div class="pipeline-actions"><button class="quiet-button" disabled={saving || Boolean(formProblem)} onclick={() => void saveCurrentPipeline()}><Icon name="check" size={14} /><span>{saving ? 'Saving…' : 'Save pipeline'}</span></button><button class="pipeline-primary-button" disabled={saving || running || Boolean(runProblem)} onclick={() => void runCurrentPipeline()}>{#if running}<span class="spinner"></span><span>Running…</span>{:else}<Icon name="play" size={14} /><span>Save &amp; run</span>{/if}</button></div>
    </section>
  {/if}

  {#if lastRunOutputs.length}
    <section class="pipeline-output-panel" aria-labelledby="pipeline-output-heading">
      <div class="catalog-section-heading"><div><h2 id="pipeline-output-heading">{lastRunName || humanize(lastRunPipelineId)} · result</h2><span>Output from the final pipeline stage</span></div><span class="section-count">{lastRunOutputs.length}</span></div>
      {#each lastRunOutputs as output, index (`${output.value}-${index}`)}
        {#if output.kind === 'text' || output.mime.startsWith('text/') || output.mime.includes('json') || output.mime.startsWith('structured/')}
          <pre class="output-text">{output.value}</pre><button class="copy-button" onclick={() => void copyOutput(output.value)}><Icon name="copy" size={13} /> Copy result</button>
        {:else}
          <div class="artifact-result-card"><span class="artifact-file-icon"><Icon name="file" size={18} /></span><div class="artifact-result-copy"><strong>{outputLabel(output, index)}</strong><span>{output.mime}</span></div><div class="artifact-actions"><button class="quiet-button" disabled={artifactBusy} onclick={() => void showArtifact(output.value, false)}><Icon name="external" size={14} /><span>Open</span></button><button class="quiet-button" disabled={artifactBusy} onclick={() => void saveArtifact(output.value)}><Icon name="folder" size={14} /><span>Save as</span></button><button class="quiet-button" disabled={artifactBusy} onclick={() => void showArtifact(output.value, true)}><Icon name="folder" size={14} /><span>Reveal</span></button></div></div>
        {/if}
      {/each}
      {#if artifactError}<div class="field-error" role="alert">{artifactError}</div>{/if}
      {#if artifactMessage}<div class="pipeline-message" role="status">{artifactMessage}</div>{/if}
    </section>
  {/if}
</section>
