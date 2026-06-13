# Megaprompt: ARGUS Improvement Research (June 2026)

> **Para:** Perplexity AI (Research mode, con búsqueda web habilitada)
> **Objetivo:** investigar mejoras accionables para ARGUS, un proyecto Rust existente
> **Output esperado:** reporte estructurado con recomendaciones priorizadas, evidencia con links, y código de ejemplo donde aplique

---

## CONTEXTO DE ARGUS (lee esto primero)

ARGUS es una **capa de accountability para código AI-generated** construida en Rust puro (100% Rust, sin Python/Node en producción). Entregable único del Reto AI Academy de Platzi que cubre los 5 proyectos:

### Arquitectura actual (11 crates, ~4,300 LOC Rust, 47 unit tests + 3 integration tests)

```
crates/
├── apohara-argus-core         Tipos, errores, 4 prompts documentados (.md), config
├── argus-crypto       ed25519 (firma), BLAKE3 (hash chain), SPIFFE-like IDs
├── argus-llm          Cliente LLM unificado (NIM primario, OpenAI-compatible)
├── argus-slop         4 analyzers (slop/security/arch/verdict) en pipeline paralelo
├── argus-github       Cliente GitHub API (PR fetch, comment posting, labels)
├── argus-guard        CLI pre-commit (Aegis Guard: ALLOW/WARN/BLOCK)
├── argus-verify       HTTP server PR review worker (Aegis Verify, POST /analyze)
├── argus-lens         Weekly digest (Aegis Lens: "CTO avatar script")
├── argus-agent        El agente como código: AgentSpec, DecisionLog, Orchestrator, CordonEnforcer
├── argus-dashboard    SSR UI con Axum + htmx + Tailwind CDN
└── apohara-argus-cli          CLI unificado: argus health/prompts/guard/verify/lens
```

### Decisiones de stack ya tomadas

- **LLM:** BYOK (Bring Your Own Key) — el usuario provee su key de NVIDIA NIM via `X-LLM-Key` header. Default: `meta/llama-3.1-70b-instruct` en `https://integrate.api.nvidia.com/v1`. Sin LangChain, sin Rig — `reqwest` + `serde` directo.
- **Crypto:** ed25519 para firma de cada acción del agente, BLAKE3 para hash chain del ledger. **Cumple EU AI Act Article 12 por construcción** (auto-recording, signed, hash-chained).
- **Agent pattern:** 4 specialists en paralelo (Tokio `join!`), 1 synthesizer (Cordon Principle: no ve raw code). Ed25519-firmado, BLAKE3-chained.
- **Frontend:** SSR con askama + htmx. Sin React/Vue/SPA. 1 script tag de htmx via CDN.
- **Deploy:** Rust binary en Fly.io (free tier). Static export a Vercel como opción. Repo en GitHub: `https://github.com/SuarezPM/apohara-argus`.

### Tesis central (el "why")

> "El software libre está muriendo de AI slop." — En 2025, GitHub tuvo +206% de proyectos AI, el código AI tiene 70% más bugs, 19/20 reportes de curl eran alucinaciones, 96% de devs no confía en código AI. La verificación es ahora el cuello de botella, no la generación.

### Lo que YA funciona (verificado end-to-end con NIM real)

- Cliente NIM conecta, 4 prompts cargan desde `apohara-argus-core::prompts/`
- Pipeline E2E detecta AWS keys como CRITICAL, emite verdict
- Lens genera weekly briefing con script del "CTO avatar"
- CLI unificado funciona: `argus health/prompts/guard/lens` todos OK
- Audit trail signed + hash-chained en `argus-crypto`
- CordonEnforcer bloquea raw code en synthesizer (12 tests verifican)

### Lo que NO está (gaps honestos)

- `argus-verify` no postea comments reales (requiere GITHUB_TOKEN que no tenemos)
- Lens no renderiza video avatar real (solo genera el script)
- Dashboard usa in-memory store, no Supabase
- `argus-llm` solo soporta NIM; no fallback a Claude/OpenAI
- Sin tests E2E con PRs reales de GitHub

---

## INVESTIGACIÓN REQUERIDA (8 dimensiones)

### DIMENSIÓN 1: Competencia en AI Code Review (May-Junio 2026)

