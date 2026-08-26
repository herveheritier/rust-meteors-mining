#!/usr/bin/env node
// Petit serveur local pour l'éditeur de la place de marché.
//
// Pourquoi : le navigateur intégré de VSCode (webview Electron) n'expose pas
// l'API File System Access - la page ouverte en fichier local ne peut alors
// ni charger ni enregistrer src/marketplace.rs directement. Servie par ce
// serveur (http), elle charge et enregistre le fichier du projet via fetch
// (GET/PUT) - cela fonctionne dans tous les navigateurs.
//
// Usage : node tools/marketplace-editor/server.mjs [--port 8123]
// Puis ouvrir http://localhost:8123 dans le navigateur (ou dans le navigateur
// intégré de VSCode : Palette de commandes → « Simple Browser: Show »).
//
// Console cargo (POST /api/cargo, { "command": … }) : "test" (cargo test),
// "run" (cargo run), "run-release" (cargo run --release) et "wasm" (build
// WASM release puis version web servie sous /wasm/ - ouvrir
// http://localhost:8123/wasm/ pour jouer dans le navigateur).
//
// Variable d'environnement MARKETPLACE_ROOT : racine du projet à servir
// (défaut : deux niveaux au-dessus de ce fichier, soit la racine du dépôt).

import http from "node:http";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { spawn } from "node:child_process";

const here = path.dirname(fileURLToPath(import.meta.url));
const ROOT = process.env.MARKETPLACE_ROOT
  ? path.resolve(process.env.MARKETPLACE_ROOT)
  : path.resolve(here, "..", "..");
const MARKETPLACE = path.join(ROOT, "src", "marketplace.rs");

const args = process.argv.slice(2);
const portIdx = args.indexOf("--port");
const PORT = portIdx >= 0 && args[portIdx + 1] ? Number(args[portIdx + 1]) : 8123;

const mime = {
  ".html": "text/html; charset=utf-8",
  ".rs": "text/plain; charset=utf-8",
  ".json": "application/json; charset=utf-8",
  ".js": "text/javascript; charset=utf-8",
  // requis par WebAssembly.instantiateStreaming (gl.js) : le type exact
  // application/wasm, sinon le chargement échoue
  ".wasm": "application/wasm",
};

// Version web locale (console cargo « wasm ») : le build WASM release est
// assemblé dans target/wasm-site/ (web/index.html + web/gl.js + le binaire
// wasm) puis servi par CE serveur sous /wasm/ - aucune URL supplémentaire,
// tout meurt avec l'éditeur. Le dossier target/ est ignoré par git et
// nettoyé par cargo clean.
const WASM_SITE = path.join(ROOT, "target", "wasm-site");
const WASM_BIN = path.join(ROOT, "target", "wasm32-unknown-unknown", "release", "rust-meteors-mining.wasm");
const WASM_URL = `http://localhost:${PORT}/wasm/`;

/// Copie les trois fichiers du site web (index.html + glue + wasm) dans
/// target/wasm-site/ - à appeler après un build WASM réussi.
function assembleWasmSite() {
  fs.mkdirSync(WASM_SITE, { recursive: true });
  fs.copyFileSync(path.join(ROOT, "web", "index.html"), path.join(WASM_SITE, "index.html"));
  fs.copyFileSync(path.join(ROOT, "web", "gl.js"), path.join(WASM_SITE, "gl.js"));
  fs.copyFileSync(WASM_BIN, path.join(WASM_SITE, "rust-meteors-mining.wasm"));
}

// Processus cargo en cours (console « cargo test / cargo run ») : une seule
// commande à la fois - cargo verrouille target/ pendant la compilation.
let cargoChild = null;

