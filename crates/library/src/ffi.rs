//! C ABI. Handles and buffers are owned allocations, never entries in a global map.
use crate::App;
use anyhow::{ensure, Result};
use std::panic::{catch_unwind, AssertUnwindSafe};

pub struct SchistApp {
    app: App,
    poisoned: bool,
}

#[repr(C)]
pub struct SchistBuffer {
    pub data: *mut u8,
    pub len: usize,
}

impl SchistBuffer {
    fn empty() -> Self {
        Self {
            data: std::ptr::null_mut(),
            len: 0,
        }
    }
    fn from_bytes(bytes: Vec<u8>) -> Self {
        if bytes.is_empty() {
            return Self::empty();
        }
        let bytes = bytes.into_boxed_slice();
        let len = bytes.len();
        Self {
            data: Box::into_raw(bytes).cast(),
            len,
        }
    }
}

#[no_mangle]
pub extern "C" fn schist_abi_version() -> u32 {
    1
}

/// Returns null if construction panics. Does not install a panic hook or logger.
#[no_mangle]
pub extern "C" fn schist_create() -> *mut SchistApp {
    catch_unwind(|| {
        Box::into_raw(Box::new(SchistApp {
            app: App::new(),
            poisoned: false,
        }))
    })
    .unwrap_or(std::ptr::null_mut())
}

/// # Safety
/// `app` must be null or a live handle returned by schist_create, freed exactly
/// once, with no concurrent calls using it.
#[no_mangle]
pub unsafe extern "C" fn schist_destroy(app: *mut SchistApp) {
    if !app.is_null() {
        let _ = catch_unwind(AssertUnwindSafe(|| drop(Box::from_raw(app))));
    }
}

/// # Safety
/// `buffer` must point to an unmodified buffer returned by this library (or an
/// empty zero-initialized buffer). Its allocation must not have been freed.
#[no_mangle]
pub unsafe extern "C" fn schist_buffer_free(buffer: *mut SchistBuffer) {
    if let Some(buffer) = buffer.as_mut() {
        if !buffer.data.is_null() {
            drop(Box::from_raw(std::ptr::slice_from_raw_parts_mut(
                buffer.data,
                buffer.len,
            )));
        }
        *buffer = SchistBuffer::empty();
    }
}

unsafe fn input<'a>(data: *const u8, len: usize) -> Result<&'a [u8]> {
    ensure!(len <= isize::MAX as usize, "Input length too large");
    if len == 0 {
        return Ok(&[]);
    }
    ensure!(!data.is_null(), "Null input pointer");
    Ok(std::slice::from_raw_parts(data, len))
}

unsafe fn invoke(
    app: *mut SchistApp,
    out: *mut SchistBuffer,
    f: impl FnOnce(&mut App) -> Result<Vec<u8>>,
) -> i32 {
    let Some(out) = out.as_mut() else {
        return 1;
    };
    *out = SchistBuffer::empty();
    let Some(handle) = app.as_mut() else {
        *out = SchistBuffer::from_bytes(b"Null application handle".to_vec());
        return 1;
    };
    if handle.poisoned {
        *out = SchistBuffer::from_bytes(b"Application panicked; destroy this handle".to_vec());
        return 2;
    }
    let (status, bytes) = match catch_unwind(AssertUnwindSafe(|| f(&mut handle.app))) {
        Ok(Ok(bytes)) => (0, bytes),
        Ok(Err(error)) => (1, format!("{error:#}").into_bytes()),
        Err(_) => {
            handle.poisoned = true;
            (2, b"Application panicked; destroy this handle".to_vec())
        }
    };
    *out = SchistBuffer::from_bytes(bytes);
    status
}

/// Execute a UTF-8 JSON request. On success, output is JSON. On failure it is a
/// UTF-8 error message. Neither is NUL terminated. Free either with buffer_free.
/// # Safety
/// `app` is exclusively borrowed for this call; `data` is readable for `len`
/// bytes; `out` points to writable, empty storage and does not alias the input.
#[no_mangle]
pub unsafe extern "C" fn schist_request(
    app: *mut SchistApp,
    data: *const u8,
    len: usize,
    out: *mut SchistBuffer,
) -> i32 {
    invoke(app, out, |app| {
        ensure!(len <= crate::MAX_REQUEST_BYTES, "Request too large");
        app.request_json(input(data, len)?)
    })
}

/// Import binary file bytes. Output is JSON containing the new session id.
/// # Safety
/// Same handle/output requirements as schist_request. Both input spans must be
/// readable for their lengths; the name is UTF-8.
#[no_mangle]
pub unsafe extern "C" fn schist_import(
    app: *mut SchistApp,
    name: *const u8,
    name_len: usize,
    data: *const u8,
    len: usize,
    out: *mut SchistBuffer,
) -> i32 {
    invoke(app, out, |app| {
        let name = std::str::from_utf8(input(name, name_len)?)?;
        let id = app.import(name, input(data, len)?)?;
        Ok(serde_json::to_vec(&serde_json::json!({"session": id}))?)
    })
}

/// Load ONNX bytes into this instance; id is face or face-embed. Success is empty.
/// # Safety
/// Same requirements as schist_import.
#[no_mangle]
pub unsafe extern "C" fn schist_load_model(
    app: *mut SchistApp,
    id: *const u8,
    id_len: usize,
    data: *const u8,
    len: usize,
    out: *mut SchistBuffer,
) -> i32 {
    invoke(app, out, |app| {
        app.people
            .load_model(std::str::from_utf8(input(id, id_len)?)?, input(data, len)?)?;
        Ok(Vec::new())
    })
}
