# Require structured Agent Harness Candidate Submissions

External discovery enters Stumble as an authenticated, idempotent Candidate Submission containing a canonical source URL, known source metadata, permitted excerpt and summary, content type and tags, discovery provenance, and an explicit typed target. A `User` target represents a direct interactive User reference and carries the private-learning controls and Interest Seed metadata that apply to that flow. A `PodPlacements` target contains one or more proposed Pod Placements with reasons and confidence, plus task and Pod Package versions when applicable. Stumble independently canonicalizes, deduplicates, authorizes, and applies Curation Policies rather than treating harness confidence as authority.

This amendment narrows the original placement requirement: proposed placements and their task and Pod Package metadata apply only to `PodPlacements` targets, not to `User` targets.
