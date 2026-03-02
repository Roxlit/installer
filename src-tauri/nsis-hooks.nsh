; Roxlit NSIS hooks
; Preserve user data (config, project registry) across reinstalls.

!macro NSIS_HOOK_PREUNINSTALL
  ; Intentionally empty — do NOT delete ~/.roxlit/.
  ; It contains config.json with project paths, place_id, and user preferences.
  ; The directory is small (~50KB) and harmless to leave behind on full uninstall.
!macroend
