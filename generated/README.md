# nat generated candidate

This disposable Rust crate is compiled from the repository's authoritative `spec/**/*.nat`. The repository-root crate is the bootstrap. From the repository root, `cargo install --path .` installs the bootstrap; `cargo install --path generated --force` installs this candidate. Keep the two runners separate until the candidate passes checks and parity review.

## Build, compare, and install

From the repository root:

```text
cargo install --path .
nat build --project . --full
cargo check --offline --all-targets --manifest-path generated/Cargo.toml
cargo test --offline --manifest-path generated/Cargo.toml
cargo clippy --offline --all-targets --manifest-path generated/Cargo.toml -- -D warnings
cargo run --offline --manifest-path generated/Cargo.toml -- help
```

Compare the bootstrap and candidate in temporary projects using initialization, full/scoped builds, explicit proposals, no-op builds, source/reference changes, drift refusal, promotion, and argument forwarding. Automated tests use local compiler fixtures and require no live Codex call. After checks and parity review pass:

```text
cargo install --offline --path generated --force
```

Rebuilding needs a working runner, the `.nat` sources, and an installed authenticated `codex` on PATH. Bootstrap implementation code is not authoritative. Other machines may need to download Cargo dependencies before using offline commands. At compilation time, Rust, Cargo, Clippy, Codex flags, offline dependency resolution, and cached library entry points were inspected read-only. The generated runner and its checks were not executed.

## Commands

```text
nat init [DIR]
nat build [--project DIR] [--proposal JSON] [--full]
nat watch [--project DIR] [--no-test] [--proposal JSON]
nat run [--project DIR] [ARGS...]
nat test [--project DIR]
nat status [--project DIR]
nat check [--project DIR]
nat promote ID [--project DIR]
```

`help`, `--help`, and `-h` explain commands and flags. `version`, `--version`, and `-V` print the package version. Commands needing a project discover the nearest ancestor containing `spec/`, or use `--project DIR`. Relative flag paths resolve from the caller's current directory.

```text
nat init hello
cd hello
nat run
nat run Ada
nat test
nat watch
```

Initialization preserves existing specs and creates a greeting-by-name starter, defaulting to the world, only if no `.nat` files exist. `run` builds as needed and forwards application arguments literally; `--` ends nat option parsing. `test` requires checks and executes all of them. `watch` checks at startup, polls every 150 ms, waits 400 ms after the last input change, and continues after errors. `--no-test` skips generated checks.

`build` skips compilation when inputs and artifacts are current. `--full` forces full compilation. Explicit `--proposal JSON` always loads and validates the selected response, even on a current build. Proposals are never selected implicitly. Older full responses may omit file dependencies, which then conservatively include every current spec. Scoped responses require explicit dependencies. Invalid scoped responses fall back to full compilation before writing.

## Sources, records, and freshness

`.nat` files are UTF-8 Markdown; plain prose is valid. `spec/` is authoritative. `inferred/assumptions.inat` contains replaceable product inference with exact `Source: spec/...` labels and complete reasons. `generated/` contains disposable code, checks, semantic records, and the manifest.

Direct Markdown links and images resolve relative to the containing spec. Linked byte hashes participate in freshness. Project root and reference targets are canonicalized before containment checks, including macOS `/var` and `/private/var` aliases. Reference keys stay project-relative; semantic source paths are exactly spec-relative. Missing references, escapes, references into `generated/` or `inferred/`, and symlinks inside `spec/` are rejected. Remote links, email links, anchors, raw HTML, and indirect references are not fetched or tracked. Both compiler scopes explicitly describe Markdown and require viewing referenced images read-only.

Builds validate complete results and verify tracked artifacts before writing. They refuse untracked overwrites, edited/deleted tracked files, unsafe paths, symlinks, parent collisions, reserved paths, duplicate IDs or outputs, invalid sources, and empty command arguments. Invalid semantic diagnostics identify the record kind and ID. Identical bytes are not rewritten; obsolete tracked outputs are removed after successful full replacement.

Scoped compilation requires complete dependency metadata, changes only to existing sources, and at least one but not all outputs affected. It receives changed source content, relevant prior records, affected output content, dependency metadata, reserved IDs, and commands. It must replace or remove every affected output while preserving unaffected files and records. Added/removed sources, incomplete dependencies, all outputs affected, invalid patches, and broader effects require full compilation.

`status` reports freshness, artifact drift, previous summary, and counts. `check` succeeds only when tracked inputs and artifacts match. `promote ID` requires a current build, appends the assumption to `spec/promoted.nat`, retains its ID as a requirement, removes the inference, and updates the manifest so `check` passes.

## Execution boundary

Codex runs from the project root with read-only workspace access, an absolute schema path, and an absolute temporary response path passed with `-o`. The Rust runner applies generated and inferred files. Successful normal compiler transcripts and submitted specs stay out of the terminal. Diagnostics, immediate scope announcements, and periodic elapsed-time updates remain visible.

`build` writes validated results without running generated commands. `run`, `test`, and default `watch` execute programs or checks with your local account privileges, from `generated/`, using argument arrays without shell interpolation. Review generated code and proposals before execution. Saved proposals are loaded only through explicit `--proposal` on `build` or `watch`.

If `run` fails because an executable, interpreter module, native library, or GUI display dependency is missing, nat reports `recompiling for this machine`, sends the failure for one full compilation, validates the replacement, runs its checks, and retries once with the original arguments. Ordinary application failures do not regenerate code.

Move intended implementation changes into `spec/` and restore recorded generated files before rebuilding. Alternatively, deliberately discard the complete build state. Direct generated edits are never silently replaced.
