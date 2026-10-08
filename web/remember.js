// Remembers the last file someone opened so it opens again next time. The
// copy lives in this browser's localStorage and nowhere else: nothing is
// sent anywhere. Every access is guarded, because storage can be full,
// turned off, or refused in a private window.

const KEY = 'canforge.lastFile';

/** Files up to this size are remembered: 2 MB. */
export const REMEMBER_LIMIT = 2 * 1024 * 1024;

/** The remembered file as {name, text}, or null. */
export function savedFile() {
  try {
    const value = JSON.parse(localStorage.getItem(KEY) || 'null');
    return value && typeof value.name === 'string' && typeof value.text === 'string' ? value : null;
  } catch {
    return null;
  }
}

/** Remember a file. Returns 'saved', 'too-large' or 'refused'. */
export function saveFile(name, text, size) {
  if (size > REMEMBER_LIMIT) {
    forgetFile();
    return 'too-large';
  }
  try {
    localStorage.setItem(KEY, JSON.stringify({ name, text }));
    return 'saved';
  } catch {
    forgetFile();
    return 'refused';
  }
}

export function forgetFile() {
  try {
    localStorage.removeItem(KEY);
  } catch {
    // Storage that cannot be read holds nothing to forget.
  }
}
