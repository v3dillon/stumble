# Discover peers through signed expiring advertisements

An opted-in announcement-serving node publishes a signed, renewable Discovery Peer Advertisement containing its node identity, public endpoint, protocol version, serving capability, and expiry. Bootstrap Nodes verify reachability and return small randomized rather than ranked peer samples, and Discovery Peers may exchange bounded samples so automatic peering can continue after bootstrap loss without creating a canonical peer directory.
