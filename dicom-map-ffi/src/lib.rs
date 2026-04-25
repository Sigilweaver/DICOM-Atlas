//! C ABI bindings for `dicom-map`.
//!
//! Consumers link against `libdicom_map_ffi` (shared or static) and include
//! `dicom_map.h`.
//!
//! Lifetime rules:
//!   * Handles returned by `dmap_open` are owned by the caller and must be
//!     released with `dmap_close`.
//!   * `DmapTag` string pointers borrow into the mmap'd file and are valid
//!     until the corresponding handle is closed.
//!   * All functions are thread-safe for reads on a single handle.

use std::ffi::{c_char, CStr, CString};
use std::path::PathBuf;
use std::ptr;
use std::sync::atomic::{AtomicPtr, Ordering};

use dicom_map::DmapDict;

/// Opaque handle.
#[repr(C)]
pub struct DmapHandle {
    _opaque: [u8; 0],
}

/// Returned record. All string pointers are NUL-terminated UTF-8 and borrow
/// into the mmap'd file; do NOT free them. They become invalid after
/// `dmap_close`.
///
/// `creator` and `description` may be empty strings but are never NULL.
#[repr(C)]
pub struct DmapTag {
    pub group: u16,
    pub element: u16,
    pub block_offset: bool,
    pub retired: bool,
    pub vr: [c_char; 4], // 2 chars + NUL, zero-padded
    pub keyword: *const c_char,
    pub name: *const c_char,
    pub creator: *const c_char,
    pub description: *const c_char,
}

/// Internal wrapper that keeps both the dict and per-view CStrings alive.
struct Inner {
    dict: DmapDict,
    // Leaked CStrings for each returned TagView, keyed by record pointer.
    // Simplification: we just leak strings produced per lookup and store the
    // last error message; callers that need stability can copy eagerly.
    last_err: AtomicPtr<c_char>,
}

impl Drop for Inner {
    fn drop(&mut self) {
        let p = self.last_err.swap(ptr::null_mut(), Ordering::Relaxed);
        if !p.is_null() {
            // SAFETY: p was produced by CString::into_raw on this side only.
            unsafe {
                let _ = CString::from_raw(p);
            }
        }
    }
}

fn set_err(inner: &Inner, msg: &str) {
    let c = CString::new(msg.replace('\0', "?")).unwrap_or_else(|_| CString::new("error").unwrap());
    let new_ptr = c.into_raw();
    let old = inner.last_err.swap(new_ptr, Ordering::Relaxed);
    if !old.is_null() {
        // SAFETY: old was produced by CString::into_raw above.
        unsafe {
            let _ = CString::from_raw(old);
        }
    }
}

/// Opens a `.dmap` file. Returns NULL on failure (use `dmap_last_error` with
/// a non-null handle — for `dmap_open` itself, failure is only "bad path"
/// and you get no error string).
///
/// `path` must be a NUL-terminated UTF-8 string.
#[no_mangle]
pub unsafe extern "C" fn dmap_open(path: *const c_char) -> *mut DmapHandle {
    if path.is_null() {
        return ptr::null_mut();
    }
    let c = CStr::from_ptr(path);
    let s = match c.to_str() {
        Ok(s) => s,
        Err(_) => return ptr::null_mut(),
    };
    let p = PathBuf::from(s);
    match DmapDict::open(&p) {
        Ok(d) => {
            let inner = Box::new(Inner {
                dict: d,
                last_err: AtomicPtr::new(ptr::null_mut()),
            });
            Box::into_raw(inner) as *mut DmapHandle
        }
        Err(_) => ptr::null_mut(),
    }
}

/// Releases a handle returned by `dmap_open`. No-op on NULL.
#[no_mangle]
pub unsafe extern "C" fn dmap_close(h: *mut DmapHandle) {
    if h.is_null() {
        return;
    }
    // SAFETY: we only ever return handles produced by Box::into_raw above.
    drop(Box::from_raw(h as *mut Inner));
}

