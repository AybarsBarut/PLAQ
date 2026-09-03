# Security Policy

PLAQ is a research prototype and should not be treated as a hardened media
sandbox. Supported releases receive parser and memory-safety fixes on the latest
minor version.

Please report suspected vulnerabilities privately through GitHub's security
advisory feature. Include a minimal synthetic reproducer where possible; do not
attach sensitive or copyrighted audio. Avoid filing a public issue until a fix
is available.

The decoder bounds metadata, block payloads, total samples, UDP block assembly,
and TCP stream sizes. Rust `unsafe` code is forbidden in the workspace, but
resource-exhaustion bugs may still exist and are in scope for reports.

