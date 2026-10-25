//! C ABI bindings for `dicom-map`.
//!
//! Consumers link against `libdicom_map_ffi` (shared or static) and include
//! `dicom_map.h`.
//!
//! Ownership:
//!   * `DmapHandle` is released with `dmap_close`.
//!   * `DmapTag` is allocated by `dmap_lookup`, read through `dmap_tag_*`
//!     getters and released with `dmap_tag_free`. Strings it returns stay
//!     valid until the tag is freed, even after the handle is closed.
//!   * `DmapErr` is allocated by `dmap_open_ex` on failure and released with
//!     `dmap_err_free`.
//!   * A handle may be used for lookups from several threads at once.
//!   * `dmap_last_error` reports the last lookup error on the calling thread
//!     (like `errno`), not per handle. Its string is valid until the next
//!     error on the same thread.

use std::cell::RefCell;
use std::ffi::{c_char, CStr, CString};
use std::path::PathBuf;
use std::ptr;

use dicom_map::{DmapDict, DmapError, TagView};

pub const DMAP_OK: i32 = 0;
pub const DMAP_ERR_IO: i32 = 1;
pub const DMAP_ERR_BAD_MAGIC: i32 = 2;
pub const DMAP_ERR_UNSUPPORTED_VERSION: i32 = 3;
pub const DMAP_ERR_BAD_ARCHIVE: i32 = 4;
pub const DMAP_ERR_TOO_SMALL: i32 = 5;
pub const DMAP_ERR_INVALID_ARGUMENT: i32 = 6;
pub const DMAP_ERR_OTHER: i32 = 255;

/// Opaque handle.
#[repr(C)]
pub struct DmapHandle {
    _opaque: [u8; 0],
}

/// Opaque tag record.
#[repr(C)]
pub struct DmapTag {
    _opaque: [u8; 0],
}

/// Opaque open error.
#[repr(C)]
pub struct DmapErr {
    _opaque: [u8; 0],
}

struct Inner {
    dict: DmapDict,
}

thread_local! {
    static LAST_ERR: RefCell<Option<CString>> = const { RefCell::new(None) };
}

struct TagOwned {
    group: u16,
    element: u16,
    flags: u32,
    vm: (u16, u16, u16),
    vr: CString,
    keyword: CString,
    name: CString,
    creator: CString,
    description: CString,
    sources: CString,
}

struct ErrOwned {
    kind: i32,
    message: CString,
    version: Option<(u16, u16, u16)>,
}

fn cstring(s: &str) -> CString {
    // Interior NULs are replaced; used for messages only.
    CString::new(s.replace('\0', "?")).unwrap_or_default()
}

fn set_err(msg: &str) {
    LAST_ERR.with(|e| *e.borrow_mut() = Some(cstring(msg)));
}

fn classify(e: &DmapError) -> ErrOwned {
    let (kind, version) = match e {
        DmapError::Io(_) => (DMAP_ERR_IO, None),
        DmapError::BadMagic { .. } => (DMAP_ERR_BAD_MAGIC, None),
        DmapError::UnsupportedVersion {
            got_major,
            got_minor,
            supported_major,
        } => (
            DMAP_ERR_UNSUPPORTED_VERSION,
            Some((*got_major, *got_minor, *supported_major)),
        ),
        DmapError::BadArchive { .. } => (DMAP_ERR_BAD_ARCHIVE, None),
        DmapError::TooSmall { .. } => (DMAP_ERR_TOO_SMALL, None),
        _ => (DMAP_ERR_OTHER, None),
    };
    ErrOwned {
        kind,
        message: cstring(&e.to_string()),
        version,
    }
}

unsafe fn open_inner(path: *const c_char) -> Result<Box<Inner>, ErrOwned> {
    let invalid = |m: &str| ErrOwned {
        kind: DMAP_ERR_INVALID_ARGUMENT,
        message: cstring(m),
        version: None,
    };
    if path.is_null() {
        return Err(invalid("path is NULL"));
    }
    let s = CStr::from_ptr(path)
        .to_str()
        .map_err(|_| invalid("path is not valid UTF-8"))?;
    let dict = DmapDict::open(PathBuf::from(s)).map_err(|e| classify(&e))?;
    Ok(Box::new(Inner { dict }))
}

