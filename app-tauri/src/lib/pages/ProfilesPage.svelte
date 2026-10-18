<script lang="ts">
  import { appState, refreshAppSnapshot } from '../state';
  import { importLegacyConfig, saveConfig } from '../api';

  let feedback = '';
  let busy = false;

  async function save() {
    busy = true;
    feedback = '';
    try {
      if (!(await saveConfig())) {
        feedback = 'Save failed';
        return;
      }
      await refreshAppSnapshot();
      feedback = 'Configuration saved';
    } catch (error) {
      feedback = `Save failed: ${error instanceof Error ? error.message : String(error)}`;
    } finally {
      busy = false;
    }
  }

  async function importLegacy() {
    busy = true;
    feedback = '';
    try {
      await importLegacyConfig();
      await refreshAppSnapshot();
      feedback = 'Legacy configuration loaded. Save to keep these changes.';
    } catch (error) {
      feedback = `Import failed: ${error instanceof Error ? error.message : String(error)}`;
    } finally {
      busy = false;
    }
  }
</script>

<section aria-labelledby="profiles-title">
  <h1 id="profiles-title">Profiles</h1>
  <div class="profile-card">
    <h2>Current configuration</h2>
    <dl>
      <div>
        <dt>Config path</dt>
        <dd><code>{$appState.snapshot?.configPath ?? 'Loading…'}</code></dd>
      </div>
      <div>
        <dt>Status</dt>
        <dd>{$appState.snapshot?.dirty ? 'Unsaved changes' : 'All changes saved'}</dd>
      </div>
    </dl>
    <div class="actions">
      <button onclick={save} disabled={busy || !$appState.snapshot}>Save</button>
      <button onclick={importLegacy} disabled={busy}>Import legacy config…</button>
    </div>
    {#if feedback}
      <p role="status">{feedback}</p>
    {/if}
  </div>
</section>

<style>
  .profile-card {
    max-width: 44rem;
    padding: var(--space-5);
    border: 1px solid var(--border);
    border-radius: var(--radius-lg);
    background: var(--surface-raised);
  }

  h2 {
    margin-top: 0;
    font-size: 1.05rem;
  }

  dl {
    display: grid;
    gap: var(--space-3);
  }

  dl > div {
    display: grid;
    grid-template-columns: 9rem minmax(0, 1fr);
    gap: var(--space-3);
  }

  dt {
    color: var(--text-muted);
  }

  dd {
    min-width: 0;
    margin: 0;
    overflow-wrap: anywhere;
  }

  code {
    color: var(--accent);
  }

  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
    margin-top: var(--space-5);
  }

  .actions button {
    min-height: 2.5rem;
    padding: 0 var(--space-3);
  }

  p[role='status'] {
    color: var(--warning);
  }
</style>
