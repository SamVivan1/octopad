<script lang="ts">
  /**
   * Buttons page — placeholder until the 8-key grid and the action editor land
   * in the next milestone. The page exists now so the navigation, the shell
   * layout and the page-switching contract are exercised end to end.
   */
  import { appState } from '../state';

  const snapshot = $derived($appState);
</script>

<section class="page" data-page="buttons" data-testid="active-page">
  <h1 class="page__title">Buttons</h1>

  <div class="card">
    <h2 class="section-title">Key grid</h2>
    <p class="muted">
      The eight physical keys and the Desktop / Home Assistant mode switch are being built next.
      Current mode: {snapshot?.config.modes[snapshot?.modeIndex ?? 0]?.name ?? 'unknown'}, selected
      key: {(snapshot?.selectedIndex ?? 0) + 1}.
    </p>
  </div>
</section>
