# VOLDIQ — DESARROLLO COMPLETO DE LA APLICACIÓN

## 1. ROL Y OBJETIVO

Actúa como un equipo senior de desarrollo de software especializado en:

- Ingeniería de sistemas y programación de alto rendimiento con Rust.
- Desarrollo nativo para Windows.
- Arquitectura de software de escritorio.
- Optimización de CPU, RAM, disco y operaciones de entrada/salida.
- Desarrollo de interfaces con Tauri, Svelte y TypeScript.
- UX/UI para herramientas profesionales de análisis de datos.
- Seguridad de sistemas de archivos.
- Testing, profiling, benchmarking y optimización.

Tu misión es **diseñar, implementar, probar y preparar para distribución una aplicación llamada Voldiq**.

Voldiq será una herramienta de escritorio para Windows dedicada al análisis visual, gestión, exploración y limpieza del almacenamiento del ordenador.

Toma como referencias funcionales WizTree, TreeSize y SpaceSniffer, y como referencia de diseño el lenguaje visual profesional de Palantir, Linear y el `design.md` proporcionado.

No copies ninguna de esas aplicaciones. Queremos un producto original, con personalidad e implementación propias.

**No quiero únicamente una propuesta, una colección de documentos o un prototipo. Quiero que construyas software real, funcional y optimizado.**

---

## 2. FILOSOFÍA DEL PROYECTO

Estas reglas son obligatorias y deben aplicarse durante todo el desarrollo.

### PRIORIDAD 1 — FUNCIONALIDAD

Toda funcionalidad presentada al usuario debe funcionar realmente.

Prohibido:

- Botones sin implementar.
- Acciones simuladas.
- Datos inventados mostrados como información del sistema.
- Gráficos con estadísticas ficticias.
- Limpiezas que solamente muestran mensajes de éxito.
- Escáneres que no analizan realmente el sistema de archivos.
- Funciones marcadas como terminadas sin haber sido probadas.

Si una característica todavía no está implementada, no debe aparentar que está disponible.

### PRIORIDAD 2 — RENDIMIENTO

Voldiq debe ser una aplicación extremadamente ligera, rápida y eficiente.

Optimiza:

- Tiempo de arranque.
- Consumo de RAM.
- Consumo de CPU.
- Operaciones de disco.
- Cantidad de lecturas innecesarias.
- Tamaño del ejecutable.
- Tiempo de escaneo.
- Velocidad de navegación.
- Capacidad de respuesta.
- Renderizado de gráficos.
- Consumo de recursos mientras está inactiva.

El objetivo es que Voldiq pueda permanecer abierta sin representar una carga significativa para el ordenador.

No confundas utilizar Rust con tener automáticamente un buen rendimiento. Toda decisión importante debe justificarse y medirse.

### PRIORIDAD 3 — EXPERIENCIA VISUAL

La interfaz debe ser moderna, minimalista, profesional, atractiva y fácil de entender.

No quiero que parezca una aplicación genérica generada con IA.

Debe parecer un producto real diseñado por un equipo especializado en software profesional.

La calidad visual y la facilidad de uso son requisitos fundamentales, pero nunca deben reducir innecesariamente el rendimiento.

### PRIORIDAD 4 — CÓDIGO PROPIO

Quiero que la mayoría de las funcionalidades diferenciadoras estén implementadas mediante código propio.

Especialmente:

- Motor de escaneo.
- Reconstrucción de árboles de directorios.
- Análisis de archivos.
- Agregación y clasificación.
- Algoritmos de visualización.
- Motor de limpieza.
- Sistema de búsqueda.
- Detección de duplicados.
- Comparación de snapshots.
- Gestión de índices.

Puedes utilizar dependencias gratuitas y consolidadas cuando sean adecuadas.

No debes reinventar sistemas complejos como SQLite, algoritmos criptográficos o infraestructura gráfica básica solamente por evitar dependencias.

Prefiere pocas dependencias, bien mantenidas, auditables y con licencias compatibles.

---

## 3. RESTRICCIONES ABSOLUTAS

- Aplicación inicialmente exclusiva para Windows 10 y Windows 11.
- Arquitectura x86-64 como objetivo inicial.
- Completamente gratuita.
- Sin suscripciones.
- Sin servicios de pago.
- Sin llamadas a APIs comerciales.
- Sin inteligencia artificial externa.
- Sin necesidad de servidores.
- Sin registro de usuario.
- Sin cuenta obligatoria.
- Sin telemetría.
- Sin publicidad.
- Sin conexión a Internet para su funcionamiento.
- Sin dependencia de servicios en la nube.
- Sin permisos de administrador permanentes.
- Sin procesos residentes innecesarios.
- Sin instalar servicios de Windows salvo justificación extraordinaria.

Se permite utilizar herramientas gratuitas durante el desarrollo.

La aplicación deberá funcionar sin conexión una vez instalada.

No añadas ninguna característica PRO, Premium ni ninguna limitación artificial.

La totalidad del código propio deberá poder mantenerse sin depender de licencias comerciales.

---

# 4. STACK TECNOLÓGICO

Utiliza preferentemente:

| Componente | Tecnología |
|---|---|
| Motor principal | Rust estable |
| Framework de escritorio | Tauri 2 |
| Frontend | Svelte 5 + TypeScript |
| Bundler | Vite |
| Integración Windows | windows-rs |
| Base de datos local | SQLite |
| Serialización | Serde |
| Hashing | BLAKE3 |
| Concurrencia CPU | Rayon cuando proceda |
| Operaciones asíncronas | Tokio cuando proceda |
| Visualización principal | Canvas 2D personalizado |
| Estilos | CSS propio con variables y tokens |
| Testing Rust | cargo test |
| Análisis estático | Clippy |
| Distribución | Instalador Windows mediante Tauri |

