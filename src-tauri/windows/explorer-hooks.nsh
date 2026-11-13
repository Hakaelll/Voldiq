!define VOLDIQ_EXPLORER_VERB "Software\Classes\Directory\shell\Voldiq.Analyze"

!macro NSIS_HOOK_POSTUNINSTALL
  ${If} $UpdateMode <> 1
    Push $0
    ReadRegStr $0 HKCU "${VOLDIQ_EXPLORER_VERB}\command" ""
    ${If} $0 S== '$\"$INSTDIR\${MAINBINARYNAME}.exe$\" --analyze-folder $\"%1$\"'
      DeleteRegValue HKCU "${VOLDIQ_EXPLORER_VERB}\command" ""
      DeleteRegKey /ifempty HKCU "${VOLDIQ_EXPLORER_VERB}\command"
      ReadRegStr $0 HKCU "${VOLDIQ_EXPLORER_VERB}" ""
      ${If} $0 S== "Analizar con Voldiq"
        DeleteRegValue HKCU "${VOLDIQ_EXPLORER_VERB}" ""
      ${EndIf}
      ReadRegStr $0 HKCU "${VOLDIQ_EXPLORER_VERB}" "MultiSelectModel"
      ${If} $0 S== "Single"
        DeleteRegValue HKCU "${VOLDIQ_EXPLORER_VERB}" "MultiSelectModel"
      ${EndIf}
      DeleteRegKey /ifempty HKCU "${VOLDIQ_EXPLORER_VERB}"
      System::Call 'shell32::SHChangeNotify(i 0x08000000, i 0, p 0, p 0)'
    ${EndIf}
    Pop $0
  ${EndIf}
!macroend
