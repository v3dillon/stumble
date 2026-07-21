# Support multiple Bootstrap Nodes from the start

Home Nodes model bootstrap configuration as a User-controlled list of replaceable endpoints even though the first release ships with only the sponsored default. Known Discovery Peers are cached and direct Pod URLs remain a fallback, so adding independent bootstrap operators later requires no protocol or storage migration and the initial sponsor is never encoded as a network-wide singleton.
