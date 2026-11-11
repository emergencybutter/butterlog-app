# Instructions

* Keep a log of important changes in a changelog.
* Commit change to git.
* No need to ask for confirmation to run `cargo check` or `cargo test`.

## Release

To release (scripts in `..\tools`):
* Build and run tests.
* Run `.\build-signed.ps1 -Bump patch` (or `minor`, with `-Notes "..."`). It bumps the version in
  package.json, src-tauri\tauri.conf.json and Cargo.toml, builds and signs the Windows installer,
  updates latest.json, and copies the installer to the website with its link, version and size.
* Commit the release ("Release x.y.z") here and in butterlog-website, and push this repo: the push
  builds the macOS and Linux installers in GitHub Actions.
* Run `.\fetch-ci-builds.ps1`. It waits for that build, copies the .dmg, .deb, .rpm and AppImage
  to the website, and updates their links and sizes. Commit the website again.
* Deploy the website with `.\deploy-website.ps1`; the apps then update themselves from latest.json.

## OS

Attempt powershell commands before unix commands.