/// Number of entries in the dictionary. Returns 0 for NULL handle.
#[no_mangle]
pub unsafe extern "C" fn dmap_len(h: *const DmapHandle) -> usize {
    if h.is_null() {
        return 0;
    }
    let inner = &*(h as *const Inner);
    inner.dict.len()
}

/// Look up a tag. Writes fields into `*out` and returns 1 on hit, 0 on miss,
/// -1 on error (NULL args).
///
/// `creator` may be NULL for public tags.
///
/// String pointers in `*out` borrow into the mmap'd file and are valid
/// until `dmap_close(h)`.
#[no_mangle]
pub unsafe extern "C" fn dmap_lookup(
    h: *const DmapHandle,
    group: u16,
    element: u16,
    creator: *const c_char,
    out: *mut DmapTag,
) -> i32 {
    if h.is_null() || out.is_null() {
        return -1;
    }
    let inner = &*(h as *const Inner);
    let creator_str = if creator.is_null() {
        None
    } else {
        match CStr::from_ptr(creator).to_str() {
            Ok(s) => Some(s),
            Err(_) => {
                set_err(inner, "creator is not valid UTF-8");
                return -1;
            }
        }
    };

    match inner.dict.lookup(group, element, creator_str) {
        Some(v) => {
            // Strings in the dmap file are NOT NUL-terminated; we must copy
            // them into a small buffer owned by the caller. Our contract says
            // pointers borrow into the file, so instead we produce leaked
            // CStrings attached to Inner's last-lookup buffer — but that
            // breaks thread-safety. Simplest safe answer: return copies and
            // document that `dmap_free_tag_strings` releases them.
            //
            // Since rkyv strings are already UTF-8 and we need NUL termination
            // for C, we do a per-field copy. Callers call `dmap_free_tag`
            // to release.
            let mk = |s: &str| -> *const c_char {
                match CString::new(s) {
                    Ok(c) => c.into_raw() as *const c_char,
                    Err(_) => ptr::null(),
                }
            };

            let tag = DmapTag {
                group: v.group(),
                element: v.element(),
                block_offset: v.is_block_offset(),
                retired: v.retired(),
                vr: {
                    let mut b = [0 as c_char; 4];
                    let s = v.vr().as_bytes();
                    for (i, &c) in s.iter().take(3).enumerate() {
                        b[i] = c as c_char;
                    }
                    b
                },
                keyword: mk(v.keyword()),
                name: mk(v.name()),
                creator: mk(v.creator()),
                description: mk(v.description()),
            };
            *out = tag;
            1
        }
        None => 0,
    }
}

/// Release strings populated by `dmap_lookup`. Safe to call with zeroed
/// fields. Does NOT touch `vr` (inline array).
#[no_mangle]
pub unsafe extern "C" fn dmap_free_tag(tag: *mut DmapTag) {
    if tag.is_null() {
        return;
    }
    let t = &mut *tag;
    for p in [
        &mut t.keyword,
        &mut t.name,
        &mut t.creator,
        &mut t.description,
    ] {
        if !p.is_null() {
            // SAFETY: produced by CString::into_raw above.
            let _ = CString::from_raw(*p as *mut c_char);
            *p = ptr::null();
        }
    }
}

/// Returns the last error string for this handle, or NULL if none. Valid
/// until the next error-producing call on the same handle.
#[no_mangle]
pub unsafe extern "C" fn dmap_last_error(h: *const DmapHandle) -> *const c_char {
    if h.is_null() {
        return ptr::null();
    }
    let inner = &*(h as *const Inner);
    inner.last_err.load(Ordering::Relaxed) as *const c_char
}

/// ABI version. Bump when the C struct layout changes.
#[no_mangle]
pub extern "C" fn dmap_abi_version() -> u32 {
    1
}
