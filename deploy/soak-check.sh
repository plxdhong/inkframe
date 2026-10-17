#!/usr/bin/env bash
# One health snapshot of the frame for the soak test (P4), appended as a CSV row.
#   PI=frame ./deploy/soak-check.sh           # → data/soak.csv
# Run it once or twice a day; a growing rss_kb or fds column means a leak, any
# change in svc_restarts or task_restarts means something was recovered.
set -euo pipefail

PI=${PI:-pi@10.12.0.1}
LOG=${LOG:-data/soak.csv}
mkdir -p "$(dirname "$LOG")"

row=$(ssh -o ConnectTimeout=10 "$PI" 'set -e
  pid=$(systemctl show -p MainPID --value inkframe)
  rss=$(awk "/VmRSS/{print \$2}" /proc/$pid/status)
  fds=$(sudo ls /proc/$pid/fd | wc -l)
  nres=$(systemctl show -p NRestarts --value inkframe)
  disk=$(df --output=pcent /data | tail -1 | tr -d " %")
  status=$(curl -q -s -m10 --noproxy "*" http://127.0.0.1:8080/api/v1/status)
  echo "$rss,$fds,$nres,$disk,$(cut -d" " -f1 /proc/uptime | cut -d. -f1)"
  echo "$status"')

host=$(head -1 <<<"$row")
status=$(tail -n +2 <<<"$row")
fields=$(python3 -I -c '
import json, sys
d = json.loads(sys.argv[1])
disp = d.get("display", {})
print(",".join(str(x) for x in [
    d.get("healthy"),
    sum(t.get("restarts", 0) for t in d.get("tasks", [])),
    disp.get("total_refreshes"),
    disp.get("consecutive_failures"),
    d.get("photos"),
    (d.get("shown") or {}).get("content", ""),
    (d.get("network") or {}).get("online"),
]))' "$status")

[ -s "$LOG" ] || echo "time,rss_kb,fds,svc_restarts,data_disk_pct,host_uptime_s,healthy,task_restarts,refreshes,refresh_failures,photos,shown,online" > "$LOG"
echo "$(date '+%F %T'),$host,$fields" | tee -a "$LOG"
