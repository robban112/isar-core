#[macro_export]
macro_rules! isar_try {
    { $($token:tt)* } => {{
        #[allow(unused_mut)] {
            let mut l = || -> isar_core::error::Result<()> {
                $($token)*
                Ok(())
            };
            match l() {
                Ok(_) => 0,
                Err(e) => {
                    eprintln!("{}",e);
                    1
                },
            }
        }
    }}
}