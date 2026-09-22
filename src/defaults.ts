/**
 * Mirrors `settings::DEFAULT_GITHUB_PROXY` in the backend, which applies this
 * accelerator whenever the stored value is empty. The frontend needs it for the
 * settings copy and the "restore default" action; the backend stays the source
 * of truth for what actually gets used.
 */
export const DEFAULT_GITHUB_PROXY = 'https://gh-proxy.com/';
