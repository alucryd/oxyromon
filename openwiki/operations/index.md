# Files

- [Builds, Distribution and Releases](build-and-release.md) - How oxyromon's helper scripts (build.sh, desktop.sh, dist.sh, docker.sh, test.sh) produce the CLI, desktop bundle and container images, which cross-compilation targets and signing steps the release matrix covers, and the ordering and blockers that govern publishing the eight format crates and oxyromon itself.
- [CI Pipelines and Runtime Environment](ci-and-environment.md) - How .github/workflows/continuous_integration.yml gates every change (apt reference tools for the format crates' tests, toolchain, trunk, the two clippy passes, release build and coverage), and exactly which runtime environment variables the binary reads, when, and for how long they stay authoritative.