**Pregunta:** ¿Qué features específicas shippearon CodeRabbit, Greptile, Qodo, Macroscope, Cursor Bugbot, Bito en May-Junio 2026 que ARGUS debería considerar?

**Lo que sé (de EXA):**
- CodeRabbit (May 2026): "Change Stack" para AI-PRs grandes, semantic diff view, "Code Peek", `coderabbit doctor` CLI
- Greptile (May 2026): codebase indexing, severity scores, hand-off a coding agents (Claude/Codex/Cursor/Devin)
- Qodo (May 2026): central dashboard con analytics 30-day, auto-import rules from `.cursor/rules/`, `.cursorrules`
- Macroscope: "code review programmable"
- Market share: CodeRabbit ~140K paid users, Greptile $25M Series A, Qodo rebranded de Codium, $40-60M ARR

**Busca específicamente:**
1. ¿Qué features de "Change Stack" de CodeRabbit (layer-by-layer walkthrough) podríamos replicar en nuestro dashboard?
2. El "hand-off a coding agent" de Greptile — ¿cómo se implementa? ¿Vale la pena para ARGUS?
3. ¿Hay un "benchmark 2026" reciente que podamos citar en el pitch para posicionarnos?
4. ¿Cuál es el pricing de "self-hosted" o "BYOK" similar al nuestro en estos tools?
5. ¿Hay algún tool open-source (PR-Agent de Qodo, aislop, slop-scan) cuyas features podríamos portar?

**Output esperado:** 5-8 recomendaciones concretas, cada una con: feature a portar, evidencia (link + fecha), esfuerzo estimado (horas), valor para el usuario.

---

### DIMENSIÓN 2: EU AI Act Article 12 — Compliance Específica

**Pregunta:** ¿Qué dice el latest EU AI Act guidance (2026) sobre logs, signing, retention, y formatos para que ARGUS esté "EU AI Act ready by default"?

**Lo que sé (de EXA):**
- Article 12 (record-keeping): obligatorio desde 2 Aug 2026 para high-risk, 2 Dec 2027 para Annex III (post-Omnibus)
- Requisitos explícitos: automatic, lifetime, traceability-relevant events
- Retention: mínimo 6 meses, recomendado 24 meses (alineado con GDPR)
- Estándar: hash-chained, signed at write time, verifiable offline
- AGLedger: ya hace exactamente esto (Ed25519 + hash chain)
- Eventos requeridos per turn: model id, prompt template version, prompt text, temperature, raw response, tool calls, decision artifact
- Penalties: €15M o 3% global turnover

**Busca específicamente:**
1. ¿Cuál es el formato exacto de "record-keeping" que pide el draft guidelines 7 May 2026? (JSON, CSV, NDJSON, OCSF, DSSE?)
2. ¿Qué retention period exacto piden los "Draft Guidelines" publicados el 8 June 2026? (el paper de Arthur Cox menciona los guidelines)
3. ¿Hay un schema estándar (e.g., OCSF, SCITT, in-toto attestation) que deberíamos usar en vez de nuestro JSON custom?
4. ¿Cómo manejan los competidores (CodeRabbit, Greptile) la compliance con Article 12? ¿Tienen features específicas para esto?
5. ¿Qué pasa con sistemas que no son "high-risk" — podemos auto-certificar como "minimal risk" para evitar obligaciones?

**Output esperado:** Lista de 5-8 cambios concretos al audit trail y al ledger para que ARGUS cumpla Article 12 by default (sin que el usuario tenga que configurar nada).

---

### DIMENSIÓN 3: Rust AI Agent Frameworks — Madurez de Producción

**Pregunta:** Dado que ARGUS está escrito SIN framework de agente (reqwest directo, no Rig ni AutoAgents), ¿vale la pena reconsiderar? ¿O nuestra decisión "from scratch" sigue siendo correcta en Jun 2026?

**Lo que sé (de EXA):**
- Rig (~6,700 stars, 5K+ en otro reporte, Apache-2.0): trait-based, 20+ LLM providers, OpenTelemetry, ~1GB peak memory, ~4ms cold start
- AutoAgents: actor model (Ractor), WASM sandbox para tools, OpenTelemetry, 1,046 MB peak
- OpenFANG: 137K LOC, "Agent OS/kernel", WASM + fuel metering
- ADK-Rust: type-safe, event streaming, A2A protocol
- Producción deployments: Cloudflare Infire (Rust inference engine), AWS Firecracker, Neon (Rig), Nethermind (Rig), St. Jude (Rig)
- Benchmarks: Rust 5x menos memory, 25-44% más latency, 13x más throughput en algunos casos

