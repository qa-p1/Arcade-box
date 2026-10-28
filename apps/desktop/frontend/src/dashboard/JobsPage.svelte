<script lang="ts">
  import Icon from '../lib/Icon.svelte';
  import { openToolInIsland } from '../lib/arcade';
  import type { JobSnapshot } from '../lib/contracts';
  import { useBox } from '../box.svelte';

  const box = useBox();
  const tone = (status: JobSnapshot['status']) =>
    status === 'succeeded' ? 'ok' : status === 'failed' || status === 'interrupted' ? 'danger' : status === 'cancelled' ? '' : 'warn';
  const percent = (job: JobSnapshot) => Math.round(Math.max(0, Math.min(1, job.progress ?? 0)) * 100);
  // A job's inputs are never kept, so Retry opens its tool to run it again.
  const retryable = (job: JobSnapshot) => job.status === 'failed' || job.status === 'interrupted' || job.status === 'cancelled';
</script>

<header class="page-head">
  <div>
    <h1>Jobs</h1>
    <p class="sub">Long tasks keep running while the Island is closed.</p>
  </div>
</header>

{#if box.jobError}<div class="notice danger" role="alert"><Icon name="alert" /><span class="row-main">Couldn't update the job. {box.jobError}</span></div>{/if}

{#if box.backgroundJobs.length === 0}
  <div class="empty-state"><Icon name="clock" size={24} /><h2>No jobs yet</h2><p>Video conversions and other long tasks appear here while they run.</p></div>
{:else}
  <div class="list" aria-label="Jobs">
    {#each box.backgroundJobs as job (job.id)}
      {@const tool = box.toolById.get(job.toolId)}
      <div class="row">
        <span class="glyph-tile sm"><Icon name={tool ? box.iconForCategory(tool.category) : 'sparkle'} size={14} /></span>
        <div class="row-main">
          <div class="row-title">{tool?.name || job.toolId}</div>
          <div class="row-sub">{job.message || (box.jobIsActive(job) ? 'Working in the background' : 'Finished')}</div>
        </div>
        {#if box.jobIsActive(job)}
          {#if job.progress === null}
            <progress class="progress" aria-label="{tool?.name || 'Job'} progress"></progress>
          {:else}
            <progress class="progress" max="100" value={percent(job)} aria-label="{tool?.name || 'Job'} progress" aria-valuetext="{percent(job)} percent"></progress>
          {/if}
        {/if}
        <span class="badge {tone(job.status)}">{box.jobStatusText(job.status)}</span>
        <div class="row-end">
          {#if box.jobIsActive(job)}
            <button type="button" class="btn sm" disabled={box.cancellingJobId === job.id || job.status === 'cancelling'} onclick={() => void box.cancelBackgroundJob(job)}>{box.cancellingJobId === job.id || job.status === 'cancelling' ? 'Cancelling…' : 'Cancel'}</button>
          {/if}
          {#if job.result && tool}
            <button type="button" class="btn sm" onclick={() => box.viewJobResult(job)}>Open output</button>
          {/if}
          {#if retryable(job) && tool}
            <button type="button" class="btn sm ghost" onclick={() => void openToolInIsland(tool.id).catch((error) => { box.jobError = String(error); })}>Retry</button>
          {/if}
        </div>
      </div>
    {/each}
  </div>
{/if}