/// Opens a `.dmap` file. Returns `DMAP_OK` and sets `*out`, or a `DMAP_ERR_*`
/// status. When `err` is not NULL a failure also sets `*err` to an error that
/// the caller releases with `dmap_err_free`.
///
/// # Safety
///
/// `path` must be NULL or a NUL-terminated string valid for reads. `out` must
/// be a valid pointer to a `DmapHandle*`. `err` must be NULL or a valid
/// pointer to a `DmapErr*`.
#[no_mangle]
pub unsafe extern "C" fn dmap_open_ex(
    path: *const c_char,
    out: *mut *mut DmapHandle,
    err: *mut *mut DmapErr,
) -> i32 {
    if !err.is_null() {
        *err = ptr::null_mut();
    }
    if out.is_null() {
        return DMAP_ERR_INVALID_ARGUMENT;
    }
    *out = ptr::null_mut();
    match open_inner(path) {
        Ok(inner) => {
            *out = Box::into_raw(inner) as *mut DmapHandle;
            DMAP_OK
        }
        Err(e) => {
            let kind = e.kind;
            if !err.is_null() {
                *err = Box::into_raw(Box::new(e)) as *mut DmapErr;
            }
            kind
        }
    }
}

/// Opens a `.dmap` file; NULL on failure with no cause. Prefer `dmap_open_ex`.
///
/// # Safety
///
/// `path` must be NULL or a NUL-terminated string valid for reads.
#[no_mangle]
pub unsafe extern "C" fn dmap_open(path: *const c_char) -> *mut DmapHandle {
    let mut h = ptr::null_mut();
    dmap_open_ex(path, &mut h, ptr::null_mut());
    h
}

/// Releases a handle. NULL-safe.
///
/// # Safety
///
/// `h` must be NULL or a handle from `dmap_open*` not yet closed.
#[no_mangle]
pub unsafe extern "C" fn dmap_close(h: *mut DmapHandle) {
    if !h.is_null() {
        drop(Box::from_raw(h as *mut Inner));
    }
}

/// Number of entries. Returns a status; `*out` is set only on `DMAP_OK`.
///
/// # Safety
///
/// `h` must be NULL or a live handle; `out` must be NULL or writable.
#[no_mangle]
pub unsafe extern "C" fn dmap_len(h: *const DmapHandle, out: *mut usize) -> i32 {
    if h.is_null() || out.is_null() {
        return DMAP_ERR_INVALID_ARGUMENT;
    }
    *out = (*(h as *const Inner)).dict.len();
    DMAP_OK
}

