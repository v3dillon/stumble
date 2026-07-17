# Do not require Home Nodes to be publicly reachable

A private Home Node may operate using outbound access only for synchronization, discovery, and Agent Harness tools. Public Origin Nodes use stable HTTPS endpoints in the first release; a later Relay Node role may cache and serve signed Pod Events for unreachable origins without gaining authority to forge them.
