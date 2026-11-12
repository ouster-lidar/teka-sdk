# Public documentation

Voice and page shape for pages a reader sees.
Specs and `AGENTS.md` stay on [conventions.md](conventions.md).

The model is the [StereoLabs docs](https://docs.stereolabs.com/docs/products/embedded/zed-box-mini): direct, concrete, and short.
Leave out product-adjective stacks, closings such as "Enjoy your device", and filler such as "leverage the latest improvements".

## Applies to

- [README.md](../README.md) at the repository root.
- Pages under [docs/](../docs/).
- App READMEs under [apps/examples/](../apps/examples/).
- Reader-facing READMEs such as [crates/teka/README.md](../crates/teka/README.md) and [crates/teka-cli/README.md](../crates/teka-cli/README.md).

## Voice

- Address the reader as "you".
- Use "we" only for what the product does.
- Do not write "the user", "the system will", or "it is recommended that".
- Contractions are fine.
- One claim per sentence. A second clause is only the consequence.
- Open with what the page is for, and name the concrete command, port, hostname, or button.
- State the rule, then the consequence. State the default and the range together.
- Use counts, sizes, part names, command names, and IDs. Expand an acronym on first use.
- Do not write leverage, seamless, robust, powerful, cutting-edge, "simply", or "it is important to note".
- Name a failure in plain words. Do not soften a limit into a reassurance.

## Page shape

- One idea per section. If it needs more than a few sentences, add a heading.
- Headings name the thing, the action, or the symptom. No slogans, and no title case.
- A paragraph is one to four sentences on one source line so the viewer soft-wraps it.
- Do not put each sentence of a public page on its own line, and do not hard-wrap a sentence.
- Parallel facts are a bullet list. Comparable numbers are a table. A procedure is numbered steps.
- One command per fence. The line before says what it does. The line after says what success looks like.
- Put the caveat on the step where the mistake happens.
- A note is a behavior the reader will misread. A warning is data loss, downtime, an unsupported combination, or a last-resort action.
- Write alerts as GitLab admonitions (`> [!note]`, `> [!warning]`).
- An overview orients and links. It does not contain the procedure.
- A quickstart is the short path. It is not a tour.
- Do not repeat a procedure another page owns. Repeat only the caveat that prevents a mistake here, then link.
- Close a task with the result and one next action. Do not recap the steps.
- Troubleshooting titles are the symptom. Support is the last section.

Pick one shape.

- Object page: what it is, what it is for, fact lists, a warning beside the case that does not work alone, a link to deeper reference.
- Concept page: one-sentence summary, the constraint then the feature, short sections, a warning before the dangerous case.
- Task page: outcome in the title or first sentence, numbered steps, the warning on the dangerous step, the result plus one next action.
- Troubleshooting page: one section per symptom, support last.