No añadas dependencias por comodidad sin evaluar su coste.

Evita Electron, motores 3D innecesarios, bibliotecas masivas de gráficos, suites de componentes visuales prefabricadas y frameworks adicionales cuando no aporten un beneficio claro.

Usa WebGL o WebGPU únicamente si las pruebas demuestran una mejora real frente a Canvas 2D para el escenario concreto.

Mantén el frontend completamente separado del motor de análisis.

Rust debe ejecutar toda la lógica intensiva.

Svelte debe encargarse principalmente de representar e interactuar con los datos.

---

# 5. ARQUITECTURA

Diseña una arquitectura modular, mantenible y orientada al rendimiento.

Estructura orientativa:

```text
voldiq/
├── crates/
│   ├── voldiq-core/
│   ├── voldiq-scanner/
│   ├── voldiq-ntfs/
│   ├── voldiq-index/
│   ├── voldiq-analyzer/
│   ├── voldiq-cleaner/
│   ├── voldiq-duplicates/
│   ├── voldiq-snapshots/
│   └── voldiq-windows/
│
├── src-tauri/
│
├── frontend/
│   ├── components/
│   ├── features/
│   ├── layouts/
│   ├── stores/
│   ├── styles/
│   └── utils/
│
├── assets/
├── docs/
├── benchmarks/
├── tests/
└── README.md
```

Puedes simplificar esta estructura si una división resulta artificial.

No crees abstracciones innecesarias ni una arquitectura excesivamente fragmentada.

Principios:

1. Separación clara de responsabilidades.
2. Dependencias mínimas entre módulos.
3. Código Rust reutilizable sin Tauri.
4. Gestión explícita de errores.
5. Recursos liberados correctamente.
6. Operaciones cancelables.
7. Código fácilmente verificable.
8. Ningún estado global mutable injustificado.
9. Seguridad en operaciones sobre archivos.
10. Pruebas independientes del frontend.

El motor de Voldiq deberá poder evolucionar posteriormente hacia una interfaz CLI sin tener que reescribir la lógica principal.

---

# 6. MOTOR DE ESCANEO — COMPONENTE MÁS IMPORTANTE

Este es el componente técnico central del proyecto.

Debe priorizarse antes de implementar funcionalidades avanzadas.

## 6.1 Detección de unidades

Detecta automáticamente las unidades del ordenador.

Obtén, cuando esté disponible:

- Identificador y letra de unidad.
- Nombre del volumen.
- Sistema de archivos.
- Capacidad total.
- Espacio utilizado.
- Espacio libre.
- Tipo de unidad.
- Estado de accesibilidad.
- Características relevantes para el escaneo.

Utiliza APIs nativas de Windows.

Distingue entre unidades NTFS, otros sistemas locales y unidades de red.

No asumas que todas pueden escanearse mediante el mismo método.

## 6.2 Escaneo NTFS de alto rendimiento

Implementa un motor especializado en NTFS.

Investiga y utiliza adecuadamente, cuando proceda:

- Master File Table.
- USN Journal.
- FSCTL_ENUM_USN_DATA.
- FSCTL_QUERY_USN_JOURNAL.
- FSCTL_READ_USN_JOURNAL.
- DeviceIoControl.
- Identificadores de archivo y directorio.
- Metadatos NTFS.

Es fundamental entender las limitaciones de cada API.

La enumeración mediante USN proporciona registros e identificadores, pero no debes asumir que contiene todos los tamaños y metadatos necesarios.

Diseña el procedimiento real para obtener esos datos de forma eficiente y correcta.

El sistema deberá:

1. Detectar las capacidades del volumen.
2. Determinar el método óptimo disponible.
3. Obtener los registros de archivos.
4. Reconstruir la jerarquía de directorios.
5. Resolver correctamente relaciones padre-hijo.
6. Obtener y agregar tamaños.
7. Construir un índice consultable.
8. Publicar resultados progresivamente.
9. Identificar elementos inaccesibles o incompletos.
10. Guardar los resultados cuando sea apropiado.

El código que interpreta datos binarios deberá verificar versiones, tamaños, offsets y límites antes de acceder a ellos.

Mantén el uso de `unsafe` limitado a las fronteras FFI imprescindibles y encapsúlalo correctamente.

No debe existir ninguna posibilidad razonable de corrupción de memoria por interpretar registros malformados.

## 6.3 Escáner alternativo

Si el método rápido no está disponible, utiliza un escáner convencional optimizado.

Características obligatorias:

- Recorrido iterativo.
- Evitar recursividad profunda innecesaria.
- Concurrencia controlada.
- Minimización de operaciones de I/O.
- Detección de errores.
- Cancelación.
- Evitar bucles mediante enlaces.
- Tolerancia a directorios inaccesibles.
- Posibilidad de continuar aunque algunos archivos fallen.

No utilices paralelismo máximo indiscriminadamente.

Adapta el grado de concurrencia al tipo de almacenamiento y a los resultados de los benchmarks.

Un HDD puede comportarse peor con un número excesivo de lecturas simultáneas.

## 6.4 Tamaños y precisión

Distingue claramente:

- Tamaño lógico.
- Espacio asignado físicamente cuando pueda conocerse.
- Tamaño agregado de directorios.
- Espacio utilizado comunicado por el sistema de archivos.
- Espacio no atribuible completamente a archivos visibles.

Gestiona correctamente, en la medida que permita el sistema:

