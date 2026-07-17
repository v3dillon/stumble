# Deliver Feed Batches pull-first with optional proactive events

Every Agent Harness can retrieve the current stable Feed Batch through a canonical pull operation. A Home Node may additionally emit a Feed-ready Event through webhook or event-stream adapters, but proactive delivery is optional and carries the same batch contract so Feed semantics do not depend on harness capabilities.