const server = http.createServer(async (req, res) => {
  const pathname = decodeURIComponent(new URL(req.url, "http://localhost").pathname);
  try {
    // page de l'éditeur
    if (req.method === "GET" && (pathname === "/" || pathname === "/index.html")) {
      const html = fs.readFileSync(path.join(here, "index.html"));
      // no-cache : la page change à chaque mise à jour de l'outil - le
      // navigateur doit toujours revalider (pas de copie périmée en cache
      // après un redémarrage du serveur)
      res.writeHead(200, { "Content-Type": "text/html; charset=utf-8", "Cache-Control": "no-cache" });
      res.end(html);
      return;
    }
    // liste des meshes du vaisseau (assets/*.json) pour le choix d'asset
    if (req.method === "GET" && pathname === "/list-assets") {
      const assetsDir = path.join(ROOT, "assets");
      const files = fs.readdirSync(assetsDir).filter(f => f.endsWith(".json")).sort();
      res.writeHead(200, { "Content-Type": "application/json; charset=utf-8", "Cache-Control": "no-cache" });
      res.end(JSON.stringify({ assets: files }));
      return;
    }
    // contenu d'un mesh assets/*.json (aperçu du vaisseau dans l'éditeur) -
    // nom seul (basename), fichier .json du dossier assets/ du projet
    if (req.method === "GET" && pathname.startsWith("/assets/")) {
      const name = path.basename(pathname.slice("/assets/".length));
      if (!name.endsWith(".json")) {
        res.writeHead(404, { "Content-Type": "text/plain; charset=utf-8" });
        res.end("404 - asset non JSON");
        return;
      }
      const file = path.join(ROOT, "assets", name);
      if (!fs.existsSync(file)) {
        res.writeHead(404, { "Content-Type": "text/plain; charset=utf-8" });
        res.end("404 - " + name);
        return;
      }
      // no-cache : le mesh peut être modifié dans l'éditeur « meshes-designer »
      // pendant que la page est ouverte - le navigateur doit toujours
      // re-télécharger la version à jour (jamais la resservir du cache)
      res.writeHead(200, { "Content-Type": "application/json; charset=utf-8", "Cache-Control": "no-cache" });
      res.end(fs.readFileSync(file, "utf8"));
      return;
    }
    // lecture du fichier généré (chargement dans l'éditeur)
    if (req.method === "GET" && pathname === "/marketplace.rs") {
      const content = fs.readFileSync(MARKETPLACE, "utf8");
      res.writeHead(200, { "Content-Type": "text/plain; charset=utf-8", "Cache-Control": "no-cache" });
      res.end(content);
      return;
    }
    // écriture du fichier généré (enregistrement depuis l'éditeur)
    if (req.method === "PUT" && pathname === "/marketplace.rs") {
      let body = "";
      for await (const chunk of req) body += chunk;
      fs.writeFileSync(MARKETPLACE, body);
      res.writeHead(200, { "Content-Type": "text/plain; charset=utf-8" });
      res.end("ok");
      return;
    }
    // version web locale (console cargo « wasm ») : sert le site assemblé
    // dans target/wasm-site/ - /wasm/ (index.html), /wasm/gl.js et
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
      res.writeHead(200, { "Content-Type": mime[ext] || "application/octet-stream" });
      res.end(fs.readFileSync(path.join(WASM_SITE, name)));
      return;
    }
    // console cargo : lance « cargo test » ou « cargo run » dans la racine du
    // projet et renvoie la sortie en flux (chunked, texte brut), terminée par
    // la ligne « [code de sortie: N] ». Une seule commande à la fois (cargo
    // verrouille target/) : 409 sinon. « cargo run » garde la connexion
    // ouverte tant que le jeu tourne - la fermeture de la fenêtre du jeu
    // termine la réponse ; si l'onglet se ferme, le jeu continue de tourner.
    if (req.method === "POST" && pathname === "/api/cargo") {
      let body = "";
      for await (const chunk of req) body += chunk;
      let command = "test";
      try {
        command = JSON.parse(body || "{}").command || "test";
      } catch (_) { /* corps absent ou invalide → test */ }
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
      res.on("error", () => { /* onglet fermé : le jeu (cargo run) continue */ });
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
    // console cargo : arrête la commande en cours (SIGTERM)
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
  console.log("Éditeur de la place de marché :");
  console.log("  http://localhost:" + PORT + "/");
  console.log("  Fichier lié : " + MARKETPLACE);
  console.log("  Console cargo : test | run | run-release | wasm (version web : " + WASM_URL + ")");
  console.log("  (Ctrl+C pour arrêter)");
});

// ─── Redémarrage automatique (watch de server.mjs) ─────────────────────────
// Mise à jour de l'outil : quand CE fichier change (enregistrement depuis un
// éditeur de code), le serveur se remplace par un processus identique (mêmes
// arguments, même stdio - le log éventuel du launcher est conservé) et la
// page se recharge proprement (no-cache). Désactivable pour un usage
// ponctuel : AUTO_RESTART=0 node tools/marketplace-editor/server.mjs
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
