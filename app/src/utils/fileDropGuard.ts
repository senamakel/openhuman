import debugFactory from 'debug';

const log = debugFactory('openhuman:file-drop-guard');

/** True when the drag carries OS files, as opposed to text or an in-app payload. */
export function isFileDrag(event: DragEvent): boolean {
  return Array.from(event.dataTransfer?.types ?? []).includes('Files');
}

/**
 * Install a document-level guard that keeps a file dropped on the app from
 * navigating the main webview to that file.
 *
 * The shell disables Tauri's native drag-drop handler (`dragDropEnabled:
 * false` in `tauri.conf.json`) so HTML5 drag events reach the page. The flip
 * side is that the webview's own default applies everywhere nothing claims the
 * drop: it opens the dropped file as the top-level document, and the app is
 * gone with no way back. Only the chat surface accepts files, so everywhere
 * else a file drag must be refused outright.
 *
 * Bubble phase, like `installExternalLinkGuard`: a real drop target (the chat
 * thread) handles the event first and calls `preventDefault`, and anything it
 * already claimed is left alone here. Unclaimed file drags get
 * `dropEffect = 'none'` — the not-allowed cursor — and the drop's default
 * navigation is cancelled.
 *
 * Only `Files` drags are touched, so in-app drags (the flow canvas palette,
 * text selections) keep their own behaviour. A native `<input type="file">`
 * keeps its built-in drop handling too.
 *
 * Returns the teardown function.
 */
export function installFileDropGuard(doc: Document = document): () => void {
  const isNativeFileInput = (target: EventTarget | null) =>
    target instanceof HTMLInputElement && target.type === 'file';

  const onDragOver = (event: DragEvent) => {
    if (event.defaultPrevented || !isFileDrag(event) || isNativeFileInput(event.target)) return;
    event.preventDefault();
    if (event.dataTransfer) event.dataTransfer.dropEffect = 'none';
  };

  const onDrop = (event: DragEvent) => {
    if (event.defaultPrevented || !isFileDrag(event) || isNativeFileInput(event.target)) return;
    event.preventDefault();
    log('[file-drop-guard] refused file drop outside a drop target');
  };

  doc.addEventListener('dragover', onDragOver);
  doc.addEventListener('drop', onDrop);
  return () => {
    doc.removeEventListener('dragover', onDragOver);
    doc.removeEventListener('drop', onDrop);
  };
}
