<script lang="ts">
  import { onMount } from 'svelte';
  import { api } from '$lib/api';
  import type { Upload } from '$lib/types';

  type Group = 'all' | 'active' | 'processing' | 'finalized' | 'failed' | 'abandoned';

  const ACTIVE = ['Created', 'Uploading'];
  const FAILED = ['FailedUpload', 'FailedProcessing', 'FailedFinalization'];

  let uploads     = $state<Upload[]>([]);
  let loading     = $state(true);
  let error       = $state<string | null>(null);
  let actionError = $state<string | null>(null);
  let group       = $state<Group>('all');
  let search      = $state('');
  let autoRefresh = $state(true);
  let lastRefresh = $state<Date | null>(null);
  let selected    = $state(new Set<string>());
  let bulkWorking = $state(false);

  let stats = $derived.by(() => ({
    total:      uploads.length,
    active:     uploads.filter(u => ACTIVE.includes(u.status)).length,
    processing: uploads.filter(u => u.status === 'Processing').length,
    finalized:  uploads.filter(u => u.status === 'Finalized').length,
    failed:     uploads.filter(u => FAILED.includes(u.status)).length,
    abandoned:  uploads.filter(u => u.status === 'Abandoned').length,
    bytes:      uploads.reduce((a, u) => a + u.upload_length, 0),
  }));

  let filtered = $derived.by(() => {
    let list = uploads;
    if (group === 'active')     list = list.filter(u => ACTIVE.includes(u.status));
    if (group === 'processing') list = list.filter(u => u.status === 'Processing');
    if (group === 'finalized')  list = list.filter(u => u.status === 'Finalized');
    if (group === 'failed')     list = list.filter(u => FAILED.includes(u.status));
    if (group === 'abandoned')  list = list.filter(u => u.status === 'Abandoned');
    if (search) {
      const q = search.toLowerCase();
      list = list.filter(u => (u.filename ?? '').toLowerCase().includes(q) || u.id.includes(q));
    }
    return list;
  });

  let allSelected = $derived(
    filtered.length > 0 && filtered.every(u => selected.has(u.id))
  );

  async function load() {
    try {
      uploads = await api.listUploads();
      lastRefresh = new Date();
      error = null;
    } catch (e) { error = String(e); }
    finally { loading = false; }
  }

  onMount(() => {
    load();
    const t = setInterval(() => { if (autoRefresh) load(); }, 3000);
    return () => clearInterval(t);
  });

  function toggleRow(id: string) {
    const next = new Set(selected);
    next.has(id) ? next.delete(id) : next.add(id);
    selected = next;
  }

  function toggleAll() {
    selected = allSelected
      ? new Set()
      : new Set(filtered.map(u => u.id));
  }

  function clearSelection() { selected = new Set(); }

  async function deleteSingle(id: string) {
    if (!confirm('Permanently delete this upload and its file?')) return;
    actionError = null;
    try {
      await api.deleteUpload(id);
      await load();
      selected = new Set([...selected].filter(s => s !== id));
    } catch (e) { actionError = String(e); }
  }

  async function bulkDelete() {
    const ids = [...selected];
    if (!confirm(`Permanently delete ${ids.length} upload(s) and their files?`)) return;
    actionError = null;
    bulkWorking = true;
    try {
      await api.purgeUploads(ids);
      await load();
      clearSelection();
    } catch (e) { actionError = String(e); }
    finally { bulkWorking = false; }
  }

  async function bulkAbandon() {
    const ids = [...selected];
    if (!confirm(`Mark ${ids.length} upload(s) as abandoned?`)) return;
    actionError = null;
    bulkWorking = true;
    try {
      await Promise.all(ids.map(id => api.markAbandoned(id)));
      await load();
      clearSelection();
    } catch (e) { actionError = String(e); }
    finally { bulkWorking = false; }
  }

  async function purgeGroup(g: Group) {
    const targets = uploads.filter(u => {
      if (g === 'finalized') return u.status === 'Finalized';
      if (g === 'abandoned') return u.status === 'Abandoned';
      if (g === 'failed')    return FAILED.includes(u.status);
      return false;
    });
    if (targets.length === 0) return;
    if (!confirm(`Delete all ${targets.length} ${g} upload(s)?`)) return;
    actionError = null;
    bulkWorking = true;
    try {
      await api.purgeUploads(targets.map(u => u.id));
      await load();
      clearSelection();
    } catch (e) { actionError = String(e); }
    finally { bulkWorking = false; }
  }

  function fmt(b: number) {
    if (b === 0) return '0 B';
    if (b < 1024)       return `${b} B`;
    if (b < 1024 ** 2)  return `${(b / 1024).toFixed(1)} KB`;
    if (b < 1024 ** 3)  return `${(b / 1024 ** 2).toFixed(1)} MB`;
    return `${(b / 1024 ** 3).toFixed(2)} GB`;
  }

  function pct(u: Upload) {
    return u.upload_length > 0 ? Math.round((u.upload_offset / u.upload_length) * 100) : 0;
  }

  function reltime(s: string) {
    const d = Date.now() - new Date(s).getTime();
    if (d < 60_000)    return `${Math.floor(d / 1000)}s ago`;
    if (d < 3_600_000) return `${Math.floor(d / 60_000)}m ago`;
    if (d < 86_400_000) return `${Math.floor(d / 3_600_000)}h ago`;
    return new Date(s).toLocaleDateString();
  }

  const CARDS: { key: Group; label: string; accent: string }[] = [
    { key: 'all',        label: 'Total',      accent: '#60a5fa' },
    { key: 'active',     label: 'Active',     accent: '#38bdf8' },
    { key: 'processing', label: 'Processing', accent: '#fcd34d' },
    { key: 'finalized',  label: 'Finalized',  accent: '#4ade80' },
    { key: 'failed',     label: 'Failed',     accent: '#f87171' },
    { key: 'abandoned',  label: 'Abandoned',  accent: '#64748b' },
  ];

  function cardCount(key: Group): number {
    return stats[key === 'all' ? 'total' : key] as number;
  }

  const PURGEABLE: Group[] = ['finalized', 'abandoned', 'failed'];

  // ── test uploader ──
  const CHUNK_PRESETS = [
    { label: '256 KB', bytes: 256 * 1024 },
    { label: '512 KB', bytes: 512 * 1024 },
    { label: '1 MB',   bytes: 1 * 1024 * 1024 },
    { label: '5 MB',   bytes: 5 * 1024 * 1024 },
    { label: '10 MB',  bytes: 10 * 1024 * 1024 },
    { label: '25 MB',  bytes: 25 * 1024 * 1024 },
  ];
  let chunkSize = $state(1 * 1024 * 1024); // default 1 MB

  let uploaderOpen   = $state(false);
  let uploadFile     = $state<File | null>(null);
  let uploadProgress = $state(0);       // 0–1
  let uploadStatus   = $state<'idle' | 'uploading' | 'done' | 'error'>('idle');
  let uploadError    = $state('');
  let uploadedId     = $state('');
  let uploadAbort    = $state<AbortController | null>(null);
  let dropActive     = $state(false);

  function onFileChange(e: Event) {
    const input = e.target as HTMLInputElement;
    uploadFile = input.files?.[0] ?? null;
    uploadStatus = 'idle'; uploadError = ''; uploadedId = ''; uploadProgress = 0;
  }

  function onDrop(e: DragEvent) {
    e.preventDefault(); dropActive = false;
    uploadFile = e.dataTransfer?.files[0] ?? null;
    uploadStatus = 'idle'; uploadError = ''; uploadedId = ''; uploadProgress = 0;
  }

  async function startUpload() {
    if (!uploadFile) return;
    const file = uploadFile;
    const ac = new AbortController();
    uploadAbort = ac;
    uploadStatus = 'uploading';
    uploadError = '';
    uploadedId = '';
    uploadProgress = 0;

    try {
      // 1. Create upload
      const bytes = new TextEncoder().encode(file.name);
      let binary = '';
      bytes.forEach(b => (binary += String.fromCharCode(b)));
      const metaValue = btoa(binary);
      const createRes = await fetch('/files', {
        method: 'POST',
        signal: ac.signal,
        headers: {
          'Tus-Resumable': '1.0.0',
          'Upload-Length': String(file.size),
          'Upload-Metadata': `filename ${metaValue}`,
        },
      });
      if (!createRes.ok) throw new Error(`Create failed: ${createRes.status}`);
      const location = createRes.headers.get('Location');
      if (!location) throw new Error('No Location header in response');

      // Extract upload ID from location path
      uploadedId = location.split('/').pop() ?? '';

      // 2. Upload chunks
      let offset = 0;
      while (offset < file.size) {
        const end = Math.min(offset + chunkSize, file.size);
        const chunk = file.slice(offset, end);
        const patchRes = await fetch(location, {
          method: 'PATCH',
          signal: ac.signal,
          headers: {
            'Tus-Resumable': '1.0.0',
            'Content-Type': 'application/offset+octet-stream',
            'Upload-Offset': String(offset),
            'Content-Length': String(chunk.size),
          },
          body: chunk,
        });
        if (!patchRes.ok) throw new Error(`Chunk upload failed: ${patchRes.status}`);
        offset = parseInt(patchRes.headers.get('Upload-Offset') ?? String(end), 10);
        uploadProgress = file.size > 0 ? offset / file.size : 1;
      }

      uploadStatus = 'done';
      load(); // refresh table
    } catch (e: unknown) {
      if ((e as Error).name === 'AbortError') {
        uploadStatus = 'idle';
      } else {
        uploadStatus = 'error';
        uploadError = e instanceof Error ? e.message : String(e);
      }
    } finally {
      uploadAbort = null;
    }
  }

  function cancelUpload() {
    uploadAbort?.abort();
  }

  function resetUploader() {
    uploadFile = null; uploadStatus = 'idle';
    uploadError = ''; uploadedId = ''; uploadProgress = 0;
  }
