# Phase 1 evidence bundle

- Collected: 2026-09-10T20:48:07.8826315+02:00
- Commit: `58ea2ba9e03a03a521d1700164fcfe7b8472acdc` (committed 2026-09-10T20:45:02.0000000+02:00)
- Working tree: clean
- Last commit touching a path outside docs/: 58ea2ba 2026-09-10T20:45:02+02:00 fix: parse error in the collector's manifest block, and my process failure that let it through
- Files it changed outside docs/: scripts/collect-evidence.ps1

  A STALE artifact predates HEAD. Whether that matters depends on what
  changed in between: a commit touching only documentation or test
  scaffolding cannot alter product behaviour, while one touching
  client/, contracts/ or scripts/ may invalidate the run that
  produced the artifact. This collector does not make that judgement --
  it reports both so the reader can.

## Checks run at collection time

- PASS  cargo test --workspace
- PASS  cargo test -p core --release
- PASS  cargo clippy -D warnings
- PASS  cargo fmt --check
- PASS  os-safety-check

## Artifact inventory

| artifact | produced by | present | last written | status |
|---|---|---|---|---|
| `REQUIREMENTS.md` | auto | yes | 2026-09-10T20:23:51.8599266+02:00 | STALE (older than HEAD) |
| `PHASE-1-CERTIFICATION.md` | auto | yes | 2026-09-08T14:36:30.9388964+02:00 | STALE (older than HEAD) |
| `LEDGER.md` | auto | yes | 2026-09-08T15:16:52.4678606+02:00 | STALE (older than HEAD) |
| `fuzz-results.txt` | auto | yes | 2026-09-09T22:00:11.0010659+02:00 | STALE (older than HEAD) |
| `fuzz-op-sequence-fullbudget.txt` | auto | yes | 2026-09-08T14:38:39.7820309+02:00 | STALE (older than HEAD) |
| `mount-functional-test.txt` | auto | yes | 2026-09-10T09:00:08.2359950+02:00 | STALE (older than HEAD) |
| `kill-matrix.txt` | auto | yes | 2026-09-10T09:11:46.4767505+02:00 | STALE (older than HEAD) |
| `mount-stress.txt` | auto | yes | 2026-09-10T09:36:58.5289365+02:00 | STALE (older than HEAD) |
| `ntstatus-matrix.md` | auto | yes | 2026-09-10T09:00:26.3584096+02:00 | STALE (older than HEAD) |
| `compatibility-matrix.md` | auto | yes | 2026-09-10T09:00:36.4909621+02:00 | STALE (older than HEAD) |
| `io-stress.txt` | auto | yes | 2026-09-10T10:08:59.9698161+02:00 | STALE (older than HEAD) |
| `soak-samples.csv` | auto | yes | 2026-09-10T18:54:39.4863903+02:00 | STALE (older than HEAD) |
| `shutdown-states.txt` | auto | yes | 2026-09-10T09:02:07.5759283+02:00 | STALE (older than HEAD) |
| `fault-injection.txt` | auto | yes | 2026-09-10T20:06:57.1328298+02:00 | STALE (older than HEAD) |
| `enumeration-scaling.md` | auto | yes | 2026-09-08T03:14:24.3630480+02:00 | STALE (older than HEAD) |
| `EXPLORER-CHECKLIST.md` | auto | yes | 2026-09-08T01:21:52.6255824+02:00 | STALE (older than HEAD) |
| `procmon-writes.csv` | human | NO |  | OUTSTANDING (needs human) |
| `explorer` | human | NO |  | OUTSTANDING (needs human) |

## Verdict

**This bundle does not certify Phase 1.**

- 16 artifact(s) STALE -- they predate the commit being certified.
- 2 human validation(s) OUTSTANDING.
