# Instructions

* Keep a log of important changes in a changelog.
* Commit change to git.
* No need to ask for confirmation to run `cargo check` or `cargo test`.

## Release

To release, follow these two steps:
* Build and run tests
* Run .\build-signed.ps1.
* Increment the minor version in both package.json and src-tauri\tauri.conf.json

## OS

Attempt powershell commands before unix commands.