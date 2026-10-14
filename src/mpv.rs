//! Minimal libmpv binding, loaded at run time from `libmpv-2.dll`, so Reyer starts (and can explain a
//! missing DLL) without linking against mpv. Covers the client API and the OpenGL render API.
use libloading::Library;
use serde_json::{Map, Value};
use std::ffi::{c_char, c_int, c_void, CStr, CString};
use std::path::{Path, PathBuf};
use std::sync::Arc;

// mpv_format
const FMT_NONE: c_int = 0;
const FMT_STRING: c_int = 1;
const FMT_FLAG: c_int = 3;
const FMT_INT64: c_int = 4;
const FMT_DOUBLE: c_int = 5;
const FMT_NODE: c_int = 6;
const FMT_NODE_ARRAY: c_int = 7;
const FMT_NODE_MAP: c_int = 8;

// mpv_event_id
const EV_SHUTDOWN: c_int = 1;
const EV_LOG_MESSAGE: c_int = 2;
const EV_COMMAND_REPLY: c_int = 5;
const EV_END_FILE: c_int = 7;
const EV_FILE_LOADED: c_int = 8;
const EV_PROPERTY_CHANGE: c_int = 22;

pub const END_REASON_ERROR: c_int = 4;

// mpv_render_param_type
const RP_API_TYPE: c_int = 1;
const RP_OPENGL_INIT_PARAMS: c_int = 2;
const RP_OPENGL_FBO: c_int = 3;
const RP_FLIP_Y: c_int = 4;
const RP_BLOCK_FOR_TARGET_TIME: c_int = 12;
pub const RENDER_UPDATE_FRAME: u64 = 1;

#[repr(C)]
union NodeU {
    string: *mut c_char,
    flag: c_int,
    int64: i64,
    double: f64,
    list: *mut NodeList,
    ba: *mut c_void,
}

#[repr(C)]
struct Node {
    u: NodeU,
    format: c_int,
}

#[repr(C)]
struct NodeList {
    num: c_int,
    values: *mut Node,
    keys: *mut *mut c_char,
}

#[repr(C)]
struct RawEvent {
    event_id: c_int,
    error: c_int,
    reply_userdata: u64,
    data: *mut c_void,
}

#[repr(C)]
struct EventProperty {
    name: *const c_char,
    format: c_int,
    data: *mut c_void,
}

#[repr(C)]
struct EventEndFile {
    reason: c_int,
    error: c_int,
}

#[repr(C)]
struct EventLogMessage {
    prefix: *const c_char,
    level: *const c_char,
    text: *const c_char,
    log_level: c_int,
}

#[repr(C)]
struct RenderParam {
    kind: c_int,
    data: *mut c_void,
}

#[repr(C)]
struct OpenGlInitParams {
    get_proc_address: extern "C" fn(*mut c_void, *const c_char) -> *mut c_void,
    ctx: *mut c_void,
}

#[repr(C)]
struct OpenGlFbo {
    fbo: c_int,
    w: c_int,
    h: c_int,
    internal_format: c_int,
}

type Handle = *mut c_void;
type RenderHandle = *mut c_void;

macro_rules! api {
    ($($name:ident: fn($($arg:ty),*) -> $ret:ty;)*) => {
        #[allow(non_snake_case)]
        struct Api {
            _lib: Library,
            $($name: unsafe extern "C" fn($($arg),*) -> $ret,)*
        }
        impl Api {
            fn load(lib: Library) -> Result<Api, String> {
                unsafe {
                    Ok(Api {
                        $($name: *lib.get::<unsafe extern "C" fn($($arg),*) -> $ret>(concat!(stringify!($name), "\0").as_bytes())
                            .map_err(|e| format!("{}: {e}", stringify!($name)))?,)*
                        _lib: lib,
                    })
                }
            }
        }
    };
}

