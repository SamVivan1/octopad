<script lang="ts">
  /**
   * Primary navigation.
   *
   * The legacy app used a GTK `NavigationSidebar`; the web version keeps the
   * same four destinations but collapses to a horizontally scrollable row below
   * the layout breakpoint, so the navigation stays usable in a narrow window.
   */
  import { PAGES } from '../navigation';
  import type { PageId } from '../navigation';

  interface Props {
    activePage: PageId;
    onNavigate: (page: PageId) => void;
  }

  let { activePage, onNavigate }: Props = $props();
</script>

<nav class="sidebar" aria-label="Primary">
  <ul class="sidebar__list">
    {#each PAGES as page (page.id)}
      <li>
        <button
          type="button"
          class="sidebar__item"
          class:is-active={activePage === page.id}
          aria-current={activePage === page.id ? 'page' : undefined}
          onclick={() => onNavigate(page.id)}
        >
          {page.label}
        </button>
      </li>
    {/each}
  </ul>
</nav>

<style>
  .sidebar {
    flex: none;
    width: var(--sidebar-width);
    padding: var(--space-4) var(--space-2);
    border-right: 1px solid var(--border-subtle);
    background-color: var(--surface-base);
    overflow-y: auto;
  }

  .sidebar__list {
    display: flex;
    flex-direction: column;
    gap: 2px;
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .sidebar__item {
    display: block;
    width: 100%;
    padding: 7px 12px;
    border: 1px solid transparent;
    border-radius: var(--radius-md);
    background: none;
    color: var(--text-secondary);
    font-family: inherit;
    font-size: var(--font-size-md);
    font-weight: 600;
    text-align: left;
    white-space: nowrap;
    cursor: pointer;
  }

  .sidebar__item:hover {
    background-color: var(--surface-hover);
    color: var(--text-primary);
  }

  .sidebar__item.is-active {
    border-color: var(--accent-soft);
    background-color: var(--accent-soft);
    color: var(--accent);
  }

  /* Narrow windows: the sidebar becomes a scrollable strip above the content. */
  @media (max-width: 860px) {
    .sidebar {
      width: 100%;
      padding: var(--space-3) var(--space-4);
      border-right: none;
      border-bottom: 1px solid var(--border-subtle);
      overflow-x: auto;
      overflow-y: hidden;
    }

    .sidebar__list {
      flex-direction: row;
      gap: var(--space-2);
    }

    .sidebar__item {
      width: auto;
    }
  }
</style>