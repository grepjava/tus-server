<script lang="ts">
  import { onMount } from 'svelte';
  import { api } from '$lib/api';
  import type { HealthStatus } from '$lib/types';

  let status   = $state<HealthStatus | null>(null);
  let loading  = $state(true);
  let error    = $state<string | null>(null);
  let lastCheck = $state<Date | null>(null);

  async function check() {
    try {
      status = await api.health();
      error = null;
    } catch (e) { error = String(e); }
    finally { loading = false; lastCheck = new Date(); }
  }

  onMount(() => {
    check();
    const t = setInterval(check, 10_000);
    return () => clearInterval(t);
  });

  function reltime(d: Date) {
    const s = Math.floor((Date.now() - d.getTime()) / 1000);
    return s < 5 ? 'just now' : `${s}s ago`;
  }
</script>

<div class="page-header">
  <h1>Health</h1>
  <div class="actions">
    {#if lastCheck}<span class="last">Checked {reltime(lastCheck)}</span>{/if}
    <button class="btn" onclick={check} disabled={loading}>↻ Check now</button>
  </div>
</div>

{#if error}
  <div class="alert">{error}</div>
{:else if loading}
  <div class="empty">Checking…</div>
{:else if status}
  <div class="status-banner" class:ok={status.status === 'ok'} class:degraded={status.status !== 'ok'}>
    <span class="status-icon">{status.status === 'ok' ? '✓' : '✕'}</span>
    <span class="status-text">{status.status === 'ok' ? 'All systems operational' : 'Service degraded'}</span>
  </div>

  <div class="checks">
    <div class="check" class:pass={status.db} class:fail={!status.db}>
      <span class="check-icon">{status.db ? '✓' : '✕'}</span>
      <div class="check-body">
        <span class="check-name">Database</span>
        <span class="check-detail">{status.db ? 'SQLite query OK' : 'Query failed'}</span>
      </div>
    </div>

    <div class="check" class:pass={status.storage} class:fail={!status.storage}>
      <span class="check-icon">{status.storage ? '✓' : '✕'}</span>
      <div class="check-body">
        <span class="check-name">Storage</span>
        <span class="check-detail">{status.storage ? 'Write probe OK' : 'Write probe failed'}</span>
      </div>
    </div>
  </div>
{/if}

<style>
  .page-header {
    display: flex; align-items: center; justify-content: space-between;
    margin-bottom: 1.5rem; flex-wrap: wrap; gap: 0.75rem;
  }
  h1 { font-size: 1.25rem; font-weight: 700; }
  .actions { display: flex; align-items: center; gap: 0.75rem; }
  .last { font-size: 0.75rem; color: #475569; }
  .btn { padding: 0.375rem 0.875rem; background: #2d3148; color: #e2e8f0; border: 1px solid #3d4263; border-radius: 6px; cursor: pointer; font-size: 0.875rem; }
  .btn:hover { background: #363b5a; }
  .btn:disabled { opacity: 0.5; cursor: default; }

  .alert { background: #3b1a1a; border: 1px solid #7f1d1d; padding: 0.75rem 1rem; border-radius: 6px; margin-bottom: 1rem; font-size: 0.875rem; }
  .empty { color: #64748b; padding: 2rem 0; }

  .status-banner {
    display: flex; align-items: center; gap: 0.875rem;
    border-radius: 10px; padding: 1.25rem 1.5rem; margin-bottom: 1.5rem;
    border: 1px solid transparent;
  }
  .status-banner.ok       { background: #0f2a1a; border-color: #166534; }
  .status-banner.degraded { background: #3a1a1a; border-color: #7f1d1d; }

  .status-icon { font-size: 1.25rem; }
  .status-banner.ok       .status-icon { color: #4ade80; }
  .status-banner.degraded .status-icon { color: #f87171; }

  .status-text { font-size: 1rem; font-weight: 600; }
  .status-banner.ok       .status-text { color: #4ade80; }
  .status-banner.degraded .status-text { color: #f87171; }

  .checks { display: flex; flex-direction: column; gap: 0.5rem; }
  .check {
    display: flex; align-items: center; gap: 1rem;
    background: #1e2130; border: 1px solid #2d3148; border-radius: 8px; padding: 1rem 1.25rem;
  }
  .check.pass { border-left: 3px solid #4ade80; }
  .check.fail { border-left: 3px solid #f87171; }

  .check-icon { font-size: 1rem; width: 1.25rem; text-align: center; }
  .check.pass .check-icon { color: #4ade80; }
  .check.fail .check-icon { color: #f87171; }

  .check-body { display: flex; flex-direction: column; gap: 0.15rem; }
  .check-name { font-weight: 600; font-size: 0.9rem; }
  .check-detail { font-size: 0.8rem; color: #64748b; }
</style>
