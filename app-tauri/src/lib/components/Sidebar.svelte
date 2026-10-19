<script lang="ts">
  const pages = [
    { id: 'Buttons', label: 'Buttons' },
    { id: 'Serial Console', label: 'Serial Console' },
    { id: 'Profiles', label: 'Profiles' },
    { id: 'Settings', label: 'Settings' }
  ] as const;

  type PageId = (typeof pages)[number]['id'];

  export let activePage: PageId;
  export let onNavigate: (page: PageId) => void;
</script>

<aside class="sidebar">
  <nav aria-label="Main navigation">
    {#each pages as page (page.id)}
      <button
        class:active={activePage === page.id}
        aria-current={activePage === page.id ? 'page' : undefined}
        onclick={() => onNavigate(page.id)}
      >
        {page.label}
      </button>
    {/each}
  </nav>
</aside>

<style>
  .sidebar {
    padding: var(--space-4);
    border-right: 1px solid var(--border);
    background: var(--surface-raised);
  }

  nav {
    display: grid;
    gap: var(--space-2);
  }

  button {
    min-height: 2.75rem;
    padding: var(--space-2) var(--space-3);
    border-color: transparent;
    background: transparent;
    color: var(--text-muted);
    text-align: left;
  }

  button.active {
    border-color: var(--border);
    background: var(--surface-hover);
    color: var(--text);
  }

  @media (max-width: 760px) {
    .sidebar {
      overflow-x: auto;
      padding: var(--space-2);
      border-right: 0;
      border-bottom: 1px solid var(--border);
    }

    nav {
      display: flex;
      min-width: max-content;
      gap: var(--space-1);
    }

    button {
      min-height: 2.5rem;
      white-space: nowrap;
    }
  }
</style>
