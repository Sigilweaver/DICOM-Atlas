//! Python bindings for `dicom-map` via PyO3.
//!
//! Minimal surface:
//!
//! ```python
//! import dicom_map
//! d = dicom_map.open("tags.dmap")
//! d.lookup(0x0008, 0x0005)              # public tag
//! d.lookup(0x0021, 0x0008, "Siemens: Thorax/Multix FD Lab Settings")
//! len(d)                                # number of entries
//! ```
//!
//! `lookup` returns a dict (or `None`) with stable keys:
//! `group`, `element`, `creator`, `keyword`, `name`, `vr`, `description`,
//! `retired`, `block_offset`, `sources`.

use std::path::PathBuf;
use std::sync::Arc;

use pyo3::exceptions::{PyIOError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::PyDict;

use ::dicom_map::{DmapDict, DmapError, TagView};

/// Opaque handle holding an mmap'd dictionary.
#[pyclass(module = "dicom_map._dicom_map", name = "Dict")]
struct PyDmapDict {
    inner: Arc<DmapDict>,
}

fn tag_view_to_pydict<'py>(py: Python<'py>, v: &TagView<'_>) -> PyResult<Bound<'py, PyDict>> {
    let d = PyDict::new_bound(py);
    d.set_item("group", v.group())?;
    d.set_item("element", v.element())?;
    d.set_item("creator", v.creator())?;
    d.set_item("keyword", v.keyword())?;
    d.set_item("name", v.name())?;
    d.set_item("vr", v.vr())?;
    d.set_item("description", v.description())?;
    d.set_item("retired", v.retired())?;
    d.set_item("block_offset", v.is_block_offset())?;
    d.set_item("sources", v.sources().collect::<Vec<_>>())?;
    Ok(d)
}

#[pymethods]
impl PyDmapDict {
    /// Look up a tag. Returns a dict or `None`.
    ///
    /// `element` is the low byte for private tags (the "xx" form).
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

    fn __len__(&self) -> usize {
        self.inner.len()
    }

    fn __repr__(&self) -> String {
        format!("<dicom_map.Dict len={}>", self.inner.len())
    }

    /// Context-manager support (no-op — mmap is released on drop).
    fn __enter__(slf: Py<Self>) -> Py<Self> {
        slf
    }

    #[pyo3(signature = (_exc_type=None, _exc=None, _tb=None))]
    fn __exit__(
        &self,
        _exc_type: Option<PyObject>,
        _exc: Option<PyObject>,
        _tb: Option<PyObject>,
    ) -> bool {
        false
    }
}

/// Open a `.dmap` file. Raises `IOError` on missing file or corruption,
/// `ValueError` on unsupported version.
#[pyfunction(name = "open")]
fn py_open(path: PathBuf) -> PyResult<PyDmapDict> {
    match DmapDict::open(&path) {
        Ok(d) => Ok(PyDmapDict { inner: Arc::new(d) }),
        Err(DmapError::UnsupportedVersion { got }) => Err(PyValueError::new_err(format!(
            "unsupported .dmap version {got}"
        ))),
        Err(e) => Err(PyIOError::new_err(format!(
            "cannot open {}: {e}",
            path.display()
        ))),
    }
}

#[pymodule]
fn _dicom_map(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PyDmapDict>()?;
    m.add_function(wrap_pyfunction!(py_open, m)?)?;
    m.add("__version__", env!("CARGO_PKG_VERSION"))?;
    Ok(())
}
