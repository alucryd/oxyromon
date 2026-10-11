# Files

- [DAT Providers and Naming Conventions](dat-providers-no-intro-redump.md) - The external catalog providers oxyromon knows about (No-Intro dat-o-matic, Redump, MAME/FBNeo), the provider URLs and Redump system-code catalogue that drive download and update-check, the download path's pacing and save option, and how shiratsu-naming No-Intro/TOSEC tokens become the regions, languages, flags and version order used by both DAT import and sorting.
- [External Tool Integration](external-tools.md)
- [Format Crates in crates/](format-crates.md) - The five in-repo format crates (gdi-rs, nsz-rs, xdelta-rs, xps-rs, xso-rs) that replaced oxyromon's external tools — their shared synchronous path-in/path-out entry-point contract with a progress callback, the identical CLI shell enforced by tests/crates.rs, the .part output discipline, error conventions, library-first manifests and per-crate release flow.
