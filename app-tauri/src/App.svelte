<script lang="ts">
  /**
   * Application root.
   *
   * Deliberately thin: the shell owns startup, cleanup and navigation, and every
   * page is a view over the shared stores. The M0 placeholder that used to live
   * here (a direct `get_version` call) is gone.
   */
  import AppShell from './lib/components/AppShell.svelte';
</script>

<AppShell />