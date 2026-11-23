# Esme

Barra dinámica ("notch vivo") para Windows 10/11, en Rust nativo: Direct2D + DirectComposition,
sin Electron, sin WebView, 100 % local.

## Estado: v0.3 — Fases 1, 2 y 3 completas, parte de la Fase 4

**Fase 1 — base**
- Ventana transparente por píxel, siempre visible, fuera de Alt+Tab, sin robar el foco.
- Notch arriba al centro del monitor principal; se expande con un resorte al pasar el mouse.
- Bandeja: Iniciar con Windows, Mascota, Notch mínimo, **Fondo transparente al expandir**,
  Ajustes… y Salir. Clic izquierdo en el ícono: fija o suelta el notch expandido.

**Fase 2 — multimedia, letras y mascota**
- Multimedia (Windows SMTC): Spotify, Brave, Firefox, YouTube, etc. Carátula, título, artista,
  anterior / play-pausa / siguiente y barra de progreso (clic para saltar).
- Letras (lrclib.net): botón ☰ en el panel. Las sincronizadas resaltan la línea actual; clic en una
  línea salta a ese momento. Con las letras activadas, la píldora contraída muestra la línea actual.
  Se guardan en `%APPDATA%\Esme\letras` para no volver a pedirlas.
- Mascotas vectoriales (Direct2D, sin imágenes), solo ojos, elegibles en la bandeja → Mascota:
  - **Esme (Perrito)**, por defecto: ojos ámbar/miel con brillo.
  - **Gato**: los ojitos blancos originales.
  - Pestañean, miran hacia el cursor cuando está cerca, se duermen ("zzz") tras 3 minutos sin usar
    la PC y saludan (^ ^) al volver.

**Modo juego / pantalla completa (adelanto de Fase 4)**
- Si en el monitor principal hay un juego o un video a pantalla completa, el notch se pasa al otro
  monitor (opción de la bandeja "En pantalla completa: pasar al otro monitor") o se esconde, y vuelve
  solo al terminar. Roblox cuenta aunque esté maximizado con barra de título; otros juegos se agregan
  en `config.json` → `"juegos_extra": ["minecraft.exe"]`.
- Sin polling: eventos de Windows (ventana en primer plano, minimizar/restaurar) + aviso
  `ABN_FULLSCREENAPP` del shell.

**Fase 3 — estante, portapapeles y OCR**
- **Estante de archivos** (botón 📁): arrastrá archivos o carpetas sobre el notch y quedan a mano.
  Clic abre el archivo; arrastralo hacia afuera para soltarlo en Discord, el navegador o una
  carpeta (copia o vínculo, nunca mueve el original); clic derecho lo quita. Solo guarda las rutas
  (hasta 10) en `%APPDATA%\Esme\estante.json`.
- **Historial del portapapeles** (botón 📋): los últimos textos copiados; clic para volver a copiarlos.
  Guarda hasta 20 en `%APPDATA%\Esme\portapapeles.json` y respeta a los gestores de contraseñas.
- **Smart OCR** (`Ctrl + Alt + T` o bandeja → "Capturar texto"): congela la pantalla, elegís un área
  y el texto se reconoce offline con `Windows.Media.Ocr` y se copia al portapapeles (aviso en la
  píldora). Esc o clic derecho cancela.

## Estilos al expandir

| Estilo | Qué se ve |
|---|---|
| Negro compacto (por defecto) | El notch crece en negro lo justo para el contenido |
| Fondo transparente | Solo la píldora negra de arriba; el contenido flota con sombra suave |

## Consumo (RTX 3070, 1920×1080)

| Estado | CPU | Memoria (private working set) |
|---|---|---|
| Reposo, mascota despierta | ~0,25 % de un núcleo | ~4 MB |
| Reposo, sin mascota o dormida | ~0,1 % | ~4 MB |
| Expandido con música | redibuja 2 veces por segundo (barra) y en cada cambio de línea de la letra | |

Todo es por eventos: los cambios de música llegan de Windows; las letras usan un timer exacto
hasta la próxima línea; el pestañeo son 4 cuadros por timer.

## Compilar

Requisitos: Rust (`x86_64-pc-windows-msvc`) y VS 2022 Build Tools con C++.

```powershell
powershell -ExecutionPolicy Bypass -File tools\build-res.ps1   # solo si cambiás res\
cargo build --release
.\target\release\Esme.exe
```

`cargo test --release` corre los tests; `cargo test --release -- --ignored` además prueba lrclib.net.

### Sin build scripts

El proyecto compila sin `build.rs` ni dependencias con build scripts:
- `windows` 0.61 (enlaza con `windows-link`) y `nanoserde` para JSON;
- el ícono sale de `assets\Esme.jpg` con `python tools\make_icon.py` (Pillow), y el ícono,
  manifiesto y versión se compilan con `tools\build-res.ps1` y se enlazan desde `main.rs`;
- HTTP con WinHTTP del sistema.

`Esme.exe` lleva nombre, versión, descripción, manifiesto e ícono propios.

## Datos

Todo en `%APPDATA%\Esme`: `config.json`, `portapapeles.json` y `letras\`. Si venías de la versión
"Noosa", la primera vez que abre Esme mueve esos datos y el inicio automático solo. Sin cuentas ni telemetría. La única conexión a
internet es a lrclib.net, y solo cuando abrís las letras.
