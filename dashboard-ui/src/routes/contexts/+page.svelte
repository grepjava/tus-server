<script lang="ts">
  import { onMount } from 'svelte';
  import { api } from '$lib/api';
  import type { Context } from '$lib/types';

  let contexts  = $state<Context[]>([]);
  let loading   = $state(true);
  let error     = $state<string | null>(null);

  // Create form
  let showCreate  = $state(false);
  let newSlug     = $state('');
  let newName     = $state('');
  let newMaxBytes = $state<string | number>('');
  let createError = $state('');
  let creating    = $state(false);
  let createdKey  = $state<{ slug: string; key: string } | null>(null);

  // Edit
  let editId      = $state<string | null>(null);
  let editName    = $state('');
  let editMaxBytes= $state<string | number>('');
  let editError   = $state('');
  let editSaving  = $state(false);

  // Key rotation
  let rotatingId  = $state<string | null>(null);
  let rotatedKey  = $state<{ id: string; key: string } | null>(null);

  async function load() {
    try {
      contexts = await api.listContexts();
      error = null;
    } catch (e) { error = String(e); }
    finally { loading = false; }
  }

  onMount(load);

  function parseMaxBytes(s: string | number): number | null {
    const v = String(s ?? '').trim();
    if (!v) return null;
    const n = Number(v);
    return isNaN(n) || n <= 0 ? null : n;
  }

  async function createCtx(e: Event) {
    e.preventDefault();
    createError = '';
    const slug = newSlug.trim().toLowerCase();
    if (!slug) { createError = 'Slug is required'; return; }
    if (!/^[a-z0-9_-]+$/.test(slug)) { createError = 'Slug may only contain letters, digits, - and _'; return; }
    if (!newName.trim()) { createError = 'Display name is required'; return; }
    creating = true;
    try {
      const result = await api.createContext(slug, newName.trim(), parseMaxBytes(newMaxBytes));
      contexts = [...contexts, { ...result }];
      showCreate = false;
      createdKey = { slug: result.slug, key: result.api_key };
      newSlug = ''; newName = ''; newMaxBytes = '';
    } catch (e) {
      createError = e instanceof Error ? e.message : 'Failed to create context';
    } finally { creating = false; }
  }

  function openEdit(ctx: Context) {
    editId = ctx.id;
    editName = ctx.display_name;
    editMaxBytes = ctx.max_upload_bytes != null ? String(ctx.max_upload_bytes) : '';
    editError = '';
  }

  async function saveEdit(e: Event) {
    e.preventDefault();
    if (!editId) return;
    editSaving = true; editError = '';
    try {
      const updated = await api.updateContext(editId, {
        display_name: editName.trim() || undefined,
        max_upload_bytes: parseMaxBytes(editMaxBytes),
      });
      contexts = contexts.map(c => c.id === editId ? updated : c);
      editId = null;
    } catch (e) {
      editError = e instanceof Error ? e.message : 'Failed to update';
    } finally { editSaving = false; }
  }

  async function removeCtx(ctx: Context) {
    if (!confirm(`Delete context "${ctx.slug}"?\n\nExisting uploads linked to it will remain but become context-less.`)) return;
    try {
      await api.deleteContext(ctx.id);
      contexts = contexts.filter(c => c.id !== ctx.id);
    } catch (e) { alert(String(e)); }
  }

  async function rotateKey(ctx: Context) {
    if (!confirm(`Rotate API key for "${ctx.slug}"?\n\nThe current key will stop working immediately.`)) return;
    rotatingId = ctx.id;
    try {
      const r = await api.rotateContextKey(ctx.id);
      rotatedKey = { id: ctx.id, key: r.api_key };
    } catch (e) { alert(String(e)); }
    finally { rotatingId = null; }
  }

  function fmt(n: number | null): string {
    if (n == null) return '—';
    if (n >= 1_073_741_824) return (n / 1_073_741_824).toFixed(1) + ' GB';
    if (n >= 1_048_576)     return (n / 1_048_576).toFixed(1) + ' MB';
    return (n / 1024).toFixed(0) + ' KB';
  }

  function reltime(s: string) {
    const d = Date.now() - new Date(s).getTime();
    if (d < 60_000) return `${Math.floor(d / 1000)}s ago`;
    if (d < 3_600_000) return `${Math.floor(d / 60_000)}m ago`;
    if (d < 86_400_000) return `${Math.floor(d / 3_600_000)}h ago`;
    return new Date(s).toLocaleDateString();
  }