**Busca específicamente:**
1. ¿Cuál es el framework Rust más maduro para **multi-agent** (no single-agent) en Jun 2026? ¿AutoAgents ganó madurez?
2. ¿Rig tiene soporte para OpenAI-compatible providers (NIM) oficialmente? ¿O hay que custom?
3. ¿Hay algún framework Rust que soporte A2A protocol nativamente? (el A2A de Google está tomando tracción)
4. ¿Qué es "OpenFANG kernel" exactamente? ¿Vale la pena mirar?
5. **Pregunta clave para nuestro caso:** dado que tenemos un pipeline Tokio custom con 4 specialists, ¿cuánto trabajo sería migrar a Rig/AutoAgents? ¿Ganamos algo concreto (velocidad? menos código?)

**Output esperado:** Recomendación clara: ¿quedarse con custom o migrar a framework? Si migrar, ¿a cuál y por qué? Si quedarse, ¿qué patrones del framework podemos portar a nuestro custom?

---

### DIMENSIÓN 4: NVIDIA NIM — Modelos Óptimos para Nuestro Caso

**Pregunta:** Dado que usamos `meta/llama-3.1-70b-instruct` como default, ¿hay un modelo mejor en el NIM catalog para nuestro caso de uso (slop detection, security review, verdict synthesis)?

**Lo que sé (de EXA):**
- 147 modelos en el catalog, 107-122 LLMs específicos
- Modelos relevantes para coding/agentic: Kimi K2.6 (1T params, multimodal, tool use), GLM-5.1 (flagship, agentic), DeepSeek V4 Flash (1M context, fast coding), Nemotron 3 Super (reasoning + tool use), Qwen3.5 122B-A10B (MoE agent-ready)
- Qwen 3.5 VLM 400B (multimodal) — relevante para futuro image input
- NVIDIA Nemotron 3 Nano: cost-efficient
- Los thinking/reasoning models (Kimi K2-thinking, Nemotron Nano 9B v2) podrían ser ideales para verdict synthesis

**Busca específicamente:**
1. ¿Cuál modelo del NIM catalog (Jun 2026) tiene **mejor F1 en code review benchmarks**? Comparado con Llama 3.1 70B.
2. ¿Hay un modelo "reasoning/thinking" que sea significativamente mejor para el verdict-synthesizer (que necesita sopesar 3 outputs)?
3. Para slop-detector (señal-based, no necesita razonamiento profundo), ¿hay un modelo más barato/ rápido que mantenga la calidad? (Qwen 3 Coder 30B, MiniMax M2.7, etc)
4. ¿Cuál es el modelo con mejor relación calidad/costo en Jun 2026? (Importante: nuestro cost analysis asume $0.05/dev/mes)
5. ¿Hay algún modelo nuevo (released post-May 2026) que deberíamos evaluar para nuestro pipeline?

**Output esperado:** Tabla comparativa de 5-8 modelos con: nombre, context window, pricing (input/output per 1M tokens), strengths, weaknesses, recommendation para nuestro caso (slop/security/arch/verdict). Top 1-2 recommendations con justificación.

---

### DIMENSIÓN 5: AI Slop Detection — Más Allá de LLM

**Pregunta:** Hoy nuestro `slop-detector` es 100% LLM. ¿Hay approaches determinísticos (regex/AST/lint) que deberíamos añadir para mejorar accuracy, latency, o cost?

**Lo que sé (de EXA):**
- `aislop` (scanaislop/aislop, MIT): 40+ reglas determinísticas, 7 lenguajes, sub-segundo, sin LLM, score 0-100, auto-fix
- `slop_scan` (modem-dev): AI slop patterns en JS/TS, deterministic CLI
- Paper arXiv:2604.16754 ("AI Slop and the Software Commons") — design principles para tools
- Paper arXiv:2604.01147 (SERSEM) — character-level weighted mask via AST + linting
- Paper Pith:2603.27130 — empirical study con detection pipeline heuristic + LLM
- Patrones comunes detectados: narrative comments, swallowed exceptions, `as any` casts, hallucinated imports, duplicated helpers, dead code, todo stubs, oversized functions, generic names

