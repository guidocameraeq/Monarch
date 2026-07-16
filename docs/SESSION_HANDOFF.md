# Session Handoff — Monarch (personal)

> **Save game.** Dónde quedamos, qué falta, y sobre todo **qué NO está verificado**.
> El por qué de cada decisión: `docs/DECISIONS.md`. Las reglas: `CLAUDE.md`.

**Última sesión:** 2026-07-16 — de la auditoría del repo al release v51.0.0 con los dos bugs
arreglados y verificados en campo.

## Estado

- **v51.0.0 publicada**: [release](https://github.com/guidocameraeq/Monarch/releases/tag/v51.0.0)
  con el MSI descargable. El updater de la app apunta a este fork (probado contra el endpoint real).
- **Los dos bugs, arreglados y confirmados por Guido en su máquina**:
  - Tray congelado tras sleep → pipeline fuera del main thread + listener real de
    `WM_POWERBROADCAST`/`WM_DISPLAYCHANGE` + waits acotados + `ShowMainWindow` IPC.
  - TV detacheada irrecuperable → **attach explícito** (ADR-003). Log de campo:
    `validate=0` → `apply=0` → `missing=0` al primer poll.
- **PR al upstream**: [#43](https://github.com/Nuzair46/Monarch/pull/43) abierto (solo el bug de
  flags del ADR-004 + la sonda). Sin respuesta del autor al cierre de la sesión.
- **CI**: push a `personal` → MSI+exe como artefacto (`.github/workflows/build-personal.yml`).
- Core: **22/22 tests**. `LICENSE` con los dos copyrights (MIT).

## Lo que NO está verificado (leer antes de prometer nada)

0. **El attach explícito (ADR-003) se vio andar UNA vez, en UNA máquina, con UN driver.** Es el fix
   central del bug 2 y el `Estado` de arriba lo declara "confirmado" — con razón, pero n=1. El
   shape (batch + source id libre per-adapter + `modeInfoIdx` inválido) **no se pudo ni sondear**
   en la máquina de desarrollo (no tiene ningún target conectado-pero-inactivo). Lo que lo protege
   en runtime es el **dry-run `SDC_VALIDATE` obligatorio**, no la evidencia. El comentario de
   `apply.rs` es explícito: *"one machine agreeing is not every driver agreeing"*.
1. **`src-tauri` nunca se compiló localmente** — ninguna ronda. Solo el CI lo compila. El core sí.
2. **El caso del ADR-010 no se reprodujo acá**: que Windows reporte `targetAvailable=FALSE`
   transitoriamente tras un resume es **observación de un log**, no algo reproducible en esta
   máquina. El fix está probado con mocks en el core.
3. **`SDC_PATH_PERSIST_IF_REQUIRED` rescatando el extend**: sigue siendo apuesta. Nunca se lo vio
   funcionar; quedó como escalón secundario detrás del attach explícito, que sí anda.
4. **El canario del ADR-010** (`:candidate_for_unresolved_output:`): si aparece en un log, la
   prueba de que el hook es inútil tenía un agujero.
5. **Un solo ciclo dormir→despertar→aplicar limpio** en el último log. Guido dice que probó varias
   veces; la evidencia respalda una.

## Próximos pasos posibles (ninguno urgente)

- **Uso real unos días.** Es el único test que falta. Si algo raro aparece: pedir el
  `diagnostics.log` de `%APPDATA%\Monarch` (**el fresco**, no una copia vieja del Escritorio).
- **Follow-up del ADR-010**: borrar `prepare_attach_targets` entero (método del trait + 2 impls +
  plumbing). Es vestigial. Solo si el canario nunca aparece.
- **Si el autor responde el PR #43**: puede pedir cambios o mergear. Si le interesa el attach
  explícito (ADR-003), es un PR aparte y grande.
- **Traer mejoras del upstream**: `git fetch upstream && git merge upstream/main` sobre `personal`.
  El fork no diverge de forma irreconciliable: los cambios son quirúrgicos y están documentados.

## Trampas conocidas (nos mordieron a todos)

- **El instalador viejo gana.** Monarch arranca solo con Windows y se esconde en el tray; el
  single-instance hace que abrir el nuevo le muestre la ventana **al viejo** y el nuevo se cierre.
  Guido perdió una tanda entera probando el binario equivocado. Verificar **siempre** el header
  (`MONARCH (personal) v51.0.0`) antes de creer que se está probando lo nuevo. Hay un script en su
  Escritorio (`Monarch-fix\LIMPIAR-MONARCH.bat`) que limpia todo (backup de perfiles primero).
- **Los logs que manda Guido pueden ser copias viejas.** Chequear el timestamp de la última línea
  contra la hora de la prueba, y buscar huellas de la versión (`enum:`, `ui_cmd:` = build nuevo).
- **Verificar el binario, no el build.** Strings del `.exe`: que esté lo nuevo Y que no esté lo
  viejo. Un CI verde no prueba que el usuario corra ese código.
