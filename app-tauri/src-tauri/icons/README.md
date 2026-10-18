# Icons

Placeholder icons generated with `tauri icon` from a solid-color source
("OP" on dark slate) so that `tauri.conf.json` (`bundle.icon`) resolves and
the NSIS/Windows bundle can be produced in CI. Replace with real branding
before any public release; regenerate with:

```bash
# 1024x1024 square PNG source, then:
pnpm tauri icon path/to/source-1024.png
```