- Hard links.
- Reparse points.
- Junctions.
- Symbolic links.
- Archivos dispersos.
- Archivos comprimidos.
- Archivos bloqueados.
- Archivos especiales.
- Archivos de solo lectura.
- Directorios protegidos.
- OneDrive y archivos cloud-only.
- Unidades desmontadas durante el escaneo.

Evita sumar dos veces los mismos datos físicos.

No confundas el tamaño lógico acumulado con la ocupación real del volumen.

Cuando una métrica sea una estimación, muéstrala como tal.

Nunca ocultes silenciosamente errores que afecten a la exactitud de los resultados.

## 6.5 Escaneo incremental

Implementa un sistema de actualización incremental cuando resulte técnicamente viable.

Aprovecha USN Journal para detectar cambios posteriores al análisis inicial.

Valida:

- Identificador del journal.
- Continuidad de los registros.
- Pérdida de eventos.
- Cambios de volumen.
- Eliminaciones.
- Renombrados.
- Movimientos.
- Modificaciones de tamaño.

Cuando el índice ya no pueda garantizarse válido, realiza una resincronización.

No muestres índices antiguos como si representaran el estado actual sin indicarlo.

## 6.6 Progreso

Muestra el avance real de las operaciones.

El usuario deberá poder:

- Iniciar un análisis.
- Cancelarlo.
- Seleccionar una unidad.
- Seleccionar una carpeta.
- Consultar los resultados parciales.
- Repetir el análisis.

Si no puede calcularse un porcentaje fiable, utiliza progreso indeterminado acompañado de métricas reales, como número de archivos procesados.

Nunca simules porcentajes.

---

# 7. OPTIMIZACIÓN DE MEMORIA Y CPU

El sistema debe funcionar con millones de archivos.

No envíes árboles completos gigantescos al frontend.

Mantén el índice principal en Rust y permite consultas parciales.

Implementa operaciones semejantes a:

```text
get_volumes()
start_scan(volume)
cancel_scan(scan_id)
get_scan_status(scan_id)

get_directory_children(id, pagination)
get_directory_stats(id)
get_largest_files(scope, limit)
get_file_type_distribution(scope)
get_treemap_nodes(scope, viewport)

search_files(query, filters, pagination)
get_file_details(file_id)
```

Diseña los contratos tipados adecuados.

Utiliza:

- IDs compactos.
- Estructuras de datos eficientes.
- Paginación.
- Agrupaciones precalculadas cuando resulte útil.
- Cachés de tamaño limitado.
- Virtualización.
- Consultas bajo demanda.
- Límites explícitos de memoria.
- Actualizaciones agrupadas.

Evita asignaciones repetitivas innecesarias.

Evita guardar copias completas de las mismas estructuras en Rust, SQLite y JavaScript simultáneamente.

Mide las estructuras de datos antes de optimizarlas.

Para comunicar resultados al frontend utiliza los mecanismos apropiados de Tauri 2, incluidos channels para operaciones de streaming.

No satures la aplicación enviando cientos de eventos visuales por segundo.

Controla la frecuencia de actualizaciones de progreso.

---

# 8. INTERFAZ — IDENTIDAD VISUAL DE VOLDIQ

## Dirección artística

Quiero una interfaz inspirada en el carácter funcional y profesional de Palantir, adaptada a una herramienta de almacenamiento.

También debes utilizar como referencia el `design.md` proporcionado.

La interfaz debe ser:

- Principalmente blanca.
- Minimalista.
- Elegante.
- Profesional.
- Moderna.
- Precisa.
- Visualmente equilibrada.
- Fácil de comprender.
- Rica en información útil.
- Nada recargada.

**Evita absolutamente el aspecto de aplicación genérica creada con IA.**

No quiero:

- Cards gigantes sin necesidad.
- Bordes excesivamente redondeados.
- Botones tipo píldora repetidos.
- Badges decorativos.
- Iconos de gran tamaño sin justificación.
- Gradientes innecesarios.
- Sombras exageradas.
- Textos explicativos redundantes.
- Paneles duplicados con la misma información.
- Gráficos de relleno.
- Exceso de colores.
- Separaciones desproporcionadas.
- Pantallas artificialmente vacías.

No conviertas la aplicación en un dashboard de marketing.

## Sistema visual

Utiliza como base:

```css
--background: #ffffff;
--surface: #fafafa;
--surface-secondary: #f5f5f5;

--border: #e5e5e5;
--border-strong: #d4d4d4;

--text-primary: #171717;
--text-secondary: #525252;
--text-muted: #737373;

--accent-primary: #2563eb;
--accent-hover: #1e40af;
```

Los colores anteriores son orientativos y pueden ajustarse para mejorar el resultado final.

Usa colores adicionales para representar categorías de archivos cuando realmente aporten información.

Por ejemplo, azul, ámbar, verde y violeta en el treemap, con tonos cuidadosamente equilibrados.

El color debe ayudar a interpretar los datos, no decorar toda la pantalla.

Tipografía:

- Preferir Segoe UI Variable cuando esté disponible.
- Mantener fallback adecuado para Windows 10.
- Permitir fuentes abiertas autoalojadas si ofrecen una mejora demostrable.
- Evitar descargar fuentes en tiempo de ejecución.
- Utilizar cifras tabulares en columnas numéricas.

Bordes finos, radios discretos, controles compactos, alineaciones precisas y jerarquía tipográfica clara.

---

# 9. REDISEÑO DE LA NAVEGACIÓN

La navegación merece especial atención.

No quiero la típica sidebar enorme de una aplicación hecha con IA.

