// Keeps the view of the example bus in the address bar, so a link opens the
// same tab, message and bytes: #t=frames&m=WheelSpeeds&b=20b20920820a96c0.
// Only the example bus has links, because someone's own file never leaves
// their browser and a link to it would open nothing.
import { currentMessage, state } from './state.js';
import { toHex } from './format.js';

export const TABS = ['frames', 'problems', 'code', 'compare'];

/** The view a link asks for, as {tab, message, bytes}, or null for none. */
export function readLink(hash = location.hash) {
  const params = new URLSearchParams(hash.replace(/^#/, ''));
  const link = { tab: params.get('t'), message: params.get('m'), bytes: params.get('b') };
  return link.tab || link.message || link.bytes ? link : null;
}

function currentHex() {
  const msg = currentMessage();
  const bytes = msg ? state.frames.get(msg.name) : null;
  return bytes ? toHex(bytes).replace(/ /g, '') : '';
}

/** The address of the current view, or of the page alone for someone's own file. */
export function linkUrl() {
  const base = location.href.split('#')[0];
  const msg = currentMessage();
  if (!state.isExample || !msg) return base;
  const params = new URLSearchParams();
  params.set('t', state.tab);
  params.set('m', msg.name);
  params.set('b', currentHex());
  return `${base}#${params}`;
}

/** A file is opening: its first view is not the one to write yet. */
export function openingView() {
  state.openingHex = undefined;
}

/** Remember how the example bus opens, so an untouched view keeps a plain address. */
export function noteOpeningView() {
  state.openingHex = state.isExample ? currentHex() : null;
}

// The address for the view: plain while the example is as it opened, and
// nothing at all while a file is still opening.
function address() {
  if (state.openingHex === undefined) return null;
  const untouched = state.tab === 'frames' && state.selected === 0 && currentHex() === state.openingHex;
  return untouched ? location.href.split('#')[0] : linkUrl();
}

function differs(url) {
  return url !== null && url !== location.href && !(url === location.href.split('#')[0] && !location.hash);
}

// Browsers limit how often a page may rewrite its address: Safari allows 100
// times in 30 seconds. A bucket of BURST writes refills at PER_SECOND, so at
// most 10 + 2.5 x 30 = 85 writes land in any 30 seconds. Ordinary changes are
// written at once; only a long burst of bit flips waits, and lands as one.
const BURST = 10;
const PER_SECOND = 2.5;
let tokens = BURST;
let refilledAt = 0;
let pending = 0;

function refill() {
  const now = performance.now();
  tokens = Math.min(BURST, tokens + ((now - refilledAt) / 1000) * PER_SECOND);
  refilledAt = now;
}

function write() {
  pending = 0;
  const url = address();
  if (!differs(url)) return;
  refill();
  tokens -= 1;
  try {
    history.replaceState(null, '', url);
  } catch {
    // The address bar keeps its old value; Copy link still copies the view.
  }
}

/** Bring the address bar up to date with the view. */
export function writeLink({ now = false } = {}) {
  clearTimeout(pending);
  if (!differs(address())) return;
  refill();
  if (now || tokens >= 1) write();
  else pending = setTimeout(write, ((1 - tokens) / PER_SECOND) * 1000);
}
