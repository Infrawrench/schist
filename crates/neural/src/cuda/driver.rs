//! Minimal CUDA Driver API surface. All raw handles stay behind a context lock.
use anyhow::{ensure, Context as _, Result};
use libloading::Library;
use std::{
    ffi::{c_char, c_int, c_uint, c_void, CStr},
    sync::{Arc, Mutex, MutexGuard, OnceLock, Weak},
};

type Handle = *mut c_void;
pub(super) type Ptr = u64;

macro_rules! driver_api {
    ($($name:ident: $symbol:literal ($($arg:ty),*) -> $ret:ty;)*) => {
        struct Api {
            $($name: unsafe extern "system" fn($($arg),*) -> $ret,)*
            _library: Library,
        }
        impl Api {
            unsafe fn new(library: Library) -> Result<Self> {
                Ok(Self {
                    $($name: unsafe { *library.get(concat!($symbol, "\0").as_bytes())? },)*
                    _library: library,
                })
            }
        }
    }
}
driver_api! {
    init: "cuInit"(c_uint) -> c_int;
    device: "cuDeviceGet"(*mut c_int, c_int) -> c_int;
    attribute: "cuDeviceGetAttribute"(*mut c_int, c_int, c_int) -> c_int;
    name: "cuDeviceGetName"(*mut c_char, c_int, c_int) -> c_int;
    retain: "cuDevicePrimaryCtxRetain"(*mut Handle, c_int) -> c_int;
    release: "cuDevicePrimaryCtxRelease_v2"(c_int) -> c_int;
    push: "cuCtxPushCurrent_v2"(Handle) -> c_int;
    pop: "cuCtxPopCurrent_v2"(*mut Handle) -> c_int;
    module_load: "cuModuleLoadData"(*mut Handle, *const c_void) -> c_int;
    module_unload: "cuModuleUnload"(Handle) -> c_int;
    function: "cuModuleGetFunction"(*mut Handle, Handle, *const c_char) -> c_int;
    stream_create: "cuStreamCreate"(*mut Handle, c_uint) -> c_int;
    stream_destroy: "cuStreamDestroy_v2"(Handle) -> c_int;
    synchronize: "cuStreamSynchronize"(Handle) -> c_int;
    memory: "cuMemGetInfo_v2"(*mut usize, *mut usize) -> c_int;
    alloc: "cuMemAlloc_v2"(*mut Ptr, usize) -> c_int;
    free: "cuMemFree_v2"(Ptr) -> c_int;
    upload: "cuMemcpyHtoD_v2"(Ptr, *const c_void, usize) -> c_int;
    download: "cuMemcpyDtoH_v2"(*mut c_void, Ptr, usize) -> c_int;
    launch: "cuLaunchKernel"(Handle, c_uint, c_uint, c_uint, c_uint, c_uint, c_uint, c_uint, Handle, *mut *mut c_void, *mut *mut c_void) -> c_int;
}

fn check(code: c_int, operation: &str) -> Result<()> {
    ensure!(code == 0, "CUDA {operation} failed ({code})");
    Ok(())
}

pub(super) struct Device {
    api: Api,
    device: c_int,
    context: Handle,
    module: Handle,
    functions: [Handle; 4],
    stream: Handle,
    lock: Mutex<()>,
    pub name: String,
}
// CUDA contexts can migrate between host threads. Every operation pushes the
// retained primary context and holds `lock`; the previous context is restored.
unsafe impl Send for Device {}
unsafe impl Sync for Device {}

pub(super) struct Guard<'a> {
    device: &'a Device,
    _lock: MutexGuard<'a, ()>,
}

impl Device {
    pub fn get() -> Result<Arc<Self>> {
        static CACHE: OnceLock<Mutex<Weak<Device>>> = OnceLock::new();
        let mut cache = CACHE
            .get_or_init(Mutex::default)
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if let Some(device) = cache.upgrade() {
            return Ok(device);
        }
        let device = Arc::new(Self::load()?);
        *cache = Arc::downgrade(&device);
        Ok(device)
    }

