; Installer hooks for Windows.
;
; The managed EasyTier core runs as a service whose host process is this same
; binary, re-invoked as `--service-host`. Before this installer replaces the
; binary it kills every process carrying its name, so an update would leave the
; core stopped with nothing left to notice it had been up.
;
; The restore lives here rather than in the app because the build doing the
; updating is the one being replaced — it cannot arrange its own successor's
; behaviour. A *new* installer runs on every update, including the update that
; first ships this file, so the hook covers the 0.3.1 → 0.3.2 hop as well as
; every hop after it.
;
; The service name must match WINDOWS_SERVICE_NAME in src-tauri/src/paths.rs.

Var ServiceWasRunning

!macro NSIS_HOOK_PREINSTALL
  StrCpy $ServiceWasRunning 0

  ; `findstr` reports through its exit code whether the status output says
  ; RUNNING, which saves parsing `sc query` in NSIS.
  nsExec::ExecToStack 'cmd /c sc query "EasyTierManager" | findstr /C:"RUNNING"'
  Pop $0
  ${If} $0 == 0
    StrCpy $ServiceWasRunning 1
    ; Stop the core deliberately so it shuts down instead of being killed
    ; mid-write. `sc stop` returns before the service has actually stopped; the
    ; installer's own process check deals with anything still holding the binary.
    nsExec::ExecToStack 'cmd /c sc stop "EasyTierManager"'
    Pop $0
  ${EndIf}
!macroend

!macro NSIS_HOOK_POSTINSTALL
  ${If} $ServiceWasRunning == 1
    nsExec::ExecToStack 'cmd /c sc start "EasyTierManager"'
    Pop $0
  ${EndIf}
!macroend
