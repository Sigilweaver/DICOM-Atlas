//! Python bindings for `dicom-map` via PyO3.
//!
//! ```python
//! import dicom_map
//! d = dicom_map.open("tags.dmap")
//! d.lookup(0x0008, 0x0005)              # public tag
//! d.lookup(0x0021, 0x0008, "Siemens: Thorax/Multix FD Lab Settings")
//! len(d)                                # number of entries
//! for tag in d: ...                     # every record, as a dict
//! ```
//!
//! `lookup` returns a dict (or `None`). Keys: `group`, `element`, `creator`,
//! `keyword`, `name`, `vr`, `vm`, `description`, `retired`, `block_offset`,
//! `flags`, `sources`. New keys can appear in later releases.

#![forbid(unsafe_code)]
// pyo3's #[pyfunction]/#[pymethods] macro expansion inserts an internal
// PyErr -> PyErr conversion for functions that already return PyResult,
// which clippy flags as useless. Function-level #[allow] doesn't reach
// into the macro-generated code, so this is silenced crate-wide.
#![allow(clippy::useless_conversion)]

use std::path::PathBuf;
use std::sync::Arc;

use pyo3::prelude::*;
use pyo3::types::PyDict;

use ::dicom_map::{ArchiveCheck, DmapDict, DmapError, TagView};

/// Read-only handle holding an mmap'd dictionary. Safe to share between
/// threads.
#[pyclass(module = "dicom_map._dicom_map", name = "Dict", frozen)]
struct PyDmapDict {
    inner: Arc<DmapDict>,
}

#[pyclass(module = "dicom_map._dicom_map", name = "DictIterator", frozen)]
struct PyDictIter {
    inner: Arc<DmapDict>,
    pos: std::sync::atomic::AtomicUsize,
}

fn tag_view_to_pydict<'py>(py: Python<'py>, v: &TagView<'_>) -> PyResult<Bound<'py, PyDict>> {
    let d = PyDict::new(py);
    d.set_item("group", v.group())?;
    d.set_item("element", v.element())?;
    d.set_item("creator", v.creator())?;
    d.set_item("keyword", v.keyword())?;
    d.set_item("name", v.name())?;
    d.set_item("vr", v.vr())?;
    d.set_item("vm", v.vm().to_string())?;
    d.set_item("description", v.description())?;
    d.set_item("retired", v.retired())?;
    d.set_item("block_offset", v.is_block_offset())?;
    d.set_item("flags", v.flags().bits())?;
    d.set_item("sources", v.sources().collect::<Vec<_>>())?;
    Ok(d)
}

fn check_name(c: &ArchiveCheck) -> String {
    c.to_string()
}

/// Map a library error onto the `dicom_map.errors` hierarchy.
fn to_py_err(py: Python<'_>, e: DmapError, path: &std::path::Path) -> PyErr {
    let build = || -> PyResult<PyErr> {
        let errors = py.import("dicom_map.errors")?;
        let msg = format!("cannot open {}: {e}", path.display());
        let obj = match &e {
            DmapError::Io(io) => {
                let cls = errors.getattr("DmapIOError")?;
                match io.raw_os_error() {
                    Some(code) => cls.call1((code, io.to_string(), path.display().to_string()))?,
                    None => cls.call1((msg,))?,
                }
            }
            DmapError::UnsupportedVersion {
                got_major,
                got_minor,
                supported_major,
            } => {
                let o = errors.getattr("UnsupportedVersionError")?.call1((msg,))?;
                o.setattr("got_major", *got_major)?;
                o.setattr("got_minor", *got_minor)?;
                o.setattr("supported_major", *supported_major)?;
                o
            }
            DmapError::BadArchive { check, .. } => {
                let o = errors.getattr("BadArchiveError")?.call1((msg,))?;
                o.setattr("check", check_name(check))?;
                o
            }
            // Bad magic, too small, and anything added later.
            _ => errors.getattr("DmapFormatError")?.call1((msg,))?,
        };
        Ok(PyErr::from_value(obj))
    };
    match build() {
        Ok(err) => err,
        Err(import_err) => import_err,
    }
}

#[pymethods]
impl PyDmapDict {
    /// Look up a tag. Returns a dict or `None`.
    ///
    /// For private tags `element` may be the low byte (the "xx" form) or the
    /// element as written in a file; only the low byte is used.
    #[pyo3(signature = (group, element, creator=None))]
    fn lookup<'py>(
        &self,
        py: Python<'py>,
        group: u16,
        element: u16,
        creator: Option<&str>,
    ) -> PyResult<Option<Bound<'py, PyDict>>> {
        match self.inner.lookup(group, element, creator) {
            Some(v) => Ok(Some(tag_view_to_pydict(py, &v)?)),
            None => Ok(None),
        }
    }

    /// Metadata written by the compiler, such as `standard_edition`.
    fn meta<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        let d = PyDict::new(py);
        for (k, v) in self.inner.meta_iter() {
            d.set_item(k, v)?;
        }
        Ok(d)
    }

    fn __len__(&self) -> usize {
        self.inner.len()
    }

    fn __iter__(&self) -> PyDictIter {
        PyDictIter {
            inner: self.inner.clone(),
            pos: 0.into(),
        }
    }

    fn __repr__(&self) -> String {
        format!("<dicom_map.Dict len={}>", self.inner.len())
    }

    /// Context-manager support (no-op - mmap is released on drop).
    fn __enter__(slf: Py<Self>) -> Py<Self> {
        slf
    }

    #[pyo3(signature = (_exc_type=None, _exc=None, _tb=None))]
    fn __exit__(
        &self,
        _exc_type: Option<Py<PyAny>>,
        _exc: Option<Py<PyAny>>,
        _tb: Option<Py<PyAny>>,
    ) -> bool {
        false
    }
}

#[pymethods]
impl PyDictIter {
    fn __iter__(slf: PyRef<'_, Self>) -> PyRef<'_, Self> {
        slf
    }

    fn __next__<'py>(&self, py: Python<'py>) -> PyResult<Option<Bound<'py, PyDict>>> {
        let i = self.pos.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        match self.inner.nth(i) {
            Some(v) => Ok(Some(tag_view_to_pydict(py, &v)?)),
            None => Ok(None),
        }
    }
}

/// Open a `.dmap` file.
///
/// Raises `dicom_map.errors.DmapError` subclasses: `DmapIOError` (also an
/// `OSError`), `UnsupportedVersionError`, `BadArchiveError`,
/// `DmapFormatError`.
#[pyfunction(name = "open")]
fn py_open(py: Python<'_>, path: PathBuf) -> PyResult<PyDmapDict> {
    match DmapDict::open(&path) {
        Ok(d) => Ok(PyDmapDict { inner: Arc::new(d) }),
        Err(e) => Err(to_py_err(py, e, &path)),
    }
}

#[pymodule]
fn _dicom_map(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PyDmapDict>()?;
    m.add_class::<PyDictIter>()?;
    m.add_function(wrap_pyfunction!(py_open, m)?)?;
    m.add("__version__", env!("CARGO_PKG_VERSION"))?;
    Ok(())
}
