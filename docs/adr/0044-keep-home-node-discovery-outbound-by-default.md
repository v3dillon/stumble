# Keep Home Node discovery outbound by default

Ordinary Home Nodes contact the Bootstrap Node and rotating Discovery Peers outbound by default but do not open or advertise an inbound discovery endpoint automatically. A User must explicitly opt into serving Announcement Streams, after which Stumble verifies the node's signed identity and public reachability before advertising it as a Discovery Peer; disabling peer gossip still leaves direct Pod URLs and configured Bootstrap access available.
