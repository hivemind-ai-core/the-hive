//! Compile-time version and build information.

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub const BUILD_TARGET: &str = env!("HIVE_BUILD_TARGET");

pub fn user_agent() -> String {
    format!("hive/{VERSION} ({BUILD_TARGET})")
}
