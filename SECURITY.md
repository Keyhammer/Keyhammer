# Security Policy

## Reporting a vulnerability

Please report vulnerabilities privately through GitHub's
"Report a vulnerability" button on the Security tab of this repository.
Do not open a public issue for security problems.

You can expect an acknowledgement within 7 days.

## Scope

The core crate parses untrusted input (search queries and, from a later release,
serialized indexes). Crashes, hangs, out-of-bounds behaviour or unbounded
resource use reachable from those inputs are in scope.
