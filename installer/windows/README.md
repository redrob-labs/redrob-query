# Redrob Query: Windows setup

`redrob_query_setup.cpp` is the branded Windows setup and its uninstaller, in one small native Win32 app
(Direct2D, DirectWrite, WIC; nothing beyond the Windows SDK). It is the Redrob Launcher setup
(`redrob-labs/redrob-launcher`, `installer/windows/`, our own code, itself from the Redrob Canvas and Redrob
Browser setups) with Redrob Query's payload layout and wording: one wizard look across the Redrob apps, a
language page, an `.exe` that carries the Redrob icon and builds one SmartScreen reputation.

For Redrob Launcher the installed product is **ProductName `Redrob Query`**, the executable is
**`bin\redrob-query.exe`** under `%LOCALAPPDATA%\Programs\Redrob Query`, and the Settings > Apps entry is
`HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\RedrobQuery` with publisher `Redrob`.

## What it does

- **Language first.** The first screen is bilingual and offers English / 한국어; every screen after it, the
  window title included, follows the choice. It is preselected from a previous install's answer, else
  Korean on a Korean Windows, else English (`--lang en|ko` overrides). The answer goes to
  `HKCU\Software\Redrob\Redrob Query\Language` so the next setup preselects it.
- **Install**, per user, no administrator rights, into `%LOCALAPPDATA%\Programs\Redrob Query`. The payload is
  the release's own signed `-portable.zip` (`bin\redrob-query.exe` and the licence set, from
  `tools/release/stage.py`), unpacked with System32's `tar.exe` beside the old install and swapped in, so a
  failed update never leaves a half-updated folder. A running Redrob Query is never closed for the person:
  the setup asks them to close it, then Retry.
- **Shortcuts**: Start menu always, desktop when the welcome screen's box is ticked (the choice is
  remembered for the next update). Both carry the AppUserModelID `ai.redrob.query`, set through
  `IPropertyStore`, which Windows needs for toast notifications from an unpackaged app.
- **Registry**: `InstallDir`, `Language`, `DesktopShortcutChoice` under `HKCU\Software\Redrob\Redrob Query`;
  the App Paths entry for Win+R `redrob-query`; and the **Settings > Apps** entry
  (`...\Uninstall\RedrobQuery`: name, version, publisher, icon, size, `UninstallString`).
- **Uninstall**: the same app built without a payload, written into the install folder as
  `Uninstall Redrob Query.exe`. It asks first, with "Also delete its settings and connection profiles" off by
  default, then removes the shortcuts, the entries and the folder. With the box: `%APPDATA%\Redrob Query`
  (connection profiles, workspace, `ui.ron`) and the registry key. Passwords live in the Windows Credential
  Manager and are not touched. `%APPDATA%\Redrob\Shared` (the launcher's sign-in every Redrob app reads) is
  never touched.

## Flags

| Flag | Does |
| --- | --- |
| (none) | the setup window; the uninstaller build opens its confirm screen |
| `--lang en` / `--lang ko` | preselect the language |
| `--uninstall` | the uninstall window (what Settings > Apps runs) |
| `--silent` | the same work with no window; the exit code is the result (0 = success, `0xE0000001` = Redrob Query is running). Never opens the app afterwards |
| `--delete-data` | with `--uninstall --silent`: also delete settings and connection profiles |
| `--demo`, `--demo-error`, `--demo-uninstall` | the window's states on a timer, touching nothing |
| `--render-stages <dir>` | draw every screen (both languages) to PNGs in `<dir>`, no window |

## Build

Visual Studio with the C++ x64 toolset (found with `vswhere`) and Python 3:

```
py -3 installer\windows\build.py --out <dir> --arch x64 --payload redrob-query-X.Y.Z-windows-x64-portable.zip --version X.Y.Z
```

The release workflow builds it in two stages so every binary is signed before it is embedded:
`--uninstaller-only`, sign that, then `--payload … --uninstaller <signed exe> --name …-setup.exe`, sign that
(`.github/workflows/release.yml`, Windows leg; signing is the shared `redrob-labs/.github/actions/sign-windows`
action through Azure Artifact Signing).

The art is committed: `res/background.png` is `res/background.html` rasterised at 1320x800 (shared with the
other Redrob setups); `res/lockup.png` (the Redrob Query lockup, white, scaled) and
`resources/icons/redrob-query.ico` come from `scripts/build-brand-assets.py` and are pinned in
`brand/SOURCES.json`.

## Verify

`verify-install.ps1` runs on a Windows machine against a built setup: install, shortcuts, the Settings entry,
the signed exe, an upgrade over the install, an update refused while Redrob Query runs, and the uninstall. It
does not exercise `--delete-data`, which would delete that machine's real connection profiles. Both languages:
`--lang ko` and `--lang en`.
