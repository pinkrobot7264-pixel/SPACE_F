//! Mapping from the config file to the VFS contract types.
//!
//! The dependency runs `space-client-core -> space-config`, never the other
//! way, so the mapping lives here. `space-config` stays a plain typed reader of
//! the TOML file with no knowledge of the `Vfs` contract.

use space_config::VfsSection;

use crate::vfs::limits::{Limits, PathLimits};

pub trait VfsSectionExt {
    fn limits(&self) -> Limits;
    fn path_limits(&self) -> PathLimits;
}

impl VfsSectionExt for VfsSection {
    fn limits(&self) -> Limits {
        let (max_bytes, max_open_handles, max_open_cursors, max_dir_entries, max_io_bytes) =
            self.as_tuple();
        Limits {
            max_bytes,
            max_open_handles,
            max_open_cursors,
            max_dir_entries,
            max_io_bytes,
        }
    }

    fn path_limits(&self) -> PathLimits {
        let (max_path_chars, max_component_chars, max_path_depth) = self.path_tuple();
        PathLimits {
            max_path_chars,
            max_component_chars,
            max_path_depth,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_config_defaults_match_the_documented_limit_table() {
        // docs/protocols/resource-limits.md. If the config defaults and the
        // code constants ever diverge, a filesystem built from a default config
        // would enforce different bounds than the ones documented.
        let s = VfsSection::default();
        assert_eq!(s.limits(), Limits::DEFAULT);
        assert_eq!(s.path_limits(), PathLimits::DEFAULT);
    }
}