Implementa una estructura integrada en la ventana.

## Comportamiento propuesto

Una barra vertical compacta con iconos, acompañada de un panel de navegación desplegable.

La barra principal debe permitir acceder a:

- Resumen.
- Explorador.
- Limpieza.
- Duplicados.
- Archivos grandes.

Configuración debe permanecer en una posición secundaria.

El panel con etiquetas debe poder expandirse y contraerse.

No mantengas permanentemente dos barras laterales completas consumiendo espacio.

Cuando esté contraído, la mayor parte del espacio horizontal se reserva para los datos.

Cuando esté expandido, debe proporcionar contexto adicional sin resultar invasivo.

La selección activa deberá indicarse discretamente.

Implementa:

- Tooltips.
- Navegación por teclado.
- Restauración del estado anterior.
- Comportamiento correcto al redimensionar.
- Transiciones breves.
- Accesibilidad mediante teclado.

Añade una barra superior integrada con:

- Selector de unidad.
- Ruta de navegación cuando corresponda.
- Búsqueda.
- Acción de reescaneo.
- Controles contextuales.

Toda la aplicación debe compartir una estructura visual coherente.

---

# 10. PANTALLA PRINCIPAL

La pantalla inicial debe permitir comprender el estado del almacenamiento inmediatamente.

No debe exigir varios clics para obtener información básica.

## Información

Mostrar:

- Unidad seleccionada.
- Capacidad total.
- Espacio utilizado.
- Espacio disponible.
- Porcentaje de ocupación.
- Fecha y estado del último análisis.
- Directorios que ocupan más espacio.
- Distribución visual del almacenamiento.

Una única franja compacta de información debería sustituir a varias tarjetas redundantes.

El elemento central será el treemap.

El objetivo es que el usuario pueda identificar los principales consumidores de espacio en pocos segundos.

## Representación de datos

El treemap debe ser visualmente atractivo, pero también preciso.

No utilices colores llamativos indiscriminadamente.

Permite pasar directamente del treemap al explorador.

La información adicional aparecerá de forma contextual.

No muestres un gráfico histórico cuando todavía no existan snapshots reales.

No inventes categorías ni agrupes incorrectamente rutas distintas solo para que el gráfico parezca bonito.

---

# 11. TREEMAP PERSONALIZADO

Implementa nuestro propio componente de treemap.

No dependas de bibliotecas pesadas de visualización si podemos crear una implementación eficiente.

Requisitos:

- Rectángulos proporcionales al tamaño.
- Jerarquía de directorios.
- Colores consistentes.
- Renderizado rápido.
- Selección.
- Hover.
- Tooltip contextual.
- Zoom dentro de carpetas.
- Navegación hacia atrás.
- Breadcrumb.
- Visualización de tamaños.
- Sincronización con la lista de archivos.
- Cambio de unidad.
- Filtros.
- Adaptación al tamaño de ventana.

Investiga e implementa un algoritmo de treemap adecuado, como squarified treemap, con gestión de rectángulos pequeños.

No dibujes etiquetas cuando no exista espacio suficiente.

Agrupa visualmente los elementos menores cuando sea necesario, manteniendo una manera de explorarlos.

Los colores deben permanecer estables entre análisis para evitar cambios visuales injustificados.

Utiliza un canvas con escalado correcto para pantallas HiDPI.

La interacción debe continuar siendo fluida con índices de millones de archivos.

No significa dibujar millones de rectángulos simultáneamente: implementa reducción de detalle, selección de nodos visibles y navegación jerárquica.

Mantén la accesibilidad a la información mediante una vista alternativa de lista.

---

# 12. EXPLORADOR DE ARCHIVOS

Implementa un explorador integrado enfocado al análisis de almacenamiento.

No pretende sustituir completamente al Explorador de Windows.

Debe permitir:

- Navegar por directorios.
- Consultar tamaños.
- Ordenar por tamaño.
- Ordenar por nombre.
- Filtrar extensiones.
- Buscar.
- Seleccionar elementos.
- Ver detalles.
- Abrir ubicaciones en el Explorador de Windows.
- Copiar rutas.
- Inspeccionar subdirectorios.
- Consultar fechas.
- Ver la distribución por tipos.

La tabla debe estar virtualizada.

Evita crear miles de filas DOM simultáneamente.

La selección de una carpeta en el treemap debe reflejarse en el explorador.

La navegación debe sentirse inmediata después del análisis.

---

# 13. DETECCIÓN DE ARCHIVOS GRANDES

Implementa una pantalla para encontrar archivos y carpetas que ocupan mucho espacio.

Características:

- Ranking por tamaño.
- Filtro por unidad.
- Filtro por directorio.
- Filtro por extensión.
- Filtro por tamaño mínimo.
- Filtro por fecha.
- Ordenación.
- Búsqueda.
- Apertura de ubicación.
- Selección múltiple.
- Acciones seguras de gestión.

Incluye filtros de archivos antiguos, pero no asumas que un archivo no modificado recientemente es innecesario.

El análisis debe utilizar los datos indexados existentes siempre que sea posible.

Evita reescanear el disco innecesariamente.

---

# 14. DETECCIÓN DE DUPLICADOS

Implementa un detector de duplicados real.

La prioridad es minimizar lecturas y evitar falsos positivos.

Flujo orientativo:

1. Agrupar por tamaño.
2. Descartar archivos con tamaños únicos.
3. Aplicar huellas parciales cuando resulte útil.
4. Calcular hashes completos BLAKE3.
5. Confirmar identidad de contenido cuando la operación lo requiera.
6. Agrupar resultados.
7. Calcular espacio potencialmente recuperable.
8. Presentar opciones de gestión.

