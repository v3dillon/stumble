# Automatically form limited Discovery Peer relationships

Home Nodes may automatically maintain a small rotating outbound set of Discovery Peers learned through bootstrap and peer exchange, because origin signatures allow public Pod Announcements to be verified without granting the delivering peer broader trust. Discovery Peers can exchange only bounded public announcement data and may be evicted automatically for invalid data, flooding, or repeated failures; serving other peers remains opt-in, while more powerful relationships continue to require explicit User approval.
