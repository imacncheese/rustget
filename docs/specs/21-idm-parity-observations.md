# IDM 6.43.18 parity observations

**Source:** `C:\Program Files (x86)\Internet Download Manager\IDMan.exe`
**SHA-256:** `a705c0154fafc7cd509decf165cb6f6c050b6521b40adb6f730dd827c86051c7`
**Method:** static string/class inventory with REA and local binary inspection. No IDM
code, assets or protocol implementations were copied.

## Confirmed IDM feature families

The binary contains classes/strings for:

- download queues, queue selection and queue tabs (`CDownloadQueue`, `CChooseQueueDlg`)
- schedulers and grabber schedulers (`CNewSchedulerDlg`, `CGrabbersSchedulers`)
- site grabber projects, custom templates and saved projects
- browser integration and browser menu customization
- proxy settings, FTP proxy, personal proxy script (PAC)
- dial-up connection/login/progress
- download limits and limit-exceeded dialogs
- batch download
- site logins/passwords
- sounds
- exit-on-Windows-shutdown
- FTP/HTTP/media download paths and MP4 audio/T S muxing classes

## RustGet gaps identified from the inventory

| Area | RustGet state | Backlog action |
|---|---|---|
| Dial-up/reconnect | absent | Add a Windows RAS profile selector and reconnect-on-drop policy |
| Sounds | absent | Add completion/error sound settings |
| Per-site password store | basic auth per download | Add an encrypted site-credential store keyed by origin |
| Custom grabber templates | fixed crawler filters | Add saved grabber templates/projects |
| Browser menu customization | fixed extension menu | Add configurable context-menu entries |
| PAC/system proxy | HTTP/SOCKS5 only | Add PAC evaluation and system-proxy discovery |
| Queue tabs/UI | one queue tab | Add queue tabs and per-queue lanes |

These are behavior-level observations only. Proprietary IDM implementation details,
artwork and protocol code remain out of scope.
