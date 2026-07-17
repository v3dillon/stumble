# Authorize Agent Harnesses with scoped grants

Each Agent Harness receives a revocable Harness Grant scoped to explicit capabilities such as reading Feed Batches, recording feedback, claiming Discovery Tasks, submitting Candidates, curating specified Pods, managing Source Rules, or administering the node. Tokens and grants remain local, every write records harness identity, and unattended workers should receive narrower authority than interactive User-facing harnesses.
