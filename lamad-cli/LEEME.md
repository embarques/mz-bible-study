# lamad — CLI Rust de estudios bíblicos (Mount Zion Church)

**lamad** (hebreo **לָמַד**) significa *aprender por instrucción, práctica o experiencia* — acostumbrarse o formarse en algo; no solo información, sino formación. (Forma relacionada en Piel **לִמֵּד** *limmed*: *enseñar*.)

Herramienta para voluntarios: un solo binario. **No necesita Python.** `lamad --help` muestra esta definición.

## Tres pasos (zip de voluntarios)

1. Edite `config.toml`:
   - Elija motor: `agent_provider = "cursor"` (por defecto) o `"chatgpt"`.
   - Pegue la API key correspondiente:
     - Cursor → `cursor_api_key` ([API Keys](https://cursor.com/dashboard/api))
     - ChatGPT → `openai_api_key` (OpenAI platform)
   - `pdftoppm_path` ya apunta a `tools/pdftoppm` (o `.exe` en Windows).
2. Ponga el PDF de escaneo en `scans/` (no en `complete/` ni `error/`).
3. Ejecute:
   ```bash
   ./lamad prepare --from 23 --to 26
   ```
   o solo `./lamad prepare` para el formulario interactivo (terminal).
   Con ChatGPT: `./lamad prepare --provider chatgpt --from 23 --to 26`

El zip **incluye** `tools/pdftoppm` — no hace falta instalar poppler aparte. Si mueve la carpeta `tools/`, actualice `pdftoppm_path` en `config.toml`.

## Requisitos

- **Internet** para el backend elegido (Cursor o OpenAI) en prepare / review.
- **macOS** + **Microsoft PowerPoint** solo si quiere exportar PDF (el `.pptx` se genera igual en Windows/Linux).
- El binario empaquetado trae poppler; en desarrollo local puede usar `brew install poppler` / `PATH`.

Tras éxito, el PDF pasa a `scans/complete/` cuando se procesó todo el archivo. Si falla: se escribe un `.log` en `scans/error/`; el PDF **permanece en `scans/`** para reintentar, salvo que queden más estudios en el PDF — entonces se mueve a `scans/error/`.

Más detalle: [README.md](README.md).