</script>

<div class="page-header">
  <div>
    <h1>Contexts</h1>
    <p class="subtitle">Named upload namespaces — each gets its own URL prefix, API key and webhooks.</p>
  </div>
  <button class="btn-add" onclick={() => { showCreate = true; }}>+ New context</button>
</div>

{#if error}
  <div class="alert">{error}</div>
{:else if loading}
  <div class="empty">Loading…</div>
{:else if contexts.length === 0}
  <div class="empty-state">
    <p>No contexts yet.</p>
    <p class="muted">Create one to give an application its own <code>/{`{slug}`}/files</code> upload endpoint.</p>
  </div>
{:else}
  <div class="ctx-list">
    {#each contexts as ctx (ctx.id)}
      <div class="ctx-row">
        <div class="ctx-main">
          <div class="ctx-slug">/{ctx.slug}/files</div>
          <div class="ctx-name">{ctx.display_name}</div>
          <div class="ctx-meta">
            <span>quota: <strong>{fmt(ctx.max_upload_bytes)}</strong></span>
            <span>·</span>
            <span>created {reltime(ctx.created_at)}</span>
          </div>
        </div>
        <div class="ctx-actions">
          <button class="btn-sm" onclick={() => openEdit(ctx)}>Edit</button>
          <button class="btn-sm" disabled={rotatingId === ctx.id} onclick={() => rotateKey(ctx)}>
            {rotatingId === ctx.id ? 'Rotating…' : 'Rotate key'}
          </button>
          <button class="btn-sm danger" onclick={() => removeCtx(ctx)}>Delete</button>
        </div>
      </div>
    {/each}
  </div>
{/if}

<!-- New API key banner (shown after create) -->
{#if createdKey}
  <div class="key-banner">
    <div class="key-banner-header">
      <span>API key for <strong>/{createdKey.slug}/files</strong> — copy it now, it won't be shown again.</span>
      <button class="close-btn" onclick={() => createdKey = null}>✕</button>
    </div>
    <div class="key-value">{createdKey.key}</div>
    <button class="copy-btn" onclick={() => { navigator.clipboard.writeText(createdKey!.key); }}>Copy</button>
  </div>
{/if}

<!-- Rotated key banner -->
{#if rotatedKey}
  {@const ctx = contexts.find(c => c.id === rotatedKey?.id)}
  <div class="key-banner">
    <div class="key-banner-header">
      <span>New API key for <strong>/{ctx?.slug}/files</strong> — copy it now.</span>
      <button class="close-btn" onclick={() => rotatedKey = null}>✕</button>
    </div>
    <div class="key-value">{rotatedKey.key}</div>
    <button class="copy-btn" onclick={() => { navigator.clipboard.writeText(rotatedKey!.key); }}>Copy</button>
  </div>
{/if}

<!-- Create modal -->
{#if showCreate}
  <div class="backdrop" role="presentation" onclick={(e) => { if (e.target === e.currentTarget) showCreate = false; }}>
    <div class="modal">
      <div class="modal-header">
        <h2>New context</h2>
        <button class="close-btn" onclick={() => showCreate = false}>✕</button>
      </div>

      <form onsubmit={createCtx} class="modal-form">
        {#if createError}<div class="form-error">{createError}</div>{/if}

        <div class="field">
          <label for="ctx-slug">Slug</label>
          <input id="ctx-slug" type="text" bind:value={newSlug} placeholder="e.g. hr-system" autocomplete="off" />
          <span class="hint">Used in the URL: /{newSlug || '{slug}'}/files</span>
        </div>

        <div class="field">
          <label for="ctx-name">Display name</label>
          <input id="ctx-name" type="text" bind:value={newName} placeholder="e.g. HR System" autocomplete="off" />
        </div>

        <div class="field">
          <label for="ctx-quota">Per-context quota (bytes, optional)</label>
          <input id="ctx-quota" type="number" min="0" bind:value={newMaxBytes} placeholder="e.g. 10737418240 for 10 GB" />
        </div>

        <div class="modal-actions">
          <button type="button" class="btn" onclick={() => showCreate = false}>Cancel</button>
          <button type="submit" class="btn-primary" disabled={creating}>
            {creating ? 'Creating…' : 'Create'}
          </button>
        </div>
      </form>
    </div>
  </div>
{/if}

<!-- Edit modal -->
{#if editId}
  <div class="backdrop" role="presentation" onclick={(e) => { if (e.target === e.currentTarget) editId = null; }}>
    <div class="modal">
      <div class="modal-header">
        <h2>Edit context</h2>
        <button class="close-btn" onclick={() => editId = null}>✕</button>
      </div>

      <form onsubmit={saveEdit} class="modal-form">
        {#if editError}<div class="form-error">{editError}</div>{/if}

        <div class="field">
          <label for="edit-name">Display name</label>
          <input id="edit-name" type="text" bind:value={editName} />
        </div>

        <div class="field">
          <label for="edit-quota">Quota (bytes, blank = unlimited)</label>
          <input id="edit-quota" type="number" min="0" bind:value={editMaxBytes} placeholder="blank = unlimited" />
        </div>

        <div class="modal-actions">
          <button type="button" class="btn" onclick={() => editId = null}>Cancel</button>
          <button type="submit" class="btn-primary" disabled={editSaving}>
            {editSaving ? 'Saving…' : 'Save'}
          </button>
        </div>
      </form>
    </div>
  </div>
{/if}

<style>
  .page-header { display: flex; align-items: flex-start; justify-content: space-between; margin-bottom: 1.75rem; gap: 1rem; }
  h1 { font-size: 1.35rem; font-weight: 800; color: #f1f5f9; }
  .subtitle { font-size: 0.8rem; color: #475569; margin-top: 0.25rem; }

  .alert { background: #3b1a1a; border: 1px solid #7f1d1d; padding: 0.75rem 1rem; border-radius: 6px; font-size: 0.875rem; margin-bottom: 1rem; }
  .empty { color: #64748b; padding: 2rem 0; }
  .empty-state { padding: 3rem 0; text-align: center; color: #94a3b8; }
  .empty-state .muted { color: #475569; margin-top: 0.5rem; font-size: 0.85rem; }
  code { background: #1e2130; padding: 0.1em 0.4em; border-radius: 4px; font-size: 0.9em; }

  .ctx-list { display: flex; flex-direction: column; gap: 0.5rem; }

  .ctx-row {
    display: flex; align-items: center; justify-content: space-between; gap: 1rem;
    background: #0f1117; border: 1px solid #1e2130; border-radius: 10px;
    padding: 0.875rem 1.125rem; transition: border-color 0.15s; flex-wrap: wrap;
  }
  .ctx-row:hover { border-color: #2d3148; }

  .ctx-main { display: flex; flex-direction: column; gap: 0.2rem; }
  .ctx-slug { font-family: monospace; font-size: 0.92rem; color: #f97316; font-weight: 600; }
  .ctx-name { font-size: 0.88rem; color: #e2e8f0; font-weight: 500; }
  .ctx-meta { font-size: 0.75rem; color: #475569; display: flex; gap: 0.4rem; align-items: center; }

  .ctx-actions { display: flex; align-items: center; gap: 0.5rem; flex-wrap: wrap; }

  .btn-sm {
    padding: 0.3rem 0.75rem; font-size: 0.78rem; font-weight: 500;
    background: #1e2130; color: #94a3b8; border: 1px solid #2d3148;
    border-radius: 5px; cursor: pointer; transition: background 0.12s, color 0.12s;
  }
  .btn-sm:hover:not(:disabled) { background: #252a3d; color: #e2e8f0; }
  .btn-sm:disabled { opacity: 0.4; cursor: default; }
  .btn-sm.danger:hover { background: #3b1a1a; color: #f87171; border-color: #7f1d1d; }

  .btn-add {
    padding: 0.45rem 1rem; background: #f97316; color: #0b0d12;
    border: none; border-radius: 7px; font-weight: 700; font-size: 0.85rem;
    cursor: pointer; white-space: nowrap; flex-shrink: 0;
    transition: background 0.15s;
  }
  .btn-add:hover { background: #fb923c; }

  /* key banner */
  .key-banner {
    margin-top: 1.25rem; background: #0a1a0a; border: 1px solid #14532d;
    border-radius: 10px; padding: 1rem 1.25rem;
    display: flex; flex-direction: column; gap: 0.75rem;
  }
  .key-banner-header { display: flex; align-items: center; justify-content: space-between; font-size: 0.85rem; color: #86efac; }
  .key-value { font-family: monospace; font-size: 0.88rem; color: #4ade80; word-break: break-all; background: #0b150b; padding: 0.6rem 0.875rem; border-radius: 6px; }
  .copy-btn { align-self: flex-start; padding: 0.3rem 0.875rem; background: #14532d; color: #86efac; border: 1px solid #166534; border-radius: 5px; font-size: 0.78rem; cursor: pointer; }
  .copy-btn:hover { background: #166534; }

  /* modal */
  .backdrop { position: fixed; inset: 0; background: rgba(0,0,0,0.65); display: flex; align-items: center; justify-content: center; z-index: 50; padding: 1.5rem; }
  .modal { background: #0f1117; border: 1px solid #2d3148; border-radius: 12px; padding: 1.5rem; width: 100%; max-width: 420px; display: flex; flex-direction: column; gap: 1.25rem; }
  .modal-header { display: flex; align-items: center; justify-content: space-between; }
  h2 { font-size: 1rem; font-weight: 700; color: #e2e8f0; }
  .close-btn { background: none; border: none; color: #64748b; font-size: 1rem; cursor: pointer; padding: 0.2rem; }
  .close-btn:hover { color: #e2e8f0; }
  .modal-form { display: flex; flex-direction: column; gap: 1rem; }
  .form-error { background: #3b1a1a; border: 1px solid #7f1d1d; color: #fca5a5; padding: 0.5rem 0.75rem; border-radius: 6px; font-size: 0.82rem; }
  .field { display: flex; flex-direction: column; gap: 0.35rem; }
  .hint { font-size: 0.72rem; color: #475569; font-family: monospace; }
  label { font-size: 0.78rem; font-weight: 500; color: #64748b; }
  input {
    padding: 0.5rem 0.75rem; background: #0b0d12; border: 1px solid #2d3148;
    border-radius: 7px; color: #e2e8f0; font-size: 0.875rem; outline: none;
    transition: border-color 0.15s; width: 100%;
  }
  input:focus { border-color: #f97316; }
  .modal-actions { display: flex; gap: 0.5rem; justify-content: flex-end; }
  .btn { padding: 0.4rem 0.875rem; background: #1e2130; color: #e2e8f0; border: 1px solid #2d3148; border-radius: 6px; cursor: pointer; font-size: 0.85rem; }
  .btn:hover { background: #252a3d; }
  .btn-primary { padding: 0.4rem 0.875rem; background: #f97316; color: #0b0d12; border: none; border-radius: 6px; cursor: pointer; font-weight: 700; font-size: 0.85rem; }
  .btn-primary:hover:not(:disabled) { background: #fb923c; }
  .btn-primary:disabled { opacity: 0.4; cursor: default; }
</style>
