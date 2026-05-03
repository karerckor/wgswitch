#!/bin/bash
#
# SwiftBar plugin: WireGuard quick switch via wgswitch.
# Filename `wgswitch.10s.sh` => SwiftBar refreshes every 10 seconds.
#
# <bitbar.title>wgswitch</bitbar.title>
# <bitbar.version>0.1.0</bitbar.version>
# <bitbar.author>Igor Olshevsky</bitbar.author>
# <bitbar.author.github>karerckor</bitbar.author.github>
# <bitbar.desc>WireGuard status + one-click connect/disconnect, powered by wgswitch.</bitbar.desc>
# <bitbar.dependencies>bash,jq,wgswitch</bitbar.dependencies>
# <bitbar.abouturl>https://github.com/karerckor/wgswitch</bitbar.abouturl>
#
# <swiftbar.title>wgswitch</swiftbar.title>
# <swiftbar.version>0.1.0</swiftbar.version>
# <swiftbar.author>Igor Olshevsky</swiftbar.author>
# <swiftbar.author.github>karerckor</swiftbar.author.github>
# <swiftbar.desc>WireGuard status + one-click connect/disconnect, powered by wgswitch.</swiftbar.desc>
# <swiftbar.dependencies>bash,jq,wgswitch</swiftbar.dependencies>
# <swiftbar.abouturl>https://github.com/karerckor/wgswitch</swiftbar.abouturl>
#
# Prerequisites:
#   * `wgswitch` installed (cargo install wgswitch)
#   * `jq` installed (brew install jq)
#   * sudoers NOPASSWD entry, e.g.:
#       %admin ALL=(root) NOPASSWD: /usr/local/bin/wgswitch connect *, \
#                                   /usr/local/bin/wgswitch off,        \
#                                   /usr/local/bin/wgswitch off *,      \
#                                   /usr/local/bin/wgswitch list*,      \
#                                   /usr/local/bin/wgswitch status*
#
# This plugin only ever calls `wgswitch status --format json`. The status
# JSON already includes every discovered profile (status under root scans
# /etc/wireguard/ itself), so no separate `list` invocation is needed here.
# `list*` is in the sudoers example above purely so you can use bare
# `sudo wgswitch list` from a terminal without typing a password.

set -euo pipefail

# ---- locate binaries (without trusting $PATH; SwiftBar trims env) -------
SUDO="${SUDO:-/usr/bin/sudo}"

# Resolve wgswitch in the usual install locations. A `cargo install` lands
# in ~/.cargo/bin, so we include $HOME paths too. SECURITY: if the binary
# lives under a user-writable directory like ~/.cargo/bin and that exact
# path is added to `sudoers NOPASSWD`, any `cargo install --force` becomes a
# privilege-escalation primitive. Prefer copying it into a root-owned
# location for the sudoers rule:
#     sudo install -m 755 -o root ~/.cargo/bin/wgswitch /usr/local/bin/wgswitch
if [ -z "${WGSWITCH:-}" ]; then
  for candidate in \
      /usr/local/bin/wgswitch \
      /opt/homebrew/bin/wgswitch \
      "$HOME/.cargo/bin/wgswitch" \
      /usr/bin/wgswitch; do
    if [ -x "$candidate" ]; then
      WGSWITCH="$candidate"
      break
    fi
  done
fi
WGSWITCH="${WGSWITCH:-}"

if [ -z "${JQ:-}" ]; then
  for candidate in /opt/homebrew/bin/jq /usr/local/bin/jq /usr/bin/jq; do
    if [ -x "$candidate" ]; then
      JQ="$candidate"
      break
    fi
  done
fi
JQ="${JQ:-}"

# ---- collect snapshot ----------------------------------------------------
if [ -z "$WGSWITCH" ] || [ -z "$JQ" ]; then
  echo "🔒 wg | color=#888"
  echo "---"
  [ -z "$WGSWITCH" ] && echo "wgswitch not found (cargo install wgswitch) | color=red"
  [ -z "$JQ" ]       && echo "jq not found (brew install jq) | color=red"
  exit 0
