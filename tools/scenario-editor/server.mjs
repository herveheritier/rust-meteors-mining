#!/usr/bin/env node
// Serveur HTTP local pour l'éditeur de scénarios et d'objectifs (DAG).
//
// Permet de charger, éditer, valider et enregistrer les scénarios sous forme
// de fichiers JSON (`scenarios/*.json`) et d'exporter directement le code Rust
// généré dans `src/scenario_objectives.rs`.
//
// Usage : node tools/scenario-editor/server.mjs [--port 8124]
// Puis ouvrir http://localhost:8124 dans le navigateur.
//
// Console cargo (POST /api/cargo, { "command": … }) : "test" (cargo test),
// "run" (cargo run), "run-release" (cargo run --release) et "wasm" (build
// WASM release puis version web servie sous /wasm/ - ouvrir
// http://localhost:8124/wasm/ pour jouer dans le navigateur).

import http from "node:http";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { spawn } from "node:child_process";

const here = path.dirname(fileURLToPath(import.meta.url));
const ROOT = process.env.MARKETPLACE_ROOT || process.env.SCENARIO_ROOT
  ? path.resolve(process.env.MARKETPLACE_ROOT || process.env.SCENARIO_ROOT)
  : path.resolve(here, "..", "..");
const SCENARIO_RS = path.join(ROOT, "src", "scenario_objectives.rs");
const SCENARIOS_DIR = path.join(ROOT, "scenarios");

if (!fs.existsSync(SCENARIOS_DIR)) {
  fs.mkdirSync(SCENARIOS_DIR, { recursive: true });
}

const args = process.argv.slice(2);
const portIdx = args.indexOf("--port");
const PORT = portIdx >= 0 && args[portIdx + 1] ? Number(args[portIdx + 1]) : 8124;

// Version web locale (console cargo « wasm ») : le build WASM release est
// assemblé dans target/wasm-site/ (web/index.html + web/gl.js + le binaire
// wasm) puis servi par CE serveur sous /wasm/ - aucune URL supplémentaire,
// tout meurt avec l'éditeur. Le dossier target/ est ignoré par git et
// nettoyé par cargo clean.
const WASM_SITE = path.join(ROOT, "target", "wasm-site");
const WASM_BIN = path.join(ROOT, "target", "wasm32-unknown-unknown", "release", "rust-meteors-mining.wasm");
const WASM_URL = `http://localhost:${PORT}/wasm/`;

// Copie les trois fichiers du site web (index.html + glue + wasm) dans
// target/wasm-site/ - à appeler après un build WASM réussi.
function assembleWasmSite() {
  fs.mkdirSync(WASM_SITE, { recursive: true });
  fs.copyFileSync(path.join(ROOT, "web", "index.html"), path.join(WASM_SITE, "index.html"));
  fs.copyFileSync(path.join(ROOT, "web", "gl.js"), path.join(WASM_SITE, "gl.js"));
  fs.copyFileSync(WASM_BIN, path.join(WASM_SITE, "rust-meteors-mining.wasm"));
}

let cargoChild = null;

