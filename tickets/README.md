# tickets

One markdown file per ticket in `open/` or `closed/`, named
`NNNN-slug.md`, from [`TEMPLATE.md`](TEMPLATE.md). Epics, groups of
tickets with one goal, live in `../epics/{open,closed}/`, named
`ENNN-slug.md`.

- Ticket numbers come from a single counter; take the next free number.
- `depends:` lists tickets that must close first. Work tickets in
  dependency order within an epic.
- A ticket moves from `open/` to `closed/` (`git mv`) when its acceptance
  criteria are met. It gains a `## Close-out (YYYY-MM-DD)` section
  recording what landed and any deviation from the plan. An epic closes
  the same way when its last ticket does.
- How silkscreen works is described in `design/`, not in tickets: a ticket
  that changes how something works updates the design document too.
