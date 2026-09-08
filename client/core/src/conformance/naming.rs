//! Names and paths (fs-semantics §7; manual §3.3.7).
//!
//! Case folding is the one capability Phase 1 ships (§11.5). **Both variants
//! run here**, each asserting its own specified behaviour: a capability selects
//! between documented behaviours, it never disables a check.

use contracts::ErrorCode;

use crate::vfs::invariants::VfsDiagnostics;
use crate::vfs::limits::Limits;
use crate::vfs::types::*;
use crate::vfs::Vfs;

use super::util::{expect_err, step, Ctx};

pub fn all<V: Vfs + VfsDiagnostics>(vfs: &V, caps: Capabilities, limits: Limits) {
    let c = Ctx::new(vfs, caps, limits, "naming");

    every_rejection_rule(&c);
    valid_names_are_accepted(&c);
    lookup_is_case_insensitive(&c);
    display_names_are_case_preserved(&c);
    case_folding_matches_the_configured_capability(&c);
    inv_ns_6_path_traversal_cannot_escape_the_namespace(&c);
}

fn every_rejection_rule<V: Vfs + VfsDiagnostics>(c: &Ctx<V>) {
    step(c.vfs, "naming rejection rules", || {
        // One entry per rule in fs-semantics §7. The error code matters: a
        // name that breaks a naming rule is ObjectNameInvalid; one that breaks
        // a length bound is NameTooLong.
        let invalid = [
            ("empty", ""),
            ("no leading backslash", "file.txt"),
            ("dot component", "\\."),
            ("dotdot component", "\\.."),
            ("embedded dotdot", "\\a\\..\\b"),
            ("repeated separator", "\\a\\\\b"),
            ("trailing separator", "\\a\\"),
            ("colon in a component", "\\a:b"),
            ("alternate data stream", "\\file.txt:stream"),
            ("reserved CON", "\\CON"),
            ("reserved con lowercase", "\\con"),
            ("reserved with extension", "\\NUL.txt"),
            ("reserved COM1", "\\COM1"),
            ("reserved LPT9", "\\LPT9"),
            ("trailing space", "\\name "),
            ("trailing dot", "\\name."),
            ("forbidden <", "\\a<b"),
            ("forbidden >", "\\a>b"),
            ("forbidden quote", "\\a\"b"),
            ("forbidden pipe", "\\a|b"),
            ("forbidden question", "\\a?b"),
            ("forbidden asterisk", "\\a*b"),
            ("embedded NUL", "\\a\0b"),
            ("control character", "\\a\u{0001}b"),
        ];
        for (rule, s) in invalid {
            let r = c.raw(s);
            assert!(r.is_err(), "naming rule not enforced: {rule} ({s:?})");
            let code = r.unwrap_err().code;
            assert_eq!(
                code,
                ErrorCode::ObjectNameInvalid,
                "{rule} ({s:?}) should be ObjectNameInvalid, got {code:?}"
            );
        }

        // Length bounds report NameTooLong, not ObjectNameInvalid.
        let long_component = format!("\\{}", "a".repeat(300));
        expect_err(
            "300-char component",
            ErrorCode::NameTooLong,
            c.raw(&long_component),
        );
        let long_path = format!("\\{}", "a".repeat(40_000));
        expect_err(
            "40,000-char path",
            ErrorCode::NameTooLong,
            c.raw(&long_path),
        );
    });
}

fn valid_names_are_accepted<V: Vfs + VfsDiagnostics>(c: &Ctx<V>) {
    step(c.vfs, "valid names", || {
        // Names that merely resemble a rejected form must still work; over-eager
        // validation is as much a defect as under-eager.
        for name in [
            "ordinary.txt",
            "with space.txt",
            "dots.in.the.middle.txt",
            "CONSOLE.txt",
            "NULL.txt",
            "COM10",
            "-leading-dash",
            "_underscore",
            "Ångström.txt",
            "日本語.txt",
            "emoji-\u{1F600}.txt",
            "UPPER.TXT",
        ] {
            c.file_with(name, b"ok");
            assert!(c.exists(name), "valid name rejected: {name:?}");
        }
    });
}

