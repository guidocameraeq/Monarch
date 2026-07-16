# CLAUDE.md — Reglas para Claude Code en este fork

> Solo reglas de comportamiento. El **por qué** de cada decisión vive en `docs/DECISIONS.md`;
> dónde quedamos, en `docs/SESSION_HANDOFF.md`.
> Test por línea: **si borro esta línea, ¿Claude se equivoca? Si no, sobra.**

## Qué es esto

Fork personal de [Nuzair46/Monarch](https://github.com/Nuzair46/Monarch) (Tauri 2: Rust + React;
detach/attach de monitores con la CCD API de Windows). Rama **`personal`** = upstream main + el
PR #30 del autor + los fixes de Guido. Existe porque dos bugs lo volvían inusable; ambos están
arreglados y verificados en campo. Ver `docs/DECISIONS.md`.

## 🚨 Las cuatro reglas duras

1. **Ante Win32: SONDÁ, no leas la doc.** La documentación de Microsoft **miente** sobre
   `SetDisplayConfig` (ADR-004). Nos hizo perder una ronda entera. Antes de tocar cualquier
   combinación de flags, corré `tools/probe-sdc-flags.ps1` o escribí una sonda nueva:
   `SDC_VALIDATE` valida sin aplicar nada, es gratis y es la única fuente de verdad. Un `87`
   significa que los flags ni llegaron al driver → falla igual en toda máquina.
2. **`src-tauri` NO compila en la máquina de Guido**, y la causa importa porque el paréntesis
   fácil es falso: el toolchain **msvc de rustup SÍ está instalado** (`rustup show` lo lista);
   lo que faltan son las **VS C++ Build Tools** — no hay linker MSVC (el único `link.exe` en
   PATH es el de Git, que es otra cosa). Y el default activo es GNU, que falla al generar las
   import libs de `raw-dylib`: **no es que no lo soporte**, es que no encuentra `dlltool.exe`
   (está en el self-contained del toolchain, no en PATH) y sin `as.exe` de mingw-w64 tampoco
   termina. Comprobalo con `where link.exe` antes de dudar de esta regla. El core (`cargo test`
   en la raíz) sí compila. Para lo demás: verificar firmas contra las fuentes reales del crate
   en `~/.cargo/registry/src/*/windows-0.60.0/`, y que compile lo agarra el CI
   (`.github/workflows/build-personal.yml`, push a `personal`).
   **Nunca digas "compila" sin haberlo compilado.**
3. **Un status 0 de `SetDisplayConfig` no prueba NADA.** Con el set activo sin cambios es un
   no-op documentado que devuelve 0. Toda recuperación se verifica **re-enumerando**, jamás por
   el código de retorno (ADR-008).
4. **La topología no se toca sin red.** Es un desktop **sin panel interno**: si un apply sale
   mal, no hay pantalla de rescate. Sin captura del pre-estado no hay cambio de topología, y
   todo camino nuevo pasa por un dry-run `SDC_VALIDATE` antes de aplicar (ADR-009).

## Reglas de evidencia

- **El log de campo manda.** Guido usa Monarch en **otra PC** (desktop, 3 displays: 2 monitores
  + "Smart TV Pro" HDMI). Acá no está instalado. Todo diagnóstico sale de que él copie
  `%APPDATA%\Monarch\diagnostics.log`. Si una hipótesis no se ve en el log, es una hipótesis.
- **Lo no verificado se escribe NO VERIFICADO**, en el reporte y en el código. El bug original
  del autor sobrevivió meses porque un comentario inventó una causa plausible y nadie la
  comprobó (ADR-004). No repetir el pecado que vinimos a arreglar.
- **Cada ronda: implementar → revisión adversarial multi-agente → aplicar solo lo confirmado.**
  Encontró bombas reales que la implementación sola no vio (ADR-011). No saltear el paso.
- Antes de darle un build a Guido: **verificar el binario**, no el build. Strings del `.exe`
  (que esté lo nuevo, que NO esté lo viejo) + versión. Ver `docs/SESSION_HANDOFF.md`.

## Mapa

| Archivo | Rol |
|---|---|
| `src/` (crate `monarch`) | Lógica pura, sin Win32. **Acá viven los tests** (22) y `MockBackend`. |
| `src-tauri/src/backend/windows/` | La CCD API real. `enumerate.rs` (qué ve), `apply.rs` (qué manda), `topology.rs` (caché + recovery). |
| `src-tauri/src/app/` | Tray, comandos, watchdogs, IPC, listener de resume. |
| `web/` | React. `tauri.ts` es la única superficie IPC. |
| `tools/probe-sdc-flags.ps1` | **La sonda.** Regla 1. |
| `docs/DECISIONS.md` | Por qué el fork es como es. Cada ADR con su cicatriz. |
| `docs/SESSION_HANDOFF.md` | Save game: estado, qué falta, qué NO está verificado. |

## Comunicación

Español rioplatense (vos), criollo, corto — **Guido no es programador**. Explicar en analogías
concretas, no en jerga. Decisiones: opciones con pros/contras + UNA recomendación. Decir QUÉ
cambió y por qué le importa a él, no narrar el proceso. Si algo no se probó, decirlo derecho.