Evita releer archivos si existen hashes válidos cuyo estado pueda verificarse.

No proceses el contenido completo de archivos innecesariamente.

Gestiona:

- Archivos bloqueados.
- Cambios durante el análisis.
- Enlaces duros.
- Archivos idénticos en distintas unidades.
- Errores de lectura.
- Cancelación.
- Progreso.

Nunca selecciones automáticamente todos los archivos de un grupo para borrarlos.

Permite mantener al menos una copia y revisar la selección antes de eliminar.

La estimación de espacio recuperable debe considerar la realidad física del almacenamiento y evitar contar incorrectamente enlaces al mismo contenido.

---

# 15. MOTOR DE LIMPIEZA

Este componente tiene que ser especialmente seguro.

No quiero una herramienta que borre archivos indiscriminadamente para anunciar que ha liberado muchos gigabytes.

Implementa un sistema de reglas de limpieza propias.

Cada regla debe especificar:

- Identificador.
- Nombre.
- Descripción.
- Ubicación o método de descubrimiento.
- Criterios de selección.
- Exclusiones.
- Nivel de riesgo.
- Requisitos de permisos.
- Política de eliminación.
- Procedimiento de verificación.

## Categorías iniciales

Implementa las que puedan limpiarse correctamente:

- Archivos temporales del usuario.
- Cachés compatibles de aplicaciones.
- Cachés de navegadores cuando puedan gestionarse de forma segura.
- Papelera de reciclaje.
- Cachés de desarrollo seleccionadas.
- Archivos temporales de proyectos.
- Miniaturas o cachés reconstruibles cuando proceda.
- Archivos antiguos seleccionados manualmente.

No borres indiscriminadamente:

- Descargas.
- Documentos.
- Carpetas personales.
- Bases de datos.
- Archivos de configuración.
- Volúmenes Docker.
- Archivos de guardado de juegos.
- Perfiles de navegadores.
- Datos de sesiones.
- Archivos del sistema.
- Componentes de Windows Update.

La limpieza de componentes internos de Windows deberá delegarse a mecanismos oficiales y documentados cuando sea necesario.

No implementes borrados directos de WinSxS, Windows Installer o directorios protegidos como alternativa a esos mecanismos.

No incluyas un limpiador del registro de Windows como supuesta herramienta de aceleración.

## Niveles

Clasifica las acciones:

**Bajo riesgo:** archivos temporales verificables que no estén siendo utilizados.

**Revisión necesaria:** cachés que podrían afectar sesiones, proyectos o datos regenerables de alto coste.

**Avanzado:** operaciones con mayor impacto o consecuencias potencialmente irreversibles.

No afirmes que una operación es totalmente segura si no puedes garantizarlo.

## Ejecución

Antes de cualquier eliminación:

1. Obtener los candidatos reales.
2. Comprobar exclusiones.
3. Verificar sus rutas.
4. Verificar que los archivos siguen siendo los esperados.
5. Mostrar el espacio estimado.
6. Presentar vista previa.
7. Solicitar confirmación.
8. Ejecutar.
9. Registrar resultados.
10. Actualizar las estadísticas.

Usa la Papelera de Windows para eliminaciones manuales cuando sea adecuado.

Distingue claramente entre mover a la Papelera y borrar permanentemente.

No sigas junctions, enlaces simbólicos ni rutas redirigidas durante operaciones destructivas sin una política explícita.

Evita condiciones de carrera y ataques mediante cambios de ruta entre la comprobación y la eliminación.

Registra fallos parciales sin declarar éxito total.

Nunca borres datos automáticamente al abrir la aplicación.

---

# 16. LIMPIEZA PARA DESARROLLADORES

Añade un apartado especializado en detectar espacio ocupado por herramientas de desarrollo.

Elementos posibles:

- Cargo build targets.
- Cachés Cargo.
- node_modules.
- Cachés npm.
- Cachés pnpm.
- Cachés de Python.
- Directorios build y dist.
- Cachés .NET.
- Artefactos temporales de compilación.
- Cachés Gradle.
- Datos de Docker.

No todos estos elementos son basura.

No debes tratarlos como archivos eliminables de forma automática.

Distingue entre:

- Dependencias instaladas.
- Caché reconstruible.
- Artefactos de compilación.
- Datos persistentes.
- Volúmenes.
- Imágenes.
- Contenedores.
- Archivos de proyectos.

Para Docker, prioriza integración mediante comandos o APIs oficiales compatibles, si están disponibles localmente.

No borres sus carpetas internas directamente.

Permite analizar cuánto espacio ocupa cada categoría sin eliminar nada.

Exige confirmación explícita para acciones potencialmente destructivas.

---

# 17. SNAPSHOTS E HISTORIAL

Implementa la posibilidad de guardar estados del almacenamiento.

El usuario debe poder comparar análisis reales.

Ejemplo:

```text
Análisis anterior: 430 GB
Análisis actual:   462 GB

Diferencia:        +32 GB
```

El sistema debe poder identificar qué directorios han aumentado o disminuido.

Requisitos:

- Snapshots locales.
- Fecha de captura.
- Unidad asociada.
- Comparación de directorios.
- Cambios de tamaño.
- Archivos añadidos.
- Archivos eliminados, cuando sea posible determinarlo.
- Historial de crecimiento.
- Gestión del almacenamiento ocupado por los propios snapshots.

Utiliza SQLite u otro formato local justificado.

Evita duplicar índices completos cuando exista una representación incremental más eficiente.

