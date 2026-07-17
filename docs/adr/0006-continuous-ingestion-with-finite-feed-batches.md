# Combine continuous ingestion with finite Feed Batches

Source Connectors and Pod synchronization run continuously, but the User consumes a finite, stable Feed Batch with an explicit Caught Up state. Stumble does not auto-load an infinite stream; the User may deliberately request another batch, and newly arriving items wait for a subsequent refresh rather than reshuffling the active batch.
