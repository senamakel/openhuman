# Node configuration

`get_node_kind_contract` is the source of truth for a node's fields, ports,
examples, and validation rules.  Read it before configuring any node; this
page records only conventions shared by several node kinds.

## Memory and deduplication

Give memory operations a stable source scope.  Scope identifies where a fact
comes from; an item ID is only a deduplication key and must not be used as the
collection scope.  Choose a deterministic deduplication key from data that is
available on every run.  Do not use a timestamp or generated UUID when the
intent is to suppress repeated work.

## Triggers

Make a trigger narrow enough that its input shape and authorization boundary
are clear.  A trigger begins a run; it does not grant an action permission.
Any side effect still needs the normal approval and policy path.

## Error handling

Set per-node error handling deliberately.  Continue only when later nodes can
produce a correct result without this node's output.  Prefer a visible failure
for required inputs and side effects; swallowing an error may make a completed
run misleading.

After configuring a node, inspect its port types and connect only compatible
outputs.  A graph proposal should state the purpose of each non-default error
policy so the user can review its consequence.
