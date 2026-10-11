# Security policy

## Supported versions

Only the latest release and the default branch receive security fixes.

## Reporting a vulnerability

Please do not open a public issue for security problems. Use the repository's private vulnerability reporting ("Security" tab, "Report a vulnerability") to send a report. Include:

- a description of the problem and its impact,
- steps or a proof of concept to reproduce it,
- the version or commit you tested.

You can expect an acknowledgement within a week. Fixes are released as soon as practical and credited to the reporter unless you prefer otherwise.

## Scope

The server is designed for same-origin use and for trusted or private networks. Run it behind a reverse proxy with TLS if you expose it to the internet, and do not load content packs from sources you do not trust.
