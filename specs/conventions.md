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

## Commits

- Use LKML-style commit messages.
- Use a subsystem-focused subject: `subsystem: component: Brief summary`.
- Keep the subject short and wrap the body at 72 columns.
- Always include a body.
- Use bullet points in the body.
- Keep commits small, atomic, and focused on one problem.
- Check `git log` for subsystem patterns in the files being changed.
