# Linux Packaging and Installation Strategy

**Status:** Packaging decision baseline for Linux x86_64. This document defines formats and release behavior; it does not claim that a distributable application binary exists yet. The current repository contains subsystem libraries and build infrastructure. Artifact production becomes executable only after the application binary and release pipeline are implemented.

## 1. Release target

- Initial supported distribution target: Linux x86_64.
- Primary tested environment: Ubuntu 24.04.
- Debian-compatible distributions are the first packaging family.
- Windows and macOS packaging remain out of scope until their platform adapters and CI qualification are implemented.

## 2. Selected artifact formats

### Primary: Debian package

Produce one architecture-specific `.deb` package:

```text
devil-eye-os-terminal_<version>_amd64.deb
```

Rationale:
- Native dependency metadata and package-manager integration.
- Predictable installation and removal through dpkg/apt.
- Appropriate for the initial Ubuntu 24.04 development/release target.
- System integration files can be installed in standard locations without a privileged application daemon.

The package must declare its runtime shared-library dependencies accurately. Do not guess or silently omit GUI toolkit, graphics, or system-library dependencies; derive them from the final linked executable and validate on the minimum supported distribution.

### Secondary: portable Linux tarball

Produce a versioned x86_64 archive:

```text
devil-eye-os-terminal_<version>_linux-x86_64.tar.zst
```

The archive is intended for manual installation on compatible Linux distributions. It contains the application binary, required bundled non-system assets, licenses, release notes, and a machine-readable manifest of external runtime dependencies.

The tarball must not modify system directories or require root privileges. A user-local installer may place versioned files under `~/.local/opt/devil-eye-os-terminal/<version>/` and a launcher under `~/.local/bin/`. Uninstall removes only files owned by that installation and does not delete user configuration or session data.

### Deferred formats

- **AppImage:** defer until the final GUI/graphics stack and its runtime bundling requirements are known. Do not ship an image that relies on unverified host libraries.
- **Flatpak/Snap:** defer until sandbox permissions, portal integration, PTY/process behavior, and update policy are explicitly designed and tested.
- **RPM:** defer until a Fedora/RHEL-family CI and dependency qualification target is established.

These are deferred, not rejected permanently.

## 3. Package identity and versioning

- Distribution package name: `devil-eye-os-terminal`.
- Debian architecture: `amd64`.
- Version source: one canonical release version shared by Cargo package metadata, application version output, package metadata, archive names, and release notes.
- Debian package version: `<upstream-version>-<package-revision>`; use package revision only for packaging-only rebuilds of the same upstream source.
- Release versions are immutable. Rebuilding the same version with different source or build inputs requires a new version/revision and new checksums.
- The final executable name is `devil-eye-terminal`; packaging must not rename it differently between formats.

## 4. Filesystem layout

### Debian package

| Path | Purpose |
| --- | --- |
| `/usr/bin/devil-eye-terminal` | Main executable |
| `/usr/share/applications/devil-eye-terminal.desktop` | Desktop launcher, once GUI integration exists |
| `/usr/share/icons/hicolor/` | Application icons, once supplied |
| `/usr/share/doc/devil-eye-os-terminal/` | Copyright, changelog, release notes |
| `/usr/share/licenses/devil-eye-os-terminal/` | Third-party license notices, if required by distribution policy |

Do not install user configuration, session state, caches, logs, or mutable data under `/usr`.

### User data

Follow the XDG Base Directory Specification:

- Configuration: `$XDG_CONFIG_HOME/devil-eye-os-terminal/` (default `~/.config/devil-eye-os-terminal/`).
- Persistent state: `$XDG_STATE_HOME/devil-eye-os-terminal/` (default `~/.local/state/devil-eye-os-terminal/`).
- Cache: `$XDG_CACHE_HOME/devil-eye-os-terminal/` (default `~/.cache/devil-eye-os-terminal/`).

Package removal must preserve user configuration and state. A separate, explicit user-facing reset operation may remove them; uninstall must not silently do so.

## 5. Installation and removal behavior

### Debian package

