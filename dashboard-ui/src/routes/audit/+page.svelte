<script lang="ts">
  import { onMount } from 'svelte';
  import { api } from '$lib/api';
  import type { AuditEntry } from '$lib/types';

  const PAGE_SIZE = 100;
  let entries     = $state<AuditEntry[]>([]);
  let loading     = $state(true);
  let loadingMore = $state(false);
  let error       = $state<string | null>(null);
  let pageLimit   = $state(PAGE_SIZE);
  let methodFilter = $state('');
  let statusFilter = $state('');

  let mayHaveMore = $derived(entries.length === pageLimit);

  let filtered = $derived.by(() => {
    let list = entries;
    if (methodFilter) list = list.filter(e => e.method === methodFilter);
    if (statusFilter) {
      const code = parseInt(statusFilter);
      if (!isNaN(code)) {
        list = list.filter(e => Math.floor(e.status_code / 100) === Math.floor(code / 100));
      }
    }
    return list;
  });

  async function load(limit = pageLimit) {
    try {
      entries = await api.listAudit({ limit });
      error = null;
    } catch (e) { error = String(e); }
    finally { loading = false; }
  }

  async function loadMore() {
    loadingMore = true;
    pageLimit += PAGE_SIZE;
    await load(pageLimit);
    loadingMore = false;
  }

  onMount(() => load());

  function reltime(s: string) {
    const d = Date.now() - new Date(s).getTime();
    if (d < 60_000)    return `${Math.floor(d / 1000)}s ago`;
    if (d < 3_600_000) return `${Math.floor(d / 60_000)}m ago`;
    if (d < 86_400_000) return `${Math.floor(d / 3_600_000)}h ago`;
    return new Date(s).toLocaleString();
  }

  function statusClass(code: number): string {
    if (code < 300) return 'ok';
    if (code < 400) return 'redirect';
    if (code < 500) return 'client';
    return 'server';
  }

  function methodClass(m: string): string {
    const map: Record<string, string> = { GET: 'get', POST: 'post', PATCH: 'patch', DELETE: 'delete', HEAD: 'head', OPTIONS: 'options' };
    return map[m] ?? 'other';
  }

  const METHODS = ['GET', 'POST', 'PATCH', 'DELETE', 'HEAD', 'OPTIONS'];
  const STATUS_GROUPS = [
    { label: 'All', value: '' },
    { label: '2xx', value: '200' },
    { label: '4xx', value: '400' },
    { label: '5xx', value: '500' },
  ];
</script>

<div class="page-header">
  <h1>Audit Log</h1>
  <button class="btn" onclick={() => load()} disabled={loading}>↻ Refresh</button>
</div>

