# Make Home Node initialization explicit and keychain-backed

Status: complete

Blocked by: 01

Make Home Node creation an explicit, safe lifecycle operation and automatically authenticate local User commands with a keychain-backed Home Node Owner Credential.

## Acceptance criteria

- [x] `node init` is the only command that creates Home Node state and fails when the selected path is already initialized.
- [x] All other commands return `node_not_initialized` for an empty or uninitialized path.
- [x] The default Home Node is `~/.stumble/nodes/home`, with environment and flag overrides.
- [x] `node init` and `node show` return the resolved path and canonical Node identity.
- [x] Initialization stores the Owner credential through an operating-system credential-store boundary rather than in the Home Node database.
- [x] Later local User commands retrieve the Owner credential automatically.
- [x] Executable tests use an isolated credential backend and never touch the developer's real keychain.

## Comments

- 2026-07-18: Implementation started after issue 01 completed and passed full workspace validation.
- 2026-07-18: Added explicit create-versus-open Home Node lifecycle APIs, deterministic default/environment/flag path resolution, macOS Keychain and Linux Secret Service credential backends, automatic local Owner authentication, resolved-path Node results, and isolated executable coverage. `cargo test -p stumble-cli` passes.