const server = http.createServer(async (req, res) => {
  const pathname = decodeURIComponent(new URL(req.url, "http://localhost").pathname);

  try {
    // 1. Page principale de l'éditeur
    if (req.method === "GET" && (pathname === "/" || pathname === "/index.html")) {
      const html = fs.readFileSync(path.join(here, "index.html"));
      // no-cache : la page change à chaque mise à jour de l'outil - le
      // navigateur doit toujours revalider (pas de copie périmée en cache
      // après un redémarrage du serveur)
      res.writeHead(200, { "Content-Type": "text/html; charset=utf-8", "Cache-Control": "no-cache" });
      res.end(html);
      return;
    }

    // 2. Lecture du fichier Rust généré src/scenario_objectives.rs
    if (req.method === "GET" && pathname === "/scenario_objectives.rs") {
      if (fs.existsSync(SCENARIO_RS)) {
        res.writeHead(200, { "Content-Type": "text/plain; charset=utf-8", "Cache-Control": "no-cache" });
        res.end(fs.readFileSync(SCENARIO_RS, "utf8"));
      } else {
        res.writeHead(404, { "Content-Type": "text/plain; charset=utf-8" });
        res.end("// src/scenario_objectives.rs non trouvé");
      }
      return;
    }

    // 3. Écriture / Sauvegarde dans src/scenario_objectives.rs
    if (req.method === "PUT" && pathname === "/scenario_objectives.rs") {
      let body = "";
      for await (const chunk of req) body += chunk;
      fs.writeFileSync(SCENARIO_RS, body, "utf8");
      res.writeHead(200, { "Content-Type": "text/plain; charset=utf-8" });
      res.end("ok");
      return;
    }

    // 4. API Liste des fichiers de scénarios (scenarios/*.json)
    if (req.method === "GET" && pathname === "/api/scenarios") {
      const files = fs.existsSync(SCENARIOS_DIR)
        ? fs.readdirSync(SCENARIOS_DIR).filter(f => f.endsWith(".json")).sort()
        : [];
      res.writeHead(200, { "Content-Type": "application/json; charset=utf-8", "Cache-Control": "no-cache" });
      res.end(JSON.stringify({ scenarios: files }));
      return;
    }

    // 5. API Chargement d'un scénario JSON
    if (req.method === "GET" && pathname.startsWith("/api/scenarios/")) {
      const filename = path.basename(pathname.slice("/api/scenarios/".length));
      const filepath = path.join(SCENARIOS_DIR, filename);
      if (fs.existsSync(filepath)) {
        res.writeHead(200, { "Content-Type": "application/json; charset=utf-8", "Cache-Control": "no-cache" });
        res.end(fs.readFileSync(filepath, "utf8"));
      } else {
        res.writeHead(404, { "Content-Type": "application/json; charset=utf-8" });
        res.end(JSON.stringify({ error: "Scénario non trouvé" }));
      }
      return;
    }

    // 6. API Sauvegarde d'un scénario JSON
    if (req.method === "POST" && pathname.startsWith("/api/scenarios/")) {
      const filename = path.basename(pathname.slice("/api/scenarios/".length));
      const filepath = path.join(SCENARIOS_DIR, filename);
      let body = "";
      for await (const chunk of req) body += chunk;
      fs.writeFileSync(filepath, body, "utf8");
      res.writeHead(200, { "Content-Type": "application/json; charset=utf-8" });
      res.end(JSON.stringify({ status: "ok", filename }));
      return;
    }

    // 7. API Suppression d'un scénario JSON
    if (req.method === "DELETE" && pathname.startsWith("/api/scenarios/")) {
      const filename = path.basename(pathname.slice("/api/scenarios/".length));
      const filepath = path.join(SCENARIOS_DIR, filename);
      if (fs.existsSync(filepath)) {
        fs.unlinkSync(filepath);
      }
      res.writeHead(200, { "Content-Type": "application/json; charset=utf-8" });
      res.end(JSON.stringify({ status: "ok" }));
      return;
    }

    // 8. Console Cargo (test / run / run-release / wasm)
    if (req.method === "POST" && pathname === "/api/cargo") {
      let body = "";
      for await (const chunk of req) body += chunk;
      let command = "test";
      try {
        command = JSON.parse(body || "{}").command || "test";
      } catch (_) {}

      // test = cargo test ; run = cargo run ; run-release = cargo run --release ;
      // wasm = build WASM release puis assemblage + service de la version web
      const CARGO_ARGS = {
        test: ["test"],
        run: ["run"],
        "run-release": ["run", "--release"],
        wasm: ["build", "--release", "--target", "wasm32-unknown-unknown"],
      };
      const args = CARGO_ARGS[command];
      if (!args) {
        res.writeHead(400, { "Content-Type": "text/plain; charset=utf-8" });
        res.end("Commande inconnue : " + command + " (test | run | run-release | wasm)");
        return;
      }

      if (cargoChild) {
        res.writeHead(409, { "Content-Type": "text/plain; charset=utf-8" });
        res.end("Une commande cargo est déjà en cours");
        return;
      }

      const child = spawn("cargo", args, { cwd: ROOT });
      cargoChild = child;
      res.writeHead(200, { "Content-Type": "text/plain; charset=utf-8", "Cache-Control": "no-cache" });

      const send = d => {
        if (!res.writableEnded && !res.destroyed) {
          try { res.write(d); } catch (_) {}
        }
      };

      let finished = false;
      const finish = code => {
        if (finished) return;
        finished = true;
        if (cargoChild === child) cargoChild = null;
        // build WASM réussi : assemble le site et indique l'URL de la version
        // web (servie par ce serveur sous /wasm/), avant le code de sortie
        if (command === "wasm" && code === 0) {
          try {
            assembleWasmSite();
            send("\n[WASM prêt : " + WASM_URL + "]\n");
          } catch (err) {
            send("\n[WASM erreur d'assemblage : " + err.message + "]\n");
          }
        }
        send("\n[code de sortie: " + (code == null ? -1 : code) + "]\n");
        try { res.end(); } catch (_) {}
      };

      child.stdout.on("data", send);
      child.stderr.on("data", send);
      child.on("error", err => { send("[erreur: " + err.message + "]\n"); finish(-1); });
      child.on("close", finish);
      return;
    }

    // 8bis. Version web locale (console cargo « wasm ») : sert le site
    // assemblé dans target/wasm-site/ - /wasm/ (index.html), /wasm/gl.js et
    // /wasm/rust-meteors-mining.wasm (noms relatifs de web/index.html)
    if (req.method === "GET" && (pathname === "/wasm/" || pathname.startsWith("/wasm/"))) {
      if (!fs.existsSync(path.join(WASM_SITE, "index.html"))) {
        res.writeHead(404, { "Content-Type": "text/plain; charset=utf-8" });
        res.end("Version WASM pas encore construite : lancez le bouton « WASM local » de la console cargo (POST /api/cargo { command: \"wasm\" }).");
        return;
      }
      const name = pathname === "/wasm/" ? "index.html" : path.basename(pathname);
      // seuls les trois fichiers du site sont servis (pas d'arbitraire de
      // chemin) - extension .wasm → type application/wasm (obligatoire)
      if (name !== "index.html" && name !== "gl.js" && name !== "rust-meteors-mining.wasm") {
        res.writeHead(404, { "Content-Type": "text/plain; charset=utf-8" });
        res.end("404 - " + name);
        return;
      }
      const ext = path.extname(name) || ".html";
      const types = {
        ".html": "text/html; charset=utf-8",
        ".js": "text/javascript; charset=utf-8",
        // requis par WebAssembly.instantiateStreaming (gl.js) : le type exact
        // application/wasm, sinon le chargement échoue
        ".wasm": "application/wasm",
      };
      res.writeHead(200, { "Content-Type": types[ext] || "application/octet-stream" });
      res.end(fs.readFileSync(path.join(WASM_SITE, name)));
      return;
    }

    // 9. Arrêt Cargo
    if (req.method === "POST" && pathname === "/api/cargo-stop") {
      if (cargoChild) {
        try { cargoChild.kill("SIGTERM"); } catch (_) {}
      }
      res.writeHead(200, { "Content-Type": "text/plain; charset=utf-8" });
      res.end("ok");
      return;
    }

    res.writeHead(404, { "Content-Type": "text/plain; charset=utf-8" });
    res.end("404 - " + pathname);
  } catch (err) {
    res.writeHead(500, { "Content-Type": "text/plain; charset=utf-8" });
    res.end("Erreur serveur : " + err.message);
  }
});

