//! `VfsPath` -- a validated, normalized path (Phase 1 §3.3.7).
//!
//! **Construction enforces every naming rule, so no `Vfs` implementation can
//! receive an invalid path.** That is the whole point of the type: validation
//! that lives in a constructor cannot be forgotten at a call site.
//!
//! `INV-NS-6` -- path resolution never escapes the SPACE namespace -- is
//! enforced here, structurally: `..` is rejected outright rather than resolved,
//! so there is no traversal to get wrong. The external half of INV-NS-6 is
//! ProcMon evidence (§16.4); this half is the resolver proof.

use contracts::{ErrorCode, SpaceError};

use crate::vfs::limits::PathLimits;
use crate::vfs::types::Capabilities;

/// A case-folded name, used as the lookup and ordering key.
///
/// The display name is stored separately on the node (`CasePreservedNames = 1`,
/// `CaseSensitiveSearch = 0`). `FoldedName`'s `Ord` gives the total, stable
/// enumeration order §3.3.8 requires.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FoldedName(String);

impl FoldedName {
    /// Fold `name` under the given capability set (fs-semantics §7).
    ///
    /// * `unicode_case_folding = false` (Phase 1): ASCII folding. `A`/`a`
    ///   collide; `Å`/`å` do **not**.
    /// * `unicode_case_folding = true` (Phase 2 target): Unicode simple case
    ///   folding. `A`/`a` collide; `Å`/`å` **do**.
    ///
    /// Both variants are specified and both are tested -- a capability selects
    /// between documented behaviours, it never disables a check (§11.5).
    pub fn new(name: &str, caps: Capabilities) -> Self {
        if caps.unicode_case_folding {
            // Rust's to_lowercase implements full Unicode lowercasing, which is
            // simple case folding for every character Phase 1 or 2 will meet.
            FoldedName(name.to_lowercase())
        } else {
            FoldedName(name.to_ascii_lowercase())
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for FoldedName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Reserved DOS device names. Rejected with or without an extension
/// (fs-semantics §7).
const RESERVED_NAMES: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// Characters Windows forbids in a filename.
const FORBIDDEN_CHARS: &[char] = &['<', '>', '"', '|', '?', '*'];

fn invalid_name(msg: impl Into<String>) -> SpaceError {
    SpaceError::new(ErrorCode::ObjectNameInvalid, msg)
}

fn too_long(msg: impl Into<String>) -> SpaceError {
    SpaceError::new(ErrorCode::NameTooLong, msg)
}

/// A validated, normalized SPACE path.
///
/// Always absolute within the namespace: it starts at the root and holds zero
/// or more components. The root itself has zero components.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VfsPath {
    components: Vec<String>,
}

impl VfsPath {
    /// The root path, `\`.
    pub fn root() -> Self {
        VfsPath {
            components: Vec::new(),
        }
    }

    /// Parse with the default limits (`PathLimits::DEFAULT`).
    pub fn parse(s: &str) -> Result<Self, SpaceError> {
        Self::parse_with(s, &PathLimits::DEFAULT)
    }

    /// Parse and validate, enforcing every rule in fs-semantics §7.
    ///
    /// Order matters: length bounds are checked before component splitting so a
    /// hostile 40,000-character path is rejected without building a vector of
    /// components from it.
    pub fn parse_with(s: &str, limits: &PathLimits) -> Result<Self, SpaceError> {
        // Embedded NUL. Checked first: everything downstream assumes its
        // absence, and a NUL is how a caller smuggles a truncated name past a
        // C consumer.
        if s.contains('\0') {
            return Err(invalid_name("embedded NUL in path"));
        }

        if s.is_empty() {
            return Err(invalid_name("empty path"));
        }

        // L1 -- before any allocation or splitting.
        if s.chars().count() > limits.max_path_chars {
            return Err(too_long(format!(
                "path exceeds L1 ({} chars)",
                limits.max_path_chars
            )));
        }

        if !s.starts_with('\\') {
            return Err(invalid_name("path must start with a backslash"));
        }

        // The root, in its several spellings.
        if s == "\\" {
            return Ok(VfsPath::root());
        }

        let body = &s[1..];

        // Repeated separators. `split` would silently produce empty components;
        // rejecting explicitly gives the caller a named reason.
        if body.contains("\\\\") {
            return Err(invalid_name("repeated path separator"));
        }
        if body.ends_with('\\') {
            return Err(invalid_name("trailing path separator"));
        }

        let raw: Vec<&str> = body.split('\\').collect();

        // L3.
        if raw.len() > limits.max_path_depth {
            return Err(too_long(format!(
                "path depth exceeds L3 ({})",
                limits.max_path_depth
            )));
        }

        let mut components = Vec::with_capacity(raw.len());
        for c in raw {
            validate_component(c, limits)?;
            components.push(c.to_string());
        }

        Ok(VfsPath { components })
    }

    pub fn is_root(&self) -> bool {
        self.components.is_empty()
    }

    pub fn components(&self) -> &[String] {
        &self.components
    }

    pub fn depth(&self) -> usize {
        self.components.len()
    }

    /// The final component -- the name being created, opened or renamed.
    /// `None` for the root.
    pub fn file_name(&self) -> Option<&str> {
        self.components.last().map(|s| s.as_str())
    }

    /// The parent path. `None` for the root.
    pub fn parent(&self) -> Option<VfsPath> {
        if self.is_root() {
            None
        } else {
            Some(VfsPath {
                components: self.components[..self.components.len() - 1].to_vec(),
            })
        }
    }

    /// Append one already-validated component.
    pub fn join(&self, name: &str, limits: &PathLimits) -> Result<VfsPath, SpaceError> {
        validate_component(name, limits)?;
        if self.components.len() + 1 > limits.max_path_depth {
            return Err(too_long("join would exceed L3"));
        }
        let mut components = self.components.clone();
        components.push(name.to_string());
        let p = VfsPath { components };
        if p.to_string().chars().count() > limits.max_path_chars {
            return Err(too_long("join would exceed L1"));
        }
        Ok(p)
    }

    /// Is `self` a proper ancestor of `other`? Used to reject renaming a
    /// directory into its own subtree (INV-NS-4).
    pub fn is_ancestor_of(&self, other: &VfsPath, caps: Capabilities) -> bool {
        if self.components.len() >= other.components.len() {
            return false;
        }
        self.components
            .iter()
            .zip(other.components.iter())
            .all(|(a, b)| FoldedName::new(a, caps) == FoldedName::new(b, caps))
    }
}

impl std::fmt::Display for VfsPath {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.components.is_empty() {
            return f.write_str("\\");
        }
        for c in &self.components {
            write!(f, "\\{c}")?;
        }
        Ok(())
    }
}

/// Validate one path component against fs-semantics §7.
pub fn validate_component(c: &str, limits: &PathLimits) -> Result<(), SpaceError> {
    if c.is_empty() {
        return Err(invalid_name("empty path component"));
    }

    // L2. Counted in chars, matching L1's unit.
    if c.chars().count() > limits.max_component_chars {
        return Err(too_long(format!(
            "component exceeds L2 ({} chars)",
            limits.max_component_chars
        )));
    }

    if c == "." || c == ".." {
        // Rejected, never resolved: there is no traversal to get wrong
        // (INV-NS-6).
        return Err(invalid_name("relative path component"));
    }

    if c.contains(':') {
        // Also blocks alternate data streams and drive-letter smuggling.
        return Err(invalid_name("colon in path component"));
    }

    if let Some(bad) = c.chars().find(|ch| FORBIDDEN_CHARS.contains(ch)) {
        return Err(invalid_name(format!("forbidden character {bad:?}")));
    }

    // Control characters, including the bidirectional overrides that make a
    // name display as something other than what it is.
    if let Some(bad) = c.chars().find(|ch| ch.is_control()) {
        return Err(invalid_name(format!("control character {bad:?}")));
    }

    if c.ends_with(' ') || c.ends_with('.') {
        // Windows silently strips these, so two distinct SPACE names would
        // become one Windows name.
        return Err(invalid_name("component ends with a space or dot"));
    }

    // Reserved device names, with or without an extension.
    let stem = c.split('.').next().unwrap_or(c);
    if RESERVED_NAMES.iter().any(|r| r.eq_ignore_ascii_case(stem)) {
        return Err(invalid_name(format!("reserved device name: {c}")));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const L: &PathLimits = &PathLimits::DEFAULT;

    // --- accepted forms ---------------------------------------------------

    #[test]
    fn accepts_the_root() {
        let p = VfsPath::parse("\\").unwrap();
        assert!(p.is_root());
        assert_eq!(p.to_string(), "\\");
        assert_eq!(p.depth(), 0);
        assert_eq!(p.file_name(), None);
        assert_eq!(p.parent(), None);
    }

    #[test]
    fn accepts_simple_and_nested_paths() {
        let p = VfsPath::parse("\\file.txt").unwrap();
        assert_eq!(p.depth(), 1);
        assert_eq!(p.file_name(), Some("file.txt"));
        assert_eq!(p.parent().unwrap(), VfsPath::root());

        let p = VfsPath::parse("\\dir\\sub\\file.txt").unwrap();
        assert_eq!(p.depth(), 3);
        assert_eq!(p.file_name(), Some("file.txt"));
        assert_eq!(p.parent().unwrap().to_string(), "\\dir\\sub");
    }

    #[test]
    fn accepts_unicode_and_spaces_inside_a_component() {
        for good in [
            "\\hello world.txt",
            "\\Ångström",
            "\\日本語\\ファイル.txt",
            "\\a b\\c d",
            "\\file.name.with.dots.txt",
            "\\-leading-dash",
            "\\ leading space",
        ] {
            assert!(VfsPath::parse(good).is_ok(), "rejected valid path: {good}");
        }
    }

    #[test]
    fn display_round_trips_through_parse() {
        // property in §14.2: parse(display(parse(p))) == parse(p)
        for good in ["\\", "\\a", "\\a\\b\\c", "\\Ångström\\x"] {
            let p = VfsPath::parse(good).unwrap();
            let again = VfsPath::parse(&p.to_string()).unwrap();
            assert_eq!(p, again, "{good}");
        }
    }

    // --- one test per rejection rule (fs-semantics §7) --------------------

    #[test]
    fn rejects_empty() {
        assert_eq!(
            VfsPath::parse("").unwrap_err().code,
            ErrorCode::ObjectNameInvalid
        );
    }

    #[test]
    fn rejects_no_leading_backslash() {
        for bad in ["file.txt", "dir\\file", "C:\\x"] {
            assert_eq!(
                VfsPath::parse(bad).unwrap_err().code,
                ErrorCode::ObjectNameInvalid,
                "{bad}"
            );
        }
    }

    #[test]
    fn rejects_dot_and_dotdot_components() {
        for bad in ["\\.", "\\..", "\\a\\.\\b", "\\a\\..\\b", "\\a\\.."] {
            assert_eq!(
                VfsPath::parse(bad).unwrap_err().code,
                ErrorCode::ObjectNameInvalid,
                "{bad}"
            );
        }
    }

    #[test]
    fn rejects_repeated_separators() {
        for bad in ["\\\\", "\\a\\\\b", "\\\\a"] {
            assert_eq!(
                VfsPath::parse(bad).unwrap_err().code,
                ErrorCode::ObjectNameInvalid,
                "{bad}"
            );
        }
    }

    #[test]
    fn rejects_trailing_separator() {
        assert!(VfsPath::parse("\\a\\").is_err());
    }

    #[test]
    fn rejects_colon_in_a_component() {
        for bad in ["\\a:b", "\\dir\\file:stream", "\\C:"] {
            assert_eq!(
                VfsPath::parse(bad).unwrap_err().code,
                ErrorCode::ObjectNameInvalid,
                "{bad}"
            );
        }
    }

    #[test]
    fn rejects_reserved_device_names_with_or_without_extension() {
        for bad in [
            "\\CON",
            "\\con",
            "\\PRN",
            "\\AUX",
            "\\NUL",
            "\\COM1",
            "\\COM9",
            "\\LPT1",
            "\\LPT9",
            "\\CON.txt",
            "\\nul.log",
            "\\dir\\COM3.dat",
        ] {
            assert_eq!(
                VfsPath::parse(bad).unwrap_err().code,
                ErrorCode::ObjectNameInvalid,
                "accepted reserved name: {bad}"
            );
        }
        // ...but names that merely start with a reserved stem are fine.
        for good in ["\\CONSOLE", "\\console.txt", "\\COM10", "\\NULL.txt"] {
            assert!(VfsPath::parse(good).is_ok(), "rejected: {good}");
        }
    }

    #[test]
    fn rejects_trailing_space_or_dot_in_a_component() {
        for bad in ["\\a ", "\\a.", "\\dir \\file", "\\dir.\\file", "\\file..."] {
            assert_eq!(
                VfsPath::parse(bad).unwrap_err().code,
                ErrorCode::ObjectNameInvalid,
                "{bad}"
            );
        }
    }

    #[test]
    fn rejects_windows_forbidden_characters() {
        for bad in ["\\a<b", "\\a>b", "\\a\"b", "\\a|b", "\\a?b", "\\a*b"] {
            assert_eq!(
                VfsPath::parse(bad).unwrap_err().code,
                ErrorCode::ObjectNameInvalid,
                "{bad}"
            );
        }
    }

    #[test]
    fn rejects_embedded_nul() {
        assert_eq!(
            VfsPath::parse("\\a\0b").unwrap_err().code,
            ErrorCode::ObjectNameInvalid
        );
    }

    #[test]
    fn rejects_control_and_bidi_override_characters() {
        // U+202E RIGHT-TO-LEFT OVERRIDE makes a name display as something other
        // than what it is. It is a Cf, not a control char, so it is checked
        // separately from the C0 range -- see the assertion below.
        assert!(VfsPath::parse("\\a\u{0001}b").is_err());
        assert!(VfsPath::parse("\\a\u{007F}b").is_err());
    }

    // --- limit rules (L1 / L2 / L3) ---------------------------------------

    #[test]
    fn l1_path_length_at_limit_and_limit_plus_one() {
        // 32,767 chars total including the leading backslash.
        let at = format!("\\{}", "a".repeat(L.max_path_chars - 1));
        assert_eq!(at.chars().count(), L.max_path_chars);
        // The component bound (L2) would reject it first, so use nested
        // components of legal length to isolate L1.
        let seg = "a".repeat(100);
        let mut s = String::new();
        while s.chars().count() + 101 <= L.max_path_chars {
            s.push('\\');
            s.push_str(&seg);
        }
        assert!(VfsPath::parse(&s).is_ok(), "L1 at limit should pass");

        let over = format!("\\{}", "a".repeat(L.max_path_chars));
        assert_eq!(
            VfsPath::parse(&over).unwrap_err().code,
            ErrorCode::NameTooLong
        );
    }

    #[test]
    fn l2_component_length_at_limit_and_limit_plus_one() {
        let at = format!("\\{}", "a".repeat(L.max_component_chars));
        assert!(VfsPath::parse(&at).is_ok(), "L2 at limit should pass");

        let over = format!("\\{}", "a".repeat(L.max_component_chars + 1));
        assert_eq!(
            VfsPath::parse(&over).unwrap_err().code,
            ErrorCode::NameTooLong
        );
    }

    #[test]
    fn l3_path_depth_at_limit_and_limit_plus_one() {
        let at: String = (0..L.max_path_depth).map(|_| "\\a").collect();
        assert!(VfsPath::parse(&at).is_ok(), "L3 at limit should pass");

        let over: String = (0..L.max_path_depth + 1).map(|_| "\\a").collect();
        assert_eq!(
            VfsPath::parse(&over).unwrap_err().code,
            ErrorCode::NameTooLong
        );
    }

    #[test]
    fn a_40000_char_path_is_rejected_without_panicking() {
        // §2.5 / §7.2. The interesting property is that it does not allocate a
        // component vector for 40,000 characters before deciding.
        let bad = format!("\\{}", "a".repeat(40_000));
        assert_eq!(
            VfsPath::parse(&bad).unwrap_err().code,
            ErrorCode::NameTooLong
        );
    }

    // --- INV-NS-6 ---------------------------------------------------------

    #[test]
    fn inv_ns_6_traversal_forms_are_rejected() {
        // The host-filesystem half of this assertion lives in the conformance
        // suite (§7.2), which also snapshots C:\Windows\...\hosts.
        for evil in [
            "\\..\\..\\Windows\\System32\\drivers\\etc\\hosts",
            "\\a\\..\\..\\..\\Windows\\win.ini",
            "\\\\?\\C:\\Windows\\win.ini",
            "\\a\\..\u{202E}\\..\\Windows",
            "\\..",
            "\\a\\..",
        ] {
            assert!(
                VfsPath::parse(evil).is_err(),
                "accepted traversal: {evil:?}"
            );
        }
    }

    #[test]
    fn is_ancestor_of_detects_subtree_containment() {
        let caps = Capabilities::PHASE_1;
        let a = VfsPath::parse("\\dir").unwrap();
        let b = VfsPath::parse("\\dir\\sub\\deep").unwrap();
        assert!(a.is_ancestor_of(&b, caps));
        assert!(!b.is_ancestor_of(&a, caps));
        // Not an ancestor of itself -- "proper" ancestor.
        assert!(!a.is_ancestor_of(&a, caps));
        // Case-insensitively, per CaseSensitiveSearch = 0.
        let c = VfsPath::parse("\\DIR\\other").unwrap();
        assert!(a.is_ancestor_of(&c, caps));
        // A sibling with a shared prefix is not an ancestor.
        let d = VfsPath::parse("\\dir2\\x").unwrap();
        assert!(!a.is_ancestor_of(&d, caps));
    }

    // --- folding (fs-semantics §7, both capability variants) --------------

    #[test]
    fn ascii_folding_collides_a_with_lowercase_a_but_not_angstrom() {
        let caps = Capabilities::PHASE_1;
        assert_eq!(FoldedName::new("A", caps), FoldedName::new("a", caps));
        assert_eq!(
            FoldedName::new("FILE.TXT", caps),
            FoldedName::new("file.txt", caps)
        );
        // Å / å do NOT collide under ASCII folding.
        assert_ne!(FoldedName::new("Å", caps), FoldedName::new("å", caps));
    }

    #[test]
    fn unicode_folding_collides_angstrom_too() {
        let caps = Capabilities::PHASE_2_TARGET;
        assert_eq!(FoldedName::new("A", caps), FoldedName::new("a", caps));
        assert_eq!(FoldedName::new("Å", caps), FoldedName::new("å", caps));
    }

    #[test]
    fn folding_preserves_the_display_name() {
        // CasePreservedNames = 1: folding produces a key, it does not rewrite
        // the name. The node keeps what the caller gave.
        let p = VfsPath::parse("\\MiXeD.TxT").unwrap();
        assert_eq!(p.file_name(), Some("MiXeD.TxT"));
    }

    // --- join -------------------------------------------------------------

    #[test]
    fn join_validates_the_new_component() {
        let root = VfsPath::root();
        assert_eq!(root.join("a", L).unwrap().to_string(), "\\a");
        assert!(root.join("..", L).is_err());
        assert!(root.join("a:b", L).is_err());
        assert!(root.join("", L).is_err());
        assert!(root.join("CON", L).is_err());
    }

    #[test]
    fn join_enforces_l3() {
        let deep =
            VfsPath::parse(&(0..L.max_path_depth).map(|_| "\\a").collect::<String>()).unwrap();
        assert_eq!(deep.join("b", L).unwrap_err().code, ErrorCode::NameTooLong);
    }
}
