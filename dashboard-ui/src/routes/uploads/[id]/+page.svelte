<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { page } from '$app/state';
  import { api } from '$lib/api';
  import type { Upload, UploadEvent } from '$lib/types';

  const id = $derived(page.params.id!);

  let upload  = $state<Upload | null>(null);
  let events  = $state<UploadEvent[]>([]);
  let error   = $state<string | null>(null);
  let actErr  = $state<string | null>(null);
  let live    = $state(false);
  let copied  = $state(false);
  let es: EventSource | null = null;

  async function load() {
    try {
      [upload, events] = await Promise.all([api.getUpload(id), api.getEvents(id)]);
      error = null;
    } catch (e) { error = String(e); }
  }

  function connectSSE() {
    es?.close();
    es = api.streamEvents(id);
    es.onopen = () => live = true;
    es.onerror = () => live = false;
    es.onmessage = (e) => {
      const ev: UploadEvent = JSON.parse(e.data);
      events = [...events, ev];
      if (['completed', 'chunk_received', 'finalized', 'processing_started',
           'processing_failed', 'abandoned', 'retry_queued'].includes(ev.event_type)) {
        api.getUpload(id).then(u => upload = u).catch(() => {});
      }
    };
  }

  onMount(() => { load(); connectSSE(); });
  onDestroy(() => { es?.close(); });

  async function retry() {
    actErr = null;
    try { await api.retryProcessing(id); await load(); }
    catch (e) { actErr = String(e); }
  }

  async function abandon() {
    if (!confirm('Mark this upload as abandoned?')) return;
    actErr = null;
    try { await api.markAbandoned(id); await load(); }
    catch (e) { actErr = String(e); }
  }

  async function hardDelete() {
    if (!confirm('Permanently delete this upload and its file? This cannot be undone.')) return;
    actErr = null;
    try {
      await api.deleteUpload(id);
      window.location.href = '/';
    }
    catch (e) { actErr = String(e); }
  }

  async function copyId() {
    await navigator.clipboard.writeText(id);
    copied = true;
    setTimeout(() => copied = false, 1500);
  }

  function fmt(b: number) {
    if (b === 0) return '0 B';
    if (b < 1024)       return `${b} B`;
    if (b < 1024 ** 2)  return `${(b / 1024).toFixed(1)} KB`;
    if (b < 1024 ** 3)  return `${(b / 1024 ** 2).toFixed(1)} MB`;
    return `${(b / 1024 ** 3).toFixed(2)} GB`;
  }

  function fmtDate(s: string) {
    return new Date(s).toLocaleString();
  }

  function reltime(s: string) {
    const diff = Date.now() - new Date(s).getTime();
    if (diff < 60_000)    return `${Math.floor(diff / 1000)}s ago`;
    if (diff < 3_600_000) return `${Math.floor(diff / 60_000)}m ago`;
    return new Date(s).toLocaleString();
  }

  function pct(u: Upload) {
    return u.upload_length > 0 ? Math.round((u.upload_offset / u.upload_length) * 100) : 0;
  }

  function canRetry(u: Upload) {
    return ['FailedProcessing', 'FailedFinalization'].includes(u.status);
  }
  function canAbandon(u: Upload) {
    return !['Finalized', 'Abandoned'].includes(u.status);
  }
  function isActive(u: Upload) {
    return ['Created', 'Uploading'].includes(u.status);
  }

  let metadata = $derived.by(() => {
    if (!upload?.metadata_json) return null;
    try { return JSON.parse(upload.metadata_json) as Record<string, string>; }
    catch { return null; }
  });

  const EV_COLORS: Record<string, string> = {
    created:            '#64748b',
    chunk_received:     '#3b82f6',
    completed:          '#22c55e',
    processing_started: '#f59e0b',
    finalized:          '#4ade80',
    processing_failed:  '#ef4444',
    failed:             '#ef4444',
    abandoned:          '#475569',
    deleted:            '#475569',
    retry_queued:       '#f97316',
  };

  function evColor(type: string) {
    return EV_COLORS[type] ?? '#94a3b8';
  }

  function evLabel(type: string) {
    return type.replace(/_/g, ' ');
  }

  // group consecutive chunk_received into one summary line
  let displayEvents = $derived.by(() => {
    const out: Array<{ ev: UploadEvent; count: number }> = [];
    for (const ev of events) {
      const last = out[out.length - 1];
      if (ev.event_type === 'chunk_received' && last?.ev.event_type === 'chunk_received') {
        last.count++;
        last.ev = ev; // keep latest so message shows final offset
      } else {
        out.push({ ev, count: 1 });
      }
    }
    return out.reverse();
  });