    fn load() -> Result<Self> {
        ensure!(
            usize::BITS == 64 && cfg!(target_endian = "little"),
            "CUDA executor requires a 64-bit little-endian host"
        );
        let library = load_library()?;
        unsafe {
            let api = Api::new(library)?;
            check((api.init)(0), "initialization")?;
            let ordinal = std::env::var("SCHIST_NEURAL_CUDA_DEVICE")
                .ok()
                .map(|v| v.parse::<c_int>())
                .transpose()
                .context("invalid CUDA device ordinal")?
                .unwrap_or(0);
            ensure!(ordinal >= 0, "negative CUDA device ordinal");
            let mut device = 0;
            check((api.device)(&mut device, ordinal), "device selection")?;
            let (mut major, mut minor) = (0, 0);
            check(
                (api.attribute)(&mut major, 75, device),
                "compute capability",
            )?;
            check(
                (api.attribute)(&mut minor, 76, device),
                "compute capability",
            )?;
            ensure!(
                major * 10 + minor >= 75,
                "CUDA kernels require compute capability 7.5+"
            );
            let mut name = [0 as c_char; 256];
            check((api.name)(name.as_mut_ptr(), 256, device), "device name")?;
            name[255] = 0;
            let mut context = std::ptr::null_mut();
            check((api.retain)(&mut context, device), "primary context")?;
            let mut result = Self {
                api,
                device,
                context,
                module: std::ptr::null_mut(),
                functions: [std::ptr::null_mut(); 4],
                stream: std::ptr::null_mut(),
                lock: Mutex::new(()),
                name: CStr::from_ptr(name.as_ptr()).to_string_lossy().into_owned(),
            };
            // Drop balances the primary-context retain on every partial failure.
            check((result.api.push)(context), "context push")?;
            let initialized = (|| -> Result<()> {
                check(
                    (result.api.module_load)(&mut result.module, super::PTX.as_ptr().cast()),
                    "embedded PTX JIT",
                )?;
                for (slot, name) in result.functions.iter_mut().zip([
                    c"tensor",
                    c"matrix",
                    c"convolution",
                    c"softmax",
                ]) {
                    check(
                        (result.api.function)(slot, result.module, name.as_ptr()),
                        "kernel lookup",
                    )?;
                }
                check(
                    (result.api.stream_create)(&mut result.stream, 1),
                    "stream creation",
                )?;
                Ok(())
            })();
            let mut previous = std::ptr::null_mut();
            let popped = check((result.api.pop)(&mut previous), "context pop");
            initialized?;
            popped?;
            log::info!(
                "neural CUDA: {} (SM {major}{minor}), embedded FP32 kernels",
                result.name
            );
            Ok(result)
        }
    }

