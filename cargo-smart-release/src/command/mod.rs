pub mod release {
    #[derive(Debug, Clone, Copy)]
    pub struct Options {
        pub dry_run: bool,
        pub allow_dirty: bool,
        pub ignore_instability: bool,
        pub skip_publish: bool,
        /// Pass --no-verify unconditionally to cargo publish. Really just for fixing things
        pub no_verify: bool,
    }
}

#[path = "release.rs"]
mod release_impl;
pub use release_impl::release;