</script>

<div class="topbar">
  <a href="/" class="back">← All uploads</a>
  <div class="live-indicator" class:on={live}>
    <span class="dot"></span>
    {live ? 'Live' : 'Offline'}
  </div>
</div>

{#if error}
  <div class="alert">{error}</div>
{:else if !upload}
  <div class="loading">Loading…</div>
{:else}
  <!-- ── header ── -->
  <div class="header">
    <div class="header-left">
      <h1>{upload.filename ?? '(unnamed)'}</h1>
      <button class="id-chip" onclick={copyId} title="Click to copy">
        <code>{id}</code>
        <span>{copied ? '✓ copied' : 'copy'}</span>
      </button>
    </div>
    <div class="header-right">
      <span class="badge {upload.status.toLowerCase()}">{upload.status}</span>
      <div class="actions">
        {#if canRetry(upload)}
          <button class="btn primary" onclick={retry}>↺ Retry</button>
        {/if}
        {#if canAbandon(upload)}
          <button class="btn danger" onclick={abandon}>⊘ Abandon</button>
        {/if}
        <button class="btn delete" onclick={hardDelete}>✕ Delete</button>
      </div>
    </div>
  </div>

  {#if actErr}
    <div class="alert">{actErr}</div>
  {/if}

  {#if upload.error_message}
    <div class="alert">
      <strong>Error:</strong> {upload.error_message}
    </div>
  {/if}

  <!-- ── progress bar (prominent if active) ── -->
  {#if isActive(upload) || upload.status === 'Uploading'}
    <div class="prog-section">
      <div class="prog-header">
        <span>{fmt(upload.upload_offset)} of {fmt(upload.upload_length)}</span>
        <span class="pct">{pct(upload)}%</span>
      </div>
      <div class="prog-track"><div class="prog-fill animated" style="width:{pct(upload)}%"></div></div>
    </div>
  {/if}

  <!-- ── info grid ── -->
  <div class="grid">
    <!-- transfer -->
    <div class="card">
      <div class="card-title">Transfer</div>
      <div class="stat-row">
        <span>Total size</span>     <strong>{fmt(upload.upload_length)}</strong>
      </div>
      <div class="stat-row">
        <span>Uploaded</span>       <strong>{fmt(upload.upload_offset)}</strong>
      </div>
      <div class="stat-row">
        <span>Progress</span>       <strong>{pct(upload)}%</strong>
      </div>
      {#if upload.status !== 'Uploading' && upload.status !== 'Created'}
        <div class="prog-mini">
          <div class="prog-fill" style="width:{pct(upload)}%"></div>
        </div>
      {/if}
    </div>

    <!-- timing -->
    <div class="card">
      <div class="card-title">Timing</div>
      <div class="stat-row">
        <span>Created</span>        <strong>{fmtDate(upload.created_at)}</strong>
      </div>
      <div class="stat-row">
        <span>Last activity</span>  <strong>{reltime(upload.updated_at)}</strong>
      </div>
      {#if upload.completed_at}
        <div class="stat-row">
          <span>Completed</span>    <strong>{fmtDate(upload.completed_at)}</strong>
        </div>
        <div class="stat-row">
          <span>Duration</span>
          <strong>{
            (() => {
              const ms = new Date(upload.completed_at).getTime() - new Date(upload.created_at).getTime();
              return ms < 1000 ? `${ms}ms` : `${(ms/1000).toFixed(1)}s`;
            })()
          }</strong>
        </div>
      {/if}
    </div>

    <!-- storage -->
    <div class="card">
      <div class="card-title">Storage</div>
      <div class="stat-row">
        <span>Path</span>
        <strong class="path">{upload.storage_path}</strong>
      </div>
      <div class="stat-row">
        <span>Status</span>
        <span class="badge {upload.status.toLowerCase()}">{upload.status}</span>
      </div>
    </div>

    <!-- metadata -->
    {#if metadata}
      <div class="card">
        <div class="card-title">Metadata</div>
        {#each Object.entries(metadata) as [k, v]}
          <div class="stat-row">
            <span>{k}</span><strong>{v}</strong>
          </div>
        {/each}
      </div>
    {/if}
  </div>

  <!-- ── event log ── -->
  <section class="events">
    <div class="events-header">
      <h2>Event log <span class="count">{events.length}</span></h2>
      {#if live}<span class="live-badge">● live</span>{/if}
    </div>

    {#if displayEvents.length === 0}
      <p class="dim">No events yet.</p>
    {:else}
      <ul>
        {#each displayEvents as { ev, count } (ev.id)}
          <li>
            <span class="ev-dot" style="background:{evColor(ev.event_type)}"></span>
            <span class="ev-type" style="color:{evColor(ev.event_type)}">{evLabel(ev.event_type)}</span>
            {#if count > 1}<span class="ev-count">×{count}</span>{/if}
            {#if ev.message}<span class="ev-msg">{ev.message}</span>{/if}
            <span class="ev-time">{reltime(ev.created_at)}</span>
          </li>
        {/each}
      </ul>
    {/if}
  </section>
{/if}

<style>
  .topbar {
    display: flex; align-items: center; justify-content: space-between;
    margin-bottom: 1.25rem;
  }
  .back { font-size: 0.875rem; color: #60a5fa; }
  .back:hover { text-decoration: underline; }

  .live-indicator {
    display: flex; align-items: center; gap: 0.375rem;
    font-size: 0.75rem; color: #475569;
  }
  .live-indicator.on { color: #4ade80; }
  .dot {
    width: 7px; height: 7px; border-radius: 50%; background: #475569;
    transition: background 0.3s;
  }
  .live-indicator.on .dot { background: #4ade80; box-shadow: 0 0 6px #4ade80; animation: pulse 1.5s infinite; }
  @keyframes pulse { 0%,100% { opacity: 1; } 50% { opacity: 0.4; } }

  .loading { color: #64748b; padding: 2rem 0; }

  .alert {
    background: #3b1a1a; border: 1px solid #7f1d1d;
    padding: 0.75rem 1rem; border-radius: 6px; margin-bottom: 1rem; font-size: 0.875rem;
  }

  /* header */
  .header {
    display: flex; justify-content: space-between; align-items: flex-start;
    gap: 1rem; margin-bottom: 1.25rem; flex-wrap: wrap;
  }
  h1 { font-size: 1.25rem; font-weight: 700; margin-bottom: 0.375rem; }

  .id-chip {
    display: inline-flex; align-items: center; gap: 0.5rem;
    background: #1e2130; border: 1px solid #2d3148; border-radius: 4px;
    padding: 0.2rem 0.5rem; cursor: pointer; font-size: 0.72rem;
  }
  .id-chip:hover { border-color: #3d4263; }
  .id-chip code { font-family: monospace; color: #94a3b8; }
  .id-chip span { color: #475569; font-size: 0.65rem; }

  .header-right { display: flex; align-items: center; gap: 0.75rem; flex-wrap: wrap; }
  .actions { display: flex; gap: 0.5rem; }
  .btn {
    padding: 0.375rem 0.875rem; border-radius: 6px; border: none;
    cursor: pointer; font-size: 0.875rem; font-weight: 500;
  }
  .btn.primary { background: #1d4ed8; color: #fff; }
  .btn.primary:hover { background: #2563eb; }
  .btn.danger  { background: #991b1b; color: #fff; }
  .btn.danger:hover  { background: #b91c1c; }
  .btn.delete  { background: #2d3148; color: #f87171; border: 1px solid #7f1d1d; }
  .btn.delete:hover  { background: #3a1a1a; }

  /* progress (active uploads) */
  .prog-section {
    background: #1e2130; border: 1px solid #2d3148; border-radius: 8px;
    padding: 1rem; margin-bottom: 1.25rem;
  }
  .prog-header { display: flex; justify-content: space-between; margin-bottom: 0.5rem; font-size: 0.875rem; color: #94a3b8; }
  .prog-header .pct { font-weight: 700; color: #e2e8f0; }
  .prog-track { height: 10px; background: #2d3148; border-radius: 5px; overflow: hidden; }
  .prog-fill { height: 100%; background: #3b82f6; border-radius: 5px; transition: width 0.4s; }
  .prog-fill.animated { background: linear-gradient(90deg, #3b82f6, #60a5fa, #3b82f6); background-size: 200%; animation: shimmer 1.5s infinite linear; }
  @keyframes shimmer { from { background-position: 100% } to { background-position: -100% } }

  /* info grid */
  .grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(220px, 1fr));
    gap: 1rem; margin-bottom: 1.5rem;
  }
  .card {
    background: #1e2130; border: 1px solid #2d3148; border-radius: 8px; padding: 1rem;
  }
  .card-title {
    font-size: 0.7rem; text-transform: uppercase; letter-spacing: 0.07em;
    color: #64748b; margin-bottom: 0.75rem; font-weight: 600;
  }
  .stat-row {
    display: flex; justify-content: space-between; align-items: baseline;
    gap: 0.5rem; margin-bottom: 0.4rem; font-size: 0.8rem;
  }
  .stat-row span:first-child { color: #64748b; flex-shrink: 0; }
  .stat-row strong { font-weight: 500; text-align: right; word-break: break-all; }
  .path { font-family: monospace; font-size: 0.72rem; color: #94a3b8; }
  .prog-mini { height: 4px; background: #2d3148; border-radius: 2px; overflow: hidden; margin-top: 0.5rem; }

  /* event log */
  .events { margin-top: 0.5rem; }
  .events-header { display: flex; align-items: center; gap: 0.75rem; margin-bottom: 0.75rem; }
  h2 { font-size: 0.9rem; font-weight: 600; display: flex; align-items: center; gap: 0.4rem; }
  .count {
    background: #2d3148; color: #94a3b8; font-size: 0.7rem;
    padding: 0.1rem 0.45rem; border-radius: 999px;
  }
  .live-badge { font-size: 0.7rem; color: #4ade80; }
  .dim { color: #64748b; font-size: 0.875rem; }

  ul { list-style: none; display: flex; flex-direction: column; gap: 2px; }
  li {
    display: flex; align-items: baseline; gap: 0.625rem;
    padding: 0.4rem 0.625rem; border-radius: 4px; font-size: 0.8rem;
    background: #161824;
  }
  li:nth-child(even) { background: #1a1f30; }

  .ev-dot { width: 6px; height: 6px; border-radius: 50%; flex-shrink: 0; margin-top: 1px; }
  .ev-type { font-weight: 600; flex-shrink: 0; }
  .ev-count { font-size: 0.7rem; color: #64748b; background: #2d3148; padding: 0 0.3rem; border-radius: 3px; }
  .ev-msg { color: #94a3b8; flex: 1; font-family: monospace; font-size: 0.75rem; }
  .ev-time { color: #475569; font-size: 0.72rem; flex-shrink: 0; margin-left: auto; font-family: monospace; }

  /* badges */
  .badge {
    display: inline-block; padding: 0.2rem 0.55rem; border-radius: 4px;
    font-size: 0.72rem; font-weight: 600; white-space: nowrap;
  }
  .badge.created          { background: #1e2a3a; color: #93c5fd; }
  .badge.uploading        { background: #1e3a5f; color: #60a5fa; }
  .badge.completed        { background: #1a3a2a; color: #86efac; }
  .badge.processing       { background: #3a2e1a; color: #fcd34d; }
  .badge.finalized        { background: #1a2f1a; color: #4ade80; }
  .badge.failedupload,
  .badge.failedprocessing,
  .badge.failedfinalization { background: #3a1a1a; color: #f87171; }
  .badge.abandoned        { background: #1e2130; color: #475569; }
</style>
