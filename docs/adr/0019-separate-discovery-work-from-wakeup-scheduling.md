# Separate Discovery Tasks from wake-up scheduling

Stumble owns due-work calculation, Discovery Task state, leases, deduplication, and completion history, while Scheduler Adapters decide when workers wake. Agent Harness scheduling such as ChatGPT Scheduled or OpenClaw cron may drive execution, and Stumble also ships a local launchd, cron, or equivalent fallback for environments whose harness has no scheduler.
