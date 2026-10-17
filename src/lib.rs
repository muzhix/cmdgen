#![forbid(unsafe_code)]

mod cli;
pub mod config;
pub mod doctor;
pub mod error;
mod fsutil;
pub mod generator;
pub mod shell;

pub use error::Error;

pub fn run() -> i32 {
    run_from(std::env::args_os())
}

pub fn run_from<I, T>(args: I) -> i32
where
    I: IntoIterator<Item = T>,
    T: Into<std::ffi::OsString> + Clone,
{
    match cli::dispatch(args) {
        Ok(code) => code,
        Err(err) => {
            if std::env::var_os("CMDGEN_DEBUG").is_some() {
                eprintln!("cmdgen: {err:?}");
            } else {
                eprintln!("cmdgen: {err}");
            }
            1
        }
    }
}
