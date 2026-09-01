use std::ffi::{CStr, c_char, c_void};
use std::path::PathBuf;
use libloading::{Library, Symbol};

pub struct SignatureGenerator {
    _lib: Option<Library>,
    generate_fn: Option<unsafe extern "C" fn(*const i16, usize) -> *mut c_void>,
    free_fn: Option<unsafe extern "C" fn(*mut c_void)>,
}

impl SignatureGenerator {
    pub fn new() -> Self {
        let mut candidates = vec![
            PathBuf::from("/home/faulter/.local/bin/musicRecognition/libsongrecfp.so"),
            PathBuf::from("/usr/lib/libsongrecfp.so"),
            PathBuf::from("/usr/local/lib/libsongrecfp.so"),
        ];

        if let Ok(home) = std::env::var("HOME") {
            candidates.push(PathBuf::from(home).join(".local/lib/libsongrecfp.so"));
        }

        for path in candidates {
            if path.exists() {
                unsafe {
                    if let Ok(lib) = Library::new(&path) {
                        let gen_sym: Result<Symbol<unsafe extern "C" fn(*const i16, usize) -> *mut c_void>, _> = lib.get(b"generate_signature");
                        let free_sym: Result<Symbol<unsafe extern "C" fn(*mut c_void)>, _> = lib.get(b"free_signature");
                        if let (Ok(g), Ok(f)) = (gen_sym, free_sym) {
                            return Self {
                                generate_fn: Some(*g),
                                free_fn: Some(*f),
                                _lib: Some(lib),
                            };
                        }
                    }
                }
            }
        }

        Self {
            _lib: None,
            generate_fn: None,
            free_fn: None,
        }
    }

    pub fn generate_from_i16(&self, pcm: &[i16]) -> Option<String> {
        let gen_fn = self.generate_fn?;
        let free_fn = self.free_fn?;

        unsafe {
            let ptr = gen_fn(pcm.as_ptr(), pcm.len());
            if ptr.is_null() {
                return None;
            }

            let c_str = CStr::from_ptr(ptr as *const c_char);
            let result = c_str.to_string_lossy().into_owned();
            free_fn(ptr);
            Some(result)
        }
    }
}
