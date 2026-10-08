# nat generated candidate

This disposable Rust crate is compiled from authoritative repository-root `spec/**/*.nat`. The root Cargo crate is the handwritten bootstrap. Keep the bootstrap and candidate separate until the candidate passes checks and parity review. Neither compilation nor regeneration installs or replaces the running compiler.

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

The first command installs the bootstrap. After testing and parity review, this separate command installs the candidate:

```text
cargo install --offline --path generated --force
```

Rebuilding needs a working runner, the human sources, and installed authenticated `codex` on PATH. Bootstrap code is not authoritative. Other machines may need to download Cargo dependencies before using offline commands. Read-only host inspection verified Rust, Cargo, Clippy, and offline resolution of the existing dependencies. This revision introduces no dependencies. Its generated program and checks were not executed during compilation.

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

Help aliases are `help`, `--help`, and `-h`; version aliases are `version`, `--version`, and `-V`. Project discovery searches the current directory and ancestors for `spec/`. `--project DIR` selects a project explicitly. Relative flag paths resolve from the caller's directory.

`init` preserves existing sources and creates a greeting-by-name starter, defaulting to the world, only when no `.nat` sources exist. Try `nat init hello`, enter that directory, then run `nat run Ada` and `nat test`.

`build` skips the compiler when inputs and tracked artifacts are current. `--full` forces full compilation. Explicit `--proposal JSON` always loads and validates that response, even on a current build. Proposals are never selected implicitly. `run` forwards application arguments literally; `--` ends nat option parsing. `test` requires checks and executes every declared check. `watch` builds at startup, polls every 150 ms, rebuilds after 400 ms of settled changes, and continues after errors. It runs checks unless `--no-test` is supplied.

## Full compilation and verified reuse

Full compilation supplies every source, previous semantic records and inference, and a compact catalog of verified prior outputs. Catalog entries contain exact generated-relative paths, SHA-256 hashes, and previous dependencies. The compiler may read those files locally. The runner freezes their verified contents before invoking Codex.

New full responses require `status` (`complete` or `incomplete`) and a string `diagnostic`, alongside `summary`, `requirements`, `assumptions`, `files`, `run`, and `checks`. Each file supplies its current dependencies and either complete string content or `null`. Null requests the exact advertised prior bytes at that exact path. It does not mean an empty file or authorize reading an arbitrary path. A full response still lists the complete file inventory; omitted prior outputs are removed after successful validation. All-null inventories are legal when only source hashes or semantic records change.

The runner resolves null entries from its frozen snapshot, validates the assembled result, and rechecks sources, prior artifacts, and the manifest immediately before publication. Fresh builds cannot reuse. Unadvertised, untracked, missing, wrong-case, duplicate, symlinked, or stale entries fail without publication. Identical files are not rewritten, preserving their bytes, modification times, permissions, and inode identity.

For a full `--proposal`, the current tracked build at invocation is the explicit reuse basis. Diagnostics report its manifest hash and verified output count. It never refers to an unknown previous compiler session. Legacy full responses without status remain supported, as do string files with omitted dependencies; those dependencies conservatively include all current sources. Null files require explicit dependencies.

An incomplete full response is rejected even if it contains a diagnostic placeholder file and plausible commands. Its diagnostic and retained response path are reported, with no automatic compiler retry. Rejected structured responses are retained in uniquely named `.nat-diagnostic-*` directories in the project root, outside `generated/` and `inferred/`. Invalid JSON and failed compiler responses are retained as raw bytes when available. A complete flag does not bypass structural checks or establish behavioral correctness. Builds do not execute behavioral checks; use `nat test` separately.

## Sources, scope, and freshness

`.nat` files are UTF-8 Markdown; plain prose remains valid. `spec/` is authoritative. `inferred/assumptions.inat` contains replaceable inference with exact `Source: spec/...` labels and complete reasons. Generated code and records are disposable.

Direct Markdown links and images resolve relative to the containing spec. Linked byte hashes participate in freshness. Project and reference targets are canonicalized before containment checks, including macOS `/var` and `/private/var` aliases, while reference keys remain project-relative. Missing files, escapes, references into generated or inferred directories, and symlinks inside `spec/` are rejected. Remote URLs, email links, anchors, raw HTML, and indirect references are not fetched or tracked. Both compiler scopes require inspecting local documents and viewing referenced images read-only.

Scoped compilation requires complete dependency metadata, changes only to existing sources, and at least one but not all outputs affected. Its input includes changed source contents, relevant records, affected old contents, dependencies, reserved IDs, and previous commands. Scoped responses require string contents and explicit dependencies, replace or remove every affected output, and preserve unaffected records and files. Invalid scoped responses or wider effects fall back to full compilation before publication. The full-only status and reuse extensions do not change scoped authority.

All builds reject unsafe paths, symlinks, parent collisions, reserved filenames, invalid or duplicate IDs and sources, duplicate outputs, empty command arguments, tracked artifact drift, and untracked overwrites. Invalid semantic diagnostics identify the record kind and ID. `status` reports freshness, drift, summary, and counts; `check` requires matching tracked inputs and artifacts. `promote ID` requires a current build, appends the assumption to `spec/promoted.nat`, preserves its ID as a requirement, removes the inference, and updates freshness records.

## Execution boundary

Codex runs from the project root with read-only workspace access and absolute `--output-schema` and `-o` paths. The Rust runner alone publishes artifacts. Successful normal transcripts and submitted specs stay out of the terminal; compiler diagnostics, immediate scope announcements, and periodic elapsed-time updates remain visible.

Generated programs and checks execute with your local account privileges from `generated/`, using argv arrays without an implicit shell. Review code and proposals before execution. A missing executable, interpreter module, native library, or GUI display dependency triggers one full recompilation for the machine, replacement checks, and one retry with the original arguments. Ordinary application errors do not regenerate code.

Move intended implementation changes into human specs and restore recorded generated files before rebuilding, or deliberately discard the complete build state. Direct generated edits are never silently replaced.

## Offline acceptance and filesystem identity

Tests use local executable Codex fixtures through the real CLI, absolute schema/output flags, and captured schemas. They cover partial and all-null reuse, declared check execution, metadata preservation, stale and invalid reuse, incomplete rejection and response retention, legacy responses, scoped builds, CLI parsing, watching, references, promotion, and runtime repair. They make no live Codex calls.

Rejection and preservation fixtures run with projects created under both the ordinary inherited temporary directory and its explicitly canonical path. They do not change `TMPDIR`. Retained-response acceptance resolves the existing response, project, generated directory, and inferred directory before comparing path components. A canonical diagnostic path therefore remains usable even when a caller uses an alias such as `/var` for `/private/var`.

The tests open each retained response through available project spellings and verify identical canonical locations and exact bytes. Additional real CLI cases pass canonical, host-provided alias, and fixture symlink spellings through `--project`. Negative cases reject a genuinely different project, traversal to another project, symlink escapes, and responses inside generated or inferred directories. Existing production traversal and symlink refusal remain covered.

## Previously measured encoding

An earlier read-only offline comparison used the actual previous six-file candidate in this project. Both encodings included identical semantic records, dependencies, commands, `status: complete`, and an empty diagnostic. Compact UTF-8 JSON with full contents occupied 123,356 bytes. Replacing all six contents with null occupied 15,070 bytes: 108,286 bytes saved, or 87.78%. This measures that response encoding only; it does not measure this revision, live tokens, model behavior, runtime savings, or wall-clock duration.
