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
    /// VR text, NUL-terminated and zero-padded: a two-letter VR, one of
    /// `US or SS`, `OB or OW`, `US or OW`, `US or SS or OW`, or `??` when the
    /// source declares no VR.
    pub vr: [c_char; 16],
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
/// a non-null handle - for `dmap_open` itself, failure is only "bad path"
/// and you get no error string).
///
/// `path` must be a NUL-terminated UTF-8 string.
///
/// # Safety
///
/// `path` must be either null or a valid pointer to a NUL-terminated C
/// string, valid for reads for the duration of this call.
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
///
/// # Safety
///
/// `h` must be either null or a handle previously returned by `dmap_open`
/// that has not already been passed to `dmap_close`. After this call, `h`
/// and any `DmapTag` string pointers borrowed from it are invalid.
#[no_mangle]
pub unsafe extern "C" fn dmap_close(h: *mut DmapHandle) {
    if h.is_null() {
        return;
    }
    // SAFETY: we only ever return handles produced by Box::into_raw above.
    drop(Box::from_raw(h as *mut Inner));
}

/// Number of entries in the dictionary. Returns 0 for NULL handle.
///
/// # Safety
///
/// `h` must be either null or a handle returned by `dmap_open` that has not
/// since been passed to `dmap_close`.
#[no_mangle]
pub unsafe extern "C" fn dmap_len(h: *const DmapHandle) -> usize {
    if h.is_null() {
        return 0;
    }
    let inner = &*(h as *const Inner);
    inner.dict.len()
}

/// Look up a tag. Writes fields into `*out` and returns 1 on hit, 0 on miss,
/// -1 on error (NULL args, a creator that is not UTF-8, or a record whose
/// text contains a NUL byte; `dmap_last_error` says which).
///
/// `creator` may be NULL for public tags.
///
/// # Safety
///
/// `h` must be either null or a handle returned by `dmap_open` that has not
/// since been passed to `dmap_close`. `creator` must be either null or a
/// valid pointer to a NUL-terminated C string, valid for reads for the
/// duration of this call. `out` must be either null or a valid pointer to
/// a writable `DmapTag`.
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
            let mut tag = DmapTag {
                group: v.group(),
                element: v.element(),
                block_offset: v.is_block_offset(),
                retired: v.retired(),
                vr: [0 as c_char; 16],
                keyword: ptr::null(),
                name: ptr::null(),
                creator: ptr::null(),
                description: ptr::null(),
            };
            let vr = v.vr().as_bytes();
            if vr.len() >= tag.vr.len() {
                set_err(inner, "VR text does not fit in DmapTag.vr");
                return -1;
            }
            for (i, &c) in vr.iter().enumerate() {
                tag.vr[i] = c as c_char;
            }
            // A string with an interior NUL cannot be handed to C intact.
            // Report it; never return NULL where the header promises a string.
            let fields = [
                ("keyword", v.keyword()),
                ("name", v.name()),
                ("creator", v.creator()),
                ("description", v.description()),
            ];
            let mut ptrs = [ptr::null::<c_char>(); 4];
            for (i, (field, text)) in fields.iter().enumerate() {
                match CString::new(*text) {
                    Ok(c) => ptrs[i] = c.into_raw() as *const c_char,
                    Err(_) => {
                        set_err(inner, &format!("{field} contains a NUL byte"));
                        for p in ptrs.iter().filter(|p| !p.is_null()) {
                            // SAFETY: produced by CString::into_raw above.
                            drop(CString::from_raw(*p as *mut c_char));
                        }
                        return -1;
                    }
                }
            }
            tag.keyword = ptrs[0];
            tag.name = ptrs[1];
            tag.creator = ptrs[2];
            tag.description = ptrs[3];
            *out = tag;
            1
        }
        None => 0,
    }
}

