<script lang="ts">
  /**
   * Profiles — M2 scope.
   *
   * Shows where the working config lives, whether it has unsaved changes, an
   * explicit Save, and an explicit legacy import. The preset gallery and
   * profile import/export stay M3.
   *
   * Two rules drive the implementation:
   *
   * - **Never save implicitly.** Import only loads the legacy config into the
   *   editor and marks the working copy dirty; writing to disk still requires
   *   the Save button.
   * - **Never fail silently.** Every command goes through `runCommand`, which
   *   records rejected and failed commands in `lastError`; the shell renders
   *   that message as an alert, and this page reports outcomes as status text.
   */
  import { get } from 'svelte/store';

  import * as api from '../api';
  import { importErrorMessage } from '../api';
  import { appState, lastError, refreshAppState, runCommand } from '../state';
  import type { ImportReport } from '../types';

  type FeedbackTone = 'ok' | 'warn' | 'error';
  interface Feedback {
    tone: FeedbackTone;
    text: string;
  }

  const TONE_CLASS: Record<FeedbackTone, string> = {
    ok: 'alert--ok',
    warn: 'alert--warn',
    error: 'alert--error',
  };

  const snapshot = $derived($appState);
  const configPath = $derived(snapshot?.configPath ?? '');
  const stateLabel = $derived(snapshot?.dirty ? 'Unsaved changes' : 'Saved');

  let busy = $state(false);
  let feedback = $state<Feedback | null>(null);

  async function save(): Promise<void> {
    busy = true;
    feedback = null;
    const saved = await runCommand('save configuration', () => api.saveConfig());
    if (saved) {
      // Re-read so the path, the preset and the dirty flag are exactly what the
      // core holds after the write, not what the UI assumed it would be.
      await refreshAppState();
      feedback = { tone: 'ok', text: `Configuration saved to ${configPath || 'the config file'}.` };
    } else {
      feedback = {
        tone: 'error',
        text: get(lastError) ?? 'The configuration could not be saved.',
      };
    }
    busy = false;
  }

  /** Turns a rejected `ImportError` into a readable message for `runCommand`. */
  async function loadLegacyReport(): Promise<ImportReport> {
    try {
      return await api.importLegacyConfig();
    } catch (error) {
      throw new Error(importErrorMessage(error));
    }
  }

  async function importLegacy(): Promise<void> {
    busy = true;
    feedback = null;

    const captured: { report: ImportReport | null } = { report: null };
    const imported = await runCommand('legacy import', async () => {
      captured.report = await loadLegacyReport();
      return captured.report;
    });

    if (imported && captured.report) {
      // The core already emitted `config-replaced`; re-read anyway so the page
      // shows the imported working copy even if the event was coalesced. No
      // save happens here: an import never writes the config file.
      await refreshAppState();
      feedback = describeImport(captured.report);
    } else {
      // A failed import leaves the working copy exactly as it was, so the
      // message must say what did not happen as well as what did.
      feedback = {
        tone: 'error',
        text:
          get(lastError) ??
          'The legacy configuration could not be imported, so the working copy is unchanged.',
      };
    }
    busy = false;
  }

  function describeImport(report: ImportReport): Feedback {
    if (!report.imported_config) {
      return {
        tone: 'warn',
        text: 'No legacy configuration was found, so the working copy is unchanged.',
      };
    }
    const warnings = report.warnings.length ? ` Warnings: ${report.warnings.join('; ')}` : '';
    return {
      tone: 'ok',
      text:
        `Legacy configuration loaded into the editor${report.presets.length > 0 ? ` (${report.presets.length} preset${report.presets.length === 1 ? '' : 's'} found)` : ''}.` +
        ` Review the buttons, then choose Save to write it to the config file.${warnings}`,
    };
  }
</script>

<section class="page" data-page="profiles" data-testid="active-page">
  <h1 class="page__title">Profiles</h1>

  <div class="card profiles__config">
    <h2 class="section-title">Working configuration</h2>

    <dl class="profiles__facts">
      <dt>Config file</dt>
      <dd class="mono" data-testid="profiles-config-path">{configPath}</dd>

      <dt>State</dt>
      <dd>
        <span
          class="pill pill--{snapshot?.dirty ? 'warn' : 'ok'}"
          data-testid="profiles-config-state"
        >
          {stateLabel}
        </span>
      </dd>
    </dl>

    <div class="profiles__actions">
      <button type="button" class="btn btn--primary" onclick={save} disabled={busy}>
        Save configuration
      </button>
      <button type="button" class="btn" onclick={importLegacy} disabled={busy}>
        Import legacy config
      </button>
    </div>

    <p class="muted profiles__hint">
      Importing replaces the working copy and marks it as unsaved. Nothing is written to disk until
      you choose Save.
    </p>
  </div>

  {#if feedback}
    <!--
      Page-local outcome text. Command failures are additionally announced by the
      shell's `role="alert"` banner, which is driven by the shared `lastError`.
    -->
    <div
      class="alert {TONE_CLASS[feedback.tone]}"
      role="status"
      data-testid="profiles-feedback"
    >
      {feedback.text}
    </div>
  {/if}

  <div class="card profiles__presets">
    <h2 class="section-title">Presets</h2>
    <p class="muted">
      Built-in presets and user preset management arrive in a later milestone. The core already
      knows which preset is active: {snapshot?.activePreset ?? 'none'}.
    </p>
  </div>
</section>

<style>

  .profiles__facts {
    display: grid;
    grid-template-columns: max-content 1fr;
    gap: var(--space-2) var(--space-5);
    margin: 0 0 var(--space-4);
    font-size: var(--font-size-md);
  }

  .profiles__facts dt {
    color: var(--text-muted);
    font-size: var(--font-size-sm);
  }

  .profiles__facts dd {
    margin: 0;
    min-width: 0;
    overflow-wrap: anywhere;
  }

  .profiles__actions {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-3);
  }

  .profiles__hint {
    margin: var(--space-4) 0 0;
    font-size: var(--font-size-sm);
  }
</style>