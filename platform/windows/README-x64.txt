RLLM for Windows (x64)
======================

Installing
1. Extract this zip (right-click the zip, choose Extract All).
2. Double-click RLLM-Setup-x64.exe and choose OK.
   RLLM is installed for your user account only. No administrator rights are needed.
3. Open RLLM from the Start menu or the desktop shortcut.

If Windows shows "Windows protected your PC"
RLLM is signed with its own RLLM certificate, which a new PC does not know yet,
so Windows lists the publisher as unknown. Choose "More info", then "Run anyway".
To make Windows recognise the publisher permanently, double-click RLLM.cer,
choose Install Certificate, and place it in "Trusted Root Certification
Authorities" and in "Trusted Publishers". Only do this if you trust the person
who sent you this file.

Getting started
RLLM is a client: it talks to a language model that you choose. Open
App settings > LLM connections > Choose or add an LLM, and enter the model's
address, model name and key.

Removing RLLM
Settings > Apps > Installed apps > RLLM > Uninstall. Your conversations and
settings in %LOCALAPPDATA%\rllm are kept.