api! {
    mpv_create: fn() -> Handle;
    mpv_initialize: fn(Handle) -> c_int;
    mpv_terminate_destroy: fn(Handle) -> ();
    mpv_set_option_string: fn(Handle, *const c_char, *const c_char) -> c_int;
    mpv_set_property_string: fn(Handle, *const c_char, *const c_char) -> c_int;
    mpv_command: fn(Handle, *mut *const c_char) -> c_int;
    mpv_command_async: fn(Handle, u64, *mut *const c_char) -> c_int;
    mpv_get_property: fn(Handle, *const c_char, c_int, *mut c_void) -> c_int;
    mpv_free_node_contents: fn(*mut Node) -> ();
    mpv_observe_property: fn(Handle, u64, *const c_char, c_int) -> c_int;
    mpv_wait_event: fn(Handle, f64) -> *mut RawEvent;
    mpv_set_wakeup_callback: fn(Handle, Option<extern "C" fn(*mut c_void)>, *mut c_void) -> ();
    mpv_request_log_messages: fn(Handle, *const c_char) -> c_int;
    mpv_render_context_create: fn(*mut RenderHandle, Handle, *mut RenderParam) -> c_int;
    mpv_render_context_render: fn(RenderHandle, *mut RenderParam) -> c_int;
    mpv_render_context_set_update_callback: fn(RenderHandle, Option<extern "C" fn(*mut c_void)>, *mut c_void) -> ();
    mpv_render_context_update: fn(RenderHandle) -> u64;
    mpv_render_context_free: fn(RenderHandle) -> ();
}

/// Where `libmpv-2.dll` is looked for: next to the executable, then the working directory, then PATH.
pub fn candidates() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        dirs.extend(exe.parent().map(Path::to_path_buf));
    }
    dirs.push(PathBuf::from("."));
    if let Some(path) = std::env::var_os("PATH") {
        dirs.extend(std::env::split_paths(&path));
    }
    dirs.iter().flat_map(|d| ["libmpv-2.dll", "mpv-2.dll"].map(|n| d.join(n))).collect()
}

pub fn find_libmpv() -> Option<PathBuf> {
    candidates().into_iter().find(|p| p.is_file())
}

fn cstr(s: &str) -> CString {
    CString::new(s.replace('\0', "")).unwrap_or_default()
}

fn node_to_value(node: &Node) -> Value {
    unsafe {
        match node.format {
            FMT_STRING => Value::String(CStr::from_ptr(node.u.string).to_string_lossy().into_owned()),
            FMT_FLAG => Value::Bool(node.u.flag != 0),
            FMT_INT64 => Value::from(node.u.int64),
            FMT_DOUBLE => serde_json::Number::from_f64(node.u.double).map(Value::Number).unwrap_or(Value::Null),
            FMT_NODE_ARRAY | FMT_NODE_MAP if !node.u.list.is_null() => {
                let list = &*node.u.list;
                if list.num <= 0 || list.values.is_null() {
                    // an empty list/map may come with null pointers, which from_raw_parts forbids
                    return if node.format == FMT_NODE_ARRAY { Value::Array(vec![]) } else { Value::Object(Map::new()) };
                }
                let values = std::slice::from_raw_parts(list.values, list.num as usize);
                if node.format == FMT_NODE_ARRAY {
                    Value::Array(values.iter().map(node_to_value).collect())
                } else {
                    let keys = std::slice::from_raw_parts(list.keys, values.len());
                    let mut map = Map::new();
                    for (k, v) in keys.iter().zip(values) {
                        map.insert(CStr::from_ptr(*k).to_string_lossy().into_owned(), node_to_value(v));
                    }
                    Value::Object(map)
                }
            }
            _ => Value::Null,
        }
    }
}

#[derive(Debug)]
pub enum Event {
    Shutdown,
    /// An asynchronous command finished (successfully or not).
    CommandReply { id: u64 },
    Property { name: String, value: Value },
    FileLoaded,
    EndFile { reason: i32 },
    Log { prefix: String, level: String },
    Other,
}

pub struct Mpv {
    api: Arc<Api>,
    handle: Handle,
}

