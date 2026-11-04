//! Standalone entry point for the same application core used by Android.
include!("lib.rs");
mod stdio;
mod http;

fn main() {
    if let Err(error) = stdio::run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
