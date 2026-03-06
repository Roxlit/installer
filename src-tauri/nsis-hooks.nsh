; Roxlit NSIS hooks

!macro NSIS_HOOK_PREINSTALL
  ; Set uninstaller icon to red logo
  !define MUI_UNICON "icons\uninstaller.ico"
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  ; Intentionally empty — do NOT delete ~/.roxlit/.
  ; It contains config.json with project paths, place_id, and user preferences.
  ; The directory is small (~50KB) and harmless to leave behind on full uninstall.
!macroend
