# xdg-desktop-portal-cosmic

XDG Desktop Portal backend for the COSMIC desktop environment (Playtron fork).

Implements the `Access`, `FileChooser`, `Screenshot`, `Settings` and `ScreenCast`
portal interfaces. This is the process that draws the interactive screenshot
picker and the screencast source picker — `cosmic-screenshot` is only the D-Bus
client that asks for them.

## Build

```sh
go-task build            # release build (implies --features wgpu)
go-task build:debug      # debug build
go-task test             # cargo test --all-features
go-task lint             # clippy, warnings as errors
go-task fmt:check        # rustfmt check
```

`wgpu` is not a default cargo feature, but the dimmed backdrop behind the
screenshot selection is gated on it, so every Taskfile build turns it on. A plain
`cargo build` will compile and run without that backdrop.

## Run a local build

The portal is a D-Bus activated backend, so a local build has to take over the
bus name from the installed copy. It exits if it ever loses that name, so the
installed process must go first — `go-task run` does that for you:

```sh
go-task run                  # or: go-task run LOG_LEVEL=debug
cosmic-screenshot            # in another shell, to trigger the UI
```

Stop it with Ctrl-C and D-Bus will re-activate the packaged portal on next use.

## Packaging

```sh
go-task dist:archive    # tarball of the install tree
go-task dist:rpm        # RPM for the host arch
go-task dist            # both
```

Cross-arch packages go through the builder image:

```sh
go-task docker:run TARGET=dist:all ARCH=x86_64  TARGET_ARCH=x86_64-unknown-linux-gnu
go-task docker:run TARGET=dist:all ARCH=aarch64 TARGET_ARCH=aarch64-unknown-linux-gnu
```

The RPM carries `Epoch: 1` so it outranks Fedora's `xdg-desktop-portal-cosmic`,
which ships a higher upstream version number than this fork's.

## Release

Releases are cut by semantic-release from `master` via the `🎉 Release` GitHub
Actions workflow (manual dispatch). It derives the version from commit messages,
rewrites `Cargo.toml` and the RPM spec, builds both arches in the builder image,
attaches the RPMs to a GitHub release, and uploads them to the Playtron package
repository.
