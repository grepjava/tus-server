<script lang="ts">
  import { onMount } from 'svelte';
  import { api } from '$lib/api';
  import type { SettingEntry } from '$lib/types';

  let settings   = $state<SettingEntry[]>([]);
  let loading    = $state(true);
  let error      = $state<string | null>(null);
  let saving     = $state<Record<string, boolean>>({});
  let edited     = $state<Record<string, string>>({});
  let saveError  = $state<Record<string, string>>({});
  let saved      = $state<Record<string, boolean>>({});

  async function load() {
    try {
      settings = await api.listSettings();
      // seed edit buffer with current values
      for (const s of settings) {
        if (!(s.key in edited)) edited[s.key] = s.value;
      }
      error = null;
    } catch (e) { error = String(e); }
    finally { loading = false; }
  }

  onMount(() => load());

  async function save(key: string) {
    saving[key] = true;
    saveError[key] = '';
    saved[key] = false;
    try {
      const updated = await api.updateSetting(key, edited[key]);
      // update in list
      const idx = settings.findIndex(s => s.key === key);
      if (idx >= 0) settings[idx] = updated;
      edited[key] = updated.value;
      saved[key] = true;
      setTimeout(() => { saved[key] = false; }, 2500);
    } catch (e) { saveError[key] = String(e); }
    finally { saving[key] = false; }
  }

  async function revert(key: string) {
    saving[key] = true;
    saveError[key] = '';
    saved[key] = false;
    try {
      await api.deleteSetting(key);
      const updated = await api.listSettings();
      const entry = updated.find(s => s.key === key);
      if (entry) {
        const idx = settings.findIndex(s => s.key === key);
        if (idx >= 0) settings[idx] = entry;
        edited[key] = entry.value;
      }
      saved[key] = true;
      setTimeout(() => { saved[key] = false; }, 2500);
    } catch (e) { saveError[key] = String(e); }
    finally { saving[key] = false; }
  }

  function isDirty(s: SettingEntry) {
    return (edited[s.key] ?? s.value) !== s.value;
  }

  let grouped = $derived.by(() => {
    const map = new Map<string, SettingEntry[]>();
    for (const s of settings) {
      const list = map.get(s.category) ?? [];
      list.push(s);
      map.set(s.category, list);
    }
    return [...map.entries()];
  });

  function sourceLabel(src: string) {
    if (src === 'db')      return { text: 'DB override', cls: 'src-db' };
    if (src === 'env')     return { text: 'From env', cls: 'src-env' };
    return { text: 'Default', cls: 'src-default' };
  }
</script>

<div class="page-header">
  <div>
    <h1>Settings</h1>
    <p class="subtitle">Configure Tuskar at runtime. DB overrides take precedence over environment variables.</p>
  </div>
  <button class="btn" onclick={() => load()} disabled={loading}>↻ Refresh</button>
</div>

