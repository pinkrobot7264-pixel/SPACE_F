//! Fuzz `wstr` + `VfsPath::parse` over arbitrary UTF-16 (§14.1).
//!
//! Asserts: no panic, and INV-NS-6 -- nothing that parses can escape the
//! namespace.

#![no_main]

use libfuzzer_sys::fuzz_target;
use space_client_core::vfs::VfsPath;

fuzz_target!(|data: &[u8]| {
    // Interpret the input as UTF-16 code units, which is what actually crosses
    // the boundary. Lone surrogates and unpaired halves are the interesting
    // cases and are deliberately not filtered out.
    let units: Vec<u16> = data
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect();

    let s = match String::from_utf16(&units) {
        Ok(s) => s,
        // Invalid UTF-16 is rejected at the boundary by `wstr`; the parser
        // never sees it. Nothing more to assert here.
        Err(_) => return,
    };

    // Must never panic, whatever the input.
    if let Ok(p) = VfsPath::parse(&s) {
        // INV-NS-6: anything that parses is inside the namespace. A parsed
        // path is absolute, has no relative components, and round-trips.
        let rendered = p.to_string();
        assert!(rendered.starts_with('\\'), "parsed path is not absolute: {rendered:?}");
        for c in p.components() {
            assert!(c != "." && c != "..", "a relative component survived parsing");
            assert!(!c.is_empty(), "an empty component survived parsing");
            assert!(!c.contains('\\'), "a separator survived inside a component");
            assert!(!c.contains(':'), "a colon survived inside a component");
        }
        // Re-parsing the rendered form yields the same path.
        let again = VfsPath::parse(&rendered).expect("a rendered path must re-parse");
        assert_eq!(p, again, "display/parse round trip diverged");
    }
});
