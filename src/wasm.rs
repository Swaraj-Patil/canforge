//! WebAssembly entry points for the browser demo.
//!
//! Strings cross the boundary as UTF-8 bytes. JavaScript reserves input
//! buffers with `cf_alloc`, calls an entry point, reads the JSON result via
//! `cf_result_ptr` and `cf_result_len`, then releases its inputs with
//! `cf_free`. There are no imports, so the module needs no glue library.
//!
//! `cf_load` parses a database once and keeps it; the `*_loaded` entry
//! points then work on it without parsing the text again.

use crate::model::{Database, FrameIndex};
use crate::{json, parse, VERSION};
use std::sync::Mutex;

static RESULT: Mutex<Vec<u8>> = Mutex::new(Vec::new());

/// The database `cf_load` parsed, kept together with its frame index so the
/// index can never describe a different database.
struct Loaded {
    db: Database,
    frames: FrameIndex,
}

static LOADED: Mutex<Option<Loaded>> = Mutex::new(None);

/// Run `f` on the loaded database, or report that nothing is loaded yet.
fn with_loaded(f: impl FnOnce(&Loaded) -> String) -> String {
    match LOADED.lock() {
        Ok(guard) => match guard.as_ref() {
            Some(loaded) => f(loaded),
            None => json::error_json("no database is loaded; call cf_load first", None),
        },
        Err(_) => json::error_json("the loaded database is unavailable", None),
    }
}

fn set_result(s: String) {
    if let Ok(mut guard) = RESULT.lock() {
        *guard = s.into_bytes();
    }
}

fn read_input(ptr: *const u8, len: usize) -> String {
    if len == 0 || ptr.is_null() {
        return String::new();
    }
    // SAFETY: JavaScript wrote `len` bytes at `ptr`, a buffer from cf_alloc.
    let bytes = unsafe { std::slice::from_raw_parts(ptr, len) };
    String::from_utf8_lossy(bytes).into_owned()
}

/// Reserve `len` bytes for JavaScript to write an input string into.
#[no_mangle]
pub extern "C" fn cf_alloc(len: usize) -> *mut u8 {
    let cap = if len == 0 { 1 } else { len };
    let mut buf: Vec<u8> = Vec::with_capacity(cap);
    let ptr = buf.as_mut_ptr();
    std::mem::forget(buf);
    ptr
}

/// Release a buffer from `cf_alloc`.
///
/// # Safety
/// `ptr` must come from `cf_alloc(len)` with the same `len`, and must not be
/// released twice.
#[no_mangle]
pub unsafe extern "C" fn cf_free(ptr: *mut u8, len: usize) {
    let cap = if len == 0 { 1 } else { len };
    drop(Vec::from_raw_parts(ptr, 0, cap));
}

#[no_mangle]
pub extern "C" fn cf_result_ptr() -> *const u8 {
    match RESULT.lock() {
        Ok(guard) => guard.as_ptr(),
        Err(_) => std::ptr::null(),
    }
}

#[no_mangle]
pub extern "C" fn cf_result_len() -> usize {
    match RESULT.lock() {
        Ok(guard) => guard.len(),
        Err(_) => 0,
    }
}

#[no_mangle]
pub extern "C" fn cf_version() {
    set_result(json::esc(VERSION));
}

#[no_mangle]
pub extern "C" fn cf_analyze(src_ptr: *const u8, src_len: usize) {
    let src = read_input(src_ptr, src_len);
    set_result(json::analyze_json(&src));
}

#[no_mangle]
pub extern "C" fn cf_decode(
    src_ptr: *const u8,
    src_len: usize,
    id_ptr: *const u8,
    id_len: usize,
    hex_ptr: *const u8,
    hex_len: usize,
) {
    let src = read_input(src_ptr, src_len);
    let id = read_input(id_ptr, id_len);
    let hex = read_input(hex_ptr, hex_len);
    set_result(json::decode_json(&src, &id, &hex));
}

#[no_mangle]
pub extern "C" fn cf_generate(
    src_ptr: *const u8,
    src_len: usize,
    lang_ptr: *const u8,
    lang_len: usize,
    prefix_ptr: *const u8,
    prefix_len: usize,
    name_ptr: *const u8,
    name_len: usize,
) {
    let src = read_input(src_ptr, src_len);
    let lang = read_input(lang_ptr, lang_len);
    let prefix = read_input(prefix_ptr, prefix_len);
    let name = read_input(name_ptr, name_len);
    set_result(json::generate_json(&src, &lang, &prefix, &name));
}

#[no_mangle]
pub extern "C" fn cf_diff(old_ptr: *const u8, old_len: usize, new_ptr: *const u8, new_len: usize) {
    let old = read_input(old_ptr, old_len);
    let new = read_input(new_ptr, new_len);
    set_result(json::diff_json(&old, &new));
}

/// Parse and analyse DBC source like `cf_analyze`, returning the same JSON,
/// and keep the database for the `*_loaded` entry points. After a parse
/// error the previously loaded database stays loaded, just as the page keeps
/// showing it.
#[no_mangle]
pub extern "C" fn cf_load(src_ptr: *const u8, src_len: usize) {
    let src = read_input(src_ptr, src_len);
    let out = match parse(&src) {
        Ok(db) => {
            let out = json::analysis(&db);
            let frames = FrameIndex::new(&db);
            if let Ok(mut guard) = LOADED.lock() {
                *guard = Some(Loaded { db, frames });
            }
            out
        }
        Err(e) => json::error_json(&e.message, Some(e.line)),
    };
    set_result(out);
}

/// Decode a frame of the loaded database, like `cf_decode` without the source.
#[no_mangle]
pub extern "C" fn cf_decode_loaded(id_ptr: *const u8, id_len: usize, hex_ptr: *const u8, hex_len: usize) {
    let id = read_input(id_ptr, id_len);
    let hex = read_input(hex_ptr, hex_len);
    set_result(with_loaded(|l| json::decoded_json(&l.db, &l.frames, &id, &hex)));
}

/// Generate code for the loaded database, like `cf_generate` without the source.
#[no_mangle]
pub extern "C" fn cf_generate_loaded(
    lang_ptr: *const u8,
    lang_len: usize,
    prefix_ptr: *const u8,
    prefix_len: usize,
    name_ptr: *const u8,
    name_len: usize,
) {
    let lang = read_input(lang_ptr, lang_len);
    let prefix = read_input(prefix_ptr, prefix_len);
    let name = read_input(name_ptr, name_len);
    set_result(with_loaded(|l| json::generated_json(&l.db, &lang, &prefix, &name)));
}
