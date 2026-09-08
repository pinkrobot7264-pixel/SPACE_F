//! Shared helpers for the conformance suite (§11.6).

use std::time::Duration;

use contracts::{ErrorCode, SpaceError};

use crate::vfs::invariants::VfsDiagnostics;
use crate::vfs::types::*;
use crate::vfs::{HandleId, OpCtx, Vfs, VfsPath};

/// A generous per-operation deadline. Conformance tests are not timing tests;
/// the deadline exists so an accidental hang fails rather than blocks CI.
pub fn cx() -> OpCtx {
    OpCtx::new(Duration::from_secs(30))
}

/// Run `f`, then assert every invariant still holds.
///
/// **Every conformance test becomes an invariant test at no authoring cost**
/// (§11.6). Keep fixtures small so the O(nodes) walk stays cheap; the
/// L6-boundary test checks once at the end rather than per step.
pub fn step<V: Vfs + VfsDiagnostics, T>(vfs: &V, label: &str, f: impl FnOnce() -> T) -> T {
    let r = f();
    if let Err(v) = vfs.check_invariants() {
        panic!("invariant {} violated after {}: {}", v.id, label, v.detail);
    }
    r
}

/// Assert a call failed with exactly `code`.
pub fn expect_err<T: std::fmt::Debug>(
    what: &str,
    code: ErrorCode,
    r: Result<T, SpaceError>,
) -> SpaceError {
    match r {
        Ok(v) => panic!("{what}: expected {code:?}, got Ok({v:?})"),
        Err(e) if e.code == code => e,
        Err(e) => panic!(
            "{what}: expected {code:?}, got {:?} ({})",
            e.code, e.message
        ),
    }
}

/// Convenience wrapper bundling a VFS with the helpers every module needs.
pub struct Ctx<'a, V: Vfs + VfsDiagnostics> {
    pub vfs: &'a V,
    pub caps: Capabilities,
    pub limits: crate::vfs::limits::Limits,
    /// Prefix that keeps one module's paths from colliding with another's, so
    /// the whole suite can share one filesystem instance.
    prefix: String,
}

impl<'a, V: Vfs + VfsDiagnostics> Ctx<'a, V> {
    pub fn new(
        vfs: &'a V,
        caps: Capabilities,
        limits: crate::vfs::limits::Limits,
        prefix: &str,
    ) -> Self {
        let c = Ctx {
            vfs,
            caps,
            limits,
            prefix: prefix.to_string(),
        };
        // Each module works inside its own directory.
        c.mkdir_raw(&format!("\\{}", prefix));
        c
    }

    /// A path inside this module's namespace.
    pub fn p(&self, rel: &str) -> VfsPath {
        let s = if rel.is_empty() {
            format!("\\{}", self.prefix)
        } else {
            format!("\\{}\\{}", self.prefix, rel)
        };
        self.vfs
            .parse_path(&s)
            .unwrap_or_else(|e| panic!("test path {s:?} is invalid: {e}"))
    }

    /// A raw path, not namespaced. For naming tests that need exact strings.
    pub fn raw(&self, s: &str) -> Result<VfsPath, SpaceError> {
        self.vfs.parse_path(s)
    }

    fn mkdir_raw(&self, s: &str) {
        let p = self.vfs.parse_path(s).unwrap();
        // Every handle this helper opens must be closed. A leak here is
        // invisible until the L7 boundary test runs out of handles early, so
        // the L7 test asserts the exact count precisely to catch it.
        if let Ok(o) = self.vfs.create(
            &cx(),
            &p,
            CreateOptions {
                create_options: FILE_DIRECTORY_FILE,
                granted_access: 0,
                file_attributes: 0,
                allocation_size: 0,
            },
        ) {
            self.vfs.cleanup(&cx(), o.handle, CleanupFlags::NONE);
            self.vfs.close(&cx(), o.handle);
        }
    }

    // --- construction helpers, all closing their handles -------------------

    pub fn create_file(&self, rel: &str) -> HandleId {
        self.try_create_file(rel)
            .unwrap_or_else(|e| panic!("create {rel:?} failed: {e}"))
    }

