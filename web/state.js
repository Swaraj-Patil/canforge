// The page's shared state, and helpers for the parts of the page that every
// view touches: the status line, button feedback and reading files.

export const $ = (id) => document.getElementById(id);

export const state = {
  cf: null,
  src: '',
  fileName: '',
  isExample: false,
  analysis: null,
  // How long cf_load took for the open file, in milliseconds.
  loadMs: 0,
  selected: 0,
  frames: new Map(),
  animate: true,
  lang: 'c',
  fileIndex: 0,
  files: [],
  oldRev: null,
  newRev: null,
};

export function setStatus(text, isError = false) {
  const el = $('status');
  el.textContent = text;
  el.classList.toggle('error', isError);
}

export function flash(button, text) {
  const original = button.textContent;
  button.textContent = text;
  setTimeout(() => {
    button.textContent = original;
  }, 1400);
}

export async function fetchText(path) {
  const response = await fetch(path);
  if (!response.ok) throw new Error(`${path} returned HTTP ${response.status}`);
  return response.text();
}

export async function readChosenFile(input) {
  const file = input.files && input.files[0];
  input.value = '';
  if (!file) return null;
  return { name: file.name, text: await file.text() };
}
