# Use SQLite as the Home Node store

The first release uses SQLite as the authoritative local database so Candidate acceptance, Pod Events, synchronization cursors, Discovery Task leases, and Feed Batch creation can be transactional and locally searchable. Existing JSON snapshots become an import, export, and one-time migration format, while PostgreSQL remains a future hosted-mode adapter.
