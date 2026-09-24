import { afterEach, beforeEach, describe, expect, it } from 'vitest';

import { installFileDropGuard } from './fileDropGuard';

function dragEvent(type: 'dragover' | 'drop', types: string[]) {
  const event = new Event(type, { bubbles: true, cancelable: true }) as DragEvent;
  const dataTransfer = { types, dropEffect: 'copy' };
  Object.defineProperty(event, 'dataTransfer', { value: dataTransfer });
  return { event, dataTransfer };
}

describe('installFileDropGuard', () => {
  let teardown: () => void = () => {};
  let host: HTMLDivElement;

  beforeEach(() => {
    host = document.createElement('div');
    document.body.appendChild(host);
    teardown = installFileDropGuard();
  });

  afterEach(() => {
    teardown();
    host.remove();
  });

  it('refuses an unclaimed file drag so the webview never opens the file', () => {
    const over = dragEvent('dragover', ['Files']);
    host.dispatchEvent(over.event);
    expect(over.event.defaultPrevented).toBe(true);
    expect(over.dataTransfer.dropEffect).toBe('none');

    const drop = dragEvent('drop', ['Files']);
    host.dispatchEvent(drop.event);
    expect(drop.event.defaultPrevented).toBe(true);
  });

  it('leaves a drag a real drop target already claimed alone', () => {
    host.addEventListener('dragover', event => {
      event.preventDefault();
      event.dataTransfer!.dropEffect = 'copy';
    });
    const over = dragEvent('dragover', ['Files']);
    host.dispatchEvent(over.event);
    expect(over.dataTransfer.dropEffect).toBe('copy');
  });

  it('ignores in-app drags that carry no files', () => {
    const over = dragEvent('dragover', ['application/tinyflows-node']);
    host.dispatchEvent(over.event);
    expect(over.event.defaultPrevented).toBe(false);
    expect(over.dataTransfer.dropEffect).toBe('copy');
  });

  it('leaves a native file input to its own drop handling', () => {
    const input = document.createElement('input');
    input.type = 'file';
    host.appendChild(input);
    const drop = dragEvent('drop', ['Files']);
    input.dispatchEvent(drop.event);
    expect(drop.event.defaultPrevented).toBe(false);
  });

  it('stops guarding once torn down', () => {
    teardown();
    teardown = () => {};
    const drop = dragEvent('drop', ['Files']);
    host.dispatchEvent(drop.event);
    expect(drop.event.defaultPrevented).toBe(false);
  });
});
