# Federate signed append-only Pod Events

Origin Nodes publish signed Pod Events for package versions, Accepted Placements, metadata updates, and Placement Tombstones, and Home Nodes verify them before updating local projections. Incremental sync resumes from the last known event; snapshots may improve efficiency, but private Feed history, feedback, saves, and Subscriptions never federate.
