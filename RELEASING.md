# RELEASING

How to cut an OpenCAMStudio release. The *why* of the numbering scheme is in
`VERSIONING.md`; this file is the mechanical checklist.

## The mechanism, in one sentence

**Push a `vX.Y.Z` tag or publish a GitHub release for that tag.** The `Release`
workflow (`.github/workflows/release.yml`) builds native desktop packages on Linux,
macOS and Windows and attaches them, plus SHA-256 checksums, to this repository's
release. A tag push creates a release using `docs/release-notes/<tag>.md` when
available; publishing through the GitHub UI keeps the release's existing notes.
Rerunning the workflow replaces the assets on an existing release.

| Platform | Packages |
|---|---|
| Linux x86-64 | AppImage (glibc 2.35 or newer) |
| macOS Apple silicon | DMG containing the application bundle |
| Windows x86-64 | MSI installer and portable ZIP (EXE plus runtime DLLs) |

All packages enable `gui,occt`, including STEP/IGES support. OCCT 7.9.3 is built
from the pinned upstream commit in `.github/actions/setup-occt/action.yml` on each
native runner. Only STEP/IGES, meshing and their toolkit dependencies are enabled;
optional third-party OCCT integrations are disabled. End users need no SDK.
Packages are unsigned (macOS uses local ad-hoc signatures, without Developer ID).

OCCT shared libraries, translator resources, LGPL text and the OCCT exception
travel inside each package. Linux uses the system C++ runtime supplied by Ubuntu
22.04 / Debian 12 or newer; Windows bundles the Visual C++ runtime. The portable
Windows download is now a ZIP because
the EXE needs the adjacent DLLs. The MSI installs the same dependency tree.

The Linux job attaches `OpenCAMStudio-OCCT-7.9.3-source.tar.gz` and its checksum to
the release as corresponding OCCT source. The archive contains the unmodified
pinned upstream source, including its license files. Build options are recorded
in `.github/scripts/build-occt.py`.

## Native verification

Each platform runs the kernel fixture suite and application STEP/IGES regression
tests with `occt`, explicitly enabling the normally ignored native STEP tests.
Tests requiring local Tigerente files are excluded. Packaging then runs the
`cam-kernel` example `occt_smoke` beside
the packaged application. It imports original STEP and IGES fixtures while the
SDK, source and build directories are temporarily renamed and SDK loader paths
are cleared. This checks relocation and runtime dependencies without opening a
GUI. Windows repeats this check on the extracted portable ZIP and installed MSI;
macOS also validates and mounts the final DMG. Linux measures the glibc floor
across the binary and bundled shared libraries.

These checks qualify the checked-in fixtures, not every STEP/IGES exporter or
interactive graphics behavior. Packaging also starts the staged GUI executable in
`--help` mode with the OCCT SDK and loader-path environment hidden, checking that
the app binary and its bundled OCCT runtime start together. A release still needs
manual desktop testing.

## First run in a fork

1. Open **Actions** in the fork and enable workflows if GitHub has disabled them.
2. Select **Release → Run workflow** and choose the branch containing your changes.
3. After all three builds pass, download `linux`, `macos` and `windows` from the
   run's **Artifacts** section. Each contains its native package(s) and checksums.

This manual run is always a rehearsal, including when run against a tag. It names
artifacts `vX.Y.Z-dev` and never creates or updates a GitHub release. No signing
secrets or personal access tokens are needed; publishing uses the workflow's
`GITHUB_TOKEN` with `contents: write` in the publishing job only.

## Checklist

Run from a clean `main` that has the work you intend to ship.

1. **Write the release notes.** Create `docs/release-notes/vX.Y.Z.md`. The workflow
   reads this file verbatim as the GitHub release body; if it is missing the release
   still publishes, but with a generated placeholder body.
2. **Bump the workspace version.** Set `version = "X.Y.Z"` in the root `Cargo.toml`
   `[workspace.package]`. **This is the load-bearing step.** The tag names the
   *artifacts*, but `CARGO_PKG_VERSION` names the *binary* (About box, `--version`);
   the workflow rejects a tag that does not match the manifest. See
   `VERSIONING.md` "The manifest / tag / About-box relationship".
3. **Commit** the notes + bump together, directly to `main`:
   `Release vX.Y.Z` (per project convention, no branch ceremony).
4. **Tag and push.** `git tag vX.Y.Z && git push && git push --tags`. The tag push
   is the trigger — do not tag a commit you have not pushed.
5. **Watch the release job go green.** All three platform builds plus the
   publishing job must succeed. For a transient failure, rerun the failed jobs.
   If a code fix is needed, commit it and use a new release version and tag.
6. **Verify provenance on a real artifact.** Download one build and confirm the About
   box / `--version` reports a **clean** `X.Y.Z` — no `-dirty`, no `-gHASH`, no
   version/tag contradiction. That single check proves the bump in step 2 landed.

Alternatively, after committing the version bump, use **Releases → Draft a new
release**, choose a matching new `vX.Y.Z` tag targeting that commit, and publish.
The workflow attaches packages once the builds finish. The tagged commit must
contain this workflow. Existing upstream tags do not rebuild automatically when
a repository is forked.

## Version-number choice

Feature batch → bump MINOR (`0.1.0 → 0.2.0`). Fix-only release between batches →
bump PATCH (`0.2.0 → 0.2.1`). `1.0.0` is a deliberate statement, not a drift. Full
rationale in `VERSIONING.md`.
