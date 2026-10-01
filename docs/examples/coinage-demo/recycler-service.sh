#!/bin/sh
# Supervises one recycler-bot run directory with a per-user launchd agent.
# launchd restarts the bot only after a non-zero exit, at most once a minute.
# The bot exits 0 when it completes, halts for inspection or is asked to stop.
set -eu

label=io.parity.coinage-demo.recycler-bot
here=$(cd "$(dirname "$0")" && pwd)
node=${NODE:-/opt/homebrew/bin/node}
domain="gui/$(id -u)"

usage() {
  echo "usage: $0 start|stop|status|uninstall <run-dir>" >&2
  exit 2
}
[ $# -eq 2 ] || usage
command=$1
run=$(cd "$here" && mkdir -p "$2" && cd "$2" && pwd)
account=$("$node" -e '
  const r = JSON.parse(require("fs").readFileSync(process.argv[1], "utf8"));
  const name = r.devAccount || "Bob";
  if (!["Alice", "Bob", "Charlie", "Dave", "Eve", "Ferdie"].includes(name)) throw Error("Unknown dev account");
  process.stdout.write(name.toLowerCase());
' "$run/run.json")
[ "$account" = bob ] || label="$label.$account"
plist="$HOME/Library/LaunchAgents/$label.plist"


case $command in
start)
  [ -f "$run/state.json" ] || {
    echo "No state.json in $run; create the run with --init first" >&2
    exit 1
  }
  rm -f "$run/STOP"
  mkdir -p "$HOME/Library/LaunchAgents"
  # node loads tsx in-process, so launchd supervises the bot itself, not a wrapper.
  # caffeinate keeps the machine from idle or system sleep while on AC power.
  # It cannot prevent sleep on lid close; the bot then reconciles on wake.
  cat >"$plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key><string>$label</string>
  <key>ProgramArguments</key>
  <array>
    <string>/usr/bin/caffeinate</string><string>-i</string><string>-s</string>
    <string>$node</string><string>--import</string><string>tsx</string>
    <string>$here/recycler-bot.ts</string>
    <string>--run</string>
    <string>--output</string><string>$run</string>
  </array>
  <key>WorkingDirectory</key><string>$here</string>
  <key>RunAtLoad</key><true/>
  <key>KeepAlive</key><dict><key>SuccessfulExit</key><false/></dict>
  <key>ThrottleInterval</key><integer>60</integer>
  <key>ProcessType</key><string>Background</string>
  <key>StandardOutPath</key><string>$run/bot.log</string>
  <key>StandardErrorPath</key><string>$run/bot.log</string>
</dict>
</plist>
EOF
  launchctl bootout "$domain/$label" 2>/dev/null || true
  launchctl bootstrap "$domain" "$plist"
  echo "Started $label for $run"
  ;;
stop)
  # The bot settles any in-flight transaction before it exits.
  touch "$run/STOP"
  for _ in $(seq 1 300); do
    launchctl print "$domain/$label" 2>/dev/null | grep -q "state = running" || break
    sleep 1
  done
  launchctl bootout "$domain/$label" 2>/dev/null || true
  echo "Stopped $label"
  ;;
status)
  launchctl print "$domain/$label" 2>/dev/null |
    grep -E "^\s+(state|pid|runs|last exit code) =" || echo "$label is not loaded"
  [ -f "$run/status.json" ] && "$node" -e '
    const s = JSON.parse(require("fs").readFileSync(process.argv[1], "utf8"));
    const age = Math.round((Date.now() - Date.parse(s.heartbeatAt)) / 1000);
    console.log(JSON.stringify({ phase: s.phase, heartbeatAgeSeconds: age, connection: s.connection,
      finalized: s.finalized?.number, action: s.action, reason: s.reason, endsAt: s.endsAt,
      held: s.held, heldValue: s.heldValue, heldByDenomination: s.heldByDenomination,
      pending: s.pending, counters: s.counters, balances: s.balances }, null, 2));
  ' "$run/status.json"
  ;;
uninstall)
  launchctl bootout "$domain/$label" 2>/dev/null || true
  rm -f "$plist"
  echo "Removed $label"
  ;;
*) usage ;;
esac
