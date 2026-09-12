# Reading a dry run

A dry run evaluates the graph without committing the real side effects.  It is
useful for checking node wiring, expression results, branches, and the actions
the graph would request.  It is not evidence that external services accepted a
request, that credentials are valid, or that a real action completed.

## Report results precisely

Say which input was used, which branches executed, and which proposed actions
were observed.  Separate values that were evaluated from actions that would
have happened.  Never describe a dry run as sending a message, writing memory,
or modifying an integration.

## Limits

Use non-secret representative data.  Test alternate branches and absent
optional values when they affect a decision.  A dry run cannot prove behavior
behind a live provider, approval prompt, network failure, rate limit, or
permission boundary; name those remaining checks before asking to run the
workflow for real.
