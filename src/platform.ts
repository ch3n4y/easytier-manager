/**
 * Which shell the app wears: the desktop window with its left rail, or the
 * phone shell with a bottom tab bar.
 *
 * The backend's `Status.platform` is the only trustworthy answer, but the shell
 * renders its first frame before that answer lands, so the viewport width has
 * to stand in until it does. Both live here so the two signals cannot drift.
 */

/** Platforms whose "window" is a phone app: no tray, no frameless drag. */
const MOBILE_PLATFORMS: readonly string[] = ['android', 'ios'];

/** Below this width even a desktop browser deserves the phone shell. */
const NARROW_VIEWPORT = 720;

/** The same breakpoint expressed for `matchMedia`, so the two cannot disagree. */
export const NARROW_VIEWPORT_QUERY = `(max-width: ${NARROW_VIEWPORT - 1}px)`;

export function isMobilePlatform(platform: string): boolean {
  return MOBILE_PLATFORMS.includes(platform.trim().toLowerCase());
}

/**
 * Fallback for the frames before the first status lands (and for a browser
 * preview, where no backend ever answers): the window is simply too narrow for
 * a 208px rail plus a readable stage.
 */
export function isNarrowViewport(): boolean {
  return window.innerWidth < NARROW_VIEWPORT;
}

/**
 * Subscribe to the width fallback. Re-reading on mount matters because the
 * initial state is a snapshot taken during render, which a phone rotating into
 * landscape (or a resized preview) has already invalidated.
 */
export function watchNarrowViewport(onChange: (narrow: boolean) => void): () => void {
  const query = window.matchMedia(NARROW_VIEWPORT_QUERY);
  const sync = () => onChange(query.matches);
  sync();
  query.addEventListener('change', sync);
  return () => query.removeEventListener('change', sync);
}
