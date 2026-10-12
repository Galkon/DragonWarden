# DragonWarden

A Windows desktop manager for a RuneScape: Dragonwilds dedicated server, built with Tauri 2, React, Vite, Tailwind, and TypeScript.

## What it does

- Installs or updates Steam app `4019830` through a local SteamCMD installation.
- Starts the server with the recommended `-log -NewConsole` arguments and safely locks config editing while it runs.
- Creates and preserves `RSDragonwilds/Saved/Config/WindowsServer/DedicatedServer.ini` values.
- Reports local UDP 7777 listener, firewall readiness, and public IP; the UI clearly marks router/NAT verification as an external test because UDP forwarding cannot be reliably self-tested from the LAN.
- Backs up only changed world saves, zips older snapshots, and evicts oldest backups above the selected storage limit.

## Run locally

```sh
npm install
npm run tauri dev
```

To produce the Windows installer from a Windows machine with the [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/) installed:

```sh
npm run tauri build
```

## Build on GitHub Actions

This repository includes a Windows runner workflow at `.github/workflows/build-windows.yml`. After pushing the repository to GitHub, open **Actions → Build Windows installer → Run workflow**. When it completes, download the `DragonWarden-windows-installer` artifact; it contains the NSIS setup `.exe`.

The workflow also runs when you publish a GitHub Release or push a version tag, such as `v0.1.0`:

```sh
git tag v0.1.0
git push origin v0.1.0
```

## Notes

The server’s required settings are `OwnerId`, `ServerName`, `DefaultWorldName`, and `AdminPassword`. World password is optional. These are the settings documented by Jagex’s [Dedicated Servers guide](https://runescapedragonwilds.help.jagex.com/hc/en-gb/articles/45365343055249-Dedicated-Servers-How-to-Guide).
