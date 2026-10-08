// The page's shared state, and helpers for the parts of the page that every
// view touches: the status line, button feedback and reading files.
import { prefixFrom } from './format.js';

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
  // The signal of the selected message open in the inspector, or null.
  inspect: null,
  // The tab on show, and what this browser keeps of the open file:
  // 'saved', 'too-large', 'refused', 'forgotten', or null for the example.
  tab: 'frames',
  stored: null,
  // The example's first frame as it opens, so an untouched view keeps a plain address.
  openingHex: null,
  frames: new Map(),
  animate: true,
  lang: 'c',
  fileIndex: 0,
  files: [],
  oldRev: null,
  newRev: null,
};

export function currentMessage() {
  return state.analysis ? state.analysis.messages[state.selected] || null : null;
}

/**
 * The name prefix for generated code: the Generated code tab's field, or one
 * made from the file name. The inspector and that tab both use it, so their
 * C is always the same.
 */
export function codePrefix() {
  return $('prefix').value.trim() || prefixFrom(state.fileName);
}

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
  return { name: file.name, text: await file.text(), size: file.size };
}
