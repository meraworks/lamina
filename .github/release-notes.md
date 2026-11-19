## Changes in 0.5.1

Lamina now has installers. Each release gives an installer for each desktop
system, with the archives as before.

- **Windows:** `lamina-<version>-<arch>-setup.exe`. It installs for the
  current user only and adds Lamina to the Start menu.
- **macOS:** `lamina-<version>-<arch>.pkg`. It installs `Lamina.app` in
  `/Applications`.
- **Linux:** `lamina-<version>-<arch>.deb`. It installs `lamina` and
  `lamina-viewer` in `/usr/bin` and adds a desktop entry.

The tools and the file format did not change from 0.5.0.
