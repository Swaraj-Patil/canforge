// Loads canforge.wasm and exposes its entry points as plain functions.
// Used by the browser app and by the Node smoke test in CI.

export async function loadCanforge(source) {
  let bytes = source;
  if (typeof source === 'string') {
    const response = await fetch(source);
    if (!response.ok) {
      throw new Error(`could not fetch ${source} (HTTP ${response.status})`);
    }
    bytes = await response.arrayBuffer();
  }
  const module = await WebAssembly.compile(bytes);
  // The module should import nothing; fail loudly if that ever changes.
  const imports = {};
  for (const imp of WebAssembly.Module.imports(module)) {
    imports[imp.module] = imports[imp.module] || {};
    imports[imp.module][imp.name] = () => {
      throw new Error(`canforge.wasm called an unexpected import: ${imp.module}.${imp.name}`);
    };
  }
  const instance = await WebAssembly.instantiate(module, imports);
  const ex = instance.exports;
  const encoder = new TextEncoder();
  const decoder = new TextDecoder();

  function call(name, ...strings) {
    const buffers = [];
    try {
      const args = [];
      for (const text of strings) {
        const data = encoder.encode(text);
        const ptr = ex.cf_alloc(data.length);
        new Uint8Array(ex.memory.buffer, ptr, data.length).set(data);
        buffers.push([ptr, data.length]);
        args.push(ptr, data.length);
      }
      ex[name](...args);
      // Views must be created after the call: memory may have grown.
      const ptr = ex.cf_result_ptr();
      const len = ex.cf_result_len();
      const text = decoder.decode(new Uint8Array(ex.memory.buffer, ptr, len).slice());
      return JSON.parse(text);
    } finally {
      for (const [ptr, len] of buffers) {
        ex.cf_free(ptr, len);
      }
    }
  }

  return {
    version: () => call('cf_version'),
    analyze: (src) => call('cf_analyze', src),
    decode: (src, frameId, hex) => call('cf_decode', src, frameId, hex),
    generate: (src, lang, prefix, fileName) => call('cf_generate', src, lang, prefix, fileName),
    diff: (oldSrc, newSrc) => call('cf_diff', oldSrc, newSrc),
    // Parse a database once and keep it in the module; the *Loaded calls
    // below then work on it without sending or parsing the text again.
    load: (src) => call('cf_load', src),
    decodeLoaded: (frameId, hex) => call('cf_decode_loaded', frameId, hex),
    generateLoaded: (lang, prefix, fileName) => call('cf_generate_loaded', lang, prefix, fileName),
    // Message and signal are indices into the analysis JSON.
    signalCodeLoaded: (message, signal, prefix) => call('cf_signal_code_loaded', String(message), String(signal), prefix),
  };
}
