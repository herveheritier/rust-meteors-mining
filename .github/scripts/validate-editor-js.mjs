// Validation syntaxique des pages des éditeurs (HTML+JS autonomes).
//
// Pour chaque fichier passé en argument : extrait chaque bloc <script>…</script>
// inline (les scripts externes, style type=module avec src, sont ignorés) et
// le compile via `new Function(...)` - une erreur de syntaxe JS fait échouer
// la commande (code de sortie ≠ 0).
//
// Usage :
//   node .github/scripts/validate-editor-js.mjs <editeur1.html> [<editeur2.html> …]
import { readFileSync, writeFileSync, unlinkSync } from "node:fs";

let ok = true;
for (const file of process.argv.slice(2)) {
  const html = readFileSync(file, "utf8");
  const scripts = [
    ...html.matchAll(/<script(?![^>]*\bsrc=)[^>]*>([\s\S]*?)<\/script>/gi),
  ];
  process.stdout.write(`→ ${file}\n`);
  if (scripts.length === 0) {
    process.stdout.write("  (aucun script inline)\n");
    continue;
  }
  scripts.forEach((m, i) => {
    const tmp = `${file}.${i}.js`;
    writeFileSync(tmp, m[1]);
    try {
      new Function(m[1]);
      process.stdout.write(`  script ${i + 1}: OK\n`);
    } catch (err) {
      ok = false;
      process.stdout.write(`  script ${i + 1}: ERREUR ${err.message}\n`);
    } finally {
      unlinkSync(tmp);
    }
  });
}
if (!ok) process.exit(1);