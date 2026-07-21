# Rank passive network discovery locally

Bootstrap and Index Nodes expose a topic-neutral, cursor-paginated Announcement Stream for passive discovery, and Home Nodes match the synchronized public metadata against private User evidence locally. Explicit User-authored searches may query an Index Node, but background discovery never sends Taste Profile data or interest-derived queries; this accepts greater catalog synchronization cost in the first release to prevent discovery infrastructure from becoming a centralized behavioral profile.
