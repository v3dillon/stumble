# Issue tracker: Local Markdown

Issues and PRDs for this repo live as Markdown files in `.scratch/`.

## Conventions

- One feature per directory: `.scratch/<feature-slug>/`
- The PRD is `.scratch/<feature-slug>/PRD.md`
- Implementation issues are `.scratch/<feature-slug>/issues/<NN>-<slug>.md`
- Triage state is recorded as a `Status:` line near the top
- Blocking dependencies are recorded as `Blocked by: NN, NN`
- Comments are appended under a `## Comments` heading

When a skill says “publish to the issue tracker,” create the appropriate file under `.scratch/<feature-slug>/`.

When a skill says “fetch the relevant ticket,” read the referenced file or issue number directly.
