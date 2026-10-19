<script lang="ts">
  /**
   * App shell: status header, primary navigation and the one mounted page.
   *
   * The shell owns exactly two pieces of local state — which page is active and
   * whether startup failed. Everything else comes from the shared stores, so the
   * shell stays a thin view over `octopad-core`:
   *
   * - `initializeAppState()` runs once on mount and registers the core event
   *   listeners *before* the first snapshot is applied.
   * - Its cleanup runs exactly once on destroy, so remounting the shell (or a
   *   hot reload during development) never leaves duplicate listeners behind.
   */
  import { onMount } from 'svelte';

  import Sidebar from './Sidebar.svelte';
  import StatusHeader from './StatusHeader.svelte';
  import ButtonsPage from '../pages/ButtonsPage.svelte';
  import ConsolePage from '../pages/ConsolePage.svelte';
  import ProfilesPage from '../pages/ProfilesPage.svelte';
  import SettingsPage from '../pages/SettingsPage.svelte';
  import { initializeAppState, lastError } from '../state';
  import { DEFAULT_PAGE } from '../navigation';
  import type { PageId } from '../navigation';

  let activePage = $state<PageId>(DEFAULT_PAGE);
  let startupError = $state<string | null>(null);

  const visibleError = $derived($lastError ?? startupError);

  function describe(error: unknown): string {
    if (error instanceof Error) return error.message;
    return String(error);
  }

  onMount(() => {
    let dispose: (() => void) | null = null;
    let destroyed = false;

    initializeAppState()
      .then((cleanup) => {
        // The shell can be torn down while the listeners are still registering;
        // in that case the cleanup runs as soon as it exists.
        if (destroyed) {
          cleanup();
          return;
        }
        dispose = cleanup;
      })
      .catch((error: unknown) => {
        if (!destroyed) {
          startupError = `Could not start OctoPad: ${describe(error)}`;
        }
      });

    return () => {
      destroyed = true;
      dispose?.();
    };
  });
</script>

<div class="app-shell">
  <StatusHeader />

  <div class="app-shell__body">
    <Sidebar {activePage} onNavigate={(page) => (activePage = page)} />

    <main class="app-shell__main">
      {#if activePage === 'buttons'}
        <ButtonsPage />
      {:else if activePage === 'console'}
        <ConsolePage />
      {:else if activePage === 'profiles'}
        <ProfilesPage />
      {:else}
        <SettingsPage />
      {/if}
    </main>
  </div>

  {#if visibleError}
    <!--
      Single canonical failure channel: `runCommand` records every rejected or
      failed command in `lastError`, so a failure can never be reported as a
      successful edit or send.
    -->
    <div class="app-shell__alert alert alert--error" role="alert" data-testid="app-error">
      {visibleError}
    </div>
  {/if}
</div>

<style>
  .app-shell {
    display: flex;
    flex-direction: column;
    height: 100%;
    min-height: 0;
    background-color: var(--surface-base);
  }

  .app-shell__body {
    display: flex;
    flex: 1 1 auto;
    min-height: 0;
  }

  .app-shell__main {
    flex: 1 1 auto;
    min-width: 0;
    min-height: 0;
    padding: var(--space-5);
    overflow: auto;
  }

  .app-shell__alert {
    flex: none;
    margin: 0 var(--space-5) var(--space-4);
  }

  @media (max-width: 860px) {
    .app-shell__body {
      flex-direction: column;
    }

    .app-shell__main {
      padding: var(--space-4);
    }
  }
</style>