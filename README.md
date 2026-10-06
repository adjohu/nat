# nat

Natural language as source code. Conventional code as a build artifact.

What if humans stopped maintaining conventional source code?

`nat` is an experiment in treating human-written intent as the canonical source, using an LLM to compile it into conventional software.

```markdown
# Conversation memory

When the user is talking, wait until they finish, then:

- Store what they said.
- Transcribe it.
- Summarize themes and store them in the database without duplicates.
```

Could compile to TypeScript today, Rust tomorrow, or something else entirely.

## Quick start

Install [Rust and Cargo](https://www.rust-lang.org/tools/install) and the [Codex CLI](https://learn.chatgpt.com/docs/codex/cli), then:

```sh
cargo install --git https://github.com/adjohu/nat --locked
codex login
nat init hello
cd hello
nat run
```

`nat init` creates a starter greeting spec in `spec/app.nat`. `nat run` builds and runs it; with no name given, the generated program greets the world. Edit the spec to change the program, then run `nat run` again. `nat watch` rebuilds and checks it as you edit.

## The idea

There are three layers:

1. **Human-written source**
   The canonical description of what the software should do.
2. **Machine-inferred source**
   Assumptions and details filled in by the compiler. Still natural language. Useful decisions can be promoted into the human-written source.
3. **Generated code**
   Disposable implementation.

The more precisely you specify something, the less freedom the compiler has.

A whole application might be described in a handful of high-level files, or the source could become granular enough to map almost 1:1 onto the generated implementation.

The generated code isn't the source of truth.

The intent is.

## Why?

Coding agents are already generating increasingly large amounts of implementation.

But the context that produced that implementation is usually scattered across conversations, prompts, code and implicit assumptions.

`nat` explores making that intent explicit and durable.

Rather than preserving generated implementation forever, regenerate it from the thing the human actually meant.

## Status

Very early experiment.

Expect everything to change.

## CLI

```text
Commands:
  nat init [DIR]
  nat build [--project DIR] [--proposal JSON] [--full]
  nat watch [--project DIR] [--no-test] [--proposal JSON]
  nat run [--project DIR] [ARGS...]
  nat test [--project DIR]
  nat status [--project DIR]
  nat check [--project DIR]
  nat promote ID [--project DIR]
```

Arguments after `nat run` are passed to the generated program. For the starter greeting, a name changes whom it greets.

`nat build` skips unchanged specs. For edits, `nat` uses compiler-reported dependencies to update affected outputs. New or removed specs and edits affecting every output get a full rebuild; the compiler can request one when the scope is uncertain. Because dependencies are inferred, use `nat build --full` whenever you want whole-project regeneration.

Build progress appears on stderr, including the compilation scope and elapsed-time
updates every 10 seconds while Codex is working. Compiler errors include diagnostic
output if compilation fails.

In compiler output and imported proposals, `source` and `sources` contain exact paths relative to `spec/` (for example, `03-warrant.nat`). Put requirement IDs, line numbers, and detailed citations in the statement or reason instead of appending them to a path. The compiler's output schema restricts these fields to existing source paths; incremental requirement and assumption records are restricted to changed sources.

## Writing

`.nat` files are Markdown. Use headings, lists, code examples, links, and images
to describe what you want. No front matter or special syntax is required, and
existing plain-text specs still work.

For example, `spec/viewer.nat` could contain:

```markdown
# Mind Viewer

A local read-only viewer for Plural Matter Mind.

Browsing must never mutate cognitive state.

## Overview

Use this existing viewer as a design reference:

![Mind v2 viewer](./references/mind-v2.png)

Preserve the information hierarchy and density, but do not copy its
implementation architecture.

## Requirements

- Show recent experiences.
- Show learned definitions and revisions.
- Show pending and unresolved work.
- Allow tracing a reply back to selected context and publications.
```

Put that image at `spec/references/mind-v2.png`. Relative links resolve from the
`.nat` file that contains them. You can also link documents, such as
`[Interaction notes](./references/interactions.md)`, or use Markdown reference-style
links. The compiler receives the original Markdown and is instructed to inspect
the referenced files and images as context for your requirements.

Local references must point to existing files inside the project, outside
`generated/` and `inferred/`. Their contents are tracked: editing a linked file
makes its referring specs stale for `build`, `check`, `status`, and `watch`.
Only files directly linked from `.nat` Markdown are tracked; links inside those
files are not followed recursively. Remote URLs and raw HTML references remain
in the spec but are not fetched or tracked by the runner. Keep reference assets
locally when their changes should trigger a rebuild.

## Building nat from nat

This repository's [spec/](spec/) describes nat itself: the CLI, source format,
compilation protocol, scoped rebuilds, execution, and self-hosting. The checked-in
Rust code under `generated/` is a build artifact, and it is the `nat` binary Cargo
installs. To rebuild it from the `.nat` source, use an authenticated Codex CLI:

```sh
cargo run -- build --project .
```

The generated runner rebuilds itself into `generated/` and updates
`inferred/assumptions.inat`. Check the result with `cargo test` and
`cargo test --manifest-path generated/Cargo.toml`. You can also run the generated
crate directly:

```sh
cargo run --manifest-path generated/Cargo.toml -- build --project .
```

The earlier hand-written runner remains available as a bootstrap if needed:

```sh
cargo run --features bootstrap --bin nat-bootstrap -- build --project .
```

More context: [Coding in natural language](https://adjohu.com/blog/coding-in-natural-language/)

## License

Copyright 2026 Adam Hutchinson.

Licensed under the [Apache License, Version 2.0](LICENSE).
