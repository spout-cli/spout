# Planning docs

Each **stage** — a coherent multi-commit design effort — has a planning doc
(written before coding) and a learning doc (written after). The numbering
intentionally has gaps where work didn't fit the stage shape.

| NN  | Topic                                   | Planning | Learning |
|-----|-----------------------------------------|:--------:|:--------:|
| 01  | MVP core (all PRD commands, registry)   | ✓        | ✓        |
| 02  | Completions, `spout ls` TUI             | ✓        | ✓        |
| 03  | Monorepo support (SPOUT_PROJECT + auto) | ✓        | ✓        |
| 04  | *(skipped — see below)*                 | —        | —        |
| 05  | UDP support                             | ✓        | ✓        |
| 06  | `spout prune`                           | ✓        | ✓        |
| 07  | `spout alloc` from `docker-compose.yml` | ✓        | ✓        |
| 08  | `--project` on `rm` and `get`           | ✓        | ✓        |
| 09  | Compose override + multi-`-f` support   | ✓        | ✓        |
| 10  | Surface recently-removed in not-found   | ✓        | —        |

## Why no `04`?

The commits between Stages 3 and 5 (GitHub Actions CI, `spout env`, the
live bound/free indicator, TUI project grouping, the `SPOUT_ICONS` env
var, `templates/CLAUDE.md`) shipped as standalone conventional commits
without a shared design thread. Some commit messages retrospectively
labelled them "Stage 4," but there was no single planning document they
descended from.

The stage concept is reserved for coherent design efforts that benefit
from up-front design and a retrospective. CI work and one-off features
don't — they land as conventional commits. CLAUDE.md §Process and
CODING_GUIDELINES.md §Documentation reflect this.

New stages continue the numbering (07, 08, …). The `04` slot stays
empty as an honest record that not every commit cluster is a stage.