fn lookup_is_case_insensitive<V: Vfs + VfsDiagnostics>(c: &Ctx<V>) {
    step(c.vfs, "case-insensitive lookup", || {
        // CaseSensitiveSearch = 0.
        c.file_with("CaseTest.txt", b"content");
        assert!(c.exists("CaseTest.txt"));
        assert!(c.exists("casetest.txt"));
        assert!(c.exists("CASETEST.TXT"));
        assert_eq!(c.read_all("cAsEtEsT.tXt"), b"content");

        // ...so creating a differently-cased twin collides.
        expect_err(
            "create a case-variant of an existing name",
            ErrorCode::FileExists,
            c.try_create_file("CASETEST.TXT"),
        );
    });
}

fn display_names_are_case_preserved<V: Vfs + VfsDiagnostics>(c: &Ctx<V>) {
    step(c.vfs, "case-preserved display names", || {
        // CasePreservedNames = 1: names are stored as given.
        c.file_with("MiXeDCaSe.TxT", b"x");
        let names = c.list("");
        assert!(
            names.iter().any(|n| n == "MiXeDCaSe.TxT"),
            "display name was not preserved: {names:?}"
        );
    });
}

fn case_folding_matches_the_configured_capability<V: Vfs + VfsDiagnostics>(c: &Ctx<V>) {
    step(c.vfs, "case folding capability", || {
        // Both variants are specified in fs-semantics §7, and both are tested:
        // the capability selects which documented behaviour is expected, it
        // never decides whether the rule is checked.
        c.file_with("Å-ring.txt", b"upper");

        if c.caps.unicode_case_folding {
            // Unicode simple case folding: A-ring and a-ring collide.
            expect_err(
                "unicode folding: Å/å must collide",
                ErrorCode::FileExists,
                c.try_create_file("å-ring.txt"),
            );
        } else {
            // ASCII folding: they are distinct names.
            let h = c
                .try_create_file("å-ring.txt")
                .expect("ascii folding: Å/å must NOT collide");
            c.close(h);
            assert!(c.exists("å-ring.txt"));
        }

        // A/a collide under both variants.
        c.file_with("Ascii.txt", b"x");
        expect_err(
            "A/a must collide under both variants",
            ErrorCode::FileExists,
            c.try_create_file("ASCII.TXT"),
        );
    });
}

fn inv_ns_6_path_traversal_cannot_escape_the_namespace<V: Vfs + VfsDiagnostics>(c: &Ctx<V>) {
    step(c.vfs, "INV-NS-6 path traversal", || {
        // Two-part assertion (§7.2): the parse rejects, *and* the host
        // filesystem is untouched. The second half is what makes this an
        // isolation test rather than a parser test.
        const HOSTS: &str = r"C:\Windows\System32\drivers\etc\hosts";
        let before = std::fs::metadata(HOSTS)
            .ok()
            .and_then(|m| m.modified().ok());

        for evil in [
            r"\..\..\Windows\System32\drivers\etc\hosts",
            r"\a\..\..\..\Windows\win.ini",
            r"\\?\C:\Windows\win.ini",
            "\\a\\..\u{202E}\\..\\Windows",
            r"\..",
            r"\a\..",
            r"C:\Windows\win.ini",
            r"\\server\share\file",
        ] {
            assert!(c.raw(evil).is_err(), "accepted traversal: {evil:?}");
        }

        if let Some(before) = before {
            let after = std::fs::metadata(HOSTS)
                .expect("hosts file vanished during the test")
                .modified()
                .expect("hosts mtime unreadable");
            assert_eq!(
                after, before,
                "a filesystem operation touched a file outside the namespace"
            );
        }
    });
}
