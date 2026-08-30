# mzbs — CLI Rust de estudios bíblicos (Monte de Sion)

Herramienta para voluntarios: un solo binario. **No necesita Python.**

## Tres pasos

1. Copie `config.example.toml` → `config.toml` y pegue su API key de Cursor  
   (Dashboard → [API Keys](https://cursor.com/dashboard/api)).
2. Ponga el PDF de escaneo en `scans/` (no en `complete/` ni `error/`).
3. Ejecute:
   ```bash
   ./mzbs prepare --from 23 --to 26
   ```
   o solo `./mzbs prepare` para el formulario interactivo (terminal).

## Requisitos

- **macOS** + **Microsoft PowerPoint** para exportar PDF (el `.pptx` se genera igual sin eso).
- **Internet** para el agente de Cursor (prepare / review).
- **pdftoppm** (poppler) para convertir páginas del PDF a imágenes antes de llamar al agente.

Tras éxito, el PDF pasa a `scans/complete/`. Si falla, a `scans/error/` con un `.log`.

Más detalle: [README.md](README.md).
