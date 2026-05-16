<script lang="ts">
  import { onMount } from 'svelte';
  import { api } from '$lib/api';
  import { getSession } from '$lib/session.svelte';
  import type { User } from '$lib/types';

  let users    = $state<User[]>([]);
  let loading  = $state(true);
  let error    = $state<string | null>(null);

  // Add user form
  let showAdd  = $state(false);
  let newName  = $state('');
  let newPass  = $state('');
  let newRole  = $state<'admin' | 'viewer'>('viewer');
  let addError = $state('');
  let adding   = $state(false);

  // Change password
  let changingId   = $state<string | null>(null);
  let newPw        = $state('');
  let currentPw    = $state('');
  let pwError      = $state('');
  let pwSaving     = $state(false);

  let session = $derived(getSession());

  async function load() {
    try {
      users = await api.listUsers();
      error = null;
    } catch (e) { error = String(e); }
    finally { loading = false; }
  }

  onMount(load);

  async function addUser(e: Event) {
    e.preventDefault();
    addError = '';
    if (!newName.trim()) { addError = 'Username is required'; return; }
    if (newPass.length < 6) { addError = 'Password must be at least 6 characters'; return; }
    adding = true;
    try {
      const u = await api.createUser(newName.trim(), newPass, newRole);
      users = [...users, u];
      showAdd = false;
      newName = ''; newPass = ''; newRole = 'viewer';
    } catch (e) {
      addError = e instanceof Error ? e.message : 'Failed to create user';
    } finally { adding = false; }
  }

  async function removeUser(u: User) {
    if (!confirm(`Delete user "${u.username}"? This cannot be undone.`)) return;
    try {
      await api.deleteUser(u.id);
      users = users.filter(x => x.id !== u.id);
    } catch (e) { alert(String(e)); }
  }

  function openChangePw(id: string) {
    changingId = id;
    newPw = ''; currentPw = ''; pwError = '';
  }

  async function savePassword(e: Event) {
    e.preventDefault();
    if (!changingId) return;
    if (newPw.length < 6) { pwError = 'Password must be at least 6 characters'; return; }
    pwSaving = true; pwError = '';
    const isSelf = changingId === session?.id;
    try {
      await api.changePassword(changingId, newPw, isSelf ? currentPw : undefined);
      changingId = null;
    } catch (e) {
      pwError = e instanceof Error ? e.message : 'Failed to change password';
    } finally { pwSaving = false; }
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
    <h1>Users</h1>
    <p class="subtitle">Manage who can access the Tuskar console.</p>
  </div>
  <button class="btn-add" onclick={() => { showAdd = true; }}>+ Add user</button>
</div>

{#if error}
  <div class="alert">{error}</div>
{:else if loading}
  <div class="empty">Loading…</div>
{:else}
  <div class="user-list">
    {#each users as u (u.id)}
      <div class="user-row" class:is-self={u.id === session?.id}>
        <div class="user-info">
          <div class="avatar">{u.username[0].toUpperCase()}</div>
          <div>
            <div class="username">
              {u.username}
              {#if u.id === session?.id}<span class="you">you</span>{/if}
            </div>
            <div class="meta">Created {reltime(u.created_at)}</div>
          </div>
        </div>

        <div class="user-actions">
          <span class="role-badge" class:admin={u.role === 'admin'}>{u.role}</span>
          <button class="btn-sm" onclick={() => openChangePw(u.id)}>Change password</button>
          {#if u.id !== session?.id}
            <button class="btn-sm danger" onclick={() => removeUser(u)}>Delete</button>
          {/if}
        </div>
      </div>
    {/each}
  </div>
{/if}

<!-- Add user modal -->
{#if showAdd}
  <div class="backdrop" role="presentation" onclick={(e) => { if (e.target === e.currentTarget) showAdd = false; }}>
    <div class="modal">
      <div class="modal-header">
        <h2>Add user</h2>
        <button class="close-btn" onclick={() => showAdd = false}>✕</button>
      </div>

      <form onsubmit={addUser} class="modal-form">
        {#if addError}<div class="form-error">{addError}</div>{/if}

        <div class="field">
          <label for="new-name">Username</label>
          <input id="new-name" type="text" bind:value={newName} autocomplete="off" placeholder="e.g. alice" />
        </div>

        <div class="field">
          <label for="new-pass">Password</label>
          <input id="new-pass" type="password" bind:value={newPass} autocomplete="new-password" placeholder="min 6 characters" />
        </div>

        <div class="field">
          <label for="new-role">Role</label>
          <select id="new-role" bind:value={newRole} class="select">
            <option value="viewer">Viewer — read-only access</option>
            <option value="admin">Admin — full access</option>
          </select>
        </div>

        <div class="modal-actions">
          <button type="button" class="btn" onclick={() => showAdd = false}>Cancel</button>
          <button type="submit" class="btn-primary" disabled={adding}>
            {adding ? 'Creating…' : 'Create user'}
          </button>
        </div>
      </form>
    </div>
  </div>
{/if}

<!-- Change password modal -->
{#if changingId}
  {@const target = users.find(u => u.id === changingId)}
  {@const isSelf = changingId === session?.id}
  <div class="backdrop" role="presentation" onclick={(e) => { if (e.target === e.currentTarget) changingId = null; }}>
    <div class="modal">
      <div class="modal-header">
        <h2>Change password — {target?.username}</h2>
        <button class="close-btn" onclick={() => changingId = null}>✕</button>
      </div>

      <form onsubmit={savePassword} class="modal-form">
        {#if pwError}<div class="form-error">{pwError}</div>{/if}

        {#if isSelf}
          <div class="field">
            <label for="cur-pw">Current password</label>
            <input id="cur-pw" type="password" bind:value={currentPw} autocomplete="current-password" />
          </div>
        {/if}

        <div class="field">
          <label for="new-pw">New password</label>
          <input id="new-pw" type="password" bind:value={newPw} autocomplete="new-password" placeholder="min 6 characters" />
        </div>

        <div class="modal-actions">
          <button type="button" class="btn" onclick={() => changingId = null}>Cancel</button>
          <button type="submit" class="btn-primary" disabled={pwSaving}>
            {pwSaving ? 'Saving…' : 'Update password'}
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

  .user-list { display: flex; flex-direction: column; gap: 0.5rem; }

  .user-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 1rem;
    background: #0f1117;
    border: 1px solid #1e2130;
    border-radius: 10px;
    padding: 0.875rem 1.125rem;
    transition: border-color 0.15s;
    flex-wrap: wrap;
  }
  .user-row:hover { border-color: #2d3148; }
  .user-row.is-self { border-color: #2d3148; }

  .user-info { display: flex; align-items: center; gap: 0.875rem; }

  .avatar {
    width: 38px; height: 38px; border-radius: 50%;
    background: linear-gradient(135deg, #f97316, #ea580c);
    display: flex; align-items: center; justify-content: center;
    font-weight: 800; font-size: 1rem; color: #0b0d12;
    flex-shrink: 0;
  }

  .username { font-weight: 600; font-size: 0.9rem; color: #e2e8f0; display: flex; align-items: center; gap: 0.4rem; }
  .you { font-size: 0.62rem; font-weight: 600; text-transform: uppercase; letter-spacing: 0.04em; background: #1e2130; color: #475569; padding: 0.1rem 0.35rem; border-radius: 3px; }
  .meta { font-size: 0.75rem; color: #475569; margin-top: 0.1rem; }

  .user-actions { display: flex; align-items: center; gap: 0.5rem; flex-wrap: wrap; }

  .role-badge {
    font-size: 0.65rem; font-weight: 700; text-transform: uppercase; letter-spacing: 0.05em;
    padding: 0.15rem 0.45rem; border-radius: 4px;
    background: #1e2130; color: #475569; border: 1px solid #2d3148;
  }
  .role-badge.admin { background: #1e2a3a; color: #60a5fa; border-color: #1d4ed8; }

  .btn-sm {
    padding: 0.3rem 0.75rem; font-size: 0.78rem; font-weight: 500;
    background: #1e2130; color: #94a3b8; border: 1px solid #2d3148;
    border-radius: 5px; cursor: pointer; transition: background 0.12s, color 0.12s;
  }
  .btn-sm:hover { background: #252a3d; color: #e2e8f0; }
  .btn-sm.danger:hover { background: #3b1a1a; color: #f87171; border-color: #7f1d1d; }

  .btn-add {
    padding: 0.45rem 1rem; background: #f97316; color: #0b0d12;
    border: none; border-radius: 7px; font-weight: 700; font-size: 0.85rem;
    cursor: pointer; white-space: nowrap; flex-shrink: 0;
    transition: background 0.15s;
  }
  .btn-add:hover { background: #fb923c; }

  /* modal */
  .backdrop {
    position: fixed; inset: 0; background: rgba(0,0,0,0.65);
    display: flex; align-items: center; justify-content: center;
    z-index: 50; padding: 1.5rem;
  }

  .modal {
    background: #0f1117; border: 1px solid #2d3148; border-radius: 12px;
    padding: 1.5rem; width: 100%; max-width: 400px;
    display: flex; flex-direction: column; gap: 1.25rem;
  }

  .modal-header { display: flex; align-items: center; justify-content: space-between; }
  h2 { font-size: 1rem; font-weight: 700; color: #e2e8f0; }
  .close-btn { background: none; border: none; color: #64748b; font-size: 1rem; cursor: pointer; padding: 0.2rem; }
  .close-btn:hover { color: #e2e8f0; }

  .modal-form { display: flex; flex-direction: column; gap: 1rem; }
  .form-error { background: #3b1a1a; border: 1px solid #7f1d1d; color: #fca5a5; padding: 0.5rem 0.75rem; border-radius: 6px; font-size: 0.82rem; }

  .field { display: flex; flex-direction: column; gap: 0.35rem; }
  label { font-size: 0.78rem; font-weight: 500; color: #64748b; }
  input, .select {
    padding: 0.5rem 0.75rem; background: #0b0d12; border: 1px solid #2d3148;
    border-radius: 7px; color: #e2e8f0; font-size: 0.875rem; outline: none;
    transition: border-color 0.15s; width: 100%;
  }
  input:focus, .select:focus { border-color: #f97316; }

  .modal-actions { display: flex; gap: 0.5rem; justify-content: flex-end; }
  .btn { padding: 0.4rem 0.875rem; background: #1e2130; color: #e2e8f0; border: 1px solid #2d3148; border-radius: 6px; cursor: pointer; font-size: 0.85rem; }
  .btn:hover { background: #252a3d; }
  .btn-primary { padding: 0.4rem 0.875rem; background: #f97316; color: #0b0d12; border: none; border-radius: 6px; cursor: pointer; font-weight: 700; font-size: 0.85rem; }
  .btn-primary:hover:not(:disabled) { background: #fb923c; }
  .btn-primary:disabled { opacity: 0.4; cursor: default; }
</style>
