import { getCurrentWindow } from '@tauri-apps/api/window';

/** Marker attribute for window drag regions (the rail and the stage header).
 *  Only the element that *directly* receives the mousedown counts, so a button
 *  living inside a bar stays clickable without opting out. */
export const DRAG_REGION_ATTR = 'data-app-drag';

/**
 * Frameless-window dragging + double-click zoom. `startDragging` is deferred
 * until the pointer actually moves: calling it on the first mousedown lets the
 * OS drag session swallow the second click of a double-click, which would make
 * the zoom toggle unreliable. Dragging still starts on the first pixel moved.
 */
export function installWindowDragHandler(
  options: { canToggleMaximize?: () => boolean } = {},
): () => void {
  const { canToggleMaximize } = options;
  let pressed: { x: number; y: number } | null = null;

  const isDragRegion = (target: EventTarget | null): target is HTMLElement =>
    target instanceof HTMLElement && target.hasAttribute(DRAG_REGION_ATTR);

  const onMouseDown = (event: MouseEvent) => {
    if (event.button !== 0 || !isDragRegion(event.target)) return;
    event.preventDefault(); // no text cursor / selection on the bar
    if (event.detail >= 2) {
      pressed = null;
      // The compact card is fixed-size, so double-click must not maximize it.
      if (canToggleMaximize?.() ?? true) void getCurrentWindow().toggleMaximize();
      return;
    }
    // Screen coordinates: client coordinates shift under the pointer the moment
    // the window itself moves.
    pressed = { x: event.screenX, y: event.screenY };
  };

  const onMouseMove = (event: MouseEvent) => {
    if (!pressed) return;
    if (Math.abs(event.screenX - pressed.x) + Math.abs(event.screenY - pressed.y) < 2) return;
    pressed = null;
    void getCurrentWindow().startDragging();
  };

  const reset = () => {
    pressed = null;
  };

  document.addEventListener('mousedown', onMouseDown, true);
  document.addEventListener('mousemove', onMouseMove, true);
  document.addEventListener('mouseup', reset, true);
  window.addEventListener('blur', reset);
  return () => {
    document.removeEventListener('mousedown', onMouseDown, true);
    document.removeEventListener('mousemove', onMouseMove, true);
    document.removeEventListener('mouseup', reset, true);
    window.removeEventListener('blur', reset);
  };
}