/// Release strings populated by `dmap_lookup`. Safe to call with zeroed
/// fields. Does NOT touch `vr` (inline array).
///
/// # Safety
///
/// `tag` must be either null or a valid pointer to a `DmapTag` whose
/// `keyword`/`name`/`creator`/`description` fields are each either null or
/// were produced by `dmap_lookup` and not yet freed. Passing pointers from
/// any other source is undefined behavior.
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
///
/// # Safety
///
/// `h` must be either null or a handle returned by `dmap_open` that has not
/// since been passed to `dmap_close`.
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
    2
}

#[cfg(test)]
mod tests {
    use super::*;
    use dicom_map::schema::{Dictionary, FileHeader, IndexEntry, TagRecord, VrCode};

    /// Hand-built archive with one public record whose name is `name`.
    fn archive_with_name(name: &str, vr: VrCode) -> Vec<u8> {
        let name_bytes = name.as_bytes();
        let dict = Dictionary {
            index: vec![IndexEntry {
                group: 0x0008,
                element: 0x0005,
                creator_hash: 0,
                record_idx: 0,
            }],
            records: vec![TagRecord {
                group: 0x0008,
                element: 0x0005,
                element_is_block_offset: false,
                retired: false,
                vr,
                vm_min: 1,
                vm_max: 1,
                vm_step: 1,
                keyword_off: 0,
                keyword_len: 0,
                name_off: 0,
                name_len: name_bytes.len() as u16,
                creator_off: 0,
                creator_len: 0,
                description_off: 0,
                description_len: 0,
                sources_off: 0,
                sources_len: 0,
            }],
            strings: name_bytes.to_vec(),
        };
        let body = rkyv::to_bytes::<rkyv::rancor::Error>(&dict).unwrap();
        let mut out = FileHeader::new(body.len() as u64).to_bytes().to_vec();
        out.extend_from_slice(&body);
        out
    }

    fn open(tag: &str, bytes: &[u8]) -> *mut DmapHandle {
        let path = std::env::temp_dir().join(format!("dmap-ffi-{}-{tag}.dmap", std::process::id()));
        std::fs::write(&path, bytes).unwrap();
        let c = CString::new(path.to_str().unwrap()).unwrap();
        let h = unsafe { dmap_open(c.as_ptr()) };
        let _ = std::fs::remove_file(&path);
        h
    }

    fn blank() -> DmapTag {
        DmapTag {
            group: 0,
            element: 0,
            block_offset: false,
            retired: false,
            vr: [0; 16],
            keyword: ptr::null(),
            name: ptr::null(),
            creator: ptr::null(),
            description: ptr::null(),
        }
    }

    #[test]
    fn interior_nul_is_reported_not_returned_as_null() {
        let h = open("nul", &archive_with_name("a\0b", VrCode::CS));
        assert!(!h.is_null());
        let mut t = blank();
        let rc = unsafe { dmap_lookup(h, 0x0008, 0x0005, ptr::null(), &mut t) };
        assert_eq!(rc, -1);
        let msg = unsafe { CStr::from_ptr(dmap_last_error(h)) }
            .to_str()
            .unwrap();
        assert!(msg.contains("name") && msg.contains("NUL"), "{msg}");
        assert!(t.name.is_null(), "no partial output");
        unsafe { dmap_close(h) };
    }

    #[test]
    fn multi_vr_text_is_not_truncated() {
        for (vr, text) in [
            (VrCode::UsOrSs, "US or SS"),
            (VrCode::ObOrOw, "OB or OW"),
            (VrCode::UsOrSsOrOw, "US or SS or OW"),
            (VrCode::Unknown, "??"),
        ] {
            let h = open("vr", &archive_with_name("ok", vr));
            let mut t = blank();
            let rc = unsafe { dmap_lookup(h, 0x0008, 0x0005, ptr::null(), &mut t) };
            assert_eq!(rc, 1);
            let got = unsafe { CStr::from_ptr(t.vr.as_ptr()) }.to_str().unwrap();
            assert_eq!(got, text);
            unsafe {
                dmap_free_tag(&mut t);
                dmap_close(h);
            }
        }
    }
}