- Installation and removal are performed by dpkg/apt.
- No `postinst`, `prerm`, or `postrm` script may start a daemon, access the network, create privileged users, or alter user data.
- Package scripts, if eventually required for desktop database/icon cache refresh, must be minimal, idempotent, offline, and follow Debian policy.
- No setuid/setgid bits, file capabilities, privileged helper, or root-owned background service are permitted in the initial package.
- Package upgrades replace application-owned files only; configuration migration is handled by the application configuration subsystem.

### Tarball

- Installation is user-local by default and does not invoke sudo.
- The installer verifies archive paths before extraction and rejects absolute paths, parent traversal, and unexpected file types.
- The installer records the exact versioned installation root and only creates the user-local launcher/symlink it owns.
- Uninstall removes that versioned root and its owned launcher; it preserves configuration, state, logs, and other versions.
- Installation and uninstall operations are idempotent and report partial failures without deleting unrelated files.

## 6. Reproducible build requirements

Every release build must record and control:

- Exact source commit and clean/dirty working-tree state.
- Pinned Rust toolchain from `rust-toolchain.toml`.
- CMake minimum version and exact C compiler identity/version used for the release.
- Locked Rust dependency graph; release builds must use `--locked`.
- Build profile, target triple, linker identity, and relevant environment.
- `SOURCE_DATE_EPOCH` derived from the release source commit timestamp.
- Stable archive entry ordering, normalized timestamps, ownership, and permissions.
- No embedded absolute build paths or user-specific environment values.
- Separate debug symbols from stripped release binaries where supported.

The same source, toolchain, dependency lock, and declared environment should produce byte-identical artifacts. Any known non-deterministic inputs must be documented and removed or normalized before claiming reproducibility.

## 7. Artifact integrity and release metadata

Each release must publish:

- `.deb` and tarball artifacts.
- SHA-256 checksums for every artifact.
- SPDX SBOM for the shipped application and bundled components.
- Source commit, target triple, toolchain/build environment manifest.
- Release notes and known limitations.
- Signature and provenance attestation, implemented by DE-023-02 (#78).

A checksum proves byte integrity relative to the published checksum file; authenticity requires a verified signature or provenance chain. Do not describe unsigned checksums as signed artifacts.

## 8. Clean install/uninstall acceptance tests

The packaging pipeline must test in clean disposable Linux environments:

1. Build each artifact from a clean checkout using the pinned toolchain and locked dependencies.
2. Inspect archive/package contents and reject unexpected files, absolute paths, traversal entries, privileged bits, or embedded secrets.
3. Install the `.deb` in a clean Ubuntu 24.04 image and verify declared dependencies resolve.
4. Launch the application binary and run a non-interactive version/diagnostic smoke test.
5. Upgrade from the previous supported package revision and verify user configuration is preserved.
6. Remove the package and verify only package-owned files are removed.
7. Extract and install the tarball as an unprivileged user with no writes outside the selected user-local prefix.
8. Uninstall the tarball and verify the owned launcher and version directory are removed while configuration/state and unrelated files remain.
9. Verify SHA-256, SBOM, signature, and provenance metadata against the exact artifact bytes.
10. Repeat the build and compare artifact hashes to establish reproducibility.

Tests must run in disposable containers/VMs; never install or remove release packages on the CI runner host itself.

## 9. Release pipeline boundaries

- DE-023-01 defines package formats, identity, paths, installation/removal semantics, and reproducibility requirements.
- DE-023-02 (#78) implements artifact production, checksums, SBOM, signing, and provenance.
- DE-024-01/#81 defines release qualification and sign-off.
- The application binary and desktop integration must exist before the first installable desktop package is considered complete.

No packaging script may silently download dependencies at install time. All build inputs are resolved and verified before artifact creation.

## 10. Current limitations

At this stage, the repository does not yet provide a production-ready desktop executable, final GUI toolkit, complete desktop assets, or release artifacts. This strategy intentionally avoids pretending that packaging is executable before those prerequisites exist. The first package qualification must revisit runtime dependencies after the GUI/rendering stack is selected.
