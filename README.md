# nat

`nat` is a Rust runner for projects whose human-authored source is natural language. It is an experiment in keeping intent separate from inference and implementation.

| Layer | Purpose | Edited by |
| --- | --- | --- |
| `spec/*.nat` | Authoritative behavior and constraints | Human |
| `inferred/assumptions.inat` | Product behavior inferred because `.nat` left a gap | Runner |
| `generated/ir.json` | Requirements mapped to source files and inferred assumptions | Runner |
| `generated/*` | Runnable code, tests, and project files | Runner |

The runner asks the local Codex CLI for one structured compilation result. It validates that result, writes generated files, and records hashes. It refuses to rebuild if a tracked generated file was edited. To own an inferred behavior, promote it into `.nat` with `nat promote ID`, or write your own rule in `.nat` and rebuild.

## Quick start

```sh
cargo install --path . --locked
nat init my-app
cd my-app
$EDITOR spec/app.nat
nat run Alice
nat watch
```

`nat run` builds the project when needed, then runs the generated program. `nat test` does the same before running its checks. `nat watch` watches `spec/**/*.nat`, rebuilds after edits settle, and runs the checks. Use `nat watch --no-test` to rebuild without running checks. Commands locate the project root from the current directory or a child directory; use `--project DIR` to select another project.

`nat build` uses an installed and authenticated `codex` CLI. Codex runs with read-only workspace access and returns JSON; the Rust runner writes the files. The generated program can use any language. `nat test` and `nat run` execute the commands declared by the compilation result from inside `generated/`, with arguments passed directly rather than through a shell. You can use `cargo run -- <command>` from this repository without installing the binary.

Before generating code, the compiler asks Codex to inspect the available runtimes and verify required imports, including GUI toolkits. If `nat run` still fails because an interpreter, module, or native library is unavailable, it sends the failure back to Codex, recompiles once for the current machine, runs the generated checks, and retries. Other program failures are reported without an automatic retry.

Use `nat status` for a short report or `nat check` as a gate before running generated code. A changed `.nat` file makes the build stale. A direct edit to a tracked `.inat` or generated file blocks rebuilding; move the desired behavior into `.nat` first.

## Try the runner without an agent call

The checked-in hello example includes a saved compilation result. This tests the complete write, check, promote, and run path without a Codex invocation:

```sh
./target/release/nat build --project examples/hello --proposal examples/hello/compilation.json
./target/release/nat check --project examples/hello
./target/release/nat run --project examples/hello -- Ada
./target/release/nat test --project examples/hello
```

The `--proposal` option is also useful for inspecting a saved agent result or for deterministic tests. The runner validates the proposal before writing anything.

## Trust boundary

`nat run` executes generated code, and `nat test` and `nat watch` execute checks declared by the compilation result. A runtime dependency failure may cause `nat run` to regenerate code and retry once. Only run projects and saved `--proposal` files you trust. Codex has read-only access during compilation, but the resulting program and checks run with your local account's permissions.

## Current scope

This is a small compiler loop, not a full natural-language language definition. The intermediate record contains requirements and assumptions rather than a complete type system or formal semantics. Compilation is model driven and may vary between runs; stable IDs and prior inferences are requested, but semantic equivalence is not mechanically proven. `watch` recompiles only when `.nat` content changes; it does not restart a long-running generated program. There is no automatic repair loop yet.

Codex CLI structured output and read-only execution follow [official OpenAI documentation](https://developers.openai.com/blog/eval-skills/).
