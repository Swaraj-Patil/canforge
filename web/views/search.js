// Search: filters the message list by message name, signal name or frame ID
// and says which signals matched. It works on names and IDs in the analysis
// JSON and never touches the bits of a frame.
import { $, state } from '../state.js';
import { escapeHtml, plural } from '../format.js';
import { selectMessage } from './frames.js';

/**
 * Read the search text. `0x` and hex digits match frame IDs that start with
 * those digits, so the list narrows while an ID is typed; decimal digits
 * match one frame ID exactly. Every query also matches message and signal
 * names that contain it, ignoring case.
 */
export function parseQuery(text) {
  const q = text.trim();
  if (!q) return null;
  const query = { text: q.toLowerCase(), hexPrefix: null, decimal: null };
  const hex = /^0x([0-9a-f]*)$/i.exec(q);
  if (hex) query.hexPrefix = hex[1].toLowerCase().replace(/^0+(?=.)/, '');
  else if (/^[0-9]+$/.test(q)) query.decimal = Number(q);
  return query;
}

export function matchMessage(msg, query) {
  const byName = msg.name.toLowerCase().includes(query.text);
  let byId = false;
  if (query.hexPrefix !== null) byId = msg.id.toString(16).startsWith(query.hexPrefix);
  else if (query.decimal !== null) byId = msg.id === query.decimal;
  const signals = [];
  msg.signals.forEach((s, si) => {
    if (s.name.toLowerCase().includes(query.text)) signals.push(si);
  });
  return { any: byName || byId || signals.length > 0, byName, byId, signals };
}

function marked(text, needle) {
  const at = text.toLowerCase().indexOf(needle);
  if (at < 0) return escapeHtml(text);
  const end = at + needle.length;
  return `${escapeHtml(text.slice(0, at))}<mark>${escapeHtml(text.slice(at, end))}</mark>${escapeHtml(text.slice(end))}`;
}

// "Signal YawRate", "Signals A and B", "Signals A, B and 3 more".
function foundHtml(msg, signals, needle) {
  if (!signals.length) return '';
  const names = signals.slice(0, 2).map((si) => marked(msg.signals[si].name, needle));
  const more = signals.length - names.length;
  if (signals.length === 1) return `Signal ${names[0]}`;
  return more ? `Signals ${names.join(', ')} and ${more} more` : `Signals ${names.join(' and ')}`;
}

// The markup each list element was last given, so a keystroke rewrites only
// the items whose highlighting changed.
const written = new WeakMap();

function setHtml(el, html) {
  if (written.get(el) === html) return;
  el.innerHTML = html;
  written.set(el, html);
}

/** Filter the message list by the text in the search field. */
export function applySearch() {
  const query = parseQuery($('search').value);
  const messages = state.analysis ? state.analysis.messages : [];
  let shown = 0;
  for (const li of $('message-list').children) {
    const button = li.firstElementChild;
    const msg = messages[Number(button.dataset.index)];
    const match = query ? matchMessage(msg, query) : null;
    li.hidden = Boolean(match) && !match.any;
    if (li.hidden) continue;
    shown += 1;
    setHtml(button.querySelector('.name'), match && match.byName ? marked(msg.name, query.text) : escapeHtml(msg.name));
    setHtml(button.querySelector('.id'), match && match.byId ? `<mark>${escapeHtml(msg.id_hex)}</mark>` : escapeHtml(msg.id_hex));
    const found = button.querySelector('.found');
    const html = match ? foundHtml(msg, match.signals, query.text) : '';
    setHtml(found, html);
    found.hidden = !html;
    // Found only through a signal: opening the message shows that signal.
    if (match && !match.byName && !match.byId && match.signals.length) button.dataset.inspect = String(match.signals[0]);
    else delete button.dataset.inspect;
  }
  const count = $('search-count');
  if (!query) count.textContent = '';
  else if (!shown) count.textContent = `No message, signal or frame ID matches "${$('search').value.trim()}".`;
  else count.textContent = `Showing ${shown} of ${plural(messages.length, 'message')}.`;
}

/** Empty the search field, for a newly opened file. */
export function clearSearch() {
  $('search').value = '';
  $('search-count').textContent = '';
}

function firstShown() {
  for (const li of $('message-list').children) {
    if (!li.hidden) return li.firstElementChild;
  }
  return null;
}

export function bindSearch({ showFrames }) {
  const input = $('search');
  input.addEventListener('input', applySearch);
  input.addEventListener('keydown', (e) => {
    if (e.key === 'Escape') {
      // Esc clears the search; a second Esc leaves the field.
      e.preventDefault();
      if (input.value) {
        input.value = '';
        applySearch();
      } else {
        input.blur();
      }
    } else if (e.key === 'Enter' && input.value.trim()) {
      e.preventDefault();
      const first = firstShown();
      if (first) selectMessage(Number(first.dataset.index), first.dataset.inspect ? Number(first.dataset.inspect) : null);
    }
  });
  // "/" focuses the search from anywhere except a field being typed in.
  document.addEventListener('keydown', (e) => {
    if (e.key !== '/' || e.ctrlKey || e.metaKey || e.altKey || e.defaultPrevented) return;
    if (e.target instanceof Element && e.target.closest('input, textarea, select, [contenteditable]')) return;
    e.preventDefault();
    showFrames();
    input.focus();
    input.select();
  });
}