// libmpv's client API is thread-safe by design.
unsafe impl Send for Mpv {}

impl Mpv {
    pub fn load(path: &Path) -> Result<Mpv, String> {
        let lib = unsafe { Library::new(path) }.map_err(|e| e.to_string())?;
        let api = Arc::new(Api::load(lib)?);
        let handle = unsafe { (api.mpv_create)() };
        if handle.is_null() {
            return Err("mpv_create failed".into());
        }
        Ok(Mpv { api, handle })
    }

    pub fn option(&self, name: &str, value: &str) -> bool {
        unsafe { (self.api.mpv_set_option_string)(self.handle, cstr(name).as_ptr(), cstr(value).as_ptr()) >= 0 }
    }

    pub fn initialize(&self) -> bool {
        unsafe { (self.api.mpv_initialize)(self.handle) >= 0 }
    }

    pub fn set(&self, name: &str, value: &str) -> bool {
        unsafe { (self.api.mpv_set_property_string)(self.handle, cstr(name).as_ptr(), cstr(value).as_ptr()) >= 0 }
    }

    pub fn command(&self, args: &[&str]) -> bool {
        let owned: Vec<CString> = args.iter().map(|a| cstr(a)).collect();
        let mut ptrs: Vec<*const c_char> = owned.iter().map(|c| c.as_ptr()).collect();
        ptrs.push(std::ptr::null());
        unsafe { (self.api.mpv_command)(self.handle, ptrs.as_mut_ptr()) >= 0 }
    }

    /// Queues a command without waiting for mpv's core; `Event::CommandReply { id }` follows.
    pub fn command_async(&self, id: u64, args: &[&str]) -> bool {
        let owned: Vec<CString> = args.iter().map(|a| cstr(a)).collect();
        let mut ptrs: Vec<*const c_char> = owned.iter().map(|c| c.as_ptr()).collect();
        ptrs.push(std::ptr::null());
        unsafe { (self.api.mpv_command_async)(self.handle, id, ptrs.as_mut_ptr()) >= 0 }
    }

    /// A property as JSON (maps and arrays included), or `None` when it is unavailable.
    pub fn get(&self, name: &str) -> Option<Value> {
        unsafe {
            let mut node: Node = std::mem::zeroed();
            if (self.api.mpv_get_property)(self.handle, cstr(name).as_ptr(), FMT_NODE, &mut node as *mut Node as *mut c_void) < 0 {
                return None;
            }
            let value = node_to_value(&node);
            (self.api.mpv_free_node_contents)(&mut node);
            Some(value)
        }
    }

    pub fn observe(&self, name: &str, id: u64) {
        unsafe { (self.api.mpv_observe_property)(self.handle, id, cstr(name).as_ptr(), FMT_NODE) };
    }

    pub fn log_messages(&self, level: &str) {
        unsafe { (self.api.mpv_request_log_messages)(self.handle, cstr(level).as_ptr()) };
    }

    /// Called from an mpv thread whenever events are waiting; must only wake the main thread.
    pub fn set_wakeup(&self, cb: extern "C" fn(*mut c_void), data: *mut c_void) {
        unsafe { (self.api.mpv_set_wakeup_callback)(self.handle, Some(cb), data) };
    }

    pub fn poll(&self) -> Option<Event> {
        unsafe {
            let ev = &*(self.api.mpv_wait_event)(self.handle, 0.0);
            Some(match ev.event_id {
                0 => return None,
                EV_SHUTDOWN => Event::Shutdown,
                EV_COMMAND_REPLY => Event::CommandReply { id: ev.reply_userdata },
                EV_FILE_LOADED => Event::FileLoaded,
                EV_END_FILE if !ev.data.is_null() => Event::EndFile { reason: (*(ev.data as *const EventEndFile)).reason },
                EV_PROPERTY_CHANGE if !ev.data.is_null() => {
                    let p = &*(ev.data as *const EventProperty);
                    let name = CStr::from_ptr(p.name).to_string_lossy().into_owned();
                    let value = if p.format == FMT_NODE && !p.data.is_null() {
                        node_to_value(&*(p.data as *const Node))
                    } else if p.format == FMT_NONE {
                        Value::Null
                    } else {
                        Value::Null
                    };
                    Event::Property { name, value }
                }
                EV_LOG_MESSAGE if !ev.data.is_null() => {
                    let m = &*(ev.data as *const EventLogMessage);
                    Event::Log {
                        prefix: CStr::from_ptr(m.prefix).to_string_lossy().into_owned(),
                        level: CStr::from_ptr(m.level).to_string_lossy().into_owned(),
                    }
                }
                _ => Event::Other,
            })
        }
    }

