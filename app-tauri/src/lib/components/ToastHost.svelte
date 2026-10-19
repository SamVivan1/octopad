<script lang="ts">
  /**
   * The fixed, non-modal toast layer for `./toast`.
   *
   * One host, mounted once by `AppShell`, renders every current toast from the
   * shared store — so a toast outlives the page that produced it. It is a
   * `role="status"` live region: the results it announces (an action test's
   * outcome, later M3 feedback) must reach assistive tech without grabbing
   * focus or blocking anything.
   *
   * Colour is the design system's, not the host's: green and red reuse the
   * shared `--success` / `--error` tokens, the same pair the status pills and
   * alerts use, with `info` staying neutral like an unmodified `.alert`.
   */
  import { dismiss, toasts } from '../toast';

  const items = $derived($toasts);
</script>

<div class="toast-host" data-testid="toast-host" role="status" aria-live="polite">
  {#each items as toast (toast.id)}
    <div
      class="toast-host__toast"
      data-testid="toast"
      data-kind={toast.kind}
    >
      <span class="toast-host__message">{toast.message}</span>
      <button
        type="button"
        class="toast-host__close"
        aria-label="Dismiss notification"
        data-testid="toast-dismiss"
        onclick={() => dismiss(toast.id)}
      >
        ✕
      </button>
    </div>
  {/each}
</div>

<style>
  .toast-host {
    position: fixed;
    left: 50%;
    bottom: var(--space-5);
    transform: translateX(-50%);
    z-index: 60;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: var(--space-2);
    /* Clicks pass through everything except the toasts themselves. */
    pointer-events: none;
    width: max-content;
    max-width: calc(100vw - var(--space-6) * 2);
  }

  .toast-host__toast {
    pointer-events: auto;
    display: flex;
    align-items: center;
    gap: var(--space-3);
    max-width: 100%;
    padding: var(--space-2) var(--space-3) var(--space-2) var(--space-4);
    border: 1px solid var(--border-default);
    border-left-width: 4px;
    border-radius: var(--radius-md);
    background-color: var(--surface-raised);
    color: var(--text-primary);
    font-size: var(--font-size-sm);
    box-shadow: 0 4px 16px rgba(0, 0, 0, 0.25);
  }

  .toast-host__message {
    overflow-wrap: anywhere;
  }

  .toast-host__toast[data-kind='success'] {
    border-left-color: var(--success);
    background-color: var(--success-soft);
    color: var(--success);
  }

  .toast-host__toast[data-kind='error'] {
    border-left-color: var(--error);
    background-color: var(--error-soft);
    color: var(--error);
  }

  .toast-host__close {
    flex: none;
    min-height: 24px;
    min-width: 24px;
    border: none;
    border-radius: var(--radius-sm);
    background-color: transparent;
    color: inherit;
    font: inherit;
    font-size: var(--font-size-xs);
    cursor: pointer;
  }

  .toast-host__close:hover {
    background-color: rgba(255, 255, 255, 0.08);
  }
</style>
