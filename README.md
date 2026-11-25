# Esme

Barra dinámica ("notch vivo") para Windows 10/11, en Rust nativo: Direct2D + DirectComposition,
sin Electron, sin WebView, 100 % local.

Gratis y de código abierto. Sin cuentas, sin publicidad y sin telemetría.

## Instalar (2 minutos)

1. Bajá **Esme.exe** de la [última versión](https://github.com/Maurito2004/Esme/releases/latest).
2. Abrilo. Como el exe no está firmado (un certificado cuesta plata), Windows puede mostrar
   **"Windows protegió su PC"**: tocá **Más información → Ejecutar de todas formas**.
3. Listo: aparece la píldora arriba al centro. Pasá el mouse para abrirla.
4. Clic derecho en el ícono de la bandeja → **Ajustes…** para elegir mascota, clima, avisos y más.

Para que arranque con Windows: Ajustes → General → Iniciar con Windows.
Para desinstalar: Salir desde la bandeja y borrá el exe (y si querés, la carpeta `%APPDATA%\Esme`).

**Ask Esme con IA es opcional**: la calculadora, las monedas y el timer andan sin nada. Para
preguntarle a Gemini necesitás una API Key gratis de Google AI Studio (Ajustes → Ask Esme →
Conseguir key).

¿Algo anda mal? Abrí un [issue](https://github.com/Maurito2004/Esme/issues) contando qué pasó y
en qué Windows.

## Estado: v1.0 — todas las fases completas

**Fase 1 — base**
- Ventana transparente por píxel, siempre visible, fuera de Alt+Tab, sin robar el foco.
- Notch arriba al centro del monitor principal; se expande con un resorte al pasar el mouse.
- Bandeja: Ask Esme, Capturar texto, Mascota, Ajustes… y Salir. Clic izquierdo en el ícono: fija o suelta el notch expandido.

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
- **Wi-Fi**: aviso al conectarse o desconectarse de una red (en PCs sin Wi-Fi no hace nada).
- Sin aviso de Bloq Mayús: requería leer el teclado de todo el sistema y Esme no lo hace.

**Fase 5 — panel "Hoy"** (botón 📅 en el panel expandido)
- **Pomodoro** 25/5: play/pausa y reinicio; mientras corre, la píldora contraída muestra la cuenta
  regresiva con su barra. Aviso y sonido al terminar cada etapa.
- **Agua**: vasos de hoy con + / −, meta configurable (Ajustes → Hoy → Meta de agua).
  Se reinicia cada día (`%APPDATA%\Esme\agua.json`).
- **Clima** de MET Norway (api.met.no): temperatura, estado y próximas 6 horas. Usa la ubicación
  aproximada de Windows o la ciudad que elijas en Ajustes → Hoy, y se actualiza cada una hora.
- **Calendario** de Google (solo lectura): copiá la "Dirección secreta en formato iCal" de tu
  calendario y elegí Ajustes → Hoy → Pegar dirección (hay pasos guiados). Muestra los
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
  monitor (Ajustes → General → "Pasar al otro monitor") o se esconde, y vuelve
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
  Gemini. Necesita tu propia API Key gratuita de Google AI Studio: copiala y elegí Ajustes →
  Ask Esme → Pegar key. Se guarda cifrada con Windows (DPAPI), atada a tu
  usuario, y viaja en un header HTTPS, nunca en la URL.

**Fase 7 — optimización**
- En reposo Esme ocupa unos **4 MB** en el Administrador de tareas (columna Memoria): a los 15 s
  sin usarla achica su superficie de dibujo al tamaño de la píldora y le devuelve a Windows la
  memoria que no usa. Al pasar el mouse vuelve al instante.
- Carga diferida: el clima y el calendario se cargan al abrir "Hoy"; las tasas de cambio, al abrir
  Ask Esme; el OCR, al usarlo.
- Las carátulas se guardan ya achicadas al tamaño en que se dibujan.
- Esme.exe pesa unos 600 KB.

**Fase 8 — Ajustes** (bandeja → "Ajustes…")
- Ventana de Ajustes con secciones: General, Apariencia, Hoy, Ask Esme, Avisos y Acerca de.
  Los cambios se guardan solos. La ventana solo existe mientras está abierta: al cerrarla, Esme
  vuelve a ocupar lo mismo que antes.
- **Tamaño del notch**: 100 %, 115 % o 130 %.
- **Clima por ciudad**, sin activar la ubicación de Windows: escribí tu ciudad y tocá Buscar
  (para homónimos: "Córdoba, España").
- **Calendario de Google** con pasos guiados y un botón que abre la página justa.
- Elegí qué mostrar en el panel **Hoy** (Pomodoro, agua, clima, calendario), los minutos del
  Pomodoro y la meta de agua.
- Encendé o apagá cada **aviso** de la píldora (volumen, salida de audio, teclado, cargador,
  brillo, batería baja).
- **Ask Esme**: botón para conseguir la API Key gratis, pegarla o borrarla, y elegir la familia
  (Flash, Flash-Lite o Pro). Esme usa el modelo más nuevo que tenga tu key; si Google lo retira o
  está saturado, pasa sola al siguiente y lo recuerda.
- Tema claro u oscuro para la ventana de Ajustes y el menú de la bandeja.

**Fase 9 — v1.0**
- **Mezclador** (botón 🔊 del panel de música): volumen del sistema y una barrita por cada app con
  sonido (Spotify, Discord, el navegador…). Clic en la barra para elegir el volumen, rueda del
  mouse para ajustarlo de a 5 % y clic en el nombre para silenciar. Solo lee el audio al abrirlo.
- **Tareas** en el panel Hoy: escribí y Enter. Con hora (`18:30 Llamar` o `Comprar pan @20:00`)
  la píldora te avisa a esa hora. Se guardan en `%APPDATA%\Esme\todo.json`.
- **Estante**: botón "Borrar todo".
- **Puntitos de página** abajo del panel para ver dónde estás y saltar entre vistas.
- **Píldora flotante** (Ajustes → Apariencia): separada del borde, con las 4 esquinas redondas.
- **Ecualizador**: con música sonando, las barritas de la píldora bailan con el color de la carátula
  y se frenan al pausar.
- **Esme en Ask**: mira lo que escribís y "piensa" mientras Gemini responde.
- **Buscar actualizaciones** (Ajustes → Acerca de): si hay una versión nueva, abre la página para
  bajarla. Esme nunca se descarga ni se reemplaza sola.

## Estilos al expandir

| Estilo | Qué se ve |
|---|---|
| Negro compacto (por defecto) | El notch crece en negro lo justo para el contenido |
| Fondo transparente | Solo la píldora negra de arriba; el contenido flota con sombra suave |
| Píldora flotante | Separada 7 px del borde de arriba, con las 4 esquinas redondeadas |

## Consumo (RTX 3070, 1920×1080)

| Estado | CPU | Memoria (private working set) |
|---|---|---|
| Reposo, mascota despierta | ~0,25 % de un núcleo | ~4 MB |
| Reposo, sin mascota o dormida | ~0,1 % | ~4 MB |
| Música sonando, píldora cerrada (ecualizador) | ~0,3 % | ~4 MB |
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

Todo en `%APPDATA%\Esme`: `config.json`, `portapapeles.json`, `estante.json`, `agua.json`,
`todo.json` y `letras\`. Si venías de la versión "Noosa", la primera vez que abre Esme mueve esos
datos y el inicio automático solo. Sin cuentas ni telemetría.

Esme solo se conecta a internet cuando usás algo que lo necesita: lrclib.net (letras), MET Norway
y Open-Meteo (clima), tu calendario de Google (si pegaste la dirección), dolarapi.com y las tasas
de cambio (monedas en Ask), Gemini (solo con tu key) y GitHub (solo al tocar "Buscar
actualizaciones").
