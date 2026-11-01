The X-Plane plugin bundled with the app, staged here by `tools/stage-xp-plugin.ps1`
(which `tools/build-signed.ps1` runs before every release build):

- `64/win.xpl`
- `aircraft-characteristics.csv`
- `version.txt`

Only this README is committed. Without a staged plugin the app builds and runs, and
Settings says this build has no plugin to install. See `src/xp_plugin_install.rs`.