**Busca específicamente:**
1. ¿Cuál es el **benchmark público** más reciente (Jun 2026) para AI slop detection tools? ¿Hay un leaderboard?
2. ¿Cuál es el patrón determinístico más confiable para detectar AI slop en **Rust** específicamente? (Rust tiene menos herramientas que Python/JS)
3. ¿Hay un **JSON schema estándar** o una taxonomía de slop signals que la comunidad haya adoptado?
4. ¿Cómo se compara la accuracy de un approach "100% LLM" vs "100% deterministic" vs "hybrid"? (cualquier paper o blog post reciente)
5. ¿Hay herramientas open-source que ya portaron su pipeline a Rust? (no solo Python)

**Output esperado:** Recomendación de arquitectura para nuestro slop-detector: ¿quedarse 100% LLM? ¿agregar capa determinística con `aislop`-style regex? ¿hybrid? Incluir código Rust de ejemplo para las 5-10 reglas determinísticas más confiables.

---

### DIMENSIÓN 6: Rust Production Backend Patterns (Jun 2026)

**Pregunta:** ¿Qué patrones de producción 2026 deberíamos adoptar en `argus-api`, `argus-verify`, `argus-dashboard`?

**Lo que sé (de EXA):**
- sqlx con `query!()` macro y `SQLX_OFFLINE=true` para builds sin DB
- sqlx::migrate!() para migraciones embedded
- Compile-time SQL checking es el estándar 2026
- Pattern "Axum + SQLx + Askama + HTMX" es recomendado sobre SPA para CRUD apps
- Maud es alternativa a Askama (compile-time templating, type-checked)
- Container: `cargo:rerun-if-changed=migrations` build script
- Connection pool sizing: `maxconnections` alineado con PgBouncer
- `statement_timeout` por role, `query_logging` feature
- `Restate` para durable workflow execution (aplicable a Lens/Verify?)

**Busca específicamente:**
1. ¿Cuál es el **template engine** recomendado en Jun 2026: askama (current), Maud (compile-time macros), o algo nuevo? Ventajas concretas para nuestro caso.
2. ¿Cómo se implementa **graceful shutdown** correctamente en Axum en 2026? (Estamos matando workers abruptamente)
3. ¿Cuál es el patrón para **distributed tracing** en un sistema multi-crate como el nuestro? (OpenTelemetry, qué exporter, dónde instrumentar)
4. ¿Cómo se hace **graceful degradation** cuando el LLM (NIM) está caído? (Tenemos defensive default pero no circuit breaker)
5. ¿Cuál es el patrón para **request idempotency** en POST /analyze? (Si el mismo PR se analiza dos veces, ¿cómo evitamos double-billing?)

**Output esperado:** 5-8 patrones específicos para implementar, cada uno con: nombre del patrón, descripción, código Rust de ejemplo, archivos del workspace donde aplicaría.

---

### DIMENSIÓN 7: AI Avatar Video — ¿Vale la pena en Jun 2026?

**Pregunta:** Nuestro Lens genera el script del "CTO avatar" pero no renderiza video real. ¿Deberíamos invertir en integrar HeyGen/D-ID/Tavus? ¿O hay alternativas?

**Lo que sé (de EXA):**
- D-ID: $4.70/mo entry, API on ALL plans (developer-friendly), $5.99/mo Lite tier, 100+ languages
- HeyGen: $29/mo entry, mejor photorealism, 175+ languages, Avatar IV
- Synthesia: $89/year, enterprise, 140+ languages
- Tavus: real-time streaming avatars, $59-89/mo, personalized video at scale
- HeyGen usa credit-based: 1 min premium avatar = ~20 credits, Creator plan = 200 credits/month
- Kompozy (BYO model): permite pastear tu propio HeyGen avatar ID, $39/mo BYO
- D-ID es el más barato y developer-friendly (API en todos los planes)

**Busca específicamente:**
1. ¿Cuál es el **costo por minuto** real de cada provider después de los free trials? (Costo total de 1 weekly briefing de 60-90s)
2. ¿Hay algún **open-source model** (SadTalker, LivePortrait, Hallo, etc) que podamos correr localmente y evite vendor lock-in?
3. Para nuestro caso (1 video/semana, 60-90s, multi-idioma), ¿cuál es el sweet spot de precio/calidad?
4. ¿Qué provider tiene el **API más simple** para integrar con nuestro Rust backend via reqwest? (Necesitamos: POST script + voice_id, poll status, GET mp4)
5. ¿Vale la pena integrar video avatar, o我们应该 dejarnos el script y ofrecer al usuario "copy this to HeyGen"? (costo vs. wow factor)

