<script lang="ts">
  import { onMount } from 'svelte';
  import Sidebar from './Sidebar.svelte';
  import StatusHeader from './StatusHeader.svelte';
  import ProfilesPage from '../pages/ProfilesPage.svelte';
  import { initializeAppState } from '../state';

  type PageId = 'Buttons' | 'Serial Console' | 'Profiles' | 'Settings';

  let activePage: PageId = 'Buttons';
  let startupError = '';

  onMount(() => {
    let disposed = false;
    let cleanup: (() => void) | undefined;

    void initializeAppState()
      .then((stopListening) => {
        if (disposed) {
          stopListening();
        } else {
          cleanup = stopListening;
        }
      })
      .catch((error: unknown) => {
        if (!disposed) startupError = error instanceof Error ? error.message : String(error);
      });

    return () => {
      disposed = true;
      cleanup?.();
    };
  });

  function navigateToPage(page: PageId) {
    activePage = page;
  }
</script>

<div class="app-shell">
  <StatusHeader />
  <div class="workspace">
    <Sidebar {activePage} onNavigate={navigateToPage} />
    <main class="page-content">
      {#if startupError}
        <p class="startup-error" role="alert">Unable to load OctoPad state: {startupError}</p>
      {/if}

      {#if activePage === 'Buttons'}
        <section aria-labelledby="buttons-title">
          <h1 id="buttons-title">Buttons</h1>
          <p class="page-placeholder">Choose a key to configure its actions.</p>
        </section>
      {:else if activePage === 'Serial Console'}
        <section aria-labelledby="console-title">
          <h1 id="console-title">Serial Console</h1>
          <p class="page-placeholder">Device messages will appear here.</p>
        </section>
      {:else if activePage === 'Profiles'}
        <ProfilesPage />
      {:else}
        <section aria-labelledby="settings-title">
          <h1 id="settings-title">Settings</h1>
          <p class="page-placeholder">Connection and timing preferences.</p>
        </section>
      {/if}
    </main>
  </div>
</div>

<style>
  .app-shell {
    display: grid;
    min-height: 100vh;
    grid-template-rows: auto 1fr;
  }

  .workspace {
    display: grid;
    min-height: 0;
    grid-template-columns: 14rem minmax(0, 1fr);
  }

  .page-content {
    min-width: 0;
    padding: var(--space-6);
  }

  h1 {
    margin: 0 0 var(--space-4);
    font-size: 1.5rem;
  }

  .page-placeholder {
    color: var(--text-muted);
  }

  .startup-error {
    padding: var(--space-3);
    border: 1px solid var(--error);
    border-radius: var(--radius-md);
    color: var(--error);
  }

  @media (max-width: 760px) {
    .workspace {
      grid-template-columns: minmax(0, 1fr);
      grid-template-rows: auto 1fr;
    }

    .page-content {
      padding: var(--space-4);
    }
  }
</style>
