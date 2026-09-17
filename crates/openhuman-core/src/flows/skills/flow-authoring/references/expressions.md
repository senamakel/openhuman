# Expressions

Use a literal value unless a field must be derived at run time.  A value that
starts with `=` is evaluated as an expression; all other values are passed to
the node unchanged.

## Inputs and results

Build expressions from the values exposed by the node contract.  Start by
reading `get_node_kind_contract` for the node being configured, then use the
documented input and output names exactly.  Do not guess a path from a label:
a graph can validate while a guessed path evaluates to `null` at run time.

Use jq only for a transformation that cannot be represented by selecting a
field.  Keep filters small, preserve the value type expected by the destination
port, and account for absent optional fields with `?` or an explicit default.

## Files and outbound actions

An attachment must come from an output that is documented as a file or binary
artifact.  Do not turn arbitrary text into a path, and do not put a local path
in a remote-action configuration.  Inspect the producing node's output
contract before wiring it to an outbound action.

## Check before proposing

Before presenting a graph, use the dry-run tool on representative, non-secret
input.  A successful expression evaluation proves only the exercised shape of
data; it does not prove that optional fields will exist in every production
run.
