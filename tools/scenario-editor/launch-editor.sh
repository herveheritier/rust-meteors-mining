#!/usr/bin/env bash
# Lance l'éditeur de scénarios et d'objectifs (DAG) en un clic : démarre le serveur
# local (server.mjs) s'il n'est pas déjà actif, puis ouvre l'outil dans le
# navigateur par défaut. Utilisé par le fichier scenario-editor.desktop.
#
# Usage :
#   launch-editor.sh            # démarre le serveur (si besoin) et ouvre le tool
#   launch-editor.sh stop       # arrête le serveur de l'éditeur
#   launch-editor.sh <port>     # serveur sur un autre port (défaut : 8124)

set -u

PORT="${1:-8124}"

if [ "${PORT}" = "stop" ]; then
  PIDFILE="${TMPDIR:-/tmp}/scenario-editor.pid"
  if [ -f "${PIDFILE}" ]; then
    kill "$(cat "${PIDFILE}")" 2>/dev/null || true
    rm -f "${PIDFILE}"
  fi
  pkill -f "scenario-editor/server.mjs" 2>/dev/null || true
  echo "Serveur de l'éditeur de scénarios arrêté."
  exit 0
fi

URL="http://localhost:${PORT}"
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
LOG="${TMPDIR:-/tmp}/scenario-editor-${PORT}.log"
PIDFILE="${TMPDIR:-/tmp}/scenario-editor-${PORT}.pid"

if curl -sf -o /dev/null "${URL}/"; then
  echo "Serveur déjà actif : ${URL}"
else
  echo "Démarrage du serveur de l'éditeur de scénarios (${URL}, log : ${LOG})…"
  nohup node "${ROOT}/tools/scenario-editor/server.mjs" --port "${PORT}" >"${LOG}" 2>&1 &
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

if [ -n "${SCENARIO_NO_BROWSER:-}" ]; then
  echo "Navigateur non ouvert (SCENARIO_NO_BROWSER) : ${URL}"
else
  xdg-open "${URL}" >/dev/null 2>&1 &
fi
