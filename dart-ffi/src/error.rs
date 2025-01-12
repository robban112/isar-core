use isar_core::error::IsarError;
use once_cell::sync::Lazy;
use std::ffi::CString;
use std::os::raw::c_char;
use std::sync::Mutex;

type ErrCounter = (Vec<(i64, String)>, i64);
static ERRORS: Lazy<Mutex<ErrCounter>> = Lazy::new(|| Mutex::new((vec![], 1)));

pub trait DartErrCode {
    fn into_dart_err_code(self) -> i64;
}

impl DartErrCode for IsarError {
    fn into_dart_err_code(self) -> i64 {
        let mut lock = ERRORS.lock().unwrap();
        let (errors, counter) = &mut (*lock);
        if errors.len() > 10 {
            errors.remove(0);
        }
        let err_code = *counter;
        errors.push((err_code, self.to_string()));
        *counter = counter.wrapping_add(1);
        if *counter == 0 {
            *counter = 1
        }
        err_code
    }
}

#[macro_export]
macro_rules! isar_try {
    { $($token:tt)* } => {{
        use crate::error::DartErrCode;
        #[allow(unused_mut)] {
            let mut l = || -> isar_core::error::Result<()> {
                $($token)*
                Ok(())
            };
            match l() {
                Ok(_) => 0,
                Err(e) => {
                    e.into_dart_err_code()
                },
            }
        }
    }}
}

#[macro_export]
macro_rules! isar_try_txn {
    { $txn:expr, $closure:expr } => {
        isar_try! {
            $txn.exec(Box::new($closure))?;
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn isar_get_error(err_code: i64) -> *mut c_char {
    let lock = ERRORS.lock().unwrap();
    let error = lock.0.iter().find(|(code, _)| *code == err_code);
    if let Some((_, err_msg)) = error {
        CString::new(err_msg.as_str()).unwrap().into_raw()
    } else {
        std::ptr::null_mut()
    }
}

#[no_mangle]
pub unsafe extern "C" fn isar_free_error(error: *mut c_char) {
    let _ = CString::from_raw(error);
}