fn to_owned(v: &TagView<'_>) -> Result<TagOwned, &'static str> {
    let c = |s: &str, what: &'static str| CString::new(s).map_err(|_| what);
    let vm = v.vm();
    Ok(TagOwned {
        group: v.group(),
        element: v.element(),
        flags: v.flags().bits(),
        vm: (vm.min, vm.max, vm.step),
        vr: c(v.vr(), "vr contains a NUL byte")?,
        keyword: c(v.keyword(), "keyword contains a NUL byte")?,
        name: c(v.name(), "name contains a NUL byte")?,
        creator: c(v.creator(), "creator contains a NUL byte")?,
        description: c(v.description(), "description contains a NUL byte")?,
        sources: c(v.sources_raw(), "sources contains a NUL byte")?,
    })
}

/// Look up a tag. Returns 1 on hit (`*out` is a new `DmapTag`, release with
/// `dmap_tag_free`), 0 on miss (`*out` untouched), -1 on error (NULL
/// arguments, a creator that is not UTF-8, or a record whose text contains a
/// NUL byte; `dmap_last_error` says which).
///
/// `creator` may be NULL for public tags.
///
/// # Safety
///
/// `h` must be NULL or a live handle. `creator` must be NULL or a
/// NUL-terminated string valid for reads. `out` must be NULL or writable.
#[no_mangle]
pub unsafe extern "C" fn dmap_lookup(
    h: *const DmapHandle,
    group: u16,
    element: u16,
    creator: *const c_char,
    out: *mut *mut DmapTag,
) -> i32 {
    if h.is_null() || out.is_null() {
        set_err("dmap_lookup: NULL handle or output pointer");
        return -1;
    }
    let inner = &*(h as *const Inner);
    let creator_str = if creator.is_null() {
        None
    } else {
        match CStr::from_ptr(creator).to_str() {
            Ok(s) => Some(s),
            Err(_) => {
                set_err("creator is not valid UTF-8");
                return -1;
            }
        }
    };
    match inner.dict.lookup(group, element, creator_str) {
        Some(v) => match to_owned(&v) {
            Ok(t) => {
                *out = Box::into_raw(Box::new(t)) as *mut DmapTag;
                1
            }
            Err(m) => {
                set_err(m);
                -1
            }
        },
        None => 0,
    }
}

/// Releases a tag. NULL-safe.
///
/// # Safety
///
/// `t` must be NULL or a tag from `dmap_lookup` not yet freed.
#[no_mangle]
pub unsafe extern "C" fn dmap_tag_free(t: *mut DmapTag) {
    if !t.is_null() {
        drop(Box::from_raw(t as *mut TagOwned));
    }
}

unsafe fn tag<'a>(t: *const DmapTag) -> Option<&'a TagOwned> {
    (t as *const TagOwned).as_ref()
}

macro_rules! getter_num {
    ($name:ident, $ty:ty, |$t:ident| $e:expr) => {
        /// Field getter; returns 0 for a NULL tag.
        ///
        /// # Safety
        ///
        /// `t` must be NULL or a live tag.
        #[no_mangle]
        pub unsafe extern "C" fn $name(t: *const DmapTag) -> $ty {
            match tag(t) {
                Some($t) => $e,
                None => 0 as $ty,
            }
        }
    };
}
macro_rules! getter_str {
    ($name:ident, $field:ident) => {
        /// String getter; NUL-terminated UTF-8, never NULL for a live tag
        /// (empty when the field is empty), valid until `dmap_tag_free`.
        /// Returns NULL for a NULL tag.
        ///
        /// # Safety
        ///
        /// `t` must be NULL or a live tag.
        #[no_mangle]
        pub unsafe extern "C" fn $name(t: *const DmapTag) -> *const c_char {
            match tag(t) {
                Some(t) => t.$field.as_ptr(),
                None => ptr::null(),
            }
        }
    };
}

getter_num!(dmap_tag_group, u16, |t| t.group);
getter_num!(dmap_tag_element, u16, |t| t.element);
getter_num!(dmap_tag_flags, u32, |t| t.flags);
getter_num!(dmap_tag_vm_min, u16, |t| t.vm.0);
getter_num!(dmap_tag_vm_max, u16, |t| t.vm.1);
getter_num!(dmap_tag_vm_step, u16, |t| t.vm.2);
getter_str!(dmap_tag_vr, vr);
getter_str!(dmap_tag_keyword, keyword);
getter_str!(dmap_tag_name, name);
getter_str!(dmap_tag_creator, creator);
getter_str!(dmap_tag_description, description);
getter_str!(dmap_tag_sources, sources);

/// Last lookup error on the calling thread, or NULL. `h` is accepted for
/// ABI compatibility and may be NULL. Valid until the next error on the same
/// thread.
///
/// # Safety
///
/// `h` is not dereferenced.
#[no_mangle]
pub unsafe extern "C" fn dmap_last_error(_h: *const DmapHandle) -> *const c_char {
    LAST_ERR.with(|e| e.borrow().as_ref().map_or(ptr::null(), |c| c.as_ptr()))
}

/// Status of an open error (a `DMAP_ERR_*` value). 0 for NULL.
///
/// # Safety
///
/// `e` must be NULL or a live error.
#[no_mangle]
pub unsafe extern "C" fn dmap_err_kind(e: *const DmapErr) -> i32 {
    (e as *const ErrOwned).as_ref().map_or(0, |e| e.kind)
}

/// Human-readable message, valid until `dmap_err_free`. NULL for NULL.
///
/// # Safety
///
/// `e` must be NULL or a live error.
#[no_mangle]
pub unsafe extern "C" fn dmap_err_message(e: *const DmapErr) -> *const c_char {
    (e as *const ErrOwned)
        .as_ref()
        .map_or(ptr::null(), |e| e.message.as_ptr())
}

/// For `DMAP_ERR_UNSUPPORTED_VERSION`: writes the file's major and minor and
/// the major this library reads, and returns 1. Returns 0 for other errors.
///
/// # Safety
///
/// `e` must be NULL or a live error; the three pointers must be writable.
#[no_mangle]
pub unsafe extern "C" fn dmap_err_version(
    e: *const DmapErr,
    got_major: *mut u16,
    got_minor: *mut u16,
    supported_major: *mut u16,
) -> i32 {
    match (e as *const ErrOwned).as_ref().and_then(|e| e.version) {
        Some((a, b, c))
            if !got_major.is_null() && !got_minor.is_null() && !supported_major.is_null() =>
        {
            *got_major = a;
            *got_minor = b;
            *supported_major = c;
            1
        }
        _ => 0,
    }
}

/// Releases an error. NULL-safe.
///
/// # Safety
///
/// `e` must be NULL or an error from `dmap_open_ex` not yet freed.
#[no_mangle]
pub unsafe extern "C" fn dmap_err_free(e: *mut DmapErr) {
    if !e.is_null() {
        drop(Box::from_raw(e as *mut ErrOwned));
    }
}

/// ABI major. Changes only when an existing function is removed or its
/// behavior changes incompatibly.
#[no_mangle]
pub extern "C" fn dmap_abi_version() -> u32 {
    2
}

/// ABI minor. Grows when functions or `DMAP_*` constants are added.
#[no_mangle]
pub extern "C" fn dmap_abi_minor() -> u32 {
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use dicom_map::{DmapBuilder, NewRecord, Vm};

    fn archive(name: &str, vr: &str) -> Vec<u8> {
        let mut b = DmapBuilder::new();
        // NUL in text is refused by the builder; patch it in afterwards.
        b.push(
            NewRecord::new(0x0008, 0x0005)
                .vr(vr)
                .name(name)
                .vm(Vm::new(1, 1, 1)),
        );
        b.finish().unwrap()
    }

    fn open(tag: &str, bytes: &[u8]) -> (i32, *mut DmapHandle, *mut DmapErr) {
        let path = std::env::temp_dir().join(format!("dmap-ffi-{}-{tag}.dmap", std::process::id()));
        std::fs::write(&path, bytes).unwrap();
        let c = CString::new(path.to_str().unwrap()).unwrap();
        let (mut h, mut e) = (ptr::null_mut(), ptr::null_mut());
        let rc = unsafe { dmap_open_ex(c.as_ptr(), &mut h, &mut e) };
        let _ = std::fs::remove_file(&path);
        (rc, h, e)
    }

    fn s(p: *const c_char) -> String {
        unsafe { CStr::from_ptr(p) }.to_str().unwrap().to_owned()
    }

    #[test]
    fn open_errors_carry_a_kind_and_message() {
        let cases: [(&str, Vec<u8>, i32); 4] = [
            ("magic", b"XXXX".repeat(16), DMAP_ERR_BAD_MAGIC),
            ("small", b"DMAP".to_vec(), DMAP_ERR_TOO_SMALL),
            (
                "bad",
                {
                    let mut v = archive("n", "CS");
                    v.truncate(v.len() - 3);
                    v
                },
                DMAP_ERR_BAD_ARCHIVE,
            ),
            (
                "ver",
                {
                    let mut v = archive("n", "CS");
                    v[4] = 9;
                    v
                },
                DMAP_ERR_UNSUPPORTED_VERSION,
            ),
        ];
        for (tag, bytes, want) in cases {
            let (rc, h, e) = open(tag, &bytes);
            assert_eq!(rc, want, "{tag}");
            assert!(h.is_null());
            assert_eq!(unsafe { dmap_err_kind(e) }, want);
            assert!(!s(unsafe { dmap_err_message(e) }).is_empty());
            if want == DMAP_ERR_UNSUPPORTED_VERSION {
                let (mut a, mut b, mut c) = (0u16, 0u16, 0u16);
                assert_eq!(unsafe { dmap_err_version(e, &mut a, &mut b, &mut c) }, 1);
                assert_eq!((a, c), (9, 4));
            }
            unsafe { dmap_err_free(e) };
        }
        let missing = CString::new("/nonexistent/x.dmap").unwrap();
        let (mut h, mut e) = (ptr::null_mut(), ptr::null_mut());
        assert_eq!(
            unsafe { dmap_open_ex(missing.as_ptr(), &mut h, &mut e) },
            DMAP_ERR_IO
        );
        unsafe { dmap_err_free(e) };
        assert!(unsafe { dmap_open(missing.as_ptr()) }.is_null());
        assert_eq!(
            unsafe { dmap_open_ex(ptr::null(), &mut h, ptr::null_mut()) },
            DMAP_ERR_INVALID_ARGUMENT
        );
    }

    #[test]
    fn len_reports_status_not_zero_for_null() {
        let mut n = 99usize;
        assert_eq!(
            unsafe { dmap_len(ptr::null(), &mut n) },
            DMAP_ERR_INVALID_ARGUMENT
        );
        assert_eq!(n, 99);
        let (rc, h, _) = open("len", &archive("n", "CS"));
        assert_eq!(rc, DMAP_OK);
        assert_eq!(unsafe { dmap_len(h, &mut n) }, DMAP_OK);
        assert_eq!(n, 1);
        unsafe { dmap_close(h) };
    }

    #[test]
    fn tag_is_read_through_getters_and_outlives_the_handle() {
        for vr in ["CS", "US or SS", "US or SS or OW", "??"] {
            let (rc, h, _) = open("vr", &archive("Name", vr));
            assert_eq!(rc, DMAP_OK);
            let mut t = ptr::null_mut();
            assert_eq!(
                unsafe { dmap_lookup(h, 0x0008, 0x0005, ptr::null(), &mut t) },
                1
            );
            unsafe { dmap_close(h) };
            unsafe {
                assert_eq!(dmap_tag_group(t), 0x0008);
                assert_eq!(dmap_tag_element(t), 0x0005);
                assert_eq!(s(dmap_tag_vr(t)), vr);
                assert_eq!(s(dmap_tag_name(t)), "Name");
                assert_eq!(s(dmap_tag_creator(t)), "");
                assert_eq!(
                    (dmap_tag_vm_min(t), dmap_tag_vm_max(t), dmap_tag_vm_step(t)),
                    (1, 1, 1)
                );
                dmap_tag_free(t);
            }
        }
        // NULL-safety.
        unsafe {
            assert_eq!(dmap_tag_group(ptr::null()), 0);
            assert!(dmap_tag_name(ptr::null()).is_null());
            dmap_tag_free(ptr::null_mut());
        }
    }

    #[test]
    fn interior_nul_is_reported_not_returned_as_null() {
        let mut bytes = archive("abc", "CS");
        // Overwrite one pooled name byte with NUL after building.
        let pos = bytes.windows(3).position(|w| w == b"abc").unwrap();
        bytes[pos + 1] = 0;
        let (rc, h, _) = open("nul", &bytes);
        assert_eq!(rc, DMAP_OK);
        let mut t = ptr::null_mut();
        assert_eq!(
            unsafe { dmap_lookup(h, 0x0008, 0x0005, ptr::null(), &mut t) },
            -1
        );
        assert!(t.is_null());
        let msg = s(unsafe { dmap_last_error(h) });
        assert!(msg.contains("name") && msg.contains("NUL"), "{msg}");
        unsafe { dmap_close(h) };
    }

    #[test]
    fn null_arguments_set_the_last_error() {
        let mut t = ptr::null_mut();
        let (rc, h, _) = open("nullargs", &archive("n", "CS"));
        assert_eq!(rc, DMAP_OK);
        assert_eq!(
            unsafe { dmap_lookup(ptr::null(), 8, 5, ptr::null(), &mut t) },
            -1
        );
        assert!(s(unsafe { dmap_last_error(h) }).contains("NULL"));
        // A different thread does not see this thread's error, and an error
        // on another thread does not replace ours.
        let before = s(unsafe { dmap_last_error(ptr::null()) });
        std::thread::spawn(|| {
            assert!(unsafe { dmap_last_error(ptr::null()) }.is_null());
            assert_eq!(
                unsafe { dmap_lookup(ptr::null(), 8, 5, ptr::null(), ptr::null_mut()) },
                -1
            );
        })
        .join()
        .unwrap();
        assert_eq!(s(unsafe { dmap_last_error(ptr::null()) }), before);
        assert_eq!(
            unsafe { dmap_lookup(h, 8, 5, ptr::null(), ptr::null_mut()) },
            -1
        );
        unsafe { dmap_close(h) };
    }
}
