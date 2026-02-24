; Roxlit NSIS hooks
; Clean up custom application data on uninstall

!macro NSIS_HOOK_PREUNINSTALL
  ; Delete Roxlit config directory (~/.roxlit/)
  IfFileExists "$PROFILE\.roxlit\*.*" 0 +2
    RMDir /r "$PROFILE\.roxlit"
!macroend
