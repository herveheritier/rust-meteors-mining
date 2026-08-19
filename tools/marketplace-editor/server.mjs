#!/usr/bin/env node
// Petit serveur local pour l'éditeur de la place de marché.
//
// Pourquoi : le navigateur intégré de VSCode (webview Electron) n'expose pas
// l'API File System Access — la page ouverte en fichier local ne peut alors
// ni charger ni enregistrer src/marketplace.rs directement. Servie par ce
// serveur (http), elle charge et enregistre le fichier du projet via fetch
// (GET/PUT) — cela fonctionne dans tous les navigateurs.
//
// Usage : node tools/marketplace-editor/server.mjs [--port 8123]
// Puis ouvrir http://localhost:8123 dans le navigateur (ou dans le navigateur
// intégré de VSCode : Palette de commandes → « Simple Browser: Show »).
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

const mime = { ".html": "text/html; charset=utf-8", ".rs": "text/plain; charset=utf-8", ".json": "application/json; charset=utf-8" };

// Processus cargo en cours (console « cargo test / cargo run ») : une seule
// commande à la fois — cargo verrouille target/ pendant la compilation.
let cargoChild = null;

const server = http.createServer(async (req, res) => {
  const pathname = decodeURIComponent(new URL(req.url, "http://localhost").pathname);
  try {
    // page de l'éditeur
    if (req.method === "GET" && (pathname === "/" || pathname === "/index.html")) {
      const html = fs.readFileSync(path.join(here, "index.html"));
      res.writeHead(200, { "Content-Type": "text/html; charset=utf-8" });
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
    // contenu d'un mesh assets/*.json (aperçu du vaisseau dans l'éditeur) —
    // nom seul (basename), fichier .json du dossier assets/ du projet
    if (req.method === "GET" && pathname.startsWith("/assets/")) {
      const name = path.basename(pathname.slice("/assets/".length));
      if (!name.endsWith(".json")) {
        res.writeHead(404, { "Content-Type": "text/plain; charset=utf-8" });
        res.end("404 — asset non JSON");
        return;
      }
      const file = path.join(ROOT, "assets", name);
      if (!fs.existsSync(file)) {
        res.writeHead(404, { "Content-Type": "text/plain; charset=utf-8" });
        res.end("404 — " + name);
        return;
      }
      // no-cache : le mesh peut être modifié dans l'éditeur « meshes-designer »
      // pendant que la page est ouverte — le navigateur doit toujours
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
    // console cargo : lance « cargo test » ou « cargo run » dans la racine du
    // projet et renvoie la sortie en flux (chunked, texte brut), terminée par
    // la ligne « [code de sortie: N] ». Une seule commande à la fois (cargo
    // verrouille target/) : 409 sinon. « cargo run » garde la connexion
    // ouverte tant que le jeu tourne — la fermeture de la fenêtre du jeu
    // termine la réponse ; si l'onglet se ferme, le jeu continue de tourner.
    if (req.method === "POST" && pathname === "/api/cargo") {
      let body = "";
      for await (const chunk of req) body += chunk;
      let command = "test";
      try {
        command = JSON.parse(body || "{}").command || "test";
      } catch (_) { /* corps absent ou invalide → test */ }
      if (command !== "test" && command !== "run") {
        res.writeHead(400, { "Content-Type": "text/plain; charset=utf-8" });
        res.end("Commande inconnue : " + command + " (test | run)");
        return;
      }
      if (cargoChild) {
        res.writeHead(409, { "Content-Type": "text/plain; charset=utf-8" });
        res.end("Une commande cargo est déjà en cours");
        return;
      }
      const child = spawn("cargo", command === "test" ? ["test"] : ["run"], { cwd: ROOT });
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
    res.end("404 — " + pathname);
  } catch (err) {
    res.writeHead(500, { "Content-Type": "text/plain; charset=utf-8" });
    res.end("Erreur serveur : " + err.message);
  }
});

server.listen(PORT, "127.0.0.1", () => {
  console.log("Éditeur de la place de marché :");
  console.log("  http://localhost:" + PORT + "/");
  console.log("  Fichier lié : " + MARKETPLACE);
  console.log("  (Ctrl+C pour arrêter)");
});
