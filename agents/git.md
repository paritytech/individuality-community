# Git

- Branch naming: `{type}/{issue#}-{initials}-{short-description}`
  - New feature or enhancement: `feat/123-ab-pallet-people`
  - Bug fix: `fix/456-ab-iteration-order`
  - Issue number is optional: `feat/ab-build`
- Do not commit anything; only write messages or stage changes
- Do not run any destructive Git commands (`reset --hard`, `push --force` etc.)
- Never add `Co-Authored-By` to commits
- When resolving merge conflicts in enums with explicit integer discriminants, never reuse a value. Assign new discriminants by final position. After resolving, verify the sequence is strictly increasing with no gaps or duplicates.
