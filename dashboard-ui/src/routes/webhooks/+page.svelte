<script lang="ts">
  import { onMount } from 'svelte';
  import type { WebhookConfig, WebhookDelivery } from '$lib/types';
  import { api } from '$lib/api';

  const ALL_EVENTS = [
    { value: 'created',            label: 'Upload created' },
    { value: 'chunk_received',     label: 'Chunk received' },
    { value: 'completed',          label: 'Upload completed' },
    { value: 'processing_started', label: 'Processing started' },
    { value: 'finalized',          label: 'Finalized' },
    { value: 'processing_failed',  label: 'Processing failed' },
    { value: 'abandoned',          label: 'Abandoned' },
    { value: 'deleted',            label: 'Deleted' },
    { value: 'retry_queued',       label: 'Retry queued' },
  ];

  let webhooks       = $state<WebhookConfig[]>([]);
  let loading        = $state(true);
  let showForm       = $state(false);
  let editing        = $state<WebhookConfig | null>(null);

  let formName       = $state('');
  let formUrl        = $state('');
  let formSecret     = $state('');
  let formEvents     = $state<Set<string>>(new Set());
  let formEnabled    = $state(true);
  let formError      = $state('');
  let formSaving     = $state(false);

  let selectedId     = $state<string | null>(null);
  let deliveries     = $state<WebhookDelivery[]>([]);
  let loadingDel     = $state(false);

  const selectedWebhook = $derived(webhooks.find(w => w.id === selectedId) ?? null);

  async function load() {
    loading = true;
    try { webhooks = await api.listWebhooks(); }
    finally { loading = false; }
  }

  onMount(load);

  function openAdd() {
    editing = null;
    formName = ''; formUrl = ''; formSecret = '';
    formEvents = new Set(); formEnabled = true; formError = '';
    showForm = true;
    selectedId = null;
  }

  function openEdit(wh: WebhookConfig) {
    editing = wh;
    formName = wh.name; formUrl = wh.url; formSecret = wh.secret ?? '';
    formEvents = new Set(wh.events); formEnabled = wh.enabled; formError = '';
    showForm = true;
    selectedId = null;
  }

  function closeForm() { showForm = false; editing = null; }

  function toggleEvent(v: string) {
    const s = new Set(formEvents);
    s.has(v) ? s.delete(v) : s.add(v);
    formEvents = s;
  }

  async function save() {
    formError = '';
    if (!formName.trim())   { formError = 'Name is required'; return; }
    if (!formUrl.trim())    { formError = 'URL is required'; return; }
    try {
      const u = new URL(formUrl.trim());
      if (u.protocol !== 'http:' && u.protocol !== 'https:') throw new Error();
    } catch {
      formError = 'URL must start with http:// or https://';
      return;
    }
    if (!formEvents.size)   { formError = 'Select at least one event'; return; }
    formSaving = true;
    try {
      const body = {
        name: formName.trim(), url: formUrl.trim(),
        secret: formSecret.trim() || null,
        events: [...formEvents], enabled: formEnabled,
      };
      if (editing) {
        const updated = await api.updateWebhook(editing.id, body);
        webhooks = webhooks.map(w => w.id === updated.id ? updated : w);
      } else {
        const created = await api.createWebhook({ name: body.name, url: body.url, secret: body.secret, events: body.events });
        webhooks = [created, ...webhooks];
      }
      closeForm();
    } catch (e) {
      formError = e instanceof Error ? e.message : 'Save failed';
    } finally {
      formSaving = false;
    }
  }

  async function remove(wh: WebhookConfig) {
    if (!confirm(`Delete webhook "${wh.name}"?`)) return;
    await api.deleteWebhook(wh.id);
    webhooks = webhooks.filter(w => w.id !== wh.id);
    if (selectedId === wh.id) selectedId = null;
    if (editing?.id === wh.id) closeForm();
  }

  async function toggle(wh: WebhookConfig) {
    const updated = await api.updateWebhook(wh.id, {
      name: wh.name, url: wh.url, secret: wh.secret,
      events: wh.events, enabled: !wh.enabled,
    });
    webhooks = webhooks.map(w => w.id === updated.id ? updated : w);
  }

  async function openDeliveries(wh: WebhookConfig) {
    selectedId = wh.id;
    showForm = false;
    loadingDel = true;
    try { deliveries = await api.listDeliveries(wh.id); }
    finally { loadingDel = false; }
  }

  function statusClass(d: WebhookDelivery): string {
    if (d.error && !d.status_code) return 'err';
    if (d.status_code && d.status_code >= 200 && d.status_code < 300) return 'ok';
    return 'fail';
  }

  function fmtTime(iso: string) {
    return new Date(iso).toLocaleString();
  }
</script>