</script>

<!-- ── stat cards ── -->
<div class="cards">
  {#each CARDS as c}
    <div
      class="card {group === c.key ? 'active' : ''}"
      style="--accent: {c.accent}"
      role="button"
      tabindex="0"
      onclick={() => { group = c.key; clearSelection(); }}
      onkeydown={(e) => e.key === 'Enter' && (group = c.key)}
    >
      <span class="card-n">{cardCount(c.key)}</span>
      <span class="card-l">{c.label}</span>
      {#if PURGEABLE.includes(c.key) && cardCount(c.key) > 0}
        <button
          class="purge-btn"
          title="Delete all {c.label.toLowerCase()}"
          onclick={(e) => { e.stopPropagation(); purgeGroup(c.key); }}
          disabled={bulkWorking}
        >✕ purge</button>
      {/if}
    </div>
  {/each}
  <div class="card storage">
    <span class="card-n">{fmt(stats.bytes)}</span>
    <span class="card-l">Storage</span>
  </div>
</div>

<!-- ── toolbar ── -->
<div class="toolbar">
  <div class="search-wrap">
    <svg viewBox="0 0 20 20" fill="none" stroke="currentColor" stroke-width="1.5"><circle cx="8.5" cy="8.5" r="5.25"/><path d="M13 13l3.5 3.5" stroke-linecap="round"/></svg>
    <input class="search" placeholder="Search filename or ID…" bind:value={search} />
    {#if search}<button class="clear-btn" onclick={() => search = ''}>✕</button>{/if}
  </div>
  <div class="toolbar-right">
    <span class="refresh-info">{#if lastRefresh}Updated {reltime(lastRefresh.toISOString())}{/if}</span>
    <label class="toggle">
      <input type="checkbox" bind:checked={autoRefresh} />
      <span>Auto-refresh</span>
    </label>
    <button class="btn" onclick={load} disabled={loading}>↻ Refresh</button>
  </div>
</div>

{#if error}<div class="alert">{error}</div>{/if}
{#if actionError}<div class="alert">{actionError}</div>{/if}

<!-- ── test uploader ── -->
<div class="uploader-wrap">
  <button class="uploader-toggle" onclick={() => (uploaderOpen = !uploaderOpen)}>
    <svg class="chevron" class:open={uploaderOpen} viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="2">
      <path d="M4 6l4 4 4-4" stroke-linecap="round" stroke-linejoin="round"/>
    </svg>
    Test Upload
  </button>

  {#if uploaderOpen}
    <div class="uploader">
      <!-- drop zone -->
      <div
        class="drop-zone"
        class:drag-over={dropActive}
        class:has-file={uploadFile !== null}
        role="button"
        tabindex="0"
        ondragover={(e) => { e.preventDefault(); dropActive = true; }}
        ondragleave={() => (dropActive = false)}
        ondrop={onDrop}
        onclick={() => (document.getElementById('file-input') as HTMLInputElement)?.click()}
        onkeydown={(e) => e.key === 'Enter' && (document.getElementById('file-input') as HTMLInputElement)?.click()}
      >
        {#if uploadFile}
          <span class="drop-file">{uploadFile.name}</span>
          <span class="drop-size">{fmt(uploadFile.size)}</span>
        {:else}
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" class="drop-icon">
            <path d="M12 16V8m0 0l-3 3m3-3l3 3" stroke-linecap="round" stroke-linejoin="round"/>
            <rect x="3" y="3" width="18" height="18" rx="3"/>
          </svg>
          <span>Drop a file here, or click to browse</span>
        {/if}
        <input id="file-input" type="file" style="display:none" onchange={onFileChange} />
      </div>

      <!-- chunk size -->
      <div class="chunk-row">
        <label for="chunk-select" class="chunk-label">Chunk size</label>
        <select id="chunk-select" class="chunk-select" bind:value={chunkSize} disabled={uploadStatus === 'uploading'}>
          {#each CHUNK_PRESETS as p}
            <option value={p.bytes}>{p.label}</option>
          {/each}
        </select>
      </div>

      <!-- progress -->
      {#if uploadStatus === 'uploading'}
        <div class="up-progress">
          <div class="up-bar"><div class="up-fill" style="width:{Math.round(uploadProgress * 100)}%"></div></div>
          <span class="up-pct">{Math.round(uploadProgress * 100)}%</span>
          {#if uploadFile}<span class="up-bytes">{fmt(Math.round(uploadProgress * uploadFile.size))} / {fmt(uploadFile.size)}</span>{/if}
        </div>
      {/if}

      <!-- status messages -->
      {#if uploadStatus === 'done'}
        <div class="up-msg ok">
          Upload complete —
          <a href="/uploads/{uploadedId}">view detail</a>
          <button class="up-again" onclick={resetUploader}>upload another</button>
        </div>
      {:else if uploadStatus === 'error'}
        <div class="up-msg err">{uploadError}</div>
      {/if}

      <!-- actions -->
      <div class="up-actions">
        {#if uploadStatus === 'uploading'}
          <button class="btn-up cancel" onclick={cancelUpload}>Cancel</button>
        {:else}
          <button
            class="btn-up primary"
            onclick={startUpload}
            disabled={!uploadFile || uploadStatus === 'done'}
          >
            {uploadStatus === 'done' ? 'Done' : 'Start upload'}
          </button>
        {/if}
      </div>
    </div>
  {/if}
</div>

<!-- ── bulk action bar ── -->
{#if selected.size > 0}
  <div class="bulk-bar">
    <span class="bulk-count">{selected.size} selected</span>
    <div class="bulk-actions">
      <button class="btn-bulk abandon" onclick={bulkAbandon} disabled={bulkWorking}>
        ⊘ Abandon
      </button>
      <button class="btn-bulk delete" onclick={bulkDelete} disabled={bulkWorking}>
        ✕ Delete
      </button>
      <button class="btn-bulk clear" onclick={clearSelection}>Deselect all</button>
    </div>
  </div>
{/if}

<!-- ── table ── -->
{#if loading && uploads.length === 0}
  <div class="empty">Loading…</div>
{:else if filtered.length === 0}
  <div class="empty">No uploads match.</div>
{:else}
  <div class="table-wrap">
    <table>
      <thead>
        <tr>
          <th class="col-check">
            <input type="checkbox" checked={allSelected} onchange={toggleAll} />
          </th>
          <th>File</th>
          <th>Status</th>
          <th>Progress</th>
          <th>Size</th>
          <th>Created</th>
          <th>Last activity</th>
          <th></th>
        </tr>
      </thead>
      <tbody>
        {#each filtered as u (u.id)}
          <tr class:row-selected={selected.has(u.id)}>
            <td class="col-check">
              <input type="checkbox" checked={selected.has(u.id)} onchange={() => toggleRow(u.id)} />
            </td>
            <td class="col-file">
              <span class="filename">{u.filename ?? '(unnamed)'}</span>
              <span class="upload-id">{u.id.slice(0, 8)}…</span>
            </td>
            <td><span class="badge {u.status.toLowerCase()}">{u.status}</span></td>
            <td>
              <div class="col-prog">
                <div class="bar"><div class="fill" style="width:{pct(u)}%"></div></div>
                <span class="pct">{pct(u)}%</span>
              </div>
            </td>
            <td class="mono">
              {#if u.upload_offset < u.upload_length && !['Finalized','Abandoned'].includes(u.status)}
                <span class="dim">{fmt(u.upload_offset)} /</span>
              {/if}
              {fmt(u.upload_length)}
            </td>
            <td class="mono dim small">{reltime(u.created_at)}</td>
            <td class="mono dim small">{reltime(u.updated_at)}</td>
            <td class="col-actions">
              <a href="/uploads/{u.id}" class="btn-link">Detail</a>
              <button class="btn-icon danger" title="Delete" onclick={() => deleteSingle(u.id)}>✕</button>
            </td>
          </tr>
        {/each}
      </tbody>
    </table>
  </div>
  <div class="footer">
    Showing {filtered.length} of {uploads.length} upload{uploads.length !== 1 ? 's' : ''}
    · {fmt(stats.bytes)} total
  </div>
{/if}

<style>
  /* ── stat cards ── */
  .cards {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(110px, 1fr));
    gap: 0.75rem; margin-bottom: 1.25rem;
  }
  .card {
    background: #1e2130; border: 1px solid #2d3148; border-radius: 8px;
    padding: 0.875rem 1rem; cursor: pointer; text-align: left;
    transition: border-color 0.15s, background 0.15s;
    display: flex; flex-direction: column; gap: 0.25rem;
    position: relative; overflow: hidden; user-select: none;
  }
  .card:focus-visible { outline: 2px solid var(--accent, #3b82f6); outline-offset: 2px; }
  .card::after {
    content: ''; position: absolute; top: 0; left: 0; right: 0; height: 2px;
    background: var(--accent, #3b82f6); opacity: 0; transition: opacity 0.15s;
  }
  .card:hover { background: #252840; border-color: #3d4263; }
  .card.active { border-color: var(--accent, #3b82f6); background: #1a1f35; }
  .card.active::after { opacity: 1; }
  .card.storage { cursor: default; --accent: #818cf8; }
  .card.storage::after { opacity: 0.4; }
  .card-n { font-size: 1.5rem; font-weight: 700; color: #e2e8f0; line-height: 1; }
  .card-l { font-size: 0.7rem; text-transform: uppercase; letter-spacing: 0.06em; color: #64748b; }

  .purge-btn {
    margin-top: 0.5rem; align-self: flex-start;
    background: none; border: 1px solid #7f1d1d; border-radius: 3px;
    color: #f87171; font-size: 0.65rem; padding: 0.1rem 0.35rem;
    cursor: pointer; opacity: 0; transition: opacity 0.15s;
  }
  .card:hover .purge-btn { opacity: 1; }
  .purge-btn:hover { background: #3b1a1a; }
  .purge-btn:disabled { opacity: 0.4; cursor: default; }

  /* ── toolbar ── */
  .toolbar {
    display: flex; align-items: center; justify-content: space-between;
    gap: 1rem; margin-bottom: 1rem; flex-wrap: wrap;
  }
  .search-wrap {
    display: flex; align-items: center; gap: 0.5rem;
    background: #1e2130; border: 1px solid #2d3148; border-radius: 6px;
    padding: 0 0.75rem; flex: 1; min-width: 200px; max-width: 380px;
  }
  .search-wrap svg { width: 15px; height: 15px; color: #64748b; flex-shrink: 0; }
  .search { background: none; border: none; outline: none; color: #e2e8f0; font-size: 0.875rem; width: 100%; padding: 0.5rem 0; }
  .search::placeholder { color: #475569; }
  .clear-btn { background: none; border: none; color: #64748b; cursor: pointer; font-size: 0.75rem; padding: 0; }
  .clear-btn:hover { color: #e2e8f0; }
  .toolbar-right { display: flex; align-items: center; gap: 0.75rem; }
  .refresh-info { font-size: 0.75rem; color: #475569; }
  .toggle { display: flex; align-items: center; gap: 0.375rem; font-size: 0.8rem; color: #94a3b8; cursor: pointer; }
  .toggle input { accent-color: #3b82f6; }
  .btn {
    padding: 0.375rem 0.875rem; background: #2d3148; color: #e2e8f0;
    border: 1px solid #3d4263; border-radius: 6px; cursor: pointer; font-size: 0.875rem;
  }
  .btn:hover { background: #363b5a; }
  .btn:disabled { opacity: 0.5; cursor: default; }

  /* ── bulk bar ── */
  .bulk-bar {
    display: flex; align-items: center; justify-content: space-between;
    background: #1e2d4a; border: 1px solid #2d4a7a; border-radius: 6px;
    padding: 0.5rem 1rem; margin-bottom: 0.75rem;
  }
  .bulk-count { font-size: 0.875rem; font-weight: 500; color: #93c5fd; }
  .bulk-actions { display: flex; gap: 0.5rem; }
  .btn-bulk {
    padding: 0.3rem 0.75rem; border-radius: 5px; border: none;
    cursor: pointer; font-size: 0.8rem; font-weight: 500;
  }
  .btn-bulk:disabled { opacity: 0.5; cursor: default; }
  .btn-bulk.abandon { background: #3a2a0a; color: #fcd34d; border: 1px solid #78350f; }
  .btn-bulk.abandon:hover:not(:disabled) { background: #451f05; }
  .btn-bulk.delete  { background: #3a1a1a; color: #f87171; border: 1px solid #7f1d1d; }
  .btn-bulk.delete:hover:not(:disabled)  { background: #500f0f; }
  .btn-bulk.clear   { background: #2d3148; color: #94a3b8; border: 1px solid #3d4263; }
  .btn-bulk.clear:hover { background: #363b5a; }

  /* ── alerts ── */
  .alert {
    background: #3b1a1a; border: 1px solid #7f1d1d;
    padding: 0.75rem 1rem; border-radius: 6px; margin-bottom: 0.75rem; font-size: 0.875rem;
  }
  .empty { color: #64748b; text-align: center; padding: 3rem 0; }

  /* ── table ── */
  .table-wrap { overflow-x: auto; }
  table { width: 100%; border-collapse: collapse; font-size: 0.875rem; }
  th {
    text-align: left; padding: 0.5rem 0.75rem;
    color: #475569; font-weight: 500; font-size: 0.72rem; text-transform: uppercase; letter-spacing: 0.05em;
    border-bottom: 1px solid #2d3148;
  }
  td { padding: 0.625rem 0.75rem; border-bottom: 1px solid #161824; vertical-align: middle; }
  tr:hover td { background: #161824; }
  .row-selected td { background: #1a2240; }
  .row-selected:hover td { background: #1e2850; }

  .col-check { width: 36px; padding-right: 0; }
  .col-check input { cursor: pointer; accent-color: #3b82f6; }

  .col-file { width: 220px; }
  .filename { display: block; font-weight: 500; }
  .upload-id { display: block; font-size: 0.7rem; font-family: monospace; color: #475569; margin-top: 0.1rem; }

  .col-prog { display: flex; align-items: center; gap: 0.5rem; width: 180px; }
  .bar { flex: 1; height: 6px; background: #2d3148; border-radius: 3px; overflow: hidden; }
  .fill { height: 100%; background: #3b82f6; border-radius: 3px; transition: width 0.4s; }
  .pct { font-size: 0.75rem; color: #94a3b8; width: 2.5rem; text-align: right; font-family: monospace; flex-shrink: 0; }

  .col-actions { white-space: nowrap; display: flex; align-items: center; gap: 0.5rem; }
  .btn-link { font-size: 0.8rem; color: #60a5fa; white-space: nowrap; }
  .btn-link:hover { text-decoration: underline; }
  .btn-icon {
    background: none; border: none; cursor: pointer;
    font-size: 0.75rem; padding: 0.2rem 0.4rem; border-radius: 3px; opacity: 0.5;
    transition: opacity 0.15s, background 0.15s;
  }
  .btn-icon:hover { opacity: 1; }
  .btn-icon.danger:hover { background: #3a1a1a; color: #f87171; }
  tr:hover .btn-icon { opacity: 0.7; }

  .mono { font-family: monospace; white-space: nowrap; }
  .dim { color: #64748b; }
  .small { font-size: 0.8rem; }

  .footer { margin-top: 0.75rem; font-size: 0.8rem; color: #475569; text-align: right; }

  /* ── test uploader ── */
  .uploader-wrap { margin-bottom: 1rem; }

  .uploader-toggle {
    display: flex; align-items: center; gap: 0.4rem;
    background: none; border: none; cursor: pointer;
    color: #94a3b8; font-size: 0.8rem; padding: 0.25rem 0;
    transition: color 0.15s;
  }
  .uploader-toggle:hover { color: #e2e8f0; }

  .chevron { width: 14px; height: 14px; transition: transform 0.2s; }
  .chevron.open { transform: rotate(180deg); }

  .uploader {
    margin-top: 0.5rem;
    background: #1e2130; border: 1px solid #2d3148; border-radius: 8px;
    padding: 1rem; display: flex; flex-direction: column; gap: 0.75rem;
  }

  .drop-zone {
    border: 2px dashed #2d3148; border-radius: 6px;
    padding: 1.5rem; text-align: center;
    cursor: pointer; color: #64748b; font-size: 0.875rem;
    display: flex; flex-direction: column; align-items: center; gap: 0.4rem;
    transition: border-color 0.15s, background 0.15s;
    user-select: none;
  }
  .drop-zone:hover, .drop-zone.drag-over { border-color: #3b82f6; background: #1a2240; color: #e2e8f0; }
  .drop-zone.has-file { border-color: #4ade80; border-style: solid; }
  .drop-icon { width: 28px; height: 28px; margin-bottom: 0.25rem; }
  .drop-file { font-weight: 600; color: #e2e8f0; font-size: 0.9rem; }
  .drop-size { font-size: 0.775rem; color: #64748b; }

  .chunk-row { display: flex; align-items: center; gap: 0.75rem; }
  .chunk-label { font-size: 0.8rem; color: #64748b; white-space: nowrap; }
  .chunk-select {
    background: #161824; border: 1px solid #2d3148; border-radius: 5px;
    color: #e2e8f0; font-size: 0.8rem; padding: 0.25rem 0.5rem; cursor: pointer;
  }
  .chunk-select:disabled { opacity: 0.4; cursor: not-allowed; }

  .up-progress { display: flex; align-items: center; gap: 0.75rem; }
  .up-bar { flex: 1; height: 6px; background: #2d3148; border-radius: 3px; overflow: hidden; }
  .up-fill { height: 100%; background: #3b82f6; border-radius: 3px; transition: width 0.2s; }
  .up-pct { font-size: 0.75rem; font-family: monospace; color: #94a3b8; width: 2.5rem; text-align: right; }
  .up-bytes { font-size: 0.75rem; color: #64748b; font-family: monospace; }

  .up-msg { font-size: 0.825rem; padding: 0.5rem 0.75rem; border-radius: 5px; }
  .up-msg.ok  { background: #1a2f1a; color: #4ade80; display: flex; align-items: center; gap: 0.5rem; }
  .up-msg.err { background: #3a1a1a; color: #f87171; }

  .up-again {
    background: none; border: none; color: #60a5fa; font-size: 0.8rem;
    cursor: pointer; padding: 0; text-decoration: underline;
  }

  .up-actions { display: flex; gap: 0.5rem; }
  .btn-up {
    padding: 0.4rem 1rem; border-radius: 6px; cursor: pointer;
    font-size: 0.875rem; border: 1px solid transparent;
  }
  .btn-up.primary { background: #1d4ed8; color: #fff; border-color: #1d4ed8; }
  .btn-up.primary:hover:not(:disabled) { background: #2563eb; }
  .btn-up.primary:disabled { opacity: 0.4; cursor: not-allowed; }
  .btn-up.cancel { background: #2d3148; color: #94a3b8; border-color: #3d4263; }
  .btn-up.cancel:hover { background: #363b5a; }

  /* ── badges ── */
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
