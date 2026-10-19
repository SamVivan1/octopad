<script lang="ts">
  import type { Config, TimingField } from '../types';

  export let values: Pick<Config, TimingField>;
  export let disabled = false;
  export let onChange: (field: TimingField, value: number) => void | Promise<void>;

  const controls = [
    { field: 'debounceMs', label: 'Debounce (ms)', min: 0, max: 1000 },
    { field: 'clickTimeoutMs', label: 'Click timeout (ms)', min: 50, max: 2000 },
    { field: 'holdTimeoutMs', label: 'Hold timeout (ms)', min: 50, max: 3000 }
  ] as const;

  let feedback = '';

  function valueFor(field: TimingField): number | string {
    const value = values[field];
    return typeof value === 'number' || typeof value === 'string' ? value : '';
  }

  function updateTiming(field: TimingField, label: string, min: number, max: number, raw: string) {
    const value = Number(raw);
    if (!raw.trim() || !Number.isSafeInteger(value) || value < min || value > max) {
      feedback = `${label} must be between ${min} and ${max} ms.`;
      return;
    }
    if ((value - min) % 5 !== 0) {
      feedback = `${label} must use increments of 5 ms.`;
      return;
    }
    feedback = '';
    onChange(field, value);
  }
</script>

<section class="timing-card" aria-labelledby="timing-title">
  <h2 id="timing-title">Button timing</h2>
  {#each controls as control}
    <label>
      {control.label}
      <input
        type="number"
        min={control.min}
        max={control.max}
        step="5"
        value={valueFor(control.field)}
        disabled={disabled}
        onchange={(event) =>
          updateTiming(
            control.field,
            control.label,
            control.min,
            control.max,
            event.currentTarget.value
          )}
      />
    </label>
  {/each}
  {#if feedback}
    <p role="alert">{feedback}</p>
  {/if}
</section>

<style>
  .timing-card {
    display: grid;
    gap: var(--space-3);
  }

  h2 {
    margin: 0;
    font-size: 1.05rem;
  }

  label {
    display: grid;
    gap: var(--space-1);
    color: var(--text-muted);
    font-size: 0.85rem;
  }

  input {
    min-width: 0;
    min-height: 2.5rem;
    padding: 0 var(--space-3);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    background: var(--surface-base);
    color: var(--text);
  }

  p[role='alert'] {
    margin: 0;
    color: var(--error);
  }
</style>
