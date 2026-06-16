; NSIS installer hooks for Runebound (wired in via bundle.windows.nsis.installerHooks).
;
; Tauri's uninstaller only deletes data stored under the bundle identifier
; ("$APPDATA\${BUNDLEID}" / "$LOCALAPPDATA\${BUNDLEID}"). Runebound deliberately
; stores its data under a "runebound.sh" folder instead, so the default cleanup
; misses it and the files survive an uninstall. The two locations (see
; core/src/config.rs and core/src/db.rs):
;
;   %APPDATA%\runebound.sh       -> config.toml, calendar.toml, entities\   (dirs::config_dir)
;   %LOCALAPPDATA%\runebound.sh  -> app.db                                  (dirs::data_local_dir)
;
; Remove those here, but ONLY under the same two guards Tauri uses for its own
; app-data deletion:
;   * $DeleteAppDataCheckboxState = 1  -> the user ticked "delete application data"
;   * $UpdateMode <> 1                 -> this is a real uninstall, not an in-place update
; The update guard is essential: without it, installing a new version (which runs
; the old uninstaller first) would wipe the user's config and campaign database.
;
; This hook runs after Tauri has finished its own uninstall cleanup, where both
; variables are still in scope.

!macro NSIS_HOOK_POSTUNINSTALL
  ${If} $DeleteAppDataCheckboxState = 1
  ${AndIf} $UpdateMode <> 1
    SetShellVarContext current
    RmDir /r "$APPDATA\runebound.sh"
    RmDir /r "$LOCALAPPDATA\runebound.sh"
  ${EndIf}
!macroend
