NiceVault - device
==================

This drive is a NiceVault device. It has two parts:

- "NiceVault" (this one): the NiceVault app for Windows, macOS and Linux.
- "VaultData": your encrypted volumes (.nvault files). File system: {fs}.
  Readable on: {readable}.

Which file to open
- Windows: NiceVault-windows-x86_64.exe
  Needs Microsoft Edge WebView2 (already on Windows 11 and on an up-to-date Windows 10).
- macOS: NiceVault.app (extracted from the signed package and checked when it was copied)
  The app is not signed by Apple. If macOS says it cannot be opened, right-click
  NiceVault.app > Open, or in Terminal:
      xattr -dr com.apple.quarantine /Volumes/NiceVault/NiceVault.app
- Linux: NiceVault-linux-x86_64 (double-click, or ./NiceVault-linux-x86_64 from a terminal).

The .sig files next to the programs are their signatures: NiceVault checks them when it updates.
When NiceVault runs from this drive it leaves nothing on the computer.
{missing}
Without its password a volume cannot be opened: nobody can recover it, not even NiceVault.