server.listen(PORT, "127.0.0.1", () => {
  console.log("Éditeur de scénarios et d'objectifs (DAG) :");
  console.log("  http://localhost:" + PORT + "/");
  console.log("  Fichier Rust lié : " + SCENARIO_RS);
  console.log("  Dossier de scénarios : " + SCENARIOS_DIR);
  console.log("  Console cargo : test | run | run-release | wasm (version web : " + WASM_URL + ")");
  console.log("  (Ctrl+C pour arrêter)");
});

// ─── Redémarrage automatique (watch de server.mjs) ─────────────────────────
// Mise à jour de l'outil : quand CE fichier change (enregistrement depuis un
// éditeur de code), le serveur se remplace par un processus identique (mêmes
// arguments, même stdio - le log éventuel du launcher est conservé) et la
// page se recharge proprement (no-cache). Désactivable pour un usage
// ponctuel : AUTO_RESTART=0 node tools/scenario-editor/server.mjs
const AUTO_RESTART = !["0", "off", "false", "no"].includes(
  (process.env.AUTO_RESTART || "").toLowerCase()
);
if (AUTO_RESTART) {
  const SELF = fileURLToPath(import.meta.url);
  let restartTimer = null;
  // watch du DOSSIER parent (pas du fichier) : robuste aux écritures
  // atomiques des éditeurs (fichier temporaire + rename - l'ancien inode
  // n'est plus suivi après le rename)
  fs.watch(path.dirname(SELF), (_event, filename) => {
    if (filename !== path.basename(SELF)) return;
    if (restartTimer) clearTimeout(restartTimer);
    // debounce : attendre la fin de la série d'écritures (300 ms de calme)
    restartTimer = setTimeout(() => {
      console.log("[watch] " + path.basename(SELF) + " modifié — redémarrage…");
      // commande cargo en cours (test / run / build wasm) : coupée, le
      // nouveau processus repartira propre
      if (cargoChild) {
        try { cargoChild.kill("SIGTERM"); } catch (_) {}
      }
      // libérer le port AVANT de lancer le remplaçant (sinon EADDRINUSE),
      // puis lui céder la place avec les mêmes arguments
      server.close(() => {
        const child = spawn(process.execPath, [SELF, ...process.argv.slice(2)], { stdio: "inherit" });
        child.on("error", err => {
          console.error("[watch] redémarrage impossible :", err.message);
          process.exit(1);
        });
        process.exit(0);
      });
      // filet de sécurité : des connexions qui traînent ne doivent pas
      // bloquer le redémarrage
      setTimeout(() => process.exit(0), 1000).unref();
    }, 300);
  });
}
