# Firma de código con SignPath Foundation

Esme está preparado para firmarse gratis con [SignPath Foundation](https://signpath.org)
(certificado para código abierto). Lo que falta lo hace el dueño del repo, porque necesita su cuenta.

## Qué ya está hecho en el repo

- **Licencia MIT** (`LICENSE`).
- **Política de firma** en el `README.md` (sección *Firma de código*), como pide SignPath.
- **El exe lleva nombre, versión, autor y copyright** (`res/esme.rc`, compilado con `tools/build-res.ps1`).
- **Compilación en GitHub Actions** (`.github/workflows/release.yml`): sube el exe sin firmar como
  artefacto `Esme-unsigned` y, si SignPath está configurado, lo manda a firmar y publica el firmado.
  Sin configurar, publica sin firmar igual que antes.
- **Configuración del artefacto** lista para pegar: `docs/signpath/artifact-configuration.xml`.

## Pasos (una sola vez)

1. **Activá la verificación en dos pasos (2FA) en GitHub** si no la tenés: es requisito.
2. **Pedí el alta** en <https://signpath.org/apply> con el repo `https://github.com/Maurito2004/Esme`.
   Se puede pedir junto con BatteryHub. Responden por mail (puede tardar días o semanas).
3. Cuando te aprueben, en <https://app.signpath.io>:
   - Instalá la app de GitHub de SignPath sobre este repo (*Trusted Build Systems → GitHub.com*).
   - En el proyecto (slug `Esme`), pegá `docs/signpath/artifact-configuration.xml` como
     *artifact configuration*.
   - Anotá el **Organization ID** y el slug de la política de firma (`release-signing`).
   - Creá un usuario de CI y copiá su **API token**.
4. En GitHub → *Settings → Secrets and variables → Actions*:
   - **Secret** `SIGNPATH_API_TOKEN` = el token.
   - **Variable** `SIGNPATH_ORGANIZATION_ID` = el Organization ID.
   - (Opcional) `SIGNPATH_PROJECT_SLUG` y `SIGNPATH_POLICY_SLUG` si usaste otros nombres.
5. Publicá una versión como siempre (tag `vX.Y.Z`). SignPath te pide aprobar cada firma.

## Comprobar que salió firmado

```powershell
Get-AuthenticodeSignature .\Esme.exe | Format-List Status, SignerCertificate
```

`Status : Valid` con el certificado de *SignPath Foundation* = firmado.