La comparación debe funcionar incluso después de reiniciar la aplicación.

No presentes datos históricos ficticios.

---

# 18. BÚSQUEDA

Quiero un buscador rápido y bien integrado.

Incluye búsqueda global mediante `Ctrl + K`.

Debe permitir encontrar archivos y carpetas y acceder a acciones relevantes.

El motor de búsqueda debe ejecutarse en Rust.

Utiliza índices existentes siempre que sea posible.

Características:

- Resultados progresivos.
- Búsqueda por nombre.
- Búsqueda por extensión.
- Filtros por tamaño.
- Filtros por ubicación.
- Ordenación.
- Navegación con teclado.
- Apertura de ubicación.
- Cancelación de consultas costosas.

Evita procesos de búsqueda que bloqueen la interfaz.

No recalcules información ya indexada para cada pulsación.

---

# 19. ANIMACIONES Y MICROINTERACCIONES

Las animaciones deben mejorar la experiencia, no añadir decoración.

Implementa:

- Transición discreta al expandir navegación.
- Selección de elementos.
- Aparición contextual de paneles.
- Zoom del treemap.
- Transición entre jerarquías.
- Estados de hover.
- Indicadores de progreso.
- Estados de carga.
- Confirmaciones de acciones.
- Transiciones de tablas y filtros cuando realmente ayuden.

Preferir:

- Duraciones breves.
- Movimiento natural.
- Cambios de opacidad.
- Transformaciones ligeras.
- Interpolaciones suaves.

Evitar:

- Spring animations exageradas.
- Rebotes.
- Efectos elásticos innecesarios.
- Efectos glassmorphism.
- Animaciones constantes.
- Elementos que se mueven sin motivo.
- Desenfoques pesados.
- Animaciones que interfieren con los datos.

Usar técnicas de renderizado eficientes.

Respetar `prefers-reduced-motion`.

No mantener bucles de animación cuando no haya nada que actualizar.

Objetivo: transiciones fluidas en equipos normales sin introducir consumo significativo de CPU o GPU.

---

# 20. CONFIGURACIÓN

Implementa un panel de ajustes sencillo.

Opciones relevantes:

- Unidad predeterminada.
- Rutas excluidas.
- Modo de escaneo.
- Límite de concurrencia.
- Preferencias de visualización.
- Preferencia de tamaño lógico o asignado.
- Tema claro y oscuro.
- Animaciones reducidas.
- Gestión de snapshots.
- Gestión del índice.
- Comportamiento de confirmaciones.
- Unidades o rutas analizadas recientemente.

El modo claro debe ser el principal y estar especialmente cuidado.

El modo oscuro, si se implementa, debe tener diseño propio coherente, no ser una inversión automática de colores.

Guarda la configuración localmente.

---

# 21. LOGOTIPO Y ASSETS

El nombre definitivo de la aplicación es:

# VOLDIQ

Utiliza el logotipo proporcionado con el proyecto.

Su símbolo está compuesto por:

- Una estructura de almacenamiento apilado.
- Una superficie superior con apariencia de CD y agujero central.
- Un pequeño segmento azul.
- Base oscura sobria.

No sustituyas el símbolo por un icono genérico de base de datos.

No redibujes incorrectamente su geometría.

Utiliza los assets disponibles para:

- Icono de la ventana.
- Icono del ejecutable.
- Barra de tareas.
- Menú de inicio.
- Instalador.
- Identidad visual de la aplicación.

Si los archivos gráficos proporcionados necesitan adaptaciones técnicas para Windows, genera esas adaptaciones preservando el diseño.

Usa un icono multirresolución `.ico` y versiones PNG adecuadas.

Si los assets originales no están presentes en el repositorio, deja claramente identificada la dependencia y utiliza un recurso provisional discreto; no afirmes haber incorporado el logo definitivo.

---

# 22. SEGURIDAD

Voldiq interactuará con archivos personales y del sistema.

La seguridad es un requisito crítico.

Implementa:

- Principio de mínimo privilegio.
- Validación de rutas.
- Tratamiento de enlaces y reparse points.
- Exclusiones de directorios críticos.
- Confirmación de operaciones destructivas.
- Gestión de permisos.
- Protección ante borrados accidentales.
- Tratamiento de archivos bloqueados.
- Registro de errores.
- Manejo de cancelaciones.
- Comprobaciones antes de eliminar.
- Separación entre lectura y escritura.

No ejecutes toda la aplicación como administrador.

Si alguna operación concreta necesita elevación, limita el privilegio a esa operación y explica al usuario por qué es necesario.

Evita comandos shell construidos mediante concatenación de entradas no confiables.

En Tauri, expón únicamente comandos y permisos necesarios.

No habilites acceso arbitrario al sistema de archivos desde el frontend.

No cargues recursos remotos de manera innecesaria.

Evita introducir funcionalidades que supongan un riesgo desproporcionado respecto a su utilidad.

---

# 23. ALMACENAMIENTO LOCAL

Utiliza SQLite para índices persistentes y snapshots cuando sea adecuado.

Optimiza:

- Escrituras agrupadas.
- Índices SQL.
- Tamaño de la base de datos.
- Consultas paginadas.
- Actualizaciones incrementales.
- Eliminación de datos obsoletos.
- Migraciones.
- Recuperación ante interrupciones.

No hagas escrituras individuales por cada archivo si eso perjudica gravemente el rendimiento.

Tampoco guardes todos los metadatos posibles sin una razón funcional.

Define políticas claras para los índices obsoletos y su reconstrucción.

El cierre inesperado de la aplicación no debe provocar una base de datos inutilizable.

