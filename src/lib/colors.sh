#!/bin/bash

# ==============================================================================
# Librería: Colores ANSI y Títulos
# ==============================================================================

# Colores ANSI para terminal
export C_RESET="\033[0m"
export C_BOLD="\033[1m"
export C_CYAN="\033[1;36m"
export C_GREEN="\033[1;32m"
export C_YELLOW="\033[1;33m"
export C_RED="\033[1;31m"
export C_WHITE="\033[1;37m"
export C_GRAY="\033[0;90m"

# Título global de la aplicación
export APP_TITLE="SERVIDOR NAS & CENTRAL DE RESPALDOS (DEBIAN 13)"

# Configuración de paleta visual nativa para Whiptail
export NEWT_COLORS="
root=,blue
window=lightgray,black
border=cyan,black
shadow=black,gray
button=black,cyan
actbutton=white,blue
compactbutton=black,lightgray
title=yellow,black
roottext=white,blue
textbox=white,black
acttextbox=white,blue
entry=white,blue
disentry=gray,black
checkbox=cyan,black
actcheckbox=black,cyan
listbox=white,black
actlistbox=black,cyan
actsellistbox=white,blue
"