    pub fn try_create_file(&self, rel: &str) -> Result<HandleId, SpaceError> {
        let p = self.p(rel);
        self.vfs
            .create(
                &cx(),
                &p,
                CreateOptions {
                    create_options: 0,
                    granted_access: 0,
                    file_attributes: 0,
                    allocation_size: 0,
                },
            )
            .map(|o| o.handle)
    }

    pub fn create_dir(&self, rel: &str) -> HandleId {
        let p = self.p(rel);
        self.vfs
            .create(
                &cx(),
                &p,
                CreateOptions {
                    create_options: FILE_DIRECTORY_FILE,
                    granted_access: 0,
                    file_attributes: 0,
                    allocation_size: 0,
                },
            )
            .unwrap_or_else(|e| panic!("mkdir {rel:?} failed: {e}"))
            .handle
    }

    pub fn open_file(&self, rel: &str) -> Result<HandleId, SpaceError> {
        let p = self.p(rel);
        self.vfs
            .open(
                &cx(),
                &p,
                OpenOptions {
                    create_options: 0,
                    granted_access: 0,
                },
            )
            .map(|o| o.handle)
    }

    pub fn open_dir(&self, rel: &str) -> Result<HandleId, SpaceError> {
        let p = self.p(rel);
        self.vfs
            .open(
                &cx(),
                &p,
                OpenOptions {
                    create_options: FILE_DIRECTORY_FILE,
                    granted_access: 0,
                },
            )
            .map(|o| o.handle)
    }

    /// Create a file with `content`, then close it.
    pub fn file_with(&self, rel: &str, content: &[u8]) {
        let h = self.create_file(rel);
        if !content.is_empty() {
            self.vfs
                .write(&cx(), h, 0, content, WriteMode::NORMAL)
                .unwrap_or_else(|e| panic!("write {rel:?} failed: {e}"));
        }
        self.vfs.cleanup(&cx(), h, CleanupFlags::NONE);
        self.vfs.close(&cx(), h);
    }

    /// Read a whole file by path.
    pub fn read_all(&self, rel: &str) -> Vec<u8> {
        let h = self.open_file(rel).unwrap();
        let size = self.vfs.file_info(&cx(), h).unwrap().file_size as usize;
        let mut buf = vec![0u8; size];
        if size > 0 {
            let n = self.vfs.read(&cx(), h, 0, &mut buf).unwrap();
            assert_eq!(n as usize, size, "short read of {rel:?}");
        }
        self.vfs.cleanup(&cx(), h, CleanupFlags::NONE);
        self.vfs.close(&cx(), h);
        buf
    }

    /// Delete by path: open, cleanup-with-delete, close (fs-semantics §1).
    pub fn delete(&self, rel: &str) {
        let p = self.p(rel);
        let h = self
            .vfs
            .open(
                &cx(),
                &p,
                OpenOptions {
                    create_options: 0,
                    granted_access: 0,
                },
            )
            .unwrap_or_else(|e| panic!("open for delete {rel:?} failed: {e}"))
            .handle;
        self.vfs.cleanup(&cx(), h, CleanupFlags::DELETE);
        self.vfs.close(&cx(), h);
    }

    /// Close a handle the way Windows does: cleanup, then close.
    pub fn close(&self, h: HandleId) {
        self.vfs.cleanup(&cx(), h, CleanupFlags::NONE);
        self.vfs.close(&cx(), h);
    }

    pub fn exists(&self, rel: &str) -> bool {
        let p = self.p(rel);
        self.vfs.probe(&cx(), &p).is_ok()
    }

    /// Enumerate a directory fully, returning entry names in order.
    pub fn list(&self, rel: &str) -> Vec<String> {
        let h = self.open_dir(rel).unwrap();
        let names = self.list_handle(h, None);
        self.close(h);
        names
    }

    pub fn list_handle(&self, h: HandleId, marker: Option<&str>) -> Vec<String> {
        let c = self.vfs.dir_open(&cx(), h, None, marker).unwrap();
        let mut out = Vec::new();
        while let Some(e) = self.vfs.dir_next(&cx(), c).unwrap() {
            out.push(e.name);
        }
        self.vfs.dir_close(&cx(), c);
        out
    }
}