{#if error}
  <div class="alert">{error}</div>
{:else if loading}
  <div class="empty">Loading settings…</div>
{:else}
  <div class="settings-layout">
    {#each grouped as [category, entries]}
      <section class="category">
        <h2>{category}</h2>
        <div class="entries">
          {#each entries as s (s.key)}
            {@const src = sourceLabel(s.source)}
            {@const dirty = isDirty(s)}
            <div class="entry" class:dirty>
              <div class="entry-meta">
                <div class="entry-head">
                  <span class="entry-label">{s.label}</span>
                  <span class="badge {src.cls}">{src.text}</span>
                  {#if s.restart_required}
                    <span class="badge restart">restart</span>
                  {/if}
                </div>
                <p class="entry-desc">{s.description}</p>
                <code class="entry-key">{s.key}</code>
              </div>

              <div class="entry-control">
                {#if s.input_type === 'boolean'}
                  <div class="toggle-wrap">
                    <label class="toggle">
                      <input
                        type="checkbox"
                        checked={edited[s.key] === 'true'}
                        onchange={(e) => { edited[s.key] = (e.currentTarget as HTMLInputElement).checked ? 'true' : 'false'; }}
                      />
                      <span class="slider"></span>
                    </label>
                    <span class="toggle-val">{edited[s.key] === 'true' ? 'Enabled' : 'Disabled'}</span>
                  </div>
                {:else if s.input_type === 'select' && s.options}
                  <select
                    bind:value={edited[s.key]}
                    class="select"
                  >
                    {#each s.options as opt}
                      <option value={opt}>{opt}</option>
                    {/each}
                  </select>
                {:else if s.input_type === 'password'}
                  <input
                    type="password"
                    class="input"
                    placeholder="(hidden)"
                    bind:value={edited[s.key]}
                  />
                {:else if s.input_type === 'number'}
                  <input
                    type="number"
                    class="input"
                    bind:value={edited[s.key]}
                  />
                {:else}
                  <input
                    type="text"
                    class="input"
                    bind:value={edited[s.key]}
                  />
                {/if}

                <div class="entry-actions">
                  <button
                    class="btn-save"
                    class:saved={saved[s.key]}
                    disabled={saving[s.key] || (!dirty && !saved[s.key])}
                    onclick={() => save(s.key)}
                  >
                    {saving[s.key] ? '…' : saved[s.key] ? '✓ Saved' : 'Save'}
                  </button>
                  {#if s.source === 'db'}
                    <button
                      class="btn-revert"
                      disabled={saving[s.key]}
                      onclick={() => revert(s.key)}
                      title="Remove DB override"
                    >
                      Revert
                    </button>
                  {/if}
                </div>

                {#if saveError[s.key]}
                  <div class="save-error">{saveError[s.key]}</div>
                {/if}
              </div>
            </div>
          {/each}
        </div>
      </section>
    {/each}
  </div>
{/if}

<style>
  .page-header {
    display: flex; align-items: flex-start; justify-content: space-between;
    margin-bottom: 1.75rem; gap: 1rem;
  }
  h1 { font-size: 1.35rem; font-weight: 800; color: #f1f5f9; }
  .subtitle { font-size: 0.8rem; color: #475569; margin-top: 0.25rem; }
  .btn {
    padding: 0.375rem 0.875rem; background: #1e2130; color: #e2e8f0;
    border: 1px solid #2d3148; border-radius: 6px; cursor: pointer; font-size: 0.8rem;
    white-space: nowrap; flex-shrink: 0;
  }
  .btn:hover { background: #252a3d; }
  .btn:disabled { opacity: 0.5; cursor: default; }
  .alert { background: #3b1a1a; border: 1px solid #7f1d1d; padding: 0.75rem 1rem; border-radius: 6px; font-size: 0.875rem; margin-bottom: 1rem; }
  .empty { color: #64748b; padding: 3rem 0; }

  /* layout */
  .settings-layout { display: flex; flex-direction: column; gap: 2rem; }

  section.category {}
  h2 {
    font-size: 0.72rem; font-weight: 600; text-transform: uppercase;
    letter-spacing: 0.08em; color: #475569;
    padding-bottom: 0.5rem; margin-bottom: 0.75rem;
    border-bottom: 1px solid #1e2130;
  }

  .entries { display: flex; flex-direction: column; gap: 0; }

  .entry {
    display: grid;
    grid-template-columns: 1fr auto;
    gap: 1.5rem;
    align-items: start;
    padding: 0.875rem 1rem;
    border-radius: 8px;
    border: 1px solid transparent;
    transition: border-color 0.15s;
  }
  .entry:hover { border-color: #1e2130; background: #0f1117; }
  .entry.dirty { border-color: #f97316; background: #120d07; }

  .entry-meta { min-width: 0; }
  .entry-head { display: flex; align-items: center; gap: 0.4rem; flex-wrap: wrap; margin-bottom: 0.3rem; }
  .entry-label { font-size: 0.875rem; font-weight: 600; color: #e2e8f0; }
  .entry-desc { font-size: 0.775rem; color: #64748b; line-height: 1.4; margin-bottom: 0.35rem; }
  .entry-key { font-size: 0.68rem; font-family: monospace; color: #334155; }

  /* badges */
  .badge {
    font-size: 0.62rem; font-weight: 600; text-transform: uppercase; letter-spacing: 0.04em;
    padding: 0.1rem 0.35rem; border-radius: 3px;
  }
  .src-db      { background: #1e3a2a; color: #4ade80; border: 1px solid #166534; }
  .src-env     { background: #1e2a4a; color: #60a5fa; border: 1px solid #1d4ed8; }
  .src-default { background: #1e2130; color: #475569; border: 1px solid #2d3148; }
  .restart     { background: #3a1f0f; color: #fb923c; border: 1px solid #9a3412; }

  /* controls */
  .entry-control { display: flex; flex-direction: column; gap: 0.5rem; align-items: flex-end; min-width: 220px; }

  .input, .select {
    width: 100%;
    padding: 0.4rem 0.625rem;
    background: #0b0d12;
    border: 1px solid #2d3148;
    border-radius: 6px;
    color: #e2e8f0;
    font-size: 0.8rem;
    font-family: monospace;
    transition: border-color 0.15s;
  }
  .input:focus, .select:focus { outline: none; border-color: #f97316; }

  /* toggle */
  .toggle-wrap { display: flex; align-items: center; gap: 0.5rem; justify-content: flex-end; width: 100%; }
  .toggle-val { font-size: 0.78rem; color: #94a3b8; min-width: 3rem; }
  .toggle { position: relative; display: inline-block; width: 40px; height: 22px; }
  .toggle input { opacity: 0; width: 0; height: 0; }
  .slider {
    position: absolute; cursor: pointer; inset: 0;
    background: #2d3148; border-radius: 22px; transition: background 0.2s;
  }
  .slider::before {
    content: ''; position: absolute;
    width: 16px; height: 16px; left: 3px; bottom: 3px;
    background: #64748b; border-radius: 50%; transition: transform 0.2s, background 0.2s;
  }
  .toggle input:checked + .slider { background: #f97316; }
  .toggle input:checked + .slider::before { transform: translateX(18px); background: #fff; }

  /* action buttons */
  .entry-actions { display: flex; gap: 0.375rem; }

  .btn-save {
    padding: 0.3rem 0.75rem;
    background: #f97316; color: #0b0d12;
    border: none; border-radius: 5px;
    cursor: pointer; font-size: 0.78rem; font-weight: 700;
    transition: background 0.15s, opacity 0.15s;
  }
  .btn-save:hover:not(:disabled) { background: #fb923c; }
  .btn-save:disabled { opacity: 0.4; cursor: default; background: #2d3148; color: #64748b; }
  .btn-save.saved { background: #166534; color: #4ade80; }

  .btn-revert {
    padding: 0.3rem 0.75rem;
    background: transparent; color: #64748b;
    border: 1px solid #2d3148; border-radius: 5px;
    cursor: pointer; font-size: 0.78rem;
    transition: background 0.15s, color 0.15s;
  }
  .btn-revert:hover:not(:disabled) { background: #1e2130; color: #94a3b8; }
  .btn-revert:disabled { opacity: 0.4; cursor: default; }

  .save-error { font-size: 0.72rem; color: #f87171; text-align: right; }

  @media (max-width: 700px) {
    .entry { grid-template-columns: 1fr; }
    .entry-control { align-items: stretch; min-width: 0; }
    .entry-actions { justify-content: flex-end; }
  }
</style>
