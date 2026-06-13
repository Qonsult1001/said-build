//! PyO3 bindings for sca-core — exposes SCAPlugin to Python.
//!
//! API matches LAM/MTEB naming: index() + search()

use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList};
use sca_core::plugin::SCAPlugin;

#[pyclass(name = "SCAPlugin")]
struct PySCAPlugin {
    inner: SCAPlugin,
}

#[pymethods]
impl PySCAPlugin {
    #[new]
    #[pyo3(signature = (chunk_size=4096))]
    fn new(chunk_size: usize) -> Self {
        Self { inner: SCAPlugin::with_chunk_size(chunk_size) }
    }

    /// Index a document (text only). Matches LAM.index(doc_id, text).
    fn index(&mut self, id: &str, text: &str) {
        self.inner.index(id, text);
    }

    /// Index a document with embedding. Matches LAM.index(doc_id, text, embedding=True).
    fn index_with_embedding(&mut self, id: &str, text: &str, embedding: Vec<f32>) {
        self.inner.index_with_embedding(id, text, &embedding);
    }

    /// Search indexed documents. Matches LAM.search(query, top_k).
    fn search(&mut self, py: Python, query: &str, top_k: usize) -> PyResult<Py<PyList>> {
        let hits = self.inner.search(query, top_k);
        hits_to_pylist(py, &hits)
    }

    /// Search with query embedding. Matches LAM.search(query, embedding=query_emb).
    fn search_with_embedding(
        &mut self, py: Python, query: &str, embedding: Vec<f32>, top_k: usize,
    ) -> PyResult<Py<PyList>> {
        let hits = self.inner.search_with_embedding(query, &embedding, top_k);
        hits_to_pylist(py, &hits)
    }

    /// Exact token match.
    fn search_exact(&self, py: Python, query: &str) -> PyResult<Py<PyList>> {
        let hits = self.inner.search_exact(query);
        hits_to_pylist(py, &hits)
    }

    /// Save index. Matches LAM.save_index(path).
    fn save(&self, path: &str) -> PyResult<()> {
        self.inner.save(path).map_err(|e| PyErr::new::<pyo3::exceptions::PyIOError, _>(e))
    }

    /// Load index. Matches LAM.load_index(path).
    #[staticmethod]
    fn load(path: &str) -> PyResult<Self> {
        SCAPlugin::load(path)
            .map(|inner| Self { inner })
            .map_err(|e| PyErr::new::<pyo3::exceptions::PyIOError, _>(e))
    }

    fn len(&self) -> usize { self.inner.len() }
    fn is_empty(&self) -> bool { self.inner.is_empty() }
    fn token_count(&self) -> usize { self.inner.token_count() }
    fn vocab_size(&self) -> usize { self.inner.vocab_size() }
    fn contains(&self, id: &str) -> bool { self.inner.contains(id) }
    fn get_document(&self, id: &str) -> Option<String> { self.inner.get_document(id).map(|s| s.to_string()) }

    fn __len__(&self) -> usize { self.inner.len() }
    fn __contains__(&self, id: &str) -> bool { self.inner.contains(id) }
    fn __repr__(&self) -> String {
        let s = self.inner.stats();
        format!("SCAPlugin(docs={}, tokens={}, vocab={})", s.docs, s.tokens, s.vocab)
    }
}

fn hits_to_pylist(py: Python, hits: &[sca_core::Hit]) -> PyResult<Py<PyList>> {
    let list = PyList::empty_bound(py);
    for hit in hits {
        let dict = PyDict::new_bound(py);
        dict.set_item("id", &hit.id)?;
        dict.set_item("score", hit.score)?;
        list.append(dict)?;
    }
    Ok(list.into())
}

#[pymodule]
fn _native(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PySCAPlugin>()?;
    m.add("__version__", "0.1.0")?;
    Ok(())
}
