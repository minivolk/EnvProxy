# Security Policy

EnvProxy is a security-sensitive project that handles secrets and environment variables. We take security seriously.

## Reporting a Vulnerability

If you discover a security vulnerability in EnvProxy, please report it responsibly.

**Do NOT open a public GitHub issue for security vulnerabilities.**

Instead, please email: **minivolk02@gmail.com** (or use GitHub's private vulnerability reporting feature on the repository's Security tab).

Include:

- A description of the vulnerability
- Steps to reproduce
- Impact assessment
- Any suggested fix

We will acknowledge receipt within 48 hours and provide a timeline for a fix.

## Scope

The following are in scope for security reports:

- Secrets leaking to `/proc/PID/environ`, logs, or other observable channels
- Unauthorized access to the agent Unix socket
- Bypass of the `LD_PRELOAD` interception mechanism
- Vulnerabilities in the wire protocol between `.so` and agent
- TLS/certificate handling issues in the Kubernetes webhook
- RBAC escalation in the Kubernetes integration
- Supply chain issues in dependencies

## Security Design Principles

- Secrets are **never written to the process environment block** — they exist only in agent memory and application heap
- The `.so` library communicates with the agent over a **Unix socket** (no network exposure)
- `ENVPROXY_` prefixed variables are **never intercepted** to prevent recursion and config leaks
- The Kubernetes webhook uses **self-signed TLS** or **cert-manager** for API server communication
- The agent runs with **least-privilege RBAC** (read-only access to Secrets)

## Supported Versions

| Version | Supported |
| ------- | --------- |
| 0.1.x   | Yes       |
