# nat

Natural language as source code. Conventional code as a build artifact.

What if humans stopped maintaining conventional source code?

`nat` is an experiment in treating human-written intent as the canonical source, using an LLM to compile it into conventional software.

```nat
if user is talking {
  wait until they finish, then {
    store what they said
    transcribe it
    summarize themes and store deduped in db
  }
}
```

Could compile to TypeScript today, Rust tomorrow, or something else entirely.

## Quick start

Install [Rust and Cargo](https://www.rust-lang.org/tools/install) and the [Codex CLI](https://learn.chatgpt.com/docs/codex/cli), then:

```sh
cargo install --git https://github.com/adjohu/nat --locked
codex login
nat init hello
cd hello
nat run Ada
```

`nat init` creates a starter greeting spec in `spec/app.nat`; `nat run Ada` builds and runs it. Edit that file to change the program, then run `nat run` again. `nat watch` rebuilds and checks it as you edit.

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
  nat build [--project DIR] [--proposal JSON]
  nat watch [--project DIR] [--no-test] [--proposal JSON]
  nat run [--project DIR] [ARGS...]
  nat test [--project DIR]
  nat status [--project DIR]
  nat check [--project DIR]
  nat promote ID [--project DIR]
```

## Writing

More context: [What if code wasn't the source?](https://adjohu.com/blog/what-if-code-wasnt-the-source)

## License

Copyright 2026 Adam Hutchinson.

Licensed under the [Apache License, Version 2.0](LICENSE).