    pub fn enter(&self) -> Result<Guard<'_>> {
        let lock = self.lock.lock().unwrap_or_else(|e| e.into_inner());
        unsafe {
            check((self.api.push)(self.context), "context push")?;
        }
        Ok(Guard {
            device: self,
            _lock: lock,
        })
    }
}
impl Drop for Device {
    fn drop(&mut self) {
        unsafe {
            if (self.api.push)(self.context) == 0 {
                if !self.stream.is_null() {
                    (self.api.synchronize)(self.stream);
                    (self.api.stream_destroy)(self.stream);
                }
                if !self.module.is_null() {
                    (self.api.module_unload)(self.module);
                }
                let mut popped = std::ptr::null_mut();
                (self.api.pop)(&mut popped);
            }
            (self.api.release)(self.device);
        }
    }
}
impl Drop for Guard<'_> {
    fn drop(&mut self) {
        unsafe {
            // Also drains partially submitted work before any error fallback or
            // allocation teardown. Never destroy buffers with kernels in flight.
            (self.device.api.synchronize)(std::ptr::null_mut());
            (self.device.api.synchronize)(self.device.stream);
            let mut popped = std::ptr::null_mut();
            (self.device.api.pop)(&mut popped);
        }
    }
}
impl Guard<'_> {
    pub fn free_bytes(&self) -> Result<usize> {
        let (mut free, mut total) = (0, 0);
        unsafe {
            check(
                (self.device.api.memory)(&mut free, &mut total),
                "memory query",
            )?;
        }
        Ok(free)
    }
    pub fn allocate(&self, bytes: usize) -> Result<Ptr> {
        ensure!(bytes > 0, "empty CUDA allocation");
        let mut pointer = 0;
        unsafe {
            check((self.device.api.alloc)(&mut pointer, bytes), "allocation")?;
        }
        Ok(pointer)
    }
    pub fn free(&self, pointer: Ptr) {
        unsafe {
            (self.device.api.free)(pointer);
        }
    }
    pub fn upload(&self, pointer: Ptr, data: &[u32]) -> Result<()> {
        unsafe {
            check(
                (self.device.api.upload)(
                    pointer,
                    data.as_ptr().cast(),
                    std::mem::size_of_val(data),
                ),
                "upload",
            )
        }
    }
    pub fn download(&self, pointer: Ptr, data: &mut [f32]) -> Result<()> {
        self.synchronize()?;
        unsafe {
            check(
                (self.device.api.download)(
                    data.as_mut_ptr().cast(),
                    pointer,
                    std::mem::size_of_val(data),
                ),
                "readback",
            )
        }
    }
    pub fn finish_uploads(&self) -> Result<()> {
        // Synchronous HtoD may return after staging pageable host memory but
        // before DMA completes. Drain its legacy stream before submitting work
        // to our nonblocking stream; host return alone is not a dependency.
        unsafe {
            check(
                (self.device.api.synchronize)(std::ptr::null_mut()),
                "upload synchronization",
            )
        }
    }
    pub fn synchronize(&self) -> Result<()> {
        unsafe {
            check(
                (self.device.api.synchronize)(self.device.stream),
                "synchronization",
            )
        }
    }
    pub fn launch(&self, kernel: usize, blocks: u32, pointers: [Ptr; 4], len: u32) -> Result<()> {
        ensure!(
            kernel < self.device.functions.len() && blocks > 0,
            "invalid CUDA launch"
        );
        let [mut a, mut b, mut out, mut params] = pointers;
        let mut len = len;
        let mut arguments: [*mut c_void; 5] = [
            (&mut a as *mut Ptr).cast(),
            (&mut b as *mut Ptr).cast(),
            (&mut out as *mut Ptr).cast(),
            (&mut params as *mut Ptr).cast(),
            (&mut len as *mut u32).cast(),
        ];
        unsafe {
            check(
                (self.device.api.launch)(
                    self.device.functions[kernel],
                    blocks,
                    1,
                    1,
                    256,
                    1,
                    1,
                    0,
                    self.device.stream,
                    arguments.as_mut_ptr(),
                    std::ptr::null_mut(),
                ),
                "kernel launch",
            )
        }
    }
}

#[cfg(target_os = "linux")]
fn load_library() -> Result<Library> {
    unsafe { Library::new("libcuda.so.1") }.context("NVIDIA driver unavailable")
}
#[cfg(target_os = "windows")]
fn load_library() -> Result<Library> {
    // Restrict nvcuda.dll to System32, never the working directory.
    Ok(
        unsafe { libloading::os::windows::Library::load_with_flags("nvcuda.dll", 0x00000800) }
            .context("NVIDIA driver unavailable")?
            .into(),
    )
}
#[cfg(not(any(target_os = "linux", target_os = "windows")))]
fn load_library() -> Result<Library> {
    anyhow::bail!("CUDA is only available on Linux and Windows")
}