<div class="page">
  <div class="toolbar">
    <h1>Webhooks</h1>
    <button class="btn primary" onclick={openAdd}>+ Add webhook</button>
  </div>

  <div class="layout">
    <!-- list -->
    <div class="list-col">
      {#if loading}
        <p class="muted">Loading…</p>
      {:else if webhooks.length === 0}
        <div class="empty">
          <p>No webhooks configured.</p>
          <p class="muted">Add one to receive HTTP callbacks when uploads change state.</p>
        </div>
      {:else}
        {#each webhooks as wh (wh.id)}
          <div class="card" class:selected={selectedId === wh.id}>
            <div class="card-top">
              <div class="card-name">
                <span class="dot" class:on={wh.enabled}></span>
                {wh.name}
              </div>
              <div class="card-actions">
                <button class="icon-btn" title="Toggle" onclick={() => toggle(wh)}>
                  {wh.enabled ? '⏸' : '▶'}
                </button>
                <button class="icon-btn" title="Edit" onclick={() => openEdit(wh)}>✎</button>
                <button class="icon-btn danger" title="Delete" onclick={() => remove(wh)}>✕</button>
              </div>
            </div>
            <div class="card-url">{wh.url}</div>
            <div class="card-meta">
              <span class="pill">{wh.events.length} event{wh.events.length !== 1 ? 's' : ''}</span>
              {#if wh.secret}<span class="pill sec">secret set</span>{/if}
            </div>
            <button class="deliveries-btn" onclick={() => openDeliveries(wh)}>
              View delivery log →
            </button>
          </div>
        {/each}
      {/if}
    </div>

    <!-- right panel: form or delivery log -->
    <div class="detail-col">
      {#if showForm}
        <div class="panel">
          <div class="panel-header">
            <h2>{editing ? 'Edit webhook' : 'Add webhook'}</h2>
            <button class="icon-btn" onclick={closeForm}>✕</button>
          </div>

          <label>
            Name
            <input bind:value={formName} placeholder="e.g. Notify my service" />
          </label>

          <label>
            URL
            <input bind:value={formUrl} placeholder="https://example.com/hooks/tus" type="url" />
          </label>

          <label>
            Secret <span class="muted">(optional — sent as X-Webhook-Secret)</span>
            <input bind:value={formSecret} placeholder="leave blank for none" type="password" />
          </label>

          <fieldset>
            <legend>Trigger on events</legend>
            <div class="events-grid">
              {#each ALL_EVENTS as ev}
                <label class="check-label">
                  <input type="checkbox" checked={formEvents.has(ev.value)} onchange={() => toggleEvent(ev.value)} />
                  {ev.label}
                </label>
              {/each}
            </div>
          </fieldset>

          {#if editing}
            <label class="check-label toggle-row">
              <input type="checkbox" bind:checked={formEnabled} />
              Enabled
            </label>
          {/if}

          {#if formError}
            <p class="form-error">{formError}</p>
          {/if}

          <div class="form-actions">
            <button class="btn" onclick={closeForm}>Cancel</button>
            <button class="btn primary" onclick={save} disabled={formSaving}>
              {formSaving ? 'Saving…' : (editing ? 'Save changes' : 'Create webhook')}
            </button>
          </div>
        </div>

      {:else if selectedId && selectedWebhook}
        <div class="panel">
          <div class="panel-header">
            <h2>Delivery log — {selectedWebhook.name}</h2>
            <button class="icon-btn" onclick={() => { selectedId = null; }}>✕</button>
          </div>

          {#if loadingDel}
            <p class="muted">Loading…</p>
          {:else if deliveries.length === 0}
            <p class="muted">No deliveries recorded yet.</p>
          {:else}
            <div class="delivery-list">
              {#each deliveries as d (d.id)}
                <div class="delivery {statusClass(d)}">
                  <div class="del-top">
                    <span class="del-event">{d.event_type}</span>
                    <span class="del-status">
                      {#if d.status_code}HTTP {d.status_code}{:else}—{/if}
                    </span>
                    <span class="del-time">{fmtTime(d.delivered_at)}</span>
                  </div>
                  {#if d.upload_id}
                    <div class="del-detail muted">upload: <a href="/uploads/{d.upload_id}">{d.upload_id.slice(0, 8)}…</a></div>
                  {/if}
                  {#if d.attempts > 1}
                    <div class="del-detail muted">{d.attempts} attempts</div>
                  {/if}
                  {#if d.error}
                    <div class="del-detail err-text">{d.error}</div>
                  {/if}
                </div>
              {/each}
            </div>
          {/if}
        </div>

      {:else}
        <div class="placeholder">
          <p>Select a webhook to view deliveries, or add a new one.</p>
        </div>
      {/if}
    </div>
  </div>
</div>

<style>
  .page { display: flex; flex-direction: column; gap: 1rem; }

  .toolbar {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }
  h1 { font-size: 1.25rem; font-weight: 600; }

  .layout { display: grid; grid-template-columns: 1fr 1.4fr; gap: 1.5rem; align-items: start; }
  @media (max-width: 700px) { .layout { grid-template-columns: 1fr; } }

  /* cards */
  .list-col { display: flex; flex-direction: column; gap: 0.75rem; }

  .card {
    background: #1e2130;
    border: 1px solid #2d3148;
    border-radius: 8px;
    padding: 0.875rem 1rem;
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
    transition: border-color 0.15s;
  }
  .card.selected { border-color: #60a5fa; }

  .card-top { display: flex; align-items: center; justify-content: space-between; }
  .card-name { display: flex; align-items: center; gap: 0.5rem; font-weight: 600; }

  .dot {
    width: 8px; height: 8px; border-radius: 50%;
    background: #475569; flex-shrink: 0;
  }
  .dot.on { background: #22c55e; }

  .card-url { font-size: 0.8rem; color: #94a3b8; word-break: break-all; }
  .card-meta { display: flex; gap: 0.4rem; flex-wrap: wrap; }

  .pill {
    font-size: 0.75rem;
    background: #2d3148;
    color: #94a3b8;
    border-radius: 999px;
    padding: 0.15rem 0.5rem;
  }
  .pill.sec { background: #1e3a2f; color: #4ade80; }

  .deliveries-btn {
    background: none; border: none; color: #60a5fa; font-size: 0.8rem;
    cursor: pointer; padding: 0; text-align: left;
  }
  .deliveries-btn:hover { text-decoration: underline; }

  .card-actions { display: flex; gap: 0.25rem; }

  /* right panel */
  .detail-col { min-width: 0; }

  .placeholder {
    background: #1e2130;
    border: 1px dashed #2d3148;
    border-radius: 8px;
    padding: 2rem;
    text-align: center;
    color: #64748b;
  }

  .panel {
    background: #1e2130;
    border: 1px solid #2d3148;
    border-radius: 8px;
    padding: 1.25rem;
    display: flex;
    flex-direction: column;
    gap: 1rem;
  }
  .panel-header {
    display: flex; align-items: center; justify-content: space-between;
  }
  h2 { font-size: 1rem; font-weight: 600; }

  label {
    display: flex; flex-direction: column; gap: 0.35rem;
    font-size: 0.85rem; color: #94a3b8;
  }
  input[type="url"], input[type="password"], input:not([type="checkbox"]) {
    background: #0f1117;
    border: 1px solid #2d3148;
    border-radius: 6px;
    color: #e2e8f0;
    padding: 0.5rem 0.75rem;
    font-size: 0.875rem;
    outline: none;
    width: 100%;
  }
  input:focus { border-color: #60a5fa; }

  fieldset {
    border: 1px solid #2d3148;
    border-radius: 6px;
    padding: 0.75rem;
  }
  legend { font-size: 0.8rem; color: #94a3b8; padding: 0 0.25rem; }

  .events-grid {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 0.5rem;
    margin-top: 0.5rem;
  }

  .check-label {
    display: flex; align-items: center; gap: 0.5rem;
    font-size: 0.825rem; color: #cbd5e1; cursor: pointer;
    flex-direction: row;
  }
  .check-label input[type="checkbox"] { width: 14px; height: 14px; accent-color: #60a5fa; }

  .toggle-row { flex-direction: row; color: #cbd5e1; }

  .form-error { color: #f87171; font-size: 0.825rem; }
  .form-actions { display: flex; gap: 0.5rem; justify-content: flex-end; }

  /* deliveries */
  .delivery-list { display: flex; flex-direction: column; gap: 0.5rem; max-height: 520px; overflow-y: auto; }

  .delivery {
    background: #0f1117;
    border-left: 3px solid #2d3148;
    border-radius: 4px;
    padding: 0.6rem 0.75rem;
    display: flex;
    flex-direction: column;
    gap: 0.25rem;
    font-size: 0.8rem;
  }
  .delivery.ok   { border-left-color: #22c55e; }
  .delivery.fail { border-left-color: #f59e0b; }
  .delivery.err  { border-left-color: #ef4444; }

  .del-top { display: flex; align-items: center; gap: 0.75rem; }
  .del-event { font-weight: 600; color: #e2e8f0; }
  .del-status { color: #94a3b8; }
  .del-time { margin-left: auto; color: #64748b; }
  .del-detail { color: #64748b; font-size: 0.775rem; }
  .err-text { color: #f87171; }

  /* shared */
  .empty { color: #64748b; display: flex; flex-direction: column; gap: 0.25rem; }
  .muted { color: #64748b; font-size: 0.875rem; }

  .btn {
    padding: 0.4rem 1rem;
    border-radius: 6px;
    border: 1px solid #2d3148;
    background: #1e2130;
    color: #e2e8f0;
    cursor: pointer;
    font-size: 0.875rem;
    transition: background 0.15s;
  }
  .btn:hover { background: #2d3148; }
  .btn.primary { background: #1d4ed8; border-color: #1d4ed8; }
  .btn.primary:hover { background: #2563eb; }
  .btn:disabled { opacity: 0.5; cursor: not-allowed; }

  .icon-btn {
    background: none; border: none; cursor: pointer;
    color: #64748b; font-size: 0.95rem; padding: 0.2rem 0.35rem;
    border-radius: 4px; transition: background 0.1s, color 0.1s;
  }
  .icon-btn:hover { background: #2d3148; color: #e2e8f0; }
  .icon-btn.danger:hover { background: #450a0a; color: #f87171; }
</style>
