// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen, within } from '@testing-library/svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';
import type { Action, JsonValue } from '../types';

import ActionCard from './ActionCard.svelte';
import ActionFields from './ActionFields.svelte';
import ActionListEditor from './ActionListEditor.svelte';

const vendorFields = { vendorData: { retained: true } };

async function expectFieldUpdate(
  action: Action,
  label: string,
  inputValue: string,
  updatedValue: JsonValue,
  field: string
) {
  const onUpdate = vi.fn();
  render(ActionFields, { props: { action, disabled: false, onUpdate } });
  await fireEvent.change(screen.getByLabelText(label), { target: { value: inputValue } });

  expect(onUpdate).toHaveBeenCalledWith(
    expect.objectContaining({
      ...action,
      [field]: updatedValue
    })
  );
  expect(onUpdate.mock.calls[0][0]).toMatchObject(vendorFields);
  cleanup();
}

describe('ActionFields', () => {
  afterEach(() => cleanup());

  it('edits shortcut modifiers and key without dropping unknown fields', async () => {
    const action: Action = {
      type: 'shortcut',
      modifiers: ['vendor_mod', 9],
      key: 'A',
      ...vendorFields
    };
    const onUpdate = vi.fn();
    render(ActionFields, { props: { action, disabled: false, onUpdate } });

    await fireEvent.click(screen.getByRole('checkbox', { name: 'CTRL' }));
    await fireEvent.change(screen.getByRole('textbox', { name: 'Key' }), {
      target: { value: 'F12' }
    });

    expect(onUpdate).toHaveBeenNthCalledWith(
      1,
      expect.objectContaining({ ...action, modifiers: ['ctrl', 'vendor_mod', 9] })
    );
    expect(onUpdate).toHaveBeenNthCalledWith(
      2,
      expect.objectContaining({ ...action, key: 'F12' })
    );
  });

  it('edits text, media, and script action fields', async () => {
    await expectFieldUpdate(
      { type: 'text', text: 'old', ...vendorFields },
      'Text',
      'new text',
      'new text',
      'text'
    );
    await expectFieldUpdate(
      { type: 'media', mediaKey: 'VOL_UP', ...vendorFields },
      'Media command',
      'PLAY_PAUSE',
      'PLAY_PAUSE',
      'mediaKey'
    );
    await expectFieldUpdate(
      { type: 'bash_script', script: 'old', ...vendorFields },
      'Script',
      'echo updated',
      'echo updated',
      'script'
    );
  });

  it('edits audio output lists, app details, and system-action identifiers', async () => {
    await expectFieldUpdate(
      { type: 'launch_app', appName: 'Old', desktopId: 'old.desktop', ...vendorFields },
      'App name',
      'New app',
      'New app',
      'appName'
    );
    await expectFieldUpdate(
      { type: 'system_action', presetId: 'lock_screen', ...vendorFields },
      'System action ID',
      'open_terminal',
      'open_terminal',
      'presetId'
    );
    await expectFieldUpdate(
      { type: 'launch_app', appName: 'App', desktopId: 'old.desktop', ...vendorFields },
      'App path',
      '/usr/share/applications/new.desktop',
      '/usr/share/applications/new.desktop',
      'desktopId'
    );
  });

  it('edits output names while preserving sink and action metadata', async () => {
    const action: Action = {
      type: 'audio_output',
      sinks: [{ name: 'sink.old', description: 'Desk', sinkMetadata: 'keep' }],
      ...vendorFields
    };
    const onUpdate = vi.fn();
    render(ActionFields, { props: { action, disabled: false, onUpdate } });

    await fireEvent.change(screen.getByRole('textbox', { name: 'Output 1 name' }), {
      target: { value: 'sink.new' }
    });
    expect(onUpdate).toHaveBeenCalledWith({
      ...action,
      sinks: [{ name: 'sink.new', description: 'Desk', sinkMetadata: 'keep' }]
    });
  });

  it('adds, removes, and reorders audio outputs without changing their order implicitly', async () => {
    const action: Action = {
      type: 'audio_output',
      sinks: [{ name: 'sink.one', description: 'First' }, { name: 'sink.two', description: 'Second' }]
    };
    const onUpdate = vi.fn();
    render(ActionFields, { props: { action, disabled: false, onUpdate } });

    await fireEvent.click(screen.getByRole('button', { name: 'Add audio output' }));
    expect(onUpdate).toHaveBeenNthCalledWith(1, {
      ...action,
      sinks: [
        { name: 'sink.one', description: 'First' },
        { name: 'sink.two', description: 'Second' },
        { name: '', description: '' }
      ]
    });
    await fireEvent.click(screen.getByRole('button', { name: 'Move audio output 2 up' }));
    expect(onUpdate).toHaveBeenNthCalledWith(2, {
      ...action,
      sinks: [{ name: 'sink.two', description: 'Second' }, { name: 'sink.one', description: 'First' }]
    });
    await fireEvent.click(screen.getByRole('button', { name: 'Remove audio output 1' }));
    expect(onUpdate).toHaveBeenNthCalledWith(3, {
      ...action,
      sinks: [{ name: 'sink.two', description: 'Second' }]
    });
  });

  it('edits Home Assistant, webhook, and delay action fields', async () => {
    await expectFieldUpdate(
      { type: 'home_assistant', entityId: '', domain: '', service: '', data: {}, ...vendorFields },
      'Entity ID',
      'light.desk',
      'light.desk',
      'entityId'
    );
    await expectFieldUpdate(
      { type: 'home_assistant', entityId: '', domain: '', service: '', data: {}, ...vendorFields },
      'Domain',
      'light',
      'light',
      'domain'
    );
    await expectFieldUpdate(
      { type: 'home_assistant', entityId: '', domain: '', service: '', data: {}, ...vendorFields },
      'Service',
      'turn_on',
      'turn_on',
      'service'
    );
    await expectFieldUpdate(
      { type: 'home_assistant', entityId: '', domain: '', service: '', data: {}, ...vendorFields },
      'Service data JSON object',
      '{"brightness":127}',
      { brightness: 127 },
      'data'
    );
    await expectFieldUpdate(
      { type: 'ha_webhook', url: '', ...vendorFields },
      'Webhook URL',
      'https://example.test/hook',
      'https://example.test/hook',
      'url'
    );
    await expectFieldUpdate(
      { type: 'delay', seconds: 0.5, ...vendorFields },
      'Delay seconds',
      '12.3',
      12.3,
      'seconds'
    );
  });

  it('rejects delays outside the supported range and exposes the 0.1-second step', async () => {
    const onUpdate = vi.fn();
    render(ActionFields, {
      props: {
        action: { type: 'delay', seconds: 1 },
        disabled: false,
        onUpdate
      }
    });

    const delay = screen.getByRole('spinbutton', { name: 'Delay seconds' }) as HTMLInputElement;
    expect([delay.min, delay.max, delay.step]).toEqual(['0', '600', '0.1']);
    await fireEvent.change(delay, { target: { value: '600.1' } });
    expect(screen.getByRole('alert').textContent).toContain('between 0 and 600');
    expect(onUpdate).not.toHaveBeenCalled();
  });

  it('rejects malformed Home Assistant service data without changing the action', async () => {
    const onUpdate = vi.fn();
    render(ActionFields, {
      props: {
        action: { type: 'home_assistant', entityId: '', domain: '', service: '', data: {}, ...vendorFields },
        disabled: false,
        onUpdate
      }
    });
    await fireEvent.change(screen.getByLabelText('Service data JSON object'), {
      target: { value: '[' }
    });
    expect(screen.getByRole('alert').textContent).toContain('JSON object');
    expect(onUpdate).not.toHaveBeenCalled();
  });

  it('shows field-free types without replacing their existing data', () => {
    for (const type of ['none', 'mode_toggle']) {
      const { unmount } = render(ActionFields, {
        props: {
          action: { type, ...vendorFields },
          disabled: false,
          onUpdate: vi.fn()
        }
      });
      expect(screen.getByText('No editable fields')).toBeTruthy();
      unmount();
    }
  });
});

