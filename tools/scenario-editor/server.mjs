#!/usr/bin/env node
// Serveur HTTP local pour l'éditeur de scénarios et d'objectifs (DAG).
//
// Permet de charger, éditer, valider et enregistrer les scénarios sous forme
// de fichiers JSON (`scenarios/*.json`) et d'exporter directement le code Rust
// généré dans `src/scenario_objectives.rs`.
//
// Usage : node tools/scenario-editor/server.mjs [--port 8124]
// Puis ouvrir http://localhost:8124 dans le navigateur.

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

let cargoChild = null;

const server = http.createServer(async (req, res) => {
  const pathname = decodeURIComponent(new URL(req.url, "http://localhost").pathname);

  try {
    // 1. Page principale de l'éditeur
    if (req.method === "GET" && (pathname === "/" || pathname === "/index.html")) {
      const html = fs.readFileSync(path.join(here, "index.html"));
      res.writeHead(200, { "Content-Type": "text/html; charset=utf-8" });
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

    // 8. Console Cargo (test / run)
    if (req.method === "POST" && pathname === "/api/cargo") {
      let body = "";
      for await (const chunk of req) body += chunk;
      let command = "test";
      try {
        command = JSON.parse(body || "{}").command || "test";
      } catch (_) {}

      if (cargoChild) {
        res.writeHead(409, { "Content-Type": "text/plain; charset=utf-8" });
        res.end("Une commande cargo est déjà en cours");
        return;
      }

      const child = spawn("cargo", command === "test" ? ["test"] : ["run"], { cwd: ROOT });
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
        send("\n[code de sortie: " + (code == null ? -1 : code) + "]\n");
        try { res.end(); } catch (_) {}
      };

      child.stdout.on("data", send);
      child.stderr.on("data", send);
      child.on("error", err => { send("[erreur: " + err.message + "]\n"); finish(-1); });
      child.on("close", finish);
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
  console.log("  (Ctrl+C pour arrêter)");
});
