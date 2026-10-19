export function handleAppShortcut(
  event: KeyboardEvent,
  onSave: () => void,
  onQuit: () => void
): boolean {
  if (!event.ctrlKey || event.altKey || event.shiftKey) return false;

  const target = event.target;
  if (
    target instanceof HTMLElement &&
    (target.isContentEditable ||
      target.closest('input, textarea, select, [contenteditable]:not([contenteditable="false"])'))
  ) {
    return false;
  }

  const key = event.key.toLowerCase();
  if (key !== 's' && key !== 'q') return false;

  event.preventDefault();
  if (key === 's') onSave();
  else onQuit();
  return true;
}