fi

JSON="$("$SUDO" -n "$WGSWITCH" status --format json 2>/dev/null || true)"
if [ -z "$JSON" ]; then
  echo "🔒 wg | color=#888"
  echo "---"
  echo "wgswitch status failed | color=red"
  echo "Check sudoers NOPASSWD for $WGSWITCH | color=#888"
  echo "---"
  echo "Refresh | refresh=true"
  exit 0
fi

# Pull the fields we render into env vars from a single jq invocation.
# jq emits one shell `name=value` line per field; we eval it.
eval "$("$JQ" -r '
  ((.profiles[] | select(.active)) // {}) as $p |
  @sh "CURRENT=\(.current // "")",
  @sh "ACTIVE_EMOJI=\($p.emoji // "")",
  @sh "ACTIVE_LABEL=\($p.label // (.current // ""))",
  @sh "ACTIVE_DESC=\($p.description // "")",
  @sh "ENDPOINT=\(($p.peers // [])[0].endpoint // "")",
  @sh "HANDSHAKE=\(($p.peers // [])[0].last_handshake_ago_human // "")",
  @sh "TOTAL_RX=\(($p.totals // {}).rx_human // "")",
  @sh "TOTAL_TX=\(($p.totals // {}).tx_human // "")",
  @sh "RX_RATE=\(($p.totals // {}).rx_per_sec_human // "")",
  @sh "TX_RATE=\(($p.totals // {}).tx_per_sec_human // "")"
' <<<"$JSON")"

# ---- menubar line --------------------------------------------------------
if [ -n "$CURRENT" ]; then
  TITLE="${ACTIVE_EMOJI:-🟢}"
  if [ -n "$RX_RATE" ] && [ -n "$TX_RATE" ]; then
    TITLE="${TITLE} ↓${RX_RATE} ↑${TX_RATE}"
  fi
  echo "$TITLE"
else
  echo "○ wg | color=#888"
fi
echo "---"

# ---- active block --------------------------------------------------------
if [ -n "$CURRENT" ]; then
  HEAD="${ACTIVE_EMOJI:+$ACTIVE_EMOJI }${ACTIVE_LABEL:-$CURRENT} ($CURRENT)"
  echo "Active: $HEAD"
  [ -n "$ACTIVE_DESC" ] && echo "  $ACTIVE_DESC | color=#888"
  [ -n "$ENDPOINT" ]    && echo "  endpoint: $ENDPOINT | font=Menlo size=12"
  [ -n "$HANDSHAKE" ]   && echo "  handshake: $HANDSHAKE | font=Menlo size=12"
  if [ -n "$TOTAL_RX" ] && [ -n "$TOTAL_TX" ]; then
    echo "  total: ↓ $TOTAL_RX / ↑ $TOTAL_TX | font=Menlo size=12"
  fi
  if [ -n "$RX_RATE" ] && [ -n "$TX_RATE" ]; then
    echo "  speed: ↓ $RX_RATE / ↑ $TX_RATE | font=Menlo size=12"
  fi
  echo "---"
  echo "Disconnect | shell=$SUDO | param0=$WGSWITCH | param1=--notify | param2=off | terminal=false | refresh=true"
  echo "---"
fi

# ---- connect submenu -----------------------------------------------------
echo "Connect"
"$JQ" -r '
  .profiles[] |
  [.id, (.emoji // ""), (.label // .id), (.active|tostring)] | @tsv
' <<<"$JSON" | while IFS=$'\t' read -r id emoji label active; do
  prefix=""
  [ -n "$emoji" ] && prefix="$emoji "
  if [ "$active" = "true" ]; then
    echo "-- ✓ ${prefix}${label} (${id}) | color=#888"
  else
    echo "-- ${prefix}${label} (${id}) | shell=$SUDO | param0=$WGSWITCH | param1=--notify | param2=connect | param3=$id | terminal=false | refresh=true"
  fi
done

echo "---"
echo "Refresh | refresh=true"