**Output esperado:** Tabla comparativa con pricing real + recomendación clara: ¿integrar o no? Si sí, ¿cuál y por qué? Si no, ¿cómo presentar el script para que el usuario lo use en HeyGen manualmente?

---

### DIMENSIÓN 8: SPIFFE/SPIRE para AI Agents — Implementación Real

**Pregunta:** Nuestro `argus-crypto` tiene una implementación custom de SPIFFE-like IDs (JWT firmado con SPIFFE URI como `sub`). ¿Deberíamos migrar a la librería real `spiffe` de Rust?

**Lo que sé (de EXA):**
- `spiffe` crate v0.15: standards-compliant Rust client para SPIFFE Workload API
- `spire-api` v0.7.0 (May 2026): Rust gRPC client para SPIRE-specific APIs
- `spiffe-rustls`: integración mTLS con SPIFFE identities
- `spiffe-rustls-tokio`: async Tokio-native TLS helpers
- `maxlambrecht/rust-spiffe` (34 stars, 17 forks): collection de crates
- NIST NCCoE paper (Feb 2026): "Accelerating the Adoption of Software and AI Agent Identity and Authorization" — SPIFFE/SPIRE es uno de los estándares que NIST está considerando para agent identity
- Nuestro custom: solo JWT, sin X.509, sin SPIRE server, sin workload attestation

**Busca específicamente:**
1. ¿Cuál es la **diferencia real** entre nuestro "SPIFFE-like" (JWT firmado) y el "SPIFFE real" (X.509-SVID, SPIRE server, workload attestation)? ¿Vale la pena migrar?
2. Para nuestro caso (BYOK, no enterprise, agents ephemeral), ¿necesitamos SPIRE server o alcanza con JWT firmado?
3. ¿Hay algún **sidecar/embed** de SPIRE que sea viable para nuestro deploy en Fly.io? (sin Kubernetes)
4. ¿Cómo usan los competidores (CodeRabbit, Greptile) SPIFFE/SPIRE? (Si lo usan)
5. ¿Hay un **estándar IETF** o W3C más liviano que SPIFFE que deberíamos considerar para nuestro caso?

**Output esperado:** Recomendación clara: ¿quedarse con JWT custom o migrar a `spiffe` crate? Si migrar, ¿qué features específicas? Si quedarse, ¿qué features del estándar real podemos añadir incrementalmente?

---

## OUTPUT FORMAT REQUERIDO

Para CADA una de las 8 dimensiones, produzcan:

```
### DIMENSIÓN N: [título]

**Resumen ejecutivo** (2-3 oraciones)

**Hallazgos clave** (5-8 bullets, cada uno con link y fecha)

**Recomendaciones accionables** (3-5 bullets, cada uno con):
- [ ] Qué hacer (1 línea)
- Esfuerzo estimado: X horas
- Archivos ARGUS afectados: `crates/X/src/Y.rs`
- Código de ejemplo (si aplica, en Rust)
- Riesgo o tradeoff

**Decisión recomendada** (1 párrafo)
```

## CONSTRAINTS

- Usá SOLO información de 2026 (preferentemente May-Junio 2026) — descartá cualquier claim de 2024 o antes
- Citá SIEMPRE la fuente con link completo
- Si un paper o tool es nuevo en Jun 2026, marcalo como "FRESH"
- Si una feature es de un competidor específico (CodeRabbit, Greptile, etc), mencionalo por nombre
- Para pricing de APIs, citá la fecha de la fuente y advertí que cambia frecuentemente
- NO recomiendes cosas que rompan el constraint "pure Rust 100%" sin justificación muy fuerte

## TIMING

Report expected: 30-60 minutes of research. Output: 8 sections, ~6,000-10,000 words.

---

## NOTA META

Este megaprompt está diseñado para Perplexity Research mode. El usuario (Pablo) ejecutará este prompt en `perplexity.ai` con búsqueda web habilitada y modo "Research" seleccionado. El output se usará para iterar sobre el código de ARGUS en los próximos días.
