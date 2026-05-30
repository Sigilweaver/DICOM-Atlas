# Security policy

## Supported versions

Only the latest minor release of DICOM-Atlas (`dicom-map`,
`dmap-compiler`, `dicom-map-ffi`, and the `dicom-map` PyPI
package) is supported with security fixes.

| Version | Supported |
| ------- | --------- |
| 0.2.x   | Yes       |
| < 0.2   | No        |

## Reporting a vulnerability

Please report security vulnerabilities privately via
[GitHub Security Advisories](https://github.com/Sigilweaver/DICOM-Atlas/security/advisories/new).

Do **not** open a public issue for security reports. We will
acknowledge within 7 days and aim to publish a fix or mitigation
within 30 days for confirmed issues.

## Scope

In scope:

- Memory-safety bugs in the dictionary reader, FFI surface, or
  Python bindings.
- Arbitrary file read/write triggered by a malformed `.dmap`
  archive.
- Crashes in the rkyv archive validator that can be triggered by a
  third-party-supplied dictionary.
- Supply-chain integrity issues affecting published crates,
  wheels, or release artifacts.

Out of scope:

- Incorrect or missing tag entries: please open a normal issue or
  PR, these are data corrections not security issues.
- Denial-of-service from intentionally oversized inputs that the
  parser correctly rejects.
- Vulnerabilities in third-party crates: please report those
  upstream.

## Disclosure

Coordinated disclosure is preferred. Once a fix is released, the
advisory will be made public and credited to the reporter unless
they request anonymity.
