# Decisiones — por qué este fork es como es

> Un ADR por decisión no obvia. Cada uno lleva **su cicatriz**: la evidencia real que lo obligó.
> Si una decisión no tiene cicatriz, probablemente no hacía falta tomarla.
> Fecha de todo lo de acá: **2026-07-12 a 2026-07-16** (la sesión que arregló los dos bugs).

Contexto de hardware, porque casi todo depende de él: **desktop, 3 displays** (2 monitores + una
"Smart TV Pro" por HDMI, `target_id=4352`), **sin panel interno**. La máquina donde se desarrolla
es otra (una laptop de 1 pantalla, sin Monarch instalado).

---

## ADR-001 — Forkear en vez de esperar al upstream

**Decisión**: fork en `guidocameraeq/Monarch`, rama `personal`, con el PR #30 del autor mergeado.

**Por qué**: los dos bugs (tray congelado tras sleep; TV detacheada irrecuperable tras reinicio)
volvían la app inusable para Guido, que la usa todos los días. El autor tenía el problema
identificado hacía meses (issues #15, #40, #41; rama `15-try-fix-major-bug`) sin cerrarlo.

**Alternativas**: (a) esperar — descartada, sin fecha; (b) parchear solo local — descartada, se
pierde en cada update; (c) reescribir desde cero — absurdo, el 95% del código anda bien.

**Sobre el PR #30 (crédito donde corresponde)**: la rama `personal` incluye el PR #30 del **autor**
—abierto y sin mergear en upstream— y sin él este fork no existiría como está. Aportó: (a) la mitad
main-thread del bug del tray (`819a1f5`: sacar el trabajo pesado del handler del menú); (b) el
enrichment con `QDC_DATABASE_CURRENT` (`8b6b9ef`), que es lo que hace que un display detacheado sea
**visible** — la materia prima del ADR-003; (c) `diagnostics.rs` entero, el log del que salieron
**todos** los fixes de acá (ADR-012). **No alcanzó** para el caso de campo: en la placa de Guido
esa query no devuelve el target detacheado — de ahí el seeder de `QDC_ALL_PATHS` y el attach
explícito.

---

## ADR-002 — Versión 51.0.0

**Decisión**: versionar `51.0.0`, con la identidad visible sin clics (header `MONARCH (personal)
v51.0.0` + tooltip del tray vía `env!("CARGO_PKG_VERSION")`).

**Cicatriz**: primero se usó `1.51.0` ("fork de 1.5.1"). Guido **probó el binario viejo una tanda
entera** creyendo que era el nuevo: `1.5.1` y `1.51.0` no se distinguen de un vistazo, y el
instalador viejo seguía en su otra PC. Se perdió una ronda completa de diagnóstico sobre un log
que no correspondía al código que analizábamos.

**Por qué 51**: el autor va por 1.5.x y nunca va a llegar a 51 → imposible de confundir.

**⚠️ Lo que este ADR afirmaba y era FALSO** (se dejó escrito como recordatorio): *"bonus: Windows
bloquea instalar un MSI upstream encima, lo ve como downgrade"*. **No.** El schema del CLI de Tauri
documenta `bundle.windows.allowDowngrades` con `"default": true` ("blocking the user from
installing an older version if set to `false`"), y `src-tauri/tauri.conf.json` no tiene clave
`windows` → aplica el default → el MSI se renderiza con `AllowDowngrades="yes"` → **no hay
bloqueo**. Peor: ese flag vive en el MSI que se está **instalando**, no en el instalado, así que
este fork no puede impedir un MSI de upstream ni cambiando su propia config. Setear
`allowDowngrades: false` solo bloquearía instalar un MSI **viejo del fork** sobre uno nuevo.

**La red real contra confundir builds es una sola: mirar el header.** Esta línea falsa se escribió
con la misma voz que el resto, y sobrevivió a que se la repitiera tres veces en la conversación.
Es el ADR-004 pasando de nuevo, esta vez de nuestro lado.

**Regla que queda**: la versión tiene que verse **sin abrir nada ni tocar nada**. Antes estaba
solo detrás del botón "buscar actualizaciones", que es igual a no estar.

---

## ADR-003 — Attach explícito en vez del extend (LA cura del bug 2)

**Decisión**: conservar el `DISPLAYCONFIG_PATH_INFO` de los targets conectados-pero-inactivos que
devuelve una pasada `QDC_ALL_PATHS`, guardarlos en `TopologySnapshot.attachable`, y **activar el
target explícitamente** (`SDC_USE_SUPPLIED_DISPLAY_CONFIG`, source id libre per-adapter, todos los
candidatos en un solo apply, dry-run `SDC_VALIDATE` antes).

**Cicatriz**: `enumerate.rs` ya tenía el path de la TV — por eso la UI podía mostrar
"Smart TV Pro" — y lo **tiraba a la basura** por prudencia. Mientras tanto, el recovery le rogaba
a Windows un "extender todo" que no podía funcionar (ADR-004). Es exactamente lo que hace
Configuración de pantalla cuando el usuario toca "Extender" a mano: el workaround que a Guido
siempre le funcionó.

**Por qué fuera de `raw.paths`**: el autor tiene una invariante sana — solo paths enumerados
llegan a `SetDisplayConfig` (`QDC_ALL_PATHS` devuelve decenas de combinaciones fantasma; medimos
**44 paths para 1 sola pantalla**). Meterlos en `raw.paths` haría que `apply_layout_against_snapshot`
los mande crudos. `attachable` es un canal aparte que solo el recovery consume.

**Verificado en campo** (no es teoría):
```
recover:explicit_attach:'Smart TV Pro' (target_id=4352, ...):source=2:validate=0
recover:explicit_attach:batch=1:apply=0
recover:settle_poll:attach:1:missing=0
recover:resolved:explicit_attach
```

---

## ADR-004 — Sondar Win32, no creerle a la documentación

**Decisión**: toda combinación de flags de `SetDisplayConfig` se valida con sondas `SDC_VALIDATE`
(`tools/probe-sdc-flags.ps1`) antes de escribir código.

**Cicatriz — el bug que costó meses**: `force_topology_extend()` llamaba
`SDC_APPLY | SDC_TOPOLOGY_EXTEND | SDC_ALLOW_CHANGES | SDC_SAVE_TO_DATABASE`. Combinación
**ilegal** → `87` (`ERROR_INVALID_PARAMETER`).

**Qué se midió y qué se dedujo** (la costura importa, y este ADR es el que menos derecho tiene a
borrarla): **medido** en 2 máquinas — la sonda en la laptop de desarrollo (1 pantalla) y el
`apply:sdc_failed:87:topology_extend` del log de campo (desktop, 3 pantallas). **Deducido**: que
falla igual en toda máquina. Lo sostiene el diferencial de la sonda: `EXTEND|ALLOW` → 87 mientras
`EXTEND` solo → 31, y un 31 solo puede venir de un pedido que **sí** se evaluó contra el hardware
→ el 87 es rechazo a nivel de parámetros, antes del driver → no depende de la placa. Es una
inferencia fuerte, pero es una inferencia.

Al lado había un comentario que decía *"some driver stacks reject direct topology-extend during
early-login / post-reboot states"*: **una causa plausible que nadie verificó y que el 87 refuta**
(los flags nunca llegaban al driver). Blindó el bug y justificó un parche (`DisplaySwitch.exe`)
que tampoco podía servir (ADR-005). Notar que la **observación** de fondo era correcta — Win+P sí
funciona, porque no pasa por los flags rotos; lo que estaba mal era la explicación.

**Quién lo escribió: NO VERIFICADO, y no importa.** Este ADR afirmaba que *"el código lo escribió
una IA, alucinó una explicación"*, apoyado en que `src-tauri/Cargo.toml` (no el de la raíz) dice
`authors = ["Codex"]`. Eso no sustenta nada: el campo es metadata del paquete, puesta al
scaffoldear el proyecto (`4a84197`, 2026-02-25), y el comentario entró 9 días después en un commit
firmado por el autor con su nombre (`09a1acf`). Acusar de alucinación con esa evidencia era
exactamente el pecado que este ADR denuncia. El punto se sostiene solo: **una causa plausible sin
sondear tapó un bug durante meses**.

**Y la doc de Microsoft miente.** Dice textual que `SDC_ALLOW_CHANGES` *"is allowed with any other
valid combination"*. Es **falso**: es ilegal con cualquier `SDC_TOPOLOGY_*`. Solo se descubre
sondando. Un fix guiado por la doc (sacar solo `SAVE_TO_DATABASE`) **habría seguido dando 87**:

| Flags (con `SDC_VALIDATE`) | Status |
|---|---|
| `EXTEND \| ALLOW \| SAVE_DB` ← el código original | **87** |
| `EXTEND \| ALLOW \| PERSIST` ← el "fix" según la doc | **87** |
| `EXTEND \| ALLOW` ← sacando solo el flag "culpable" | **87** |
| `EXTEND \| PERSIST` ← el fix real | 31 (flags OK) |
| `CLONE \| ALLOW` vs `CLONE` | **87** vs 31 → aísla al culpable |

Ofrecido al upstream: [Nuzair46/Monarch#43](https://github.com/Nuzair46/Monarch/pull/43).

---

## ADR-005 — Por qué arreglar los flags no alcanzaba

**Decisión**: el extend queda como **escalón secundario**, detrás del attach explícito.

**Por qué**: `SDC_TOPOLOGY_EXTEND` está documentado como *"requests the last extended configuration
from the persistence database"* — **relee la base, no enumera hardware**. Como el detach se aplica
con `SDC_SAVE_TO_DATABASE`, la última topología extendida guardada ya **no incluye la TV**. Por eso
el fallback `DisplaySwitch.exe /extend` del autor "salía con éxito" y la TV no volvía: hacía
exactamente lo que se le pedía. `SDC_PATH_PERSIST_IF_REQUIRED` puede forzar la persistence de
vuelta, pero es apuesta; el attach explícito es determinista.

**Escalación final**: attach explícito → poll → extend → poll → `DisplaySwitch` → poll → error.
Cada escalón **verificado re-enumerando** (ADR-008).

---

## ADR-006 — Todos los candidatos en un solo apply (batch)

**Decisión**: el batch crece de a un candidato validando con `SDC_VALIDATE` (que es gratis), y
aplica **una sola vez** al final.

**Cicatriz (razonada, no observada)**: la revisión adversarial **dedujo** — de la semántica
documentada de `SDC_USE_SUPPLIED_DISPLAY_CONFIG`, que interpreta el array como la topología
**completa** — que attachear de a uno **habría detacheado** lo anterior: el segundo apply mandaría
la lista de activos vieja (sin el path que el primero acababa de crear) y Windows apagaría el
primero. Nunca se probó de a uno: el batch se implementó antes. Coincidieron 3 lentes por
separado, que es acuerdo entre revisores, no evidencia empírica.

**Por qué batch y no re-consultar entre attaches**: re-consultar implica N flips físicos de
topología en una máquina **sin pantalla de rescate**, y cada flip es una ventana de riesgo. El
batch es 1 validate + 1 apply y **estructuralmente** no puede tirar un path.

---

## ADR-007 — Geometría centinela `0x0` para los displays seedeados

**Decisión**: un display seedeado se siembra con resolución `0x0` explícita; los merges preservan
la geometría real cacheada frente a un centinela; el recovery copia la geometría real post-attach;
`apply_desired_source_mode` ignora un `0x0`.

**Cicatriz**: la primera versión del seeder resolvía la resolución con un lookup por `sourceInfo.id`.
Pero `QDC_ALL_PATHS` **solo trae modes de paths activos** → el seeder le aliaseaba a la TV **la
resolución del monitor primario** (o `0x0`). Ese dato inventado le pisaba la geometría real
cacheada en cada tick del watchdog de topología (**1.8s** — el de 1.2s enumera pero no escribe la
caché) y en cada refresh de la UI, y al attachear se le escribía a la TV → **error 87 o la TV
clonada encima del monitor**. Una revisión lo cazó antes de llegar a la máquina de Guido.

---

## ADR-008 — La escalación no confía en un código de retorno

**Decisión**: **cada escalón de la escalación** (attach → extend → DisplaySwitch) se juzga
**re-enumerando**, nunca por el status.

**Alcance honesto**: no es "nada confía en un status", como afirmaba antes este ADR. El apply final
y el rollback siguen reportando por código de retorno: `recover:restore_ok` (topology.rs) significa
*"Windows aceptó"*, **no** *"la topología volvió"* — si el restore fue un no-op, loguea `ok` igual.
Es deuda conocida, en el escalón menos crítico (el rollback ya es el peor caso).

**Cicatriz**: `attached` salía del status del apply. Pero un `SetDisplayConfig` con el set activo
sin cambios es un **no-op documentado que devuelve 0**, y `SDC_VALIDATE` acepta hasta targets con
`targetAvailable=FALSE`. Un attach fantasma "exitoso" mataba el fallback. Corolario que casi se
nos pasa: al arreglar los flags (ADR-004), el extend puede devolver 0 sin hacer nada — y eso
**mataba el `DisplaySwitch`**, que antes corría solo porque el extend fallaba siempre. Arreglar un
bug reveló que un parche dependía de que estuviera roto.

---

## ADR-009 — El pre-estado es precondición dura, no `Option`

**Decisión**: sin captura del pre-estado (con un retry), **no se toca la topología**: se aborta con
`recover:abort:no_pre_state_captured`. El tipo no es `Option` — el skip silencioso es imposible por
construcción.

**Cicatriz**: (a) el extend incondicional, cuando fallaba, dejaba **todas las pantallas attacheadas
y persistidas** con `SDC_SAVE_TO_DATABASE`: el usuario pedía una cosa y le quedaba el escritorio
cambiado; (b) los rollbacks eran `if let Some(pre) = ...` sin `else` → si la captura fallaba, el
attach riesgoso corría **sin red y sin log**. Es un desktop sin panel interno: la red no es lujo.

---

## ADR-010 — `prepare_attach_targets` es diagnóstico puro

**Decisión**: no toca la topología. Enumera, nombra los outputs no resueltos, loguea por qué son
inusables. Sin extend, sin poll, sin rollback.

**Cicatriz**: tras un resume, Windows reportó transitoriamente un **monitor** (`target_id=4353`)
con `targetAvailable=FALSE`. Al aplicar el perfil "PC" — que **detachea** la TV — Monarch respondía
forzando un extend, que attachea **todo** lo conectado-inactivo (incluida la TV que se quería
apagar), esperaba 3.5s al pedo y terminaba en `apply:sdc_failed:31`.

**El argumento que lo cierra** (más fuerte que el caso): **resolver ≠ estar activo**. Desde que
existe el seeder de `QDC_ALL_PATHS` (ADR-003), todo display conectado-pero-detacheado ya entra al
layout como `is_active=false`. Entonces: si es attacheable, **ya resuelve** → no hay nada que
preparar. Y si no resuelve, es porque **Windows no lo enumera** → ni el attach (que prende el flag
`ACTIVE` de un path que debe preexistir) ni el extend (que relee la DB) pueden inventar hardware
ausente. El hook quedó **vestigial** cuando el seeder lo dejó sin trabajo, y no lo notamos.

**Deuda**: es vestigial y se puede borrar entero en un follow-up. Se dejó porque sus logs son
baratos y valiosos, y borrar un método del trait + 2 impls + plumbing en la última ronda era blast
radius sin ganancia. El log `:candidate_for_unresolved_output:` es el **canario**: si aparece
alguna vez, la prueba de arriba tenía un agujero.

---

## ADR-011 — Cada ronda pasa por revisión adversarial

**Decisión**: implementar → revisión multi-agente por lentes (Win32, concurrencia, lógica,
seguridad del usuario) → verificación adversarial de cada hallazgo → aplicar **solo lo confirmado**.

**Cicatriz**: no es ceremonia, encontró bombas reales que la implementación sola no vio: la
geometría inventada (ADR-007), el extend sin rollback (ADR-009), el attach que detacheaba al
anterior (ADR-006), el status 0 que mentía (ADR-008), y un backup de perfiles que corría **después**
de desinstalar. También refutó cosas: la propuesta de meter el attach explícito en
`prepare_attach_targets` era código muerto (ADR-010), y se descartó **con prueba**, no por opinión.

---

## ADR-012 — Densificar el log del autor, no inventar uno

**La infraestructura es del autor, no nuestra.** `src-tauri/src/diagnostics.rs` — el log siempre
activo en `%APPDATA%\Monarch\diagnostics.log` y la rotación a 512 KB — lo escribió Nuzair46 en
`8b6b9ef`, parte del PR #30. `git diff 8b6b9ef HEAD -- src-tauri/src/diagnostics.rs` sale
**vacío**: no lo tocamos ni una línea. Este ADR antes lo presentaba como decisión propia; no lo es.

**La decisión de este fork es QUÉ se loguea.** El PR #30 tenía 16 llamadas, ninguna con prefijo
`ui_cmd:`/`enum:` ni status de `SetDisplayConfig`. Agregamos: comandos etiquetados por origen
(`ui_cmd:*`), los status de `SetDisplayConfig` con código y contexto, y una línea `enum:` por
enumeración con qué vio, qué seedeó y qué descartó **con su razón** (anti-spam por firma: los
watchdogs enumeran cada 1.2/1.8s).

**Alcance real del logueo de status** (no es "todo", como decía antes): el validate, el attach y el
extend loguean **siempre**, incluido el 0. Los dos `SetDisplayConfig` del apply principal
(`apply.rs`, exact_flags y allow_changes) loguean **solo si el status ≠ 0**. Consecuencia al leer
un log: **la ausencia de `apply:sdc_*` no significa que no se llamó — significa que salió 0.**

**Por qué**: la máquina de campo es **otra PC**. Sin log no hay diagnóstico posible, solo
adivinanza. **Todos** los fixes de acá salieron de esos logs — o sea, de un módulo que escribió el
autor; el root cause del ADR-004 salió de una sola línea (`apply:sdc_failed:87:topology_extend`). Los misterios que quedaban (el error 87
que no aparecía, los applies fantasma 20s post-arranque) se cerraron agregando cámaras, no
teorizando: los applies fantasma eran el `confirm_watchdog` revirtiendo por timeout.

---

## ADR-013 — No renombrar la app (por ahora)

**Decisión**: queda "Monarch (personal)". El nombre propio se decide si algún día se unifica con
Millennium-Clipboard (la suite de tools de Guido; también Tauri 2).

**Por qué**: renombrar cambia `productName` → cambia `%APPDATA%\Monarch` (donde viven los perfiles)
y la identidad de upgrade del MSI. Es una migración con riesgo real de perder datos, a cambio de
cero funcionalidad. La identidad ya está resuelta por la versión y el sufijo (ADR-002).