    /// Creates the OpenGL render context. The GL context must be current; `get_proc` is called by mpv
    /// (during this call and later) to resolve GL functions.
    pub fn create_render_context(&self, get_proc: extern "C" fn(*mut c_void, *const c_char) -> *mut c_void, ctx: *mut c_void) -> Result<RenderContext, String> {
        let mut init = OpenGlInitParams { get_proc_address: get_proc, ctx };
        let mut api_type = cstr("opengl");
        let mut params = [
            RenderParam { kind: RP_API_TYPE, data: api_type.as_ptr() as *mut c_void },
            RenderParam { kind: RP_OPENGL_INIT_PARAMS, data: &mut init as *mut _ as *mut c_void },
            RenderParam { kind: 0, data: std::ptr::null_mut() },
        ];
        let mut handle: RenderHandle = std::ptr::null_mut();
        let rc = unsafe { (self.api.mpv_render_context_create)(&mut handle, self.handle, params.as_mut_ptr()) };
        api_type = CString::default();
        let _ = &api_type;
        if rc < 0 || handle.is_null() {
            return Err(format!("mpv_render_context_create failed ({rc})"));
        }
        Ok(RenderContext { api: self.api.clone(), handle })
    }

    /// Dropping an `Mpv` destroys the player; the render context must be gone by then.
    pub fn terminate(self) {
        drop(self);
    }
}

impl Drop for Mpv {
    fn drop(&mut self) {
        unsafe { (self.api.mpv_terminate_destroy)(self.handle) };
    }
}

pub struct RenderContext {
    api: Arc<Api>,
    handle: RenderHandle,
}

impl RenderContext {
    /// mpv calls this from any thread when a new frame is ready (or the window should be redrawn).
    pub fn set_update_callback(&self, cb: Option<extern "C" fn(*mut c_void)>, data: *mut c_void) {
        unsafe { (self.api.mpv_render_context_set_update_callback)(self.handle, cb, data) };
    }

    pub fn update(&self) -> u64 {
        unsafe { (self.api.mpv_render_context_update)(self.handle) }
    }

    /// Draws the current frame into `fbo` (a GL framebuffer object name) of the given size, never waiting for vsync.
    pub fn render(&self, fbo: u32, w: i32, h: i32) {
        let mut target = OpenGlFbo { fbo: fbo as c_int, w, h, internal_format: 0 };
        let mut flip: c_int = 1; // mpv draws top-down; the render target is sampled bottom-up (GL convention)
        let mut block: c_int = 0;
        let mut params = [
            RenderParam { kind: RP_OPENGL_FBO, data: &mut target as *mut _ as *mut c_void },
            RenderParam { kind: RP_FLIP_Y, data: &mut flip as *mut _ as *mut c_void },
            RenderParam { kind: RP_BLOCK_FOR_TARGET_TIME, data: &mut block as *mut _ as *mut c_void },
            RenderParam { kind: 0, data: std::ptr::null_mut() },
        ];
        unsafe { (self.api.mpv_render_context_render)(self.handle, params.as_mut_ptr()) };
    }
}

impl Drop for RenderContext {
    fn drop(&mut self) {
        unsafe {
            (self.api.mpv_render_context_set_update_callback)(self.handle, None, std::ptr::null_mut());
            (self.api.mpv_render_context_free)(self.handle);
        }
    }
}
