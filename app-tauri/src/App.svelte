<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  let version = '';

  async function getVersion() {
    version = await invoke('get_version');
  }

  getVersion();
</script>

<main class="container">
  <h1>OctoPad Tauri M0 scaffold</h1>
  <p>Version: {version}</p>
</main>

<style>
  .container {
    font-family: system-ui, -apple-system, sans-serif;
    padding: 2rem;
  }
</style>