---

# 24. METAS DE RENDIMIENTO

Establece métricas medibles.

Los siguientes valores son objetivos iniciales de ingeniería, no garantías independientes del hardware.

### Arranque

Objetivo de arranque en caliente inferior a 2 segundos en un PC moderno, sin incluir análisis completo del disco.

### Interfaz

Objetivo de 60 FPS durante interacciones habituales.

Las operaciones del sistema de archivos no deben bloquear el hilo de interfaz.

### Memoria

Buscar un consumo en reposo muy contenido, idealmente alrededor de 150–200 MB o menos para el conjunto de procesos principales en equipos de referencia, si resulta viable con WebView2.

Durante los análisis, establecer presupuestos de memoria basados en benchmarks y documentar los casos en los que sea necesario superarlos.

### CPU

Consumo mínimo en reposo.

No implementar polling continuo cuando existen mecanismos basados en eventos.

### Disco

Minimizar lecturas redundantes.

Reutilizar índices válidos.

Controlar la concurrencia para no degradar innecesariamente el rendimiento del resto del equipo.

### Treemap

Renderizar exclusivamente la información necesaria en el viewport.

Evitar recalcular geometrías completas durante cada hover.

### Búsqueda

Latencia baja sobre índices ya construidos.

Virtualizar resultados.

### Escaneo

Medir tiempo por unidad y número de archivos, y comparar el motor NTFS especializado con el escáner genérico.

No prometas igualar WizTree sin benchmarks reales.

Si se detecta un cuello de botella, analiza primero su causa antes de introducir optimizaciones especulativas.

---

# 25. TESTING Y BENCHMARKS

No consideres terminado un módulo simplemente porque compila.

Implementa pruebas para:

- Reconstrucción del árbol.
- Cálculo de tamaños.
- Hard links.
- Symbolic links.
- Junctions.
- Directorios profundos.
- Nombres Unicode.
- Rutas largas de Windows.
- Archivos inaccesibles.
- Archivos dispersos.
- Archivos eliminados durante un escaneo.
- Cancelación.
- Indexación.
- Consultas paginadas.
- Comparación de snapshots.
- Hashing.
- Detección de duplicados.
- Exclusiones.
- Reglas de limpieza.
- Prevención de borrados fuera de rutas permitidas.

Crea fixtures de prueba que no dependan de archivos personales.

Las pruebas destructivas deben ejecutarse exclusivamente en directorios temporales controlados.

Añade benchmarks para:

- Enumeración de archivos.
- Agregación de tamaños.
- Reconstrucción del árbol.
- Serialización.
- Consultas.
- Memoria del índice.
- Generación del treemap.
- Detección de duplicados.
- Inicio de la aplicación.

Prueba perfiles de carga pequeños, medianos y grandes.

Si no dispones de un entorno Windows real para ejecutar determinadas pruebas, indícalo expresamente y prepara una batería de validación reproducible para Windows.

---

# 26. DOCUMENTACIÓN

Mantén la documentación técnica dentro del repositorio.

Archivos esperados:

```text
README.md
docs/ARCHITECTURE.md
docs/DESIGN.md
docs/SECURITY.md
docs/PERFORMANCE.md
docs/TESTING.md
NEXT_TODOS.md
```

### ARCHITECTURE.md

Explica la arquitectura y los límites de cada módulo.

### DESIGN.md

Documenta el sistema visual definitivo.

No te limites a copiar el documento de referencia. Adáptalo a Voldiq y registra las decisiones adoptadas.

### SECURITY.md

Documenta los permisos y las garantías del sistema de limpieza.

### PERFORMANCE.md

Incluye benchmarks, hardware de referencia, metodología, cuellos de botella y mejoras.

### TESTING.md

Instrucciones reproducibles para validar el sistema.

### NEXT_TODOS.md

Mantén un seguimiento real de las tareas.

Cada tarea debe tener:

- ID.
- Nombre.
- Prioridad.
- Estado.
- Dependencias.
- Criterios de aceptación.
- Archivos relacionados cuando proceda.

Estados permitidos:

```text
TODO
IN_PROGRESS
BLOCKED
DONE
```

Marca DONE únicamente cuando los criterios de aceptación estén satisfechos.

No elimines tareas pendientes para hacer aparentar que el proyecto está terminado.

---

# 27. PLAN DE IMPLEMENTACIÓN

Trabaja por fases, pero construye funcionalidades reales desde el principio.

## FASE 1 — Base técnica

- Inspeccionar el repositorio.
- Configurar Rust.
- Configurar Tauri.
- Configurar Svelte y TypeScript.
- Crear arquitectura modular.
- Establecer sistema de errores.
- Configurar testing.
- Configurar documentación.
- Generar una ventana Windows funcional.

**Resultado exigido:** aplicación compilable y ejecutable.

## FASE 2 — Motor de análisis

- Detección de unidades.
- Escáner genérico.
- Reconstrucción del árbol.
- Cálculo de tamaños.
- Gestión de errores.
- Cancelación.
- Índice inicial.
- Benchmarks.

**Resultado exigido:** análisis real de archivos con resultados consultables.

## FASE 3 — Motor NTFS optimizado

- Integración Windows.
- Estrategia de enumeración NTFS.
- Obtención de tamaños.
- Reconstrucción eficiente.
- Manejo de permisos.
- Validación.
- Pruebas comparativas.
- Posteriormente, análisis incremental.

**Resultado exigido:** escaneo NTFS especializado, medido y con fallback.

## FASE 4 — Interfaz principal

