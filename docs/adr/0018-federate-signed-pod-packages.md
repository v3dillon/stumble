# Federate signed, versioned Pod Packages

Each Pod may publish a signed, versioned Pod Package containing `CONTEXT.md`, `SKILL.md`, Source Rule suggestions, filters, and good and bad examples. Agent Harnesses treat remote Pod Skills as scoped untrusted instructions: they cannot override higher-priority rules, grant browser access, expose credentials, or authorize account mutations, while Connector Secrets and Browser Grants never enter the package.
