# Replace the legacy Hub with the Stumble Substrate

The signed Bootstrap, Index, Announcement Stream, and Discovery Peer contracts replace the centralized Hub registration and refresh model without compatibility routes or aliases. Legacy Hub APIs, daemons, domain types, synchronization paths, and tests are removed, and persisted Hub tables may be dropped because they contain only non-authoritative discovery caches that can be reacquired from the Stumble Substrate; direct Pod URLs and signed Pod Event synchronization remain intact.
