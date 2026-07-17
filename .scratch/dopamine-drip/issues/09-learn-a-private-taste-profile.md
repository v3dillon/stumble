# Learn a private Taste Profile

Status: complete

Blocked by: 08

Improve subsequent Feed Batches using a private Taste Profile that combines
explicit preferences with explainable learned weights while keeping the User in
control.

## Acceptance

- [x] Users can inspect and edit explicit interests, blocks, and recurrence preferences.
- [x] Feedback Signals and Add to Pod actions update learned weights locally.
- [x] Explicit settings override learned inference when they conflict.
- [x] A single weak signal cannot create a permanent preference.
- [x] Users can inspect evidence for learned weights and reset some or all of them.
- [x] Feed explanations identify relevant explicit and learned signals without exposing sensitive raw history unnecessarily.
- [x] Taste Profiles and their evidence are absent from every federation and public export surface.

## Comments

- 2026-07-17: Implementation started with strict red-green TDD at the temporary-SQLite `AgentTools` and public export seams.
- 2026-07-17: Private explicit preferences, corroborated aggregate learning, reset controls, ranking explanations, adapters, persistence, and export privacy completed.
- 2026-07-17: Standards and Spec review findings were corrected and both final review axes reported no remaining material findings.
