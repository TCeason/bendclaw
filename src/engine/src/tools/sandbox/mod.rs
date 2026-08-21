mod core;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;

pub use core::check_available;
pub use core::wrap_command;
pub use core::SandboxSupport;
