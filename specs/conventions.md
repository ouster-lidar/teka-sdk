# Conventions

- These conventions apply across the repo.
- Put new conventions here unless they need a full spec.
- Keep this file general; do not describe one app or directory here.
- Put shared subsystem rules in a scoped spec.
- Put exceptions in a local spec next to the affected files.
- Local specs override global conventions within their scope.

## Markdown

- Do not maintain file indexes when the directory structure is sufficient for discovery.
- Do not hard-wrap prose. Let the viewer soft-wrap.
- Keep each prose sentence on one line. Less diff churn.
- Prefer bulleted lists over standalone sentences for related points.
- Apply sentence-per-line rules to prose, not tables or code.
- Keep headings, lists, tables, code fences, and shell blocks structurally formatted.

## Ordering

- Order unordered collections alphabetically unless order carries meaning.

## Rust

- Forbid unsafe code in all repository Rust projects whenever possible, including examples, tests, build scripts, and standalone Rust files.
- Use `[lints.rust] unsafe_code = "forbid"` for Cargo packages so every target is covered, or `#![forbid(unsafe_code)]` at each crate root when package lints are unavailable.
- Prefer safe standard-library APIs or maintained safe dependency interfaces over local unsafe blocks and implementations.
- If unsafe code cannot reasonably be removed, document the constraint and narrow exception in a local spec; keep safe targets protected.
- Build all affected targets and test changed behavior before committing safety changes; dependency internals are outside this restriction.

## Commits

- Use LKML-style commit messages.
- Use a subsystem-focused subject: `subsystem: component: Brief summary`.
- Keep the subject short and wrap the body at 72 columns.
- Always include a body.
- Use bullet points in the body.
- Keep commits small, atomic, and focused on one problem.
- Check `git log` for subsystem patterns in the files being changed.
