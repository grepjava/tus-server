<script lang="ts">
  import { goto } from '$app/navigation';
  import { page } from '$app/state';
  import { onMount } from 'svelte';
  import { api } from '$lib/api';
  import { setSession } from '$lib/session.svelte';

  let username = $state('');
  let password = $state('');
  let error    = $state('');
  let loading  = $state(false);
  let oidcEnabled = $state(false);

  onMount(async () => {
    if (page.url.searchParams.get('sso_error') === '1') {
      error = 'SSO sign-in failed. Please try again or use your password.';
    }
    try {
      const res = await fetch('/api/auth/config');
      if (res.ok) {
        const cfg = await res.json();
        oidcEnabled = !!cfg.oidc;
      }
    } catch { /* ignore — SSO button simply won't appear */ }
  });

  async function submit(e: Event) {
    e.preventDefault();
    if (!username.trim() || !password) return;
    loading = true;
    error = '';
    try {
      const session = await api.login(username.trim(), password);
      setSession(session);
      goto('/');
    } catch {
      error = 'Invalid username or password';
    } finally {
      loading = false;
    }
  }
</script>

<div class="screen">
  <div class="card">
    <div class="hero">
      <img src="/tuskar.png" alt="Tuskar" class="logo" />
      <h1 class="brand">Tuskar</h1>
      <p class="tagline">File Transfer Server</p>
    </div>

    <form onsubmit={submit} class="form">
      {#if error}
        <div class="alert">{error}</div>
      {/if}

      <div class="field">
        <label for="username">Username</label>
        <input
          id="username"
          type="text"
          bind:value={username}
          autocomplete="username"
          placeholder="admin"
          disabled={loading}
        />
      </div>

      <div class="field">
        <label for="password">Password</label>
        <input
          id="password"
          type="password"
          bind:value={password}
          autocomplete="current-password"
          placeholder="••••••••"
          disabled={loading}
        />
      </div>

      <button type="submit" class="btn-submit" disabled={loading || !username || !password}>
        {loading ? 'Signing in…' : 'Sign in'}
      </button>
    </form>

    {#if oidcEnabled}
      <div class="sso-divider">
        <span>or</span>
      </div>
      <a href="/api/auth/oidc/login" class="btn-sso">
        <svg viewBox="0 0 20 20" fill="none" stroke="currentColor" stroke-width="1.5" width="18" height="18">
          <path d="M10 2a8 8 0 100 16A8 8 0 0010 2z"/>
          <path d="M10 2c-2.5 0-4.5 3.6-4.5 8s2 8 4.5 8m0-16c2.5 0 4.5 3.6 4.5 8s-2 8-4.5 8"/>
          <path d="M2.5 7.5h15M2.5 12.5h15"/>
        </svg>
        Sign in with SSO
      </a>
    {/if}
  </div>
</div>

<style>
  :global(body) { background: #0b0d12; }

  .screen {
    min-height: 100vh;
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 1.5rem;
    background:
      radial-gradient(ellipse 60% 40% at 50% 0%, rgba(249,115,22,0.08) 0%, transparent 70%),
      #0b0d12;
  }

  .card {
    width: 100%;
    max-width: 380px;
    background: #0f1117;
    border: 1px solid #1e2130;
    border-radius: 16px;
    padding: 2.5rem 2rem 2rem;
    display: flex;
    flex-direction: column;
    gap: 2rem;
    box-shadow: 0 25px 60px rgba(0,0,0,0.5);
  }

  .hero {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 0.625rem;
  }

  .logo {
    width: 100%;
    height: auto;
    object-fit: contain;
    border-radius: 16px;
    filter: drop-shadow(0 0 48px rgba(249,115,22,0.55));
  }

  h1.brand {
    font-size: 2rem;
    font-weight: 900;
    letter-spacing: -0.03em;
    color: #f97316;
    margin: 0;
  }

  .tagline {
    font-size: 0.8rem;
    color: #475569;
    letter-spacing: 0.05em;
    text-transform: uppercase;
  }

  .form {
    display: flex;
    flex-direction: column;
    gap: 1rem;
  }

  .alert {
    background: #3b1a1a;
    border: 1px solid #7f1d1d;
    color: #fca5a5;
    padding: 0.625rem 0.875rem;
    border-radius: 8px;
    font-size: 0.85rem;
  }

  .field {
    display: flex;
    flex-direction: column;
    gap: 0.375rem;
  }

  label {
    font-size: 0.78rem;
    font-weight: 500;
    color: #64748b;
    letter-spacing: 0.02em;
  }

  input {
    width: 100%;
    padding: 0.625rem 0.875rem;
    background: #0b0d12;
    border: 1px solid #2d3148;
    border-radius: 8px;
    color: #e2e8f0;
    font-size: 0.9rem;
    transition: border-color 0.15s;
    outline: none;
  }
  input:focus { border-color: #f97316; }
  input:disabled { opacity: 0.5; }

  .btn-submit {
    width: 100%;
    padding: 0.7rem;
    background: #f97316;
    color: #0b0d12;
    border: none;
    border-radius: 8px;
    font-size: 0.95rem;
    font-weight: 700;
    cursor: pointer;
    transition: background 0.15s, opacity 0.15s;
    margin-top: 0.25rem;
  }
  .btn-submit:hover:not(:disabled) { background: #fb923c; }
  .btn-submit:disabled { opacity: 0.45; cursor: default; }

  .sso-divider {
    display: flex;
    align-items: center;
    gap: 0.75rem;
    color: #334155;
    font-size: 0.75rem;
  }
  .sso-divider::before,
  .sso-divider::after {
    content: '';
    flex: 1;
    height: 1px;
    background: #1e2130;
  }

  .btn-sso {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 0.5rem;
    width: 100%;
    padding: 0.65rem;
    background: transparent;
    border: 1px solid #2d3148;
    border-radius: 8px;
    color: #94a3b8;
    font-size: 0.9rem;
    font-weight: 500;
    text-decoration: none;
    cursor: pointer;
    transition: border-color 0.15s, color 0.15s, background 0.15s;
  }
  .btn-sso:hover {
    border-color: #f97316;
    color: #f97316;
    background: rgba(249,115,22,0.05);
  }
</style>
