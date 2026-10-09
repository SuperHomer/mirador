; Installer hooks for the NSIS bundle (bundle.windows.nsis.installerHooks).
;
; Session holders are `mira __hold` processes, and they outlive the app on
; purpose — an update must not end the user's shells. But Windows refuses to
; open a running .exe for writing, so the installer could not extract the
; new mira.exe and stopped on "Error opening file for writing". Ignoring it
; left the old mira.exe in place: the CLI and every holder started after
; the update ran the previous release.
;
; A running image can still be renamed, though. Move the old mira.exe aside
; so the new one extracts under its name; running holders keep executing the
; renamed file, as they would on macOS, where the bundle is replaced and the
; old inode lives on. Each update first sweeps the copies set aside before —
; Delete fails silently on one a holder still runs, which is then retried
; next time. Numbered names, because such a survivor occupies its name.

!include LogicLib.nsh

!macro NSIS_HOOK_PREINSTALL
  Delete "$INSTDIR\mira.exe.old*"
  ${If} ${FileExists} "$INSTDIR\mira.exe"
    Push $R9
    StrCpy $R9 1
    ${DoWhile} ${FileExists} "$INSTDIR\mira.exe.old$R9"
      IntOp $R9 $R9 + 1
    ${Loop}
    ; If this fails too, extraction asks as it always did.
    Rename "$INSTDIR\mira.exe" "$INSTDIR\mira.exe.old$R9"
    Pop $R9
  ${EndIf}
!macroend

; The uninstaller removes the files it installed; the set-aside copies are
; not among them, and would keep the install directory from being removed.
!macro NSIS_HOOK_PREUNINSTALL
  Delete "$INSTDIR\mira.exe.old*"
!macroend
