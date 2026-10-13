; Roll keeps its data (database, thumbnails, logs) in %LOCALAPPDATA%\Roll.
; Tauri's per-user installer would put the program in the same folder, so
; it goes to %LOCALAPPDATA%\Programs\Roll instead, like other per-user apps.
; The directory page shows the new default (GUI init runs after .onInit);
; silent installs get it in the pre-install hook. An earlier install's
; location from the registry is kept. Tauri includes this file before it
; defines PRODUCTNAME, so the name is spelled out (it is the data folder's
; name in crates/core/src/paths.rs, too).

!define MUI_CUSTOMFUNCTION_GUIINIT RollInstallDir

!macro ROLL_INSTALL_DIR
  ${If} $INSTDIR == "$LOCALAPPDATA\Roll"
    StrCpy $INSTDIR "$LOCALAPPDATA\Programs\Roll"
  ${EndIf}
!macroend

Function RollInstallDir
  !insertmacro ROLL_INSTALL_DIR
FunctionEnd

; Runs right after `SetOutPath $INSTDIR`, so the output path moves too.
!macro NSIS_HOOK_PREINSTALL
  !insertmacro ROLL_INSTALL_DIR
  SetOutPath $INSTDIR
!macroend
