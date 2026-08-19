#!/usr/bin/env bash
# Lance l'éditeur de la place de marché en un clic : démarre le serveur local
# (server.mjs) s'il n'est pas déjà actif, puis ouvre l'outil dans le
# navigateur par défaut. Utilisé par le fichier marketplace-editor.desktop.
#
# Usage :
#   launch-editor.sh            # démarre le serveur (si besoin) et ouvre le tool
#   launch-editor.sh stop       # arrête le serveur de l'éditeur
#   launch-editor.sh <port>     # serveur sur un autre port (défaut : 8123)
#
# Le serveur tourne en arrière-plan (log : /tmp/marketplace-editor-<port>.log,
# PID : /tmp/marketplace-editor-<port>.pid) ; un second clic sur le raccourci
# rouvre simplement le navigateur sans redémarrer le serveur.
set -u

PORT="${1:-8123}"

if [ "${PORT}" = "stop" ]; then
  PIDFILE="${TMPDIR:-/tmp}/marketplace-editor.pid"
  if [ -f "${PIDFILE}" ]; then
    kill "$(cat "${PIDFILE}")" 2>/dev/null || true
    rm -f "${PIDFILE}"
  fi
  pkill -f "marketplace-editor/server.mjs" 2>/dev/null || true
  echo "Serveur de l'éditeur arrêté."
  exit 0
fi

URL="http://localhost:${PORT}"
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
LOG="${TMPDIR:-/tmp}/marketplace-editor-${PORT}.log"
PIDFILE="${TMPDIR:-/tmp}/marketplace-editor-${PORT}.pid"

if curl -sf -o /dev/null "${URL}/"; then
  echo "Serveur déjà actif : ${URL}"
else
  echo "Démarrage du serveur de l'éditeur (${URL}, log : ${LOG})…"
  nohup node "${ROOT}/tools/marketplace-editor/server.mjs" --port "${PORT}" >"${LOG}" 2>&1 &
  echo $! > "${PIDFILE}"
  i=0
  while ! curl -sf -o /dev/null "${URL}/"; do
    i=$((i + 1))
    if [ "${i}" -ge 20 ]; then
      echo "Le serveur n'a pas démarré — voir ${LOG}" >&2
      exit 1
    fi
    sleep 0.5
  done
fi

if [ -n "${MARKETPLACE_NO_BROWSER:-}" ]; then
  echo "Navigateur non ouvert (MARKETPLACE_NO_BROWSER) : ${URL}"
else
  xdg-open "${URL}" >/dev/null 2>&1 &
fi
