use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList};
use std::sync::Arc;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

/// Python wrapper for a single SQL row
#[pyclass]
pub struct PyRow {
    pub values: Vec<(String, PyObject)>,
}

#[pymethods]
impl PyRow {
    pub fn keys(&self, py: Python<'_>) -> PyObject {
        let list = PyList::new_bound(py, self.values.iter().map(|(k, _)| k.as_str()));
        list.into()
    }

    pub fn __getitem__(&self, py: Python<'_>, key: &str) -> PyResult<PyObject> {
        self.values.iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.clone_ref(py))
            .ok_or_else(|| pyo3::exceptions::PyKeyError::new_err(key.to_string()))
    }

    pub fn as_dict(&self, py: Python<'_>) -> PyObject {
        let dict = PyDict::new_bound(py);
        for (k, v) in &self.values {
            dict.set_item(k, v.clone_ref(py)).unwrap();
        }
        dict.into()
    }

    pub fn __repr__(&self, py: Python<'_>) -> String {
        let items: Vec<String> = self.values.iter()
            .map(|(k, v)| format!("{}: {}", k, v.bind(py).repr().unwrap()))
            .collect();
        format!("Row({{{}}})", items.join(", "))
    }
}

static SESSION_COUNTER: AtomicU64 = AtomicU64::new(1);

/// Python wrapper for the icedb engine
#[pyclass]
pub struct PyConnection {
    engine: Arc<sql::engine::QueryEngine>,
    session_id: String,
    in_transaction: bool,
}

#[pymethods]
impl PyConnection {
    /// Execute a SQL statement and return a list of rows.
    pub fn execute(&self, py: Python<'_>, sql: &str) -> PyResult<Vec<PyRow>> {
        let result = self.engine.execute_session(&self.session_id, sql)
            .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))?;

        let rows = result.rows.into_iter().map(|row| {
            let values = row.schema.iter().zip(row.values.iter())
                .map(|((col_name, _col_type), value)| {
                    let py_val = value_to_python(py, value);
                    (col_name.clone(), py_val)
                })
                .collect();
            PyRow { values }
        }).collect();

        Ok(rows)
    }

    /// Execute a SQL statement and return the number of affected rows.
    pub fn execute_dml(&self, sql: &str) -> PyResult<u64> {
        let result = self.engine.execute_session(&self.session_id, sql)
            .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))?;
        Ok(result.rows_affected)
    }

    /// Begin a transaction.
    pub fn begin(&mut self) -> PyResult<()> {
        self.engine.execute_session(&self.session_id, "BEGIN")
            .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))?;
        self.in_transaction = true;
        Ok(())
    }

    /// Commit the current transaction.
    pub fn commit(&mut self) -> PyResult<()> {
        self.engine.execute_session(&self.session_id, "COMMIT")
            .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))?;
        self.in_transaction = false;
        Ok(())
    }

    /// Rollback the current transaction.
    pub fn rollback(&mut self) -> PyResult<()> {
        self.engine.execute_session(&self.session_id, "ROLLBACK")
            .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))?;
        self.in_transaction = false;
        Ok(())
    }

    /// Context manager entry — begins a transaction.
    pub fn __enter__(mut slf: PyRefMut<'_, Self>) -> PyResult<PyRefMut<'_, Self>> {
        slf.engine.execute_session(&slf.session_id.clone(), "BEGIN")
            .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))?;
        slf.in_transaction = true;
        Ok(slf)
    }

    /// Context manager exit — commits on success, rolls back on exception.
    pub fn __exit__(
        &mut self,
        exc_type: Option<&Bound<'_, pyo3::types::PyAny>>,
        _exc_val: Option<&Bound<'_, pyo3::types::PyAny>>,
        _exc_tb: Option<&Bound<'_, pyo3::types::PyAny>>,
    ) -> PyResult<bool> {
        if exc_type.is_some() {
            let _ = self.engine.execute_session(&self.session_id, "ROLLBACK");
            self.in_transaction = false;
            Ok(false) // re-raise the exception
        } else {
            self.engine.execute_session(&self.session_id, "COMMIT")
                .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))?;
            self.in_transaction = false;
            Ok(false)
        }
    }
}

fn value_to_python(py: Python<'_>, value: &sql::value::Value) -> PyObject {
    match value {
        sql::value::Value::Null => py.None(),
        sql::value::Value::Bool(b) => b.into_py(py),
        sql::value::Value::Int4(i) => i.into_py(py),
        sql::value::Value::Int8(i) => i.into_py(py),
        sql::value::Value::Float8(f) => f.into_py(py),
        sql::value::Value::Text(s) => s.into_py(py),
        sql::value::Value::Bytes(b) => pyo3::types::PyBytes::new_bound(py, b).into(),
        sql::value::Value::Date(d) => d.into_py(py),
        sql::value::Value::Timestamp(ts) => ts.into_py(py),
        sql::value::Value::Numeric(s) => s.into_py(py),
        sql::value::Value::Uuid(s) => s.into_py(py),
        sql::value::Value::Array(items) => {
            let list = PyList::new_bound(py, items.iter().map(|v| value_to_python(py, v)));
            list.into()
        }
        sql::value::Value::Json(j) => j.to_string().into_py(py),
    }
}

/// Open an embedded icedb connection
#[pyfunction]
pub fn connect(data_dir: &str) -> PyResult<PyConnection> {
    let data_dir = PathBuf::from(data_dir);
    std::fs::create_dir_all(&data_dir)
        .map_err(|e| pyo3::exceptions::PyIOError::new_err(e.to_string()))?;

    let wal_writer = Arc::new(wal::WalWriter::open(&data_dir)
        .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))?);
    let txn_manager = Arc::new(txn::TransactionManager::new(Arc::clone(&wal_writer)));
    let catalog = Arc::new(catalog::CatalogManager::open(&data_dir, Arc::clone(&wal_writer), Arc::clone(&txn_manager))
        .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))?);
    let engine = Arc::new(sql::QueryEngine::new(txn_manager, catalog, data_dir));

    let session_id = format!("py-session-{}", SESSION_COUNTER.fetch_add(1, Ordering::Relaxed));
    Ok(PyConnection { engine, session_id, in_transaction: false })
}

/// Python module definition
#[pymodule]
fn icedb(_py: Python<'_>, m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(connect, m)?)?;
    m.add_class::<PyConnection>()?;
    m.add_class::<PyRow>()?;
    Ok(())
}