- Estructura visual.
- Navegación desplegable.
- Selector de disco.
- Indicadores.
- Treemap propio.
- Paneles de análisis.
- Explorador.
- Búsqueda.

**Resultado exigido:** análisis visual completamente conectado al motor Rust.

## FASE 5 — Gestión del almacenamiento

- Archivos grandes.
- Clasificación.
- Filtros.
- Duplicados.
- Selección.
- Operaciones seguras sobre archivos.

**Resultado exigido:** herramientas útiles y realmente operativas.

## FASE 6 — Limpieza

- Motor de reglas.
- Exclusiones.
- Escaneo de candidatos.
- Vista previa.
- Confirmación.
- Ejecución.
- Registro.
- Limpieza especializada para desarrolladores.

**Resultado exigido:** sistema de limpieza funcional y seguro.

## FASE 7 — Historial

- Snapshots.
- Persistencia.
- Comparaciones.
- Crecimiento del almacenamiento.
- Actualización incremental cuando esté disponible.

## FASE 8 — Pulido final

- Optimización visual.
- Animaciones profesionales.
- Accesibilidad.
- Ajustes de interfaz.
- Rendimiento.
- Testing.
- Corrección de errores.
- Preparación del instalador.
- Validación de ejecución sin Internet.

No desarrolles simultáneamente todos los módulos dejando funciones incompletas.

Cada fase debe proporcionar resultados verificables.

---

# 28. CRITERIOS DE ACEPTACIÓN

Voldiq no estará terminada hasta que se pueda:

1. Instalar y abrir en Windows sin herramientas de desarrollo.
2. Detectar unidades reales.
3. Escanear una unidad o carpeta.
4. Consultar datos reales de ocupación.
5. Navegar visualmente por el treemap.
6. Seleccionar carpetas y ver sus detalles.
7. Buscar archivos.
8. Encontrar archivos grandes.
9. Detectar duplicados reales.
10. Revisar candidatos de limpieza.
11. Ejecutar eliminaciones permitidas con confirmación.
12. Cancelar operaciones largas.
13. Recuperarse de errores de acceso sin bloquearse.
14. Guardar y comparar snapshots.
15. Mantener la interfaz operativa durante el análisis.
16. Funcionar sin conexión a Internet.
17. Utilizar correctamente el icono de Voldiq.
18. Ejecutar las pruebas principales sin fallos conocidos.
19. Producir una compilación de distribución.
20. Disponer de documentación y benchmarks reproducibles.

La aplicación debe ofrecer estados vacíos reales cuando todavía no existan análisis.

No inventar información para rellenar pantallas.

---

# 29. REGLAS PARA LA TOMA DE DECISIONES

Durante el desarrollo aplica los siguientes criterios:

**Si una característica puede implementarse de dos maneras, prioriza la que ofrezca mejor equilibrio entre corrección, rendimiento, mantenimiento y seguridad.**

Si una dependencia añade mucho peso y aporta poco valor, no la utilices.

Si una animación perjudica el rendimiento, optimízala o elimínala.

Si una funcionalidad resulta peligrosa, limita su alcance y exige confirmación.

Si una optimización reduce la exactitud, expón esa limitación claramente o no la implementes.

Si una interfaz contiene información redundante, simplifícala.

Si una API de Windows permite obtener datos de forma más eficiente, evalúa utilizarla.

Si necesitas privilegios adicionales, solicítalos únicamente para la operación que los necesita.

Si el código se vuelve excesivamente complejo, simplifica la arquitectura sin sacrificar la calidad.

No utilices soluciones temporales como implementación definitiva sin documentarlo.

---

# 30. INSTRUCCIONES FINALES DE EJECUCIÓN

Antes de modificar archivos:

1. Analiza la estructura del repositorio.
2. Identifica los assets y documentos de referencia.
3. Comprueba el entorno de desarrollo disponible.
4. Determina qué componentes existen.
5. Decide una arquitectura inicial razonable.
6. Registra el plan en NEXT_TODOS.md.

Después comienza la implementación.

No te quedes indefinidamente en la planificación.

No generes únicamente documentación.

Escribe el código.

Ejecuta compilaciones y pruebas cuando el entorno lo permita.

Verifica los resultados.

Cuando una implementación falle, investiga y corrige la causa.

No sustituyas funcionalidades complejas por simulaciones para presentar avances visuales.

Al terminar cada fase, comunica:

- Qué se ha implementado.
- Qué funciona realmente.
- Qué pruebas se han ejecutado.
- Qué rendimiento se ha medido.
- Qué limitaciones existen.
- Qué falta por desarrollar.

Si la sesión o el contexto se agotan antes de completar Voldiq, deja el proyecto en un estado compilable siempre que sea posible y documenta con exactitud la siguiente tarea.

No declares el proyecto completo antes de haber satisfecho sus criterios de aceptación.

# OBJETIVO FINAL

Construir **Voldiq**, una aplicación de análisis y gestión de almacenamiento para Windows que combine:

**La velocidad de una herramienta nativa de alto rendimiento.**

**La claridad de un explorador visual de archivos.**

**La utilidad de una herramienta profesional de limpieza.**

**La calidad visual de un producto de software cuidadosamente diseñado.**

Todo ello con un motor predominantemente propio en Rust, una interfaz excepcionalmente cuidada, bajo consumo de recursos y funcionamiento completamente gratuito y local.

La aplicación debe sentirse rápida, ligera y precisa desde el primer segundo.

**No quiero una demostración de concepto. Quiero construir una herramienta de escritorio real, fiable y profesional.**

Comienza examinando el repositorio y continúa con la implementación siguiendo el orden de prioridades establecido.