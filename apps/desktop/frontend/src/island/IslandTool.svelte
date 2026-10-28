<script lang="ts">
  import { fade } from 'svelte/transition';
  import Icon from '../lib/Icon.svelte';
  import DiffInput from '../lib/DiffInput.svelte';
  import ImageResult from '../lib/ImageResult.svelte';
  import ToolOutputView from '../lib/ToolOutputView.svelte';
  import ResultActions from '../lib/ResultActions.svelte';
  import { cancelResultLinkAction } from '../lib/arcade';
  import { inputPlaceholder, runLabel } from '../lib/tool-presentation';
  import StandardToolForm from '../lib/StandardToolForm.svelte';
  import VideoAssist from '../lib/VideoAssist.svelte';
  import AudioAssist from '../lib/AudioAssist.svelte';
  import SelectedFilesInput from '../lib/SelectedFilesInput.svelte';
  import SelectedFolderInput from '../lib/SelectedFolderInput.svelte';
  import ScreenColorSampler from '../lib/ScreenColorSampler.svelte';
  import ScreenRuler from '../lib/ScreenRuler.svelte';
  import ClipboardHistoryView from '../lib/ClipboardHistoryView.svelte';
  import TimerView from '../lib/TimerView.svelte';
  import { acceptsTextInput, isRunnable, privacyLabel, usesFileInput } from '../lib/tool-utils';
  import { standardUiOptionsProblem } from '../lib/standard-ui';
  import { useBox } from '../box.svelte';

  const box = useBox();
