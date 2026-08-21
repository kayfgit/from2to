# Packaging

Four channels, three shapes. Only crates.io ships source; everything else points
at the binaries attached to a GitHub release, which is why the release has to
exist before any of them can be updated.

| Channel | Lives in | Updated by | Review? |
|---|---|---|---|
| crates.io | — | `cargo publish`, by hand | none |
| GitHub Releases | — | pushing a `v*` tag | none |
| `.deb` / `.rpm` | [`../nfpm.yaml`](../nfpm.yaml) | the release workflow | none |
| Scoop | [`../bucket/from2to.json`](../bucket/from2to.json) | the release workflow, committed back to `main` | none |
| AUR | [`aur/PKGBUILD.in`](aur/PKGBUILD.in) | by hand, see below | none |
| winget | not stored here | `wingetcreate`, by hand | **yes, moderated** |

## Scoop

The manifest lives in `bucket/` in this repo, so there is no separate bucket
repository to maintain — Scoop reads manifests out of a `bucket/` directory in
any git repo. Users add it once:

```powershell
scoop bucket add from2to https://github.com/kayfgit/from2to
scoop install from2to
```

The `scoop` job in the release workflow rewrites `version`, `url`, `hash` and
`extract_dir`, then commits the result to `main`. Nothing to do by hand.

The manifest also carries `checkver` and `autoupdate` blocks. Those are unused
while the workflow does the updating, but they are what a third party (or the
official `extras` bucket, if this ever lands there) would rely on, so they are
kept correct.

## AUR

`aur/PKGBUILD.in` is the template; the release workflow substitutes the version
and the two checksums and attaches the result to the release. Pushing it is
manual — see the release runbook. The AUR also requires a `.SRCINFO` file
generated from the `PKGBUILD`, which needs `makepkg` and therefore an Arch
environment:

```bash
docker run --rm -v "$PWD:/pkg" -w /pkg archlinux:base-devel bash -c '
  useradd -m builder && chown -R builder /pkg &&
  su builder -c "makepkg --printsrcinfo > .SRCINFO"
'
```

## winget

**Not like the others.** Scoop and the AUR are self-service: you push, and it is
live. winget has one central repository, `microsoft/winget-pkgs`, and getting in
means opening a pull request that passes automated validation and then a human
review. Expect hours to days for the first submission, and no guarantee.

The package identifier is `kayfgit.from2to`. Submissions are made with
Microsoft's own tool rather than by hand-writing the three YAML manifests:

```powershell
winget install Microsoft.WingetCreate

# First submission — interactive; point it at the Windows zip from a release.
wingetcreate new https://github.com/kayfgit/from2to/releases/download/v0.3.0/from2to-0.3.0-x86_64-pc-windows-msvc.zip

# Every release after that.
wingetcreate update kayfgit.from2to `
  --version 0.3.1 `
  --urls https://github.com/kayfgit/from2to/releases/download/v0.3.1/from2to-0.3.1-x86_64-pc-windows-msvc.zip `
  --submit --token <GITHUB_PAT>
```

Two things the tool will ask about that are easy to get wrong. The archive is a
zip rather than an installer, so the manifest needs `InstallerType: zip` with a
`NestedInstallerType: portable` and a `RelativeFilePath` pointing at
`from2to-<version>-x86_64-pc-windows-msvc\from2to.exe`. And `--submit` needs a
GitHub personal access token with `public_repo` scope, because it forks
`winget-pkgs` and opens the PR as you.

Left manual on purpose: a moderated PR is not something to fire off from CI
without reading what it says.
