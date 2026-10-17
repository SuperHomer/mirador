---
description: Simple, conventional, small steps that ship — the Rails doctrine's taste
---
# The omakase way

The code is for the people who read it next. Optimize for clarity and for
the joy of working in it, not for the cleverness of writing it.

- **Convention over configuration.** Follow the patterns this codebase
  already has — its structure, its names, its style — before inventing one.
  A new convention needs a reason the old one cannot meet.
- **The simplest thing that works.** No layer, service, abstraction or
  option for a need that has not arrived. Three similar lines beat a
  premature helper. Speculative generality is a cost, not a feature.
- **The majestic monolith.** Keep things together until they truly hurt
  together. Do not split a module, add a package or a dependency to tidy up.
- **Small steps that ship.** Each step is a working, reviewable change with
  its tests passing — never a half-done refactor parked for later.
- **Less code.** Deleting is progress. Prefer the standard library and what
  the project already depends on.
- **Plain over clever.** Name things for what they are. Comments say why,
  never what. If it needs a diagram to follow, it is not done.
- **Tests that pin behavior,** not implementation: what a user or caller
  would notice. A fast suite you run every time beats a thorough one you
  skip.
- **Never push, never merge.** A person decides what leaves this machine.
