A first preview of Roll: a local photo and video library that sorts itself.
It is in active development; see the README for what works today and
docs/plan.md for what is next. Roll never deletes anything permanently and
every change can be undone, but please try it on a copy of your photos first.

## Downloads

| System | File | Notes |
|---|---|---|
| Windows 10 and 11 | `Roll_*_x64-setup.exe` | Installs for your user only, no administrator needed. ExifTool and FFmpeg are included. |
| macOS 11 or later (Apple silicon and Intel) | `Roll_*_universal.dmg` | Needs ExifTool and FFmpeg: `brew install exiftool ffmpeg`. |
| Debian, Ubuntu and derivatives | `roll_*_amd64.deb` | Installs ExifTool and FFmpeg as dependencies. |
| Other Linux | `roll_*_amd64.AppImage` | Needs `exiftool` and `ffmpeg` from your distribution. |

`SHA256SUMS` lists the checksum of every file.

## Not signed yet

The builds are not code-signed, so the systems warn on first start:

- **Windows:** SmartScreen says "Windows protected your PC". Choose More info, then Run anyway.
- **macOS:** the app is signed ad hoc, not notarized. Open it once with right-click › Open, or allow it in System Settings › Privacy & Security.

There is no automatic update yet; new versions are published here.