<!-- ── filters ── -->
<div class="filters">
  <div class="filter-group">
    <label>Method</label>
    <div class="filter-pills">
      <button class="pill" class:active={methodFilter === ''} onclick={() => methodFilter = ''}>All</button>
      {#each METHODS as m}
        <button class="pill {methodClass(m)}" class:active={methodFilter === m} onclick={() => methodFilter = m}>{m}</button>
      {/each}
    </div>
  </div>
  <div class="filter-group">
    <label>Status</label>
    <div class="filter-pills">
      {#each STATUS_GROUPS as g}
        <button class="pill" class:active={statusFilter === g.value} onclick={() => statusFilter = g.value}>{g.label}</button>
      {/each}
    </div>
  </div>
</div>

{#if error}
  <div class="alert">{error}</div>
{:else if loading}
  <div class="empty">Loading…</div>
{:else if filtered.length === 0}
  <div class="empty">No audit entries match.</div>
{:else}
  <div class="table-wrap">
    <table>
      <thead>
        <tr>
          <th>Time</th>
          <th>Method</th>
          <th>Path</th>
          <th>Status</th>
          <th>Actor</th>
          <th>IP</th>
          <th>Upload</th>
        </tr>
      </thead>
      <tbody>
        {#each filtered as e (e.id)}
          <tr>
            <td class="mono small dim" title={e.created_at}>{reltime(e.created_at)}</td>
            <td><span class="method {methodClass(e.method)}">{e.method}</span></td>
            <td class="path-cell mono small" title={e.path}>{e.path.length > 60 ? e.path.slice(0, 60) + '…' : e.path}</td>
            <td><span class="status {statusClass(e.status_code)}">{e.status_code}</span></td>
            <td class="mono small">{e.actor}</td>
            <td class="mono small dim">{e.source_ip ?? '—'}</td>
            <td class="mono small">
              {#if e.upload_id}
                <a href="/uploads/{e.upload_id}">{e.upload_id.slice(0, 8)}…</a>
              {:else}
                <span class="dim">—</span>
              {/if}
            </td>
          </tr>
        {/each}
      </tbody>
    </table>
  </div>
  <div class="footer">
    <span>Showing {filtered.length}{filtered.length !== entries.length ? ` of ${entries.length}` : ''} entries</span>
    {#if mayHaveMore && !methodFilter && !statusFilter}
      <button class="btn-more" onclick={loadMore} disabled={loadingMore}>
        {loadingMore ? 'Loading…' : 'Load more'}
      </button>
    {/if}
  </div>
{/if}

<style>
  .page-header { display: flex; align-items: center; justify-content: space-between; margin-bottom: 1.25rem; }
  h1 { font-size: 1.25rem; font-weight: 700; }
  .btn { padding: 0.375rem 0.875rem; background: #2d3148; color: #e2e8f0; border: 1px solid #3d4263; border-radius: 6px; cursor: pointer; font-size: 0.875rem; }
  .btn:hover { background: #363b5a; }
  .btn:disabled { opacity: 0.5; cursor: default; }
  .alert { background: #3b1a1a; border: 1px solid #7f1d1d; padding: 0.75rem 1rem; border-radius: 6px; margin-bottom: 1rem; font-size: 0.875rem; }
  .empty { color: #64748b; text-align: center; padding: 3rem 0; }

  .filters { display: flex; gap: 1.5rem; margin-bottom: 1rem; flex-wrap: wrap; }
  .filter-group { display: flex; align-items: center; gap: 0.5rem; }
  .filter-group label { font-size: 0.75rem; color: #64748b; white-space: nowrap; }
  .filter-pills { display: flex; gap: 0.25rem; flex-wrap: wrap; }
  .pill {
    padding: 0.2rem 0.55rem; font-size: 0.72rem; font-weight: 500;
    border-radius: 4px; border: 1px solid #2d3148; background: #1e2130;
    color: #64748b; cursor: pointer; transition: all 0.15s;
  }
  .pill:hover { border-color: #3d4263; color: #94a3b8; }
  .pill.active { background: #2d3148; color: #e2e8f0; border-color: #3d4263; }

  /* method colours when active */
  .pill.get.active     { background: #1a3a2a; color: #4ade80; border-color: #166534; }
  .pill.post.active    { background: #1e3a5f; color: #60a5fa; border-color: #1d4ed8; }
  .pill.patch.active   { background: #2a1f3a; color: #c084fc; border-color: #6d28d9; }
  .pill.delete.active  { background: #3a1a1a; color: #f87171; border-color: #7f1d1d; }
  .pill.head.active,
  .pill.options.active { background: #3a2e1a; color: #fcd34d; border-color: #78350f; }

  /* table */
  .table-wrap { overflow-x: auto; }
  table { width: 100%; border-collapse: collapse; font-size: 0.875rem; }
  th { text-align: left; padding: 0.5rem 0.75rem; color: #475569; font-weight: 500; font-size: 0.72rem; text-transform: uppercase; letter-spacing: 0.05em; border-bottom: 1px solid #2d3148; }
  td { padding: 0.5rem 0.75rem; border-bottom: 1px solid #161824; vertical-align: middle; }
  tr:hover td { background: #161824; }

  .mono { font-family: monospace; }
  .small { font-size: 0.8rem; }
  .dim { color: #64748b; }
  .path-cell { max-width: 300px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; color: #94a3b8; }

  .method {
    font-size: 0.7rem; font-weight: 700; padding: 0.15rem 0.4rem;
    border-radius: 3px; font-family: monospace;
  }
  .method.get     { background: #1a3a2a; color: #4ade80; }
  .method.post    { background: #1e3a5f; color: #60a5fa; }
  .method.patch   { background: #2a1f3a; color: #c084fc; }
  .method.delete  { background: #3a1a1a; color: #f87171; }
  .method.head,
  .method.options { background: #3a2e1a; color: #fcd34d; }
  .method.other   { background: #2d3148; color: #94a3b8; }

  .status {
    font-size: 0.75rem; font-weight: 700; font-family: monospace;
    padding: 0.15rem 0.4rem; border-radius: 3px;
  }
  .status.ok       { background: #1a3a2a; color: #4ade80; }
  .status.redirect { background: #1e2a3a; color: #93c5fd; }
  .status.client   { background: #3a2e1a; color: #fcd34d; }
  .status.server   { background: #3a1a1a; color: #f87171; }

  .footer { margin-top: 0.75rem; font-size: 0.8rem; color: #475569; display: flex; justify-content: space-between; align-items: center; }
  .btn-more { padding: 0.3rem 0.875rem; background: #2d3148; color: #94a3b8; border: 1px solid #3d4263; border-radius: 5px; cursor: pointer; font-size: 0.8rem; }
  .btn-more:hover:not(:disabled) { background: #363b5a; color: #e2e8f0; }
  .btn-more:disabled { opacity: 0.5; cursor: default; }
</style>