</script>
{#if box.selectedTool}
<div class="tool-view" in:fade|global={{ duration: 140 }}>
  <header class="tool-header">
    <button type="button" class="btn ghost icon-only sm tool-back" onclick={box.backFromTool} aria-label="Back to search" title="Back (Esc)"><Icon name="back" size={15} /></button>
    <span class="glyph-tile sm"><Icon name={box.iconForCategory(box.selectedTool.category)} size={14} /></span>
    <div class="tool-title-block">
      <div class="tool-title-line">
        <h1 class="t-title-sm" bind:this={box.selectedToolHeading} tabindex="-1">{box.selectedTool.name}</h1>
        {#if privacyLabel(box.selectedTool.privacyClass) !== 'LOCAL'}<span class="badge warn" title="Uses an online service">online</span>{/if}
      </div>
      <p>{box.selectedTool.description}</p>
    </div>
    <button type="button" class="btn ghost icon-only sm" class:favorite-on={box.isFavorite(box.selectedTool.id)} aria-label={box.isFavorite(box.selectedTool.id) ? `Remove ${box.selectedTool.name} from favorites` : `Add ${box.selectedTool.name} to favorites`} aria-pressed={box.isFavorite(box.selectedTool.id)} title={box.isFavorite(box.selectedTool.id) ? 'Remove from favorites' : 'Add to favorites'} onclick={(event) => box.selectedTool && void box.toggleFavorite(box.selectedTool, event)}><Icon name="star" size={15} /></button>
    <button type="button" class="btn ghost sm" onclick={box.toggleDashboard}><Icon name="apps" size={14} />Open dashboard</button>
  </header>

  {#if !isRunnable(box.selectedTool)}
    <div class="notice-card planned-notice">
      <span class="notice-icon"><Icon name="clock" size={18} /></span>
      <div>
        <strong>This tool is planned</strong>
        <p>It is listed for discovery and search, but the local runtime does not expose it yet.</p>
      </div>
    </div>
  {:else if !box.isScreenTool(box.selectedTool.id) && !acceptsTextInput(box.selectedTool) && !usesFileInput(box.selectedTool) && box.selectedTool.ui?.input.kind !== 'none' && box.selectedTool.ui?.input.kind !== 'folder'}
    <div class="notice-card integration-notice">
      <span class="notice-icon"><Icon name="file" size={18} /></span>
      <div>
        <strong>This input is not available in the current view</strong>
        <p>This tool accepts {box.selectedTool.inputs.join(', ') || 'a non-text input'}.</p>
      </div>
    </div>
  {:else}
    <div class="input-panel">
      {#if box.selectedTool.id === 'arcade.system.clipboard-history'}
        {#if box.overlaps.clipboardPick}
          <p class="field-note">Choose a clip from Arcade Clipboard. Your clipboard stays unchanged.</p>
          <button class="run-button" disabled={box.running} onclick={() => void box.submitTool()}><Icon name="clipboard" size={15} />{box.running ? 'Choosing…' : 'Choose a clip'}</button>
        {:else}<ClipboardHistoryView />{/if}
      {:else if box.selectedTool.id === 'arcade.utility.timer'}
        <TimerView />
      {:else}
      {#if box.isScreenTool(box.selectedTool.id)}
        <section class="screen-action-status" aria-label="Screen capture availability" aria-live="polite">
          <div class="screen-status-heading"><span class="runtime-dot" class:offline={(box.isScreenRecorderTool(box.selectedTool.id) ? box.screenStatus?.recordingAvailable : box.screenStatus?.captureAvailable) === false}></span><strong>{box.isScreenRecorderTool(box.selectedTool.id) ? (box.screenStatus?.recordingAvailable ? 'Screen recording is available' : box.screenStatus ? 'Screen recording unavailable' : 'Checking screen recording') : (box.screenStatus?.captureAvailable ? 'Screen selection is available' : box.screenStatus ? 'Screen selection unavailable' : 'Checking screen capture')}</strong><span>{box.isScreenRecorderTool(box.selectedTool.id) ? box.screenRecording?.platform || box.screenStatus?.platform || 'checking' : box.screenStatus?.selectionMode || 'checking'}</span></div>
          <p>{box.isScreenRecorderTool(box.selectedTool.id) ? (box.screenRecording?.recording || box.screenRecording?.starting || box.screenRecording?.finalizing || box.screenRecording?.message === 'Recording discarded.' ? box.screenRecording.message : box.screenStatus?.recordingMessage || box.screenRecording?.message || 'Arcade Box is checking screen recording support.') : box.screenStatus?.message || 'Arcade Box is checking your desktop’s native capture support.'}</p>
          {#if box.selectedTool.id === 'arcade.screen.qr'}<small>Decoded destinations are shown first. Arcade Box will not open them automatically.</small>{/if}
          {#if box.selectedTool.id === 'arcade.screen.ocr'}<small>Capture and OCR run locally. The selected image is passed through a scoped file grant.</small>{/if}
          {#if box.selectedTool.id === 'arcade.screen.ocr' && !box.screenOcrAvailable}<p class="field-error" role="status">{box.providersLoaded ? (box.providerError || 'Screen OCR is unavailable. Install Tesseract with its English language data, or download it from Engines & dependencies.') : 'Checking local Tesseract availability…'}</p>{/if}
          {#if box.selectedTool.id === 'arcade.screen.color'}<small>Choose a screen area, then click a pixel or use arrow keys to inspect its exact color locally.</small>{/if}
          {#if box.isScreenRecorderTool(box.selectedTool.id) && box.screenRecording?.recording}
            <div class="screen-recording-controls">
              <span><strong>{box.screenRecording.finalizing ? (box.screenRecording.message.toLowerCase().includes('discard') ? 'Discarding recording' : 'Saving recording') : 'Recording active'}</strong><small>{Math.floor((box.screenRecording.elapsedSeconds || 0) / 60).toString().padStart(2, '0')}:{((box.screenRecording.elapsedSeconds || 0) % 60).toString().padStart(2, '0')} elapsed · closing the Island keeps it running</small></span>
              <button type="button" class="quiet-button" disabled={box.screenRecording.finalizing || box.running} onclick={() => void box.finishScreenRecording()}><Icon name="check" size={14} /><span>{box.screenRecording.finalizing ? (box.screenRecording.message.toLowerCase().includes('discard') ? 'Discarding…' : 'Saving…') : 'Stop & save'}</span></button>
              <button type="button" class="quiet-button" disabled={box.screenRecording.finalizing} onclick={() => void box.discardScreenRecording()}><Icon name="close" size={14} /><span>Discard</span></button>
            </div>
          {/if}
        </section>
      {/if}
      {#if box.selectedTool.id === 'arcade.system.paste-plain'}
        <section class="screen-action-status" aria-label="Plain-text paste availability" aria-live="polite">
          <div class="screen-status-heading"><span class="runtime-dot" class:offline={box.plainPasteStatus?.available === false}></span><strong>{box.plainPasteStatus?.available ? `Ready · ${box.plainPasteStatus.shortcut}` : box.plainPasteStatus ? 'Unavailable on this desktop' : 'Checking paste support'}</strong><span>{box.plainPasteStatus?.platform || 'checking'}</span></div>
          <p>{box.plainPasteStatus?.message || 'Arcade Box is checking whether this desktop can send the plain-text paste shortcut.'}</p>
        </section>
      {/if}
      {#if box.hasTextFileSwitch(box.selectedTool)}
        <div class="input-mode-switch" role="group" aria-label="Input type">
          <button class:mode-active={box.textInputMode === 'text'} aria-pressed={box.textInputMode === 'text'} onclick={() => { box.textInputMode = 'text'; box.selectedFiles = []; box.fileSelectionError = ''; }}><Icon name="type" size={14} /> Text</button>
          <button class:mode-active={box.textInputMode === 'file'} aria-pressed={box.textInputMode === 'file'} onclick={() => { box.textInputMode = 'file'; box.inputText = ''; box.fileSelectionError = ''; }}><Icon name="file" size={14} /> File</button>
        </div>
      {/if}
      {#if box.selectedTool.id === 'arcade.text.diff' && !box.fileWorkflowActive(box.selectedTool)}
        <DiffInput bind:left={box.inputText} bind:right={box.diffRight} bind:base={box.diffBase} merge={box.toolOptions.operation === 'merge'} />
      {:else if box.selectedTool.ui?.input.kind === 'folder'}
        <div class="field-label-row"><span class="field-caption">{box.selectedTool.ui.input.label}</span><span class="input-type"><Icon name="lock" size={13} /> {(box.selectedTool.ui.input.maxItems ?? 1) > 1 ? 'Selected folders' : 'Selected folder'}</span></div>
        <SelectedFolderInput
          values={box.selectedInputFolders}
          maxItems={box.selectedTool.ui.input.maxItems ?? 1}
          selectionGeneration={box.viewGeneration}
          disabled={box.running || Boolean(box.activeJob && box.jobIsActive(box.activeJob))}
          onSelect={box.inputFolderSelectionHandler(box.viewGeneration, box.selectedTool.id)}
          onRemove={box.removeInputFolder}
        />
        {#if box.fileSelectionError}<div class="field-error" role="alert">{box.fileSelectionError}</div>{/if}
      {:else if box.fileWorkflowActive(box.selectedTool)}
        <div class="field-label-row"><span class="field-caption">{box.fileInputLabel(box.selectedTool)}</span><span class="input-type"><Icon name="lock" size={13} /> Selected files</span></div>
        <SelectedFilesInput
          files={box.selectedFiles}
          disabled={box.running || Boolean(box.activeJob && box.jobIsActive(box.activeJob))}
          label={box.fileInputLabel(box.selectedTool)}
          multiple={box.selectedTool.id === 'arcade.text.diff' || box.selectedTool.ui?.input.kind === 'files'}
          sortable={box.selectedTool.id === 'arcade.text.diff' || (box.selectedTool.ui?.input.sortable ?? false)}
          maxItems={box.selectedTool.id === 'arcade.text.diff' ? (box.toolOptions.operation === 'merge' ? 3 : 2) : box.selectedTool.ui?.input.maxItems}
          onAdd={() => void box.chooseInputFile()}
          onRemove={box.removeSelectedFileToken}
          onReorder={box.reorderSelectedFiles}
          formatSize={box.formatBytes}
        />
        {#if box.selectedTool.inputs.includes('file/image') && !box.selectedTool.id.startsWith('arcade.audio.') && (box.selectedTool.id !== 'arcade.pdf.watermark' || box.toolOptions.mode === 'image')}
          <button type="button" class="screen-capture-button" disabled={box.screenCaptureBusy || box.running || Boolean(box.activeJob && box.jobIsActive(box.activeJob))} onclick={() => void box.chooseScreenArea()}>
            <Icon name="image" size={14} />
            <span>{box.screenCaptureBusy ? 'Waiting for screen selection…' : 'Capture screen content'}</span>
            <small>Uses your desktop’s capture picker</small>
          </button>
        {/if}
        {#if box.selectedTool.id === 'arcade.pdf.watermark'}
          <p class="field-note">Text mode uses one PDF. Image and PDF overlay modes use the source PDF first and one mark file second. Text marks support {'{page}'} and {'{pages}'} page-number tokens.</p>
        {:else if box.selectedTool.ui?.version === 1 && box.selectedTool.ui.input.kind === 'files' && box.selectedFiles.length < (box.selectedTool.ui.input.minItems ?? 1)}<p class="field-note">Select at least {box.selectedTool.ui.input.minItems ?? 1} {(box.selectedTool.ui.input.minItems ?? 1) === 1 ? 'file' : 'files'}{box.selectedTool.ui.input.sortable ? '. Their order here controls the result.' : '.'}</p>{/if}
        {@const pdfCapability = box.requiredPdfCapability(box.selectedTool)}
        {#if pdfCapability && !box.loadingCatalog && !box.providerError && !box.providerSupports(pdfCapability)}
          <div class="provider-inline-missing" role="status"><Icon name="document" size={14} /><span>No compatible qpdf provider is currently available. Install qpdf or review provider detection in Engines &amp; Dependencies.</span><button type="button" onclick={box.openEngines}>View engines</button></div>
        {/if}
        {#if box.selectedTool.id === 'arcade.pdf.compress'}<p class="field-note">This performs lossless structural optimization. It does not reduce image quality.</p>{/if}
      {:else}
        {#if box.isScreenTool(box.selectedTool.id) || box.selectedTool.ui?.input.kind === 'none'}
          <p class="field-note no-input-note">{box.selectedTool.id === 'arcade.system.paste-plain' ? 'Choose Paste as plain text to hide Arcade Box and send the desktop’s common plain-text paste shortcut to the previously focused app. The clipboard itself stays unchanged.' : box.isScreenRecorderTool(box.selectedTool.id) ? 'On Linux X11, Box records the full display. Wayland, Windows, and macOS use the native picker to choose a display or window. You can close Arcade Island and stop or discard the recording later.' : box.isScreenTool(box.selectedTool.id) ? 'Run this action to open your desktop’s capture picker.' : ''}</p>
        {:else}
          <div class="field-label-row">
            <label for="tool-input">{box.selectedTool.ui?.version === 1 ? box.selectedTool.ui.input.label : 'Input'}</label>
            <span class="input-type"><Icon name={box.selectedTool.ui?.input.kind === 'url' ? 'globe' : 'type'} size={13} /> {box.selectedTool.ui?.input.kind === 'url' ? 'URL' : 'Text'}</span>
          </div>
          {#if box.selectedTool.ui?.input.kind === 'url'}
            <input id="tool-input" class="tool-url-input" type="url" bind:value={box.inputText} placeholder={inputPlaceholder(box.selectedTool, box.toolOptions)} spellcheck="false" aria-keyshortcuts="Control+Enter Meta+Enter" onkeydown={(event) => { if ((event.metaKey || event.ctrlKey) && event.key === 'Enter') { event.preventDefault(); void box.submitTool(); } }} />
          {:else}
            <textarea
              id="tool-input"
              bind:value={box.inputText}
              placeholder={inputPlaceholder(box.selectedTool, box.toolOptions)}
              spellcheck="false"
              aria-keyshortcuts="Control+Enter Meta+Enter"
              onkeydown={(event) => {
                if ((event.metaKey || event.ctrlKey) && event.key === 'Enter') {
                  event.preventDefault();
                  void box.submitTool();
                }
              }}
            ></textarea>
          {/if}
        {/if}
      {/if}
      {#if box.fileSelectionError}<div class="field-error" role="alert">{box.fileSelectionError}</div>{/if}
      {#if box.isVideoEditorTool(box.selectedTool.id) && box.selectedFiles.length}
        <VideoAssist tool={box.selectedTool} files={box.selectedFiles} values={box.toolOptions} disabled={box.running || Boolean(box.activeJob && box.jobIsActive(box.activeJob))} onValueChange={(key, value) => { box.toolOptions[key] = value; }} />
      {:else if box.selectedTool.id.startsWith('arcade.audio.') && box.selectedFiles.length}
        <AudioAssist tool={box.selectedTool} files={box.selectedFiles} values={box.toolOptions} disabled={box.running || Boolean(box.activeJob && box.jobIsActive(box.activeJob))} onValueChange={(key, value) => { box.toolOptions[key] = value; }} />
      {/if}
      {#if box.selectedTool.ui?.version === 1}
        <StandardToolForm ui={box.selectedTool.ui} values={box.toolOptions} capabilities={box.providerCapabilities} idPrefix={`tool-${box.selectedTool.id}`} disabled={box.running || Boolean(box.activeJob && box.jobIsActive(box.activeJob))} onValueChange={(key, value) => {
          box.toolOptions[key] = value;
          if (box.selectedTool?.id === 'arcade.pdf.watermark' && key === 'mode' && box.selectedFiles.length > 1) {
            const second = box.selectedFiles[1];
            const keepSecond = value === 'image'
              ? second.mime.startsWith('file/image')
              : value === 'pdf-overlay' && second.mime === 'file/pdf';
            if (!keepSecond) box.selectedFiles = box.selectedFiles.slice(0, 1);
            box.fileSelectionError = '';
          }
        }} />
        {#if standardUiOptionsProblem(box.selectedTool.ui, box.toolOptions)}<div class="field-error standard-option-error" role="alert">{standardUiOptionsProblem(box.selectedTool.ui, box.toolOptions)}</div>{/if}
      {/if}
      <div class="input-footer">
        <span class="privacy-hint"><Icon name={privacyLabel(box.selectedTool.privacyClass) === 'LOCAL' ? 'lock' : 'globe'} size={13} /> {privacyLabel(box.selectedTool.privacyClass)} · {box.privacyHintFor(box.selectedTool)}</span>
        <button class="run-button" onclick={() => void box.submitTool()} disabled={box.running || !box.canSubmitSelectedTool() || Boolean(box.activeJob && box.jobIsActive(box.activeJob))}>
          {#if box.running}<span class="spinner"></span><span>Starting</span>{:else if box.activeJob && box.jobIsActive(box.activeJob)}<span class="spinner"></span><span>In background</span>{:else}<Icon name={box.isScreenTool(box.selectedTool.id) ? 'image' : 'play'} size={15} /><span>{runLabel(box.selectedTool, box.toolOptions)}</span>{/if}
          <kbd>{box.commandKey} ↵</kbd>
        </button>
      </div>
      {/if}
    </div>

    {#if box.running && box.peerRequestId && (box.overlaps.lensCapture || box.overlaps.clipboardPick)}<div class="active-job-card" role="status"><span>Waiting for your selection…</span><button class="quiet-button" onclick={() => void cancelResultLinkAction(box.peerRequestId)}>Cancel action</button></div>{/if}
    {#if box.activeJob && box.jobIsActive(box.activeJob)}
      <div class="active-job-card" role="region" aria-label="{box.selectedTool.name} background job">
        <span class="sr-only" role="status" aria-live="polite" aria-atomic="true">{box.jobStatusText(box.activeJob.status)} in the background.</span>
        <span class="active-job-symbol"><span class="spinner"></span></span>
        <div class="active-job-copy"><strong>{box.jobStatusText(box.activeJob.status)} in the background</strong><span>{box.activeJob.message || 'You can close Arcade Island; this work will continue.'}</span><div class="job-progress-track" role="progressbar" aria-label="{box.selectedTool.name} progress" aria-valuemin="0" aria-valuemax="100" aria-valuenow={box.activeJob.progress === null ? undefined : Math.round(Math.max(0, Math.min(1, box.activeJob.progress)) * 100)} aria-valuetext={box.activeJob.progress === null ? 'Progress is not available' : `${Math.round(Math.max(0, Math.min(1, box.activeJob.progress)) * 100)} percent`}><span style={`width:${Math.max(0, Math.min(100, (box.activeJob.progress ?? 0) * 100))}%`}></span></div></div>
        <button class="quiet-button cancel-job-button" disabled={box.cancellingJobId === box.activeJob.id || box.activeJob.status === 'cancelling'} onclick={() => { if (box.activeJob) void box.cancelBackgroundJob(box.activeJob); }}><Icon name="close" size={14} /><span>Cancel</span></button>
      </div>
    {/if}

    {#if box.runError}
      <div class="notice-card error-notice" role="alert">
        <span class="notice-icon"><Icon name="close" size={18} /></span>
        <div><strong>Couldn’t finish</strong><p>{box.runError}</p></div>
      </div>
    {/if}

    {#if box.activeResult && (box.activeResult.outputs.length > 0 || box.activeResult.message)}
      <section class="result-panel" class:result-failed={box.activeResult.status !== 'success'} aria-labelledby="result-heading" aria-live="off">
        <div class="result-heading" id="result-heading">
          <div class="result-label"><span class="result-check"><Icon name={box.activeResult.status === 'success' ? 'check' : 'close'} size={14} /></span><div><strong>{box.activeResult.status === 'success' ? 'Done' : 'Couldn’t finish'}</strong><span>{box.activeResult.outputs.length} {box.activeResult.outputs.length === 1 ? 'result' : 'results'}</span></div></div>
          <span class="result-mime" title="Copy result" role="status">{box.copyState === 'copied' ? 'Copied' : box.copyState === 'error' ? 'Copy failed — try again' : `${box.commandKey} Shift C`}</span>
        </div>
        <ResultActions toolId={box.selectedTool.id} outputs={box.activeResult.outputs} presets={box.selectedTool.presets} />
        {#if (box.selectedTool.id === 'arcade.screen.color' || box.selectedTool.id === 'arcade.screen.ruler') && !box.activeResult.metadata?.handedOver}
          {@const screenImage = box.activeResult.outputs.find((output) => output.kind === 'artifact' || output.kind === 'file')}
          {#if screenImage}
            {#if box.selectedTool.id === 'arcade.screen.color'}<ScreenColorSampler token={screenImage.value} />{:else}<ScreenRuler token={screenImage.value} />{/if}
            <div class="screen-color-source-actions"><span><Icon name="lock" size={13} /> Captured image stays local</span><button type="button" class="quiet-button" disabled={box.artifactActionBusy} onclick={() => void box.showArtifact(screenImage.value, false)}><Icon name="external" size={13} /><span>Open capture</span></button><button type="button" class="quiet-button" disabled={box.artifactActionBusy} onclick={() => void box.saveArtifact(screenImage.value)}><Icon name="folder" size={13} /><span>Save as</span></button></div>
          {:else}<div class="field-error" role="alert">The screen capture did not return an image for sampling.</div>{/if}
        {:else if box.selectedTool.id === 'arcade.video.inspect' && box.mediaProbe}
          <div class="media-summary">
            <div class="media-facts">
              <div><span>Container</span><strong>{box.mediaProbe.format?.format_long_name || box.mediaProbe.format?.format_name || 'Unknown'}</strong></div>
              <div><span>Duration</span><strong>{box.formatDuration(box.mediaProbe.format?.duration)}</strong></div>
              <div><span>File size</span><strong>{box.formatBytes(box.mediaProbe.format?.size)}</strong></div>
              <div><span>Bitrate</span><strong>{box.mediaProbe.format?.bit_rate ? `${Math.round(Number(box.mediaProbe.format.bit_rate) / 1000)} kb/s` : 'Unknown'}</strong></div>
              <div><span>Tracks</span><strong>{[`${box.streamCount(box.mediaProbe, 'video')} video`, `${box.streamCount(box.mediaProbe, 'audio')} audio`, box.streamCount(box.mediaProbe, 'subtitle') ? `${box.streamCount(box.mediaProbe, 'subtitle')} subtitle` : ''].filter(Boolean).join(' · ')}</strong></div>
              {#if box.mediaProbe.chapters?.length}<div><span>Chapters</span><strong>{box.mediaProbe.chapters.length}</strong></div>{/if}
            </div>
            <div class="media-stream-list">
              {#each box.mediaProbe.streams || [] as stream, index (`${stream.index ?? index}-${stream.codec_type ?? 'stream'}`)}
                <div class="media-stream-row"><span class="media-stream-icon"><Icon name={stream.codec_type === 'video' ? 'video' : stream.codec_type === 'audio' ? 'audio' : 'file'} size={16} /></span><span class="media-stream-copy"><strong>{stream.codec_type || 'Stream'}{stream.codec_name ? ` · ${stream.codec_name}` : ''}{stream.profile ? ` · ${stream.profile}` : ''}</strong><small>{box.streamDetail(stream)}</small></span></div>
              {/each}
              {#if !box.mediaProbe.streams?.length}<p class="field-note">No stream details were returned by the media provider.</p>{/if}
            </div>
            <details class="technical-details"><summary>Show raw probe data</summary><pre class="output-text">{box.activeResult.outputs[0]?.value || ''}</pre></details>
            {#if box.resultMetadataText('providerPath')}<div class="provider-footnote">Inspected by <code>{box.resultMetadataText('providerPath')}</code></div>{/if}
          </div>
        {:else if (box.selectedTool.id === 'arcade.barcode.decode' || box.selectedTool.id === 'arcade.screen.qr') && box.barcodeScan}
          <div class="barcode-scan-results" aria-label="Decoded barcode results">
            {#if box.barcodeScan.results.length === 0}
              <div class="inspector-empty"><Icon name="image" size={17} /><span>{box.barcodeScan.message || 'No readable barcode was found in this image.'}</span></div>
            {:else}
              {#each box.barcodeScan.results as barcode, index (`${barcode.format || 'code'}-${index}`)}
                <article class="barcode-scan-card">
                  <div class="barcode-scan-heading"><span class="barcode-format-tag">{barcode.format || 'Barcode'}</span><span>{barcode.contentType || 'Decoded locally'}</span></div>
                  <pre class="barcode-content">{barcode.text || 'No readable text content'}</pre>
                  <div class="barcode-scan-actions">
                    {#if barcode.text}<button class="quiet-button" onclick={() => void box.copyOutput(barcode.text || '')}><Icon name={box.copyState === 'copied' ? 'check' : 'copy'} size={14} /><span>{box.copyState === 'copied' ? 'Copied' : 'Copy content'}</span></button>{/if}
                    {#if barcode.text && box.reviewedHttpUrl(barcode.text)}<button class="quiet-button barcode-open-link" disabled={box.artifactActionBusy} onclick={() => void box.openScannedLink(barcode.text || '')}><Icon name="external" size={14} /><span>Open reviewed link</span></button>{/if}
                  </div>
                </article>
              {/each}
              <p class="barcode-safety-note"><Icon name="shield" size={13} /> Links stay unopened until you choose Open reviewed link.</p>
            {/if}
            {#if box.artifactActionError}<div class="field-error" role="alert">{box.artifactActionError}</div>{/if}
            {#if box.artifactActionMessage}<div class="pipeline-message" role="status">{box.artifactActionMessage}</div>{/if}
          </div>
        {:else if box.selectedTool.id === 'arcade.system.window-pin' && box.activeWindowPinCapability}
          <div class="window-pin-results">
            <div class="window-pin-state"><span class="window-pin-icon"><Icon name="pin" size={17} /></span><div><strong>{box.activeWindowPinCapability.available ? 'Ready to pin the active window' : 'Window pinning unavailable'}</strong><p>{box.activeWindowPinCapability.message}</p></div></div>
            {#if box.activeWindowPinCapability.available}
              <div class="window-pin-actions"><button class="quiet-button window-pin-primary" onclick={() => void box.changeForegroundWindowPin(true)}><Icon name="pin" size={14} /><span>Keep active window on top</span></button><button class="quiet-button" onclick={() => void box.changeForegroundWindowPin(false)}><span>Remove always-on-top</span></button></div>
              <p class="inspector-limitation">Arcade Box hides itself before changing the window that was active, so that window regains focus. The desktop window manager may still refuse the change.</p>
            {/if}
          </div>
        {:else if box.selectedTool.id === 'arcade.system.process' && box.processList}
          <div class="process-results">
            <div class="process-results-heading"><div><strong>{box.processList.totalMatches} process{box.processList.totalMatches === 1 ? '' : 'es'}</strong><span>{box.processList.query ? `Matching “${box.processList.query}”` : 'Sorted by memory use'}</span></div><button class="quiet-button" disabled={box.running} onclick={() => void box.submitTool()}><Icon name="refresh" size={13} /><span>Refresh</span></button></div>
            {#if box.processList.processes.length}
              <div class="process-list" aria-label="Running processes">
                {#each box.processList.processes as process (`${process.pid}-${process.startTime}`)}
                  {@const identity = `${process.pid}:${process.startTime}`}
                  <article class="process-row">
                    <div class="process-row-main"><strong>{process.name || 'Unnamed process'}</strong><span>PID {process.pid} · {box.formatBytes(process.memoryBytes)} memory · {process.cpuPercent.toFixed(1)}% CPU</span>{#if process.executable}<code title={process.executable}>{process.executable}</code>{/if}</div>
                    <div class="process-row-action">
                      {#if box.processTerminationRequests[identity] === 'requested'}<span class="process-action-status" role="status">Termination requested</span>
                      {:else if process.protected}<span class="process-action-status">Protected</span>
                      {:else}<button class="quiet-button process-terminate-button" disabled={box.processTerminationRequests[identity] === 'working'} aria-label={`End ${process.name}, PID ${process.pid}`} onclick={() => void box.requestProcessTermination(process)}><Icon name="close" size={13} /><span>{box.processTerminationRequests[identity] === 'working' ? 'Sending…' : 'End process'}</span></button>{/if}
                      {#if box.processTerminationRequests[identity] && box.processTerminationRequests[identity] !== 'working' && box.processTerminationRequests[identity] !== 'requested'}<span class="process-action-error" role="alert">{box.processTerminationRequests[identity]}</span>{/if}
                    </div>
                  </article>
                {/each}
              </div>
            {:else}<div class="inspector-empty"><Icon name="search" size={16} /><span>No process matched this search.</span></div>{/if}
            {#if box.processList.truncated}<p class="field-note">Showing the first {box.processList.listedCount} matches. Add a process name or PID to narrow the list.</p>{/if}
            {#if box.processList.limitations?.length}<p class="inspector-limitation">{box.processList.limitations.join(' ')}</p>{/if}
            <p class="inspector-limitation">Ending a process can lose unsaved work. Protected core processes cannot be ended here; permissions and platform policy can also block the request.</p>
          </div>
        {:else if box.selectedTool.id === 'arcade.system.system-info' && box.systemInformation}
          <div class="system-info-results">
            <div class="system-fact-grid">
              <div><span>Operating system</span><strong>{[box.systemInformation.operatingSystem?.name, box.systemInformation.operatingSystem?.version].filter(Boolean).join(' ') || 'Unknown'}</strong><small>{box.systemInformation.operatingSystem?.kernel || box.systemInformation.operatingSystem?.longVersion || box.systemInformation.operatingSystem?.family || ''}</small></div>
              <div><span>Processor</span><strong>{box.systemInformation.processor?.model || 'Processor details unavailable'}</strong><small>{[box.systemInformation.processor?.physicalCores ? `${box.systemInformation.processor.physicalCores} physical` : '', box.systemInformation.processor?.logicalCores ? `${box.systemInformation.processor.logicalCores} logical cores` : ''].filter(Boolean).join(' · ')}</small></div>
              <div><span>Memory</span><strong>{box.formatBytes(box.systemInformation.memory?.totalBytes)}</strong><small>{box.systemInformation.memory?.availableBytes === undefined ? 'Available memory unavailable' : `${box.formatBytes(box.systemInformation.memory.availableBytes)} available`}</small></div>
              <div><span>Architecture</span><strong>{box.systemInformation.operatingSystem?.architecture || 'Unknown'}</strong><small>{box.systemInformation.operatingSystem?.hostName ? `Device: ${box.systemInformation.operatingSystem.hostName}` : ''}</small></div>
            </div>
            {#if box.systemInformation.storage?.length}
              <details class="inspector-section"><summary>Storage volumes <span>{box.systemInformation.storage.length}</span></summary><div class="inspector-row-list">{#each box.systemInformation.storage as volume, index (`${volume.mountPoint}-${index}`)}<div><strong>{volume.mountPoint || volume.name || `Volume ${index + 1}`}</strong><span>{box.formatBytes(volume.availableBytes)} free of {box.formatBytes(volume.totalBytes)}{volume.fileSystem ? ` · ${volume.fileSystem}` : ''}</span></div>{/each}</div></details>
            {/if}
            {#if box.systemInformation.networkInterfaces?.length}
              <details class="inspector-section"><summary>Network interfaces <span>{box.systemInformation.networkInterfaces.length}</span></summary><div class="inspector-row-list">{#each box.systemInformation.networkInterfaces as network, index (`${network.name}-${index}`)}<div><strong>{network.name || `Interface ${index + 1}`}</strong><span>Received {box.formatBytes(network.receivedBytes)} · Sent {box.formatBytes(network.transmittedBytes)}</span></div>{/each}</div></details>
            {/if}
            {#if box.systemInformation.limitations?.length}<p class="inspector-limitation">{box.systemInformation.limitations.join(' ')}</p>{/if}
            <button class="copy-button" onclick={() => void box.copyOutput(box.activeResult?.outputs[0]?.value || '')}><Icon name={box.copyState === 'copied' ? 'check' : 'copy'} size={14} />{box.copyState === 'copied' ? 'Diagnostics copied' : 'Copy diagnostics'}</button>
          </div>
        {:else if box.artifactOutputs().length}
          <div class="artifact-result-list" aria-label="Generated files">
            {#each box.artifactOutputs().slice(box.outputPage * 20, (box.outputPage + 1) * 20) as artifact, pageIndex (`${artifact.value}-${pageIndex}`)}
              {@const index = box.outputPage * 20 + pageIndex}
              {@const size = box.artifactOutputSize(index)}
              {@const nextTools = box.toolsForOutput(artifact)}
              <div class="artifact-output-entry">
                {#if artifact.mime.startsWith('file/image') || artifact.mime === 'file/svg'}
                  <ImageResult output={artifact} name={box.artifactOutputName(index)} {size} busy={box.artifactActionBusy} onSave={() => box.saveArtifact(artifact.value)} onOpen={() => box.showArtifact(artifact.value, false)} onReveal={() => box.showArtifact(artifact.value, true)} />
                {:else}
                <div class="artifact-result-card">
                  <span class="artifact-file-icon"><Icon name={box.artifactIcon(artifact.mime)} size={20} /></span>
                  <div class="artifact-result-copy"><strong>{box.artifactOutputName(index)}</strong><span>{#if size}{size} <span aria-hidden="true">·</span> {/if}{artifact.mime}</span></div>
                  <div class="artifact-actions"><button class="quiet-button" aria-label={`Open ${box.artifactOutputName(index)}`} disabled={box.artifactActionBusy} onclick={() => void box.showArtifact(artifact.value, false)}><Icon name="external" size={14} /><span>Open</span></button><button class="quiet-button" aria-label={`Save ${box.artifactOutputName(index)} as`} disabled={box.artifactActionBusy} onclick={() => void box.saveArtifact(artifact.value)}><Icon name="folder" size={14} /><span>Save as</span></button><button class="quiet-button" aria-label={`Reveal ${box.artifactOutputName(index)} in folder`} disabled={box.artifactActionBusy} onclick={() => void box.showArtifact(artifact.value, true)}><Icon name="folder" size={14} /><span>Reveal</span></button></div>
                </div>
                {/if}
                {#if nextTools.length}<details class="result-next-actions"><summary>Continue in another tool <span>{nextTools.length}</span></summary><div>{#each nextTools as nextTool (nextTool.id)}<button onclick={() => box.continueWithOutput(nextTool, artifact, index)}><Icon name={box.iconForCategory(nextTool.category)} size={13} /><span>{nextTool.name}</span></button>{/each}</div></details>{/if}
              </div>
            {/each}
          </div>
          {#if box.fileOutputs.length > 20}
            <div class="artifact-pagination"><button class="quiet-button" disabled={box.outputPage === 0} onclick={() => box.outputPage--}>Previous files</button><span>{box.outputPage * 20 + 1}–{Math.min((box.outputPage + 1) * 20, box.fileOutputs.length)} of {box.fileOutputs.length}</span><button class="quiet-button" disabled={(box.outputPage + 1) * 20 >= box.fileOutputs.length} onclick={() => box.outputPage++}>Next files</button></div>
          {/if}
          {#each box.activeResult.outputs.filter((output) => output.kind === 'text' || output.kind === 'url') as output}
            {#if output.mime === 'structured/audio-loudness' || output.mime === 'structured/audio-metadata'}
              <ToolOutputView {output} toolId={box.selectedTool.id} />
            {:else}
              <details class="technical-details"><summary>Result details</summary><ToolOutputView {output} toolId={box.selectedTool.id} /></details>
            {/if}
          {/each}
          {#if box.artifactActionError}<div class="field-error" role="alert">{box.artifactActionError}</div>{/if}
          {#if box.artifactActionMessage}<div class="pipeline-message" role="status">{box.artifactActionMessage}</div>{/if}
        {:else}
          {#each box.activeResult.outputs as output, index}
            <ToolOutputView {output} toolId={box.selectedTool.id} />
            {@const nextTools = box.toolsForOutput(output)}
            {#if nextTools.length}<details class="result-next-actions"><summary>Continue in another tool <span>{nextTools.length}</span></summary><div>{#each nextTools as nextTool (nextTool.id)}<button onclick={() => box.continueWithOutput(nextTool, output, index)}><Icon name={box.iconForCategory(nextTool.category)} size={13} /><span>{nextTool.name}</span></button>{/each}</div></details>{/if}
          {/each}
        {/if}
        {#if box.resultMetadataText('providerPath')}<details class="technical-details"><summary>Provider details · {box.resultMetadataText('providerId') === 'ocr.lens' ? 'Arcade Lens (local)' : box.resultMetadataText('providerId')}</summary><div class="provider-footnote"><span>{box.resultMetadataText('providerVersion')}</span><code>{box.resultMetadataText('providerPath')}</code></div></details>{/if}
        {#if box.activeResult.message}<p class="result-message">{box.activeResult.message}</p>{/if}
        {#if box.activeResult.warnings?.length}
          <div class="result-warnings">{#each box.activeResult.warnings as warning}<span><Icon name="dots" size={13} />{warning}</span>{/each}</div>
        {/if}
      </section>
    {/if}
  {/if}
  {#if !box.activeResult}<ResultActions toolId={box.selectedTool.id} presets={box.selectedTool.presets} />{/if}
  {#if isRunnable(box.selectedTool)}
    <details class="alias-settings">
      <summary><Icon name="command" size={14} /><span>Set a search alias</span><Icon name="chevron" size={13} /></summary>
      <form class="alias-form" onsubmit={(event) => { event.preventDefault(); void box.saveAlias(); }}>
        <label for="tool-alias">Type a short phrase to open this tool</label>
        <div><input id="tool-alias" bind:value={box.aliasValue} maxlength="80" placeholder="e.g. clean text" /><button type="submit" disabled={box.aliasSaving || !box.aliasValue.trim()}>{box.aliasSaving ? 'Saving…' : 'Save alias'}</button></div>
        {#if box.aliasMessage}<span class="alias-message" role="status">{box.aliasMessage}</span>{/if}
      </form>
    </details>
  {/if}

</div>
{/if}
