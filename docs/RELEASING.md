# Releasing

The source lives on GitLab. GitLab push-mirrors every branch and tag to
[github.com/mycelium-app/mycelium](https://github.com/mycelium-app/mycelium), and GitHub Actions builds
and publishes each `vX.Y.Z` tag. Never push to GitHub directly: the next mirror run overwrites it.

## Cut a release

1. Add a `## vX.Y.Z` section to the top of `CHANGELOG.md`. It becomes the release notes.
2. Commit it as `fix: [ai] vX.Y.Z: <summary>` (or `feat:`) and tag the commit `vX.Y.Z`.
3. Push `main` and the tag to GitLab with `git push origin main vX.Y.Z`. The mirror reaches GitHub
   within five minutes; starting the mirror from GitLab's API makes it immediate.

4. The tag starts the `Release` workflow on GitHub. It publishes nothing until every job succeeds.
5. Check the published release, not your build:

   ```sh
   ci/verify-release.sh vX.Y.Z
   docker pull ghcr.io/mycelium-app/mycelium:X.Y.Z
   ```

## What the workflow publishes

| Job | Output |
|---|---|
| `binary` | `mycelium-linux-x86_64`, a static musl build of the node, control plane and CLI |
| `desktop-web` | `mycelium-desktop-linux-x86_64.deb` and `.tar.gz`, `mycelium-web.tar.gz` |
| `android`, `sign-android` | `mycelium-android.apk`, signed with the key in the `release` environment. The signing job fails when the APK's versionCode or versionName does not match the tag or does not exceed the previous tag. |
| `image` | `ghcr.io/mycelium-app/mycelium:X.Y.Z`, and `:latest` when the tag is the newest release. It reuses the `binary` and web builds. |
| `flake` | `mycelium-flake.tar.gz` for NixOS and home-manager |
| `release` | A GitHub release with every file, its `.sha256`, `install.sh` and a `latest` file holding the tag. The release stays a draft until every upload succeeds. |

Pushes to `main` that change a lockfile run the build jobs without publishing. They keep the GitHub
Actions caches warm, since a tag run can only restore caches made on `main`.

## Signing key

The Android key is the `ANDROID_KEYSTORE_BASE64` and `ANDROID_KEYSTORE_PASSWORD` secrets of the
`release` environment, which only `v*` tags can use. Its SHA-256 certificate fingerprint starts
`AB:C5:7E:2E`. A different key cannot update an installed app, so keep the backup safe.
