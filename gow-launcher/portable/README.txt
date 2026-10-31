God of War (2005): PC Launcher, portable copy
==============================================

This is the same launcher as the installer, without installing it. It contains no game files: you need your own
God of War (USA, SCUS-97399) disc image.

How to use
  1. Unzip the whole folder somewhere with a path without accents or special characters (for example C:\GodOfWarPC).
  2. Open "God Of War - PC Launcher.exe". The setup wizard starts by itself and does the rest.
  3. Keep the "kit" folder next to the .exe: the installation scripts are in it.

If Windows says "Windows protected your PC" (SmartScreen)
  The launcher is not code-signed yet, so Windows does not know the publisher. Two ways around it:
  - Before unzipping: right-click the .zip, Properties, tick "Unblock", OK. Then unzip and open the .exe.
  - Or when the warning shows: More info, Run anyway.
  To check that the zip is the one published on GitHub, compare its SHA-256 with the SHA256SUMS-windows.txt file of the
  same release (PowerShell: Get-FileHash <file>).

What it needs
  - Windows 10 or 11, 64-bit, with the Microsoft Edge WebView2 Runtime. Windows 11 and up-to-date Windows 10 have it. If
    the launcher does not open, install it from https://developer.microsoft.com/microsoft-edge/webview2/ (Evergreen
    Standalone Installer). The setup program of the launcher bundles it; this zip does not.
  - About 30 GB free and 30 to 90 minutes for the first build. Details: https://github.com/danielalves96/gow-pc-port

Where it keeps things
  The program is this folder. Settings, logs and the extracted kit go to %APPDATA%\GOW_PC\Launcher, and the game and
  its build go to the folder you choose in the wizard. To remove everything, delete those folders.

------------------------------------------------------------------------------------------------------------------

God of War (2005): PC Launcher, cópia portátil

É o mesmo launcher do instalador, sem instalar. Não contém arquivos do jogo: você precisa da sua própria imagem de
disco de God of War (USA, SCUS-97399).

Como usar
  1. Descompacte a pasta inteira em um lugar com caminho sem acentos nem caracteres especiais (por exemplo
     C:\GodOfWarPC).
  2. Abra "God Of War - PC Launcher.exe". O assistente de instalação abre sozinho e faz o resto.
  3. Mantenha a pasta "kit" ao lado do .exe: os scripts de instalação estão nela.

Se o Windows mostrar "O Windows protegeu o computador" (SmartScreen)
  O launcher ainda não tem assinatura de código, então o Windows não conhece o editor. Há dois caminhos:
  - Antes de descompactar: clique com o botão direito no .zip, Propriedades, marque "Desbloquear", OK. Depois
    descompacte e abra o .exe.
  - Ou quando o aviso aparecer: Mais informações, Executar assim mesmo.
  Para conferir que o zip é o publicado no GitHub, compare o SHA-256 dele com o arquivo SHA256SUMS-windows.txt do mesmo
  release (PowerShell: Get-FileHash <arquivo>).

O que precisa
  - Windows 10 ou 11 de 64 bits, com o Microsoft Edge WebView2 Runtime. O Windows 11 e o Windows 10 atualizado já têm.
    Se o launcher não abrir, instale pelo site developer.microsoft.com/microsoft-edge/webview2 (instalador Evergreen
    Standalone). O instalador do launcher traz esse componente; o zip não.
  - Cerca de 30 GB livres e de 30 a 90 minutos na primeira compilação. Detalhes:
    https://github.com/danielalves96/gow-pc-port

Onde ele guarda as coisas
  O programa é esta pasta. Configurações, logs e o kit extraído ficam em %APPDATA%\GOW_PC\Launcher, e o jogo e a
  compilação ficam na pasta que você escolher no assistente. Para remover tudo, apague essas pastas.