describe('ActionListEditor', () => {
  afterEach(() => cleanup());

  it('changes an action type without discarding metadata', async () => {
    const action: Action = { type: 'none', ...vendorFields };
    const onUpdate = vi.fn();
    render(ActionCard, {
      props: {
        action,
        triggerLabel: 'Single',
        index: 0,
        count: 1,
        disabled: false,
        onUpdate,
        onRemove: vi.fn(),
        onMove: vi.fn()
      }
    });

    await fireEvent.change(screen.getByRole('combobox', { name: 'Single action 1 type' }), {
      target: { value: 'text' }
    });
    expect(onUpdate).toHaveBeenCalledWith(0, { ...action, type: 'text' });
  });

  it('accepts a legacy object and preserves list ordering and unknown fields', async () => {
    const onUpdate = vi.fn();
    const noop = vi.fn();
    const legacyAction: Action = {
      type: 'text',
      text: 'legacy',
      ...vendorFields
    };
    const legacy = render(ActionListEditor, {
      props: {
        trigger: 'single',
        actionSlot: legacyAction,
        disabled: false,
        onAdd: noop,
        onRemove: noop,
        onMove: noop,
        onUpdate
      }
    });
    expect(
      (screen.getByRole('combobox', { name: 'Single action 1 type' }) as HTMLSelectElement).value
    ).toBe('text');
    await fireEvent.change(screen.getByRole('textbox', { name: 'Text' }), {
      target: { value: 'edited legacy' }
    });
    expect(onUpdate).toHaveBeenCalledWith(
      0,
      expect.objectContaining({ ...legacyAction, text: 'edited legacy' })
    );
    legacy.unmount();
    cleanup();

    render(ActionListEditor, {
      props: {
        trigger: 'double',
        actionSlot: [
          { type: 'media', mediaKey: 'VOL_UP', ...vendorFields },
          { type: 'text', text: 'second' }
        ],
        disabled: false,
        onAdd: noop,
        onRemove: noop,
        onMove: noop,
        onUpdate: noop
      }
    });
    const first = screen.getByRole('combobox', {
      name: 'Double action 1 type'
    }) as HTMLSelectElement;
    const second = screen.getByRole('combobox', {
      name: 'Double action 2 type'
    }) as HTMLSelectElement;
    expect([first.value, second.value]).toEqual(['media', 'text']);
  });

  it('routes add, remove, and reorder requests and retains one action after removing the last', async () => {
    const onAdd = vi.fn();
    const onRemove = vi.fn();
    const onMove = vi.fn();
    const onUpdate = vi.fn();
    const initialSlot = { type: 'text', text: 'only action' };
    const rendered = render(ActionListEditor, {
      props: {
        trigger: 'hold',
        actionSlot: initialSlot,
        disabled: false,
        onAdd,
        onRemove,
        onMove,
        onUpdate
      }
    });

    await fireEvent.click(screen.getByRole('button', { name: 'Add hold action' }));
    await fireEvent.click(screen.getByRole('button', { name: 'Remove Hold action 1' }));
    expect(onAdd).toHaveBeenCalledOnce();
    expect(onRemove).toHaveBeenCalledWith(0);

    rendered.rerender({
      trigger: 'hold',
      actionSlot: { type: 'none' },
      disabled: false,
      onAdd,
      onRemove,
      onMove,
      onUpdate
    });
    expect(screen.getAllByRole('combobox', { name: /Hold action \d+ type/ })).toHaveLength(1);
    expect(
      (screen.getByRole('combobox', { name: 'Hold action 1 type' }) as HTMLSelectElement).value
    ).toBe('none');

    rendered.rerender({
      trigger: 'hold',
      actionSlot: [{ type: 'text', text: 'first' }, { type: 'media', mediaKey: 'MUTE' }],
      disabled: false,
      onAdd,
      onRemove,
      onMove,
      onUpdate
    });
    await fireEvent.click(screen.getByRole('button', { name: 'Move Hold action 2 up' }));
    expect(onMove).toHaveBeenCalledWith(1, -1);
    expect(
      within(screen.getByRole('group', { name: 'Hold action list' })).getAllByRole('combobox', {
        name: /Hold action \d+ type/
      })
    ).toHaveLength(2);
  });
});
