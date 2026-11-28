# Airglow 2.2.1 — In-App Updater Hotfix

Airglow 2.2.1 fixes the Windows in-app **Update & Restart** swapper so updates downloaded from the Settings -> Updates tab replace `Airglow.exe` and relaunch reliably.

## Fixes in 2.2.1
- **Fixed Updater Process Creation (`CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP`)**: Removed the invalid `CREATE_NO_WINDOW | DETACHED_PROCESS` flag combination (`ERROR_INVALID_PARAMETER` / OS error 87) that prevented `powershell.exe` from spawning the in-place binary swap script on Windows.
- **Safer Backup Preservation & Working Directory**: Preserves `Airglow.exe.bak` across swap retries, waits for Windows image lock release, logs swap diagnostics to `update-last-error.log` on failure, and relaunches the updated binary with `-WorkingDirectory` set to the Airglow folder.
- **Keep UI Alive on Updater Error**: `Message::ApplyStagedUpdateAndRestart` now flushes window state and lets the engine drive process exit upon successful swapper launch rather than closing before displaying any updater error.

> [!NOTE]
> If you are updating *from* **2.1.0** or **2.2.0**, replace `Airglow.exe` manually one time with this **2.2.1** binary — all subsequent in-app **Update & Restart** upgrades will swap and relaunch automatically.

## Downloads
- **`Airglow.exe`** — Standalone portable Windows x64 executable (double-click to run).
- **`Airglow-2.2.1-windows-x64-portable.zip`** — Portable archive with `Airglow.exe`, `airglow.ico`, `README.md`, and `LICENSE`.
- **`SHA256SUMS.txt`** — SHA-256 verification checksums.
