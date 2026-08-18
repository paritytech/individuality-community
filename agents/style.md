# Style

- Use Google English spelling in all code, comments, documentation, and string literals
- Skip the comma before "and" if possible
- Avoid em dash (—) for parenthetical asides in comments and documentation
- Prefer `collect::<Vec<_>>()` over `let x: Vec<T> = iter.collect()`
- Prefer `for x in &collection` over `for x in collection.iter()`
- Functions use verb forms (`clean_up`), while types and variables use noun forms (`cleanup`)
- Avoid wildcard imports (`use foo::*`) and import items explicitly in the main code. Wildcard imports are permitted in tests.
- Avoid uncommon compound-adjective constructions (e.g. "ring-stale aliases"); spell them out instead
- When a condition guards against inconsistent state (not user error), log an error rather than silently skipping
- Omit trailing commas in macro invocations so that rustfmt keeps arguments on one line where possible. E.g. `assert_ok!(Pallet::call(origin, arg1, arg2))` not `assert_ok!(Pallet::call(origin, arg1, arg2,))`
- Use sentence case for headings. E.g. `## Reward amount` not `## Reward Amount`
- Avoid boolean parameters; match on a meaningful enum instead
- Doc comments describe the current state, not changes relative to a prior version
- Target three short sentences in a field, variant or trait-method doc, one message each: what it is, then what a caller cannot get right unaided. Fewer is better; rationale belongs in the module doc
- When editing an item, rewrite its doc comment rather than appending to it, so it does not grow with every change
- Comments state facts plainly. Follow [ASD-STE100](https://asd-ste100.org/) Simplified Technical English: one idea per sentence, active voice, present tense, the same term for the same thing, and the plain verb over a noun phrase. Avoid editorialising ("this is what keeps it out of a block", "the two can never drift apart"); say what holds and why a caller cannot infer it. Delete a comment that only restates the code
