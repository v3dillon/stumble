# Use a hybrid decentralized discovery substrate

Every Stumble node has a signed identity and may exchange compact public Pod Announcements with trusted peers, while direct Pod URLs remain canonical and full content synchronizes only after Subscription. Optional Index Nodes aggregate announcements for efficient search but are replaceable and non-authoritative; the first release uses direct addressing and indexes while leaving room for bounded gossip without requiring blockchain, global consensus, or a DHT.
