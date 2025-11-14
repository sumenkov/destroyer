# Contributing

Thanks for your interest!

## Reporting bugs
- Include OS (Linux/macOS), kernel/macOS version, device model, and command-line used.
- Paste the full error output.

## Pull requests
- One logical change per PR.
- Keep code formatted (`rustfmt`), avoid unnecessary dependencies.
- Update README if behavior/flags change.

## Build & test workflow
- **Release builds:** `cargo +nightly build --release -Zbuild-std=std,panic_abort` (panic-abort std + thin LTO).
- **Tests:** `cargo test --features test-support` (helpers stay out of release artifacts).
- **Clippy:** `cargo clippy --release -- -W clippy::perf`.
- **Benchmarks:** `cargo bench --features test-support` (Criterion suite under `benches/`).

## CI / quality gates
- Track binary growth: `cargo bloat --release -n 20`.
- Inspect hot paths: `cargo asm --release destroyer::wipe::pass_random` (and related routines).
- Benchmark regularly (NVMe/HDD baselines) and update docs when defaults change.
