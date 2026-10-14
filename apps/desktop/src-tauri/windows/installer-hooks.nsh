; Tauri NSIS hooks backed by Arcade Link's checksum-pinned shared macros
; (vendored in packaging/windows, see VENDORED.md).
!define ARCADE_ID "arcade.box"
!define ARCADE_NAME "${PRODUCTNAME}"
!define ARCADE_VERSION "${VERSION}"
!define ARCADE_EXE "${MAINBINARYNAME}.exe"
!define ARCADE_CLI "arcade-desktop"
!define ARCADE_HOMEPAGE "https://github.com/qa-p1/Arcade-catalog"
!define ARCADE_START_AT_LOGIN_DEFAULT 0
!include "${__FILEDIR__}\..\..\..\..\packaging\windows\arcade.nsh"

!macro NSIS_HOOK_PREINSTALL
  ; Stop a running copy before replacing its payload.
  ${If} ${FileExists} "$INSTDIR\${MAINBINARYNAME}.exe"
    ExecWait '"$INSTDIR\${MAINBINARYNAME}.exe" --quit'
    Sleep 800
  ${EndIf}
!macroend

!macro NSIS_HOOK_POSTINSTALL
  ; Writes the shared receipt, Start menu/optional shortcuts, CLI shim and PATH.
  !insertmacro ArcadeInstall
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  ; Updates keep shared integration in place; normal uninstall is receipt-owned.
  ${If} $UpdateMode == 1
    StrCpy $ArcadeExitCode 0
  ${Else}
    !insertmacro ArcadeUninstall
  ${EndIf}
!macroend
