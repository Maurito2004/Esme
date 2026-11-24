# Esme

Barra dinámica ("notch vivo") para Windows 10/11, en Rust nativo: Direct2D + DirectComposition,
sin Electron, sin WebView, 100 % local.

## Descargar

Bajá **Esme.exe** de la última versión en
[Releases](https://github.com/Maurito2004/Esme/releases/latest), abrilo y listo: aparece la barra
arriba al centro. Clic derecho en el ícono de la bandeja para configurarla.

## Estado: v0.7 — Fases 1, 2, 3 y 5 completas, Fase 4 casi completa, Fase 6 en curso

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

**Fase 4 — overlays del sistema** (en la píldora, 1,6 s, todo por eventos de Windows)
- Volumen y silencio (barra ámbar), vía callback de Core Audio.
- Cambio de salida de audio, por ejemplo al conectar auriculares Bluetooth (muestra el nombre).
- Cambio de idioma/distribución del teclado (shell hook).
- Cargador conectado/desconectado con porcentaje (solo en equipos con batería).
- **Notebooks**: brillo de la pantalla (barra) y aviso de batería baja al bajar de 20 % y 10 %.
- Sin aviso de Bloq Mayús: requería leer el teclado de todo el sistema y Esme no lo hace.

**Fase 5 — panel "Hoy"** (botón 📅 en el panel expandido)
- **Pomodoro** 25/5: play/pausa y reinicio; mientras corre, la píldora contraída muestra la cuenta
  regresiva con su barra. Aviso y sonido al terminar cada etapa.
- **Agua**: vasos de hoy con + / −, meta configurable (bandeja → Configuración → Meta de agua).
  Se reinicia cada día (`%APPDATA%\Esme\agua.json`).
- **Clima** de MET Norway (api.met.no): temperatura, estado y próximas 6 horas. Usa la ubicación
  aproximada de Windows (tiene que estar activada) y se actualiza cada una hora.
- **Calendario** de Google (solo lectura): copiá la "Dirección secreta en formato iCal" de tu
  calendario y elegí bandeja → Configuración → Calendario → Pegar URL iCal copiada. Muestra los
  próximos eventos (incluye repeticiones diarias, semanales, mensuales y anuales).

**Animaciones** (cortas y solo cuando pasa algo; en reposo no corre ninguna)
- Saludo al iniciar según la hora y mascota saludando con un saltito.
- Borde de luz azul → color de los ojos de la mascota al expandir; barras con ese mismo color.
- Con música, el borde toma el color predominante de la carátula (cambia suave entre canciones).
- Los ojos de la mascota hacen "¡oh!" (se agrandan un instante) al pasar el mouse o al volver de un aviso.
- Fundido de carátula al cambiar de canción, barra de volumen que se desliza y entrada escalonada
  de los paneles.

**Modo juego / pantalla completa**
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

**Fase 6 — Ask Esme** (`Ctrl + Alt + Espacio`, botón ⚡ del panel o bandeja → "Ask Esme / Traductor")
- Cuadro de texto dentro del notch. Solo toma el teclado mientras está abierto: Esme nunca lee el
  teclado del resto del sistema. Con un juego en pantalla completa el atajo no hace nada (no le roba
  el foco al juego); con clic sí se abre.
- **Calculadora** local al terminar en `=`: `1500*3+(450/2)=` → 4725 (paréntesis, potencias, %).
- **Monedas** al terminar en `=`: `100 usd a ars=`, `50 eur en usd=`, y para Argentina
  `100 usd a blue=` / `50000 mep en usd=` (dólar blue y MEP de dolarapi.com). Las tasas se
  actualizan una vez por día y quedan guardadas para usarlas sin internet.
- **Timer rápido**: `timer 10m`, `timer 45s`, `timer 1h 30m` + Enter; la píldora muestra la cuenta.
- **Preguntar / Traducir** (Tab cambia de modo) y **Resumir clip** / **Corregir** lo copiado, con
  Gemini. Necesita tu propia API Key gratuita de Google AI Studio: copiala y elegí bandeja →
  Configuración → IA Gemini → Pegar API Key. Se guarda cifrada con Windows (DPAPI), atada a tu
  usuario, y viaja en un header HTTPS, nunca en la URL.

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

Cada tag `vX.Y.Z` que se sube compila Esme en GitHub Actions y publica el Release con el exe
(`.github/workflows/release.yml`; notas en `docs/release-notes/`).


Funciona en Windows 10 (1903+) y Windows 11, de escritorio y notebooks.

Requisitos para compilar: Rust (`x86_64-pc-windows-msvc`) y VS 2022 Build Tools con C++.

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
