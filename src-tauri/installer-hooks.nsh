; Stop owned scheduled work before uninstalling its executable. User reports and preferences remain.
!macro NSIS_HOOK_PREUNINSTALL
  nsExec::ExecToStack '"$INSTDIR\mirrorproof.exe" --remove-schedules'
  Pop $0
  Pop $1
  ${If} $0 != 0
    MessageBox MB_ICONSTOP|MB_OK "MirrorProof could not remove its scheduled scans. Close any running scan, check Task Scheduler permissions and try uninstalling again." /SD IDOK
    Abort
  ${EndIf}
!macroend
