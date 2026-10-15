#!/system/bin/sh
# Samples the CPU time of the app's threads on the device, so sampling costs
# no adb round trips. Waits for the process, then prints one line per interval:
#
#   <epoch seconds> main <ns> js <ns> render <ns> total <ns>
#
# Cumulative CPU nanoseconds from /proc/<pid>/task/*/schedstat: `main` is the
# UI thread, `js` React Native's JavaScript thread (comm mqt_v_js), `render`
# the RenderThread and `total` every thread of the process.
#
# Usage: sampler-android.sh <package> [interval seconds] [max samples]
pkg=$1
interval=${2:-0.05}
max=${3:-400}

pid=""
i=0
while [ -z "$pid" ] && [ "$i" -lt 2000 ]; do
  pid=$(pidof "$pkg")
  i=$((i + 1))
  [ -z "$pid" ] && sleep 0.01
done
if [ -z "$pid" ]; then
  echo "no process for $pkg"
  exit 1
fi
echo "pid $pid"

now() {
  if [ -n "$EPOCHREALTIME" ]; then echo "$EPOCHREALTIME"; else date +%s.%N; fi
}

n=0
while [ "$n" -lt "$max" ] && [ -d "/proc/$pid" ]; do
  n=$((n + 1))
  total=0; js=0; render=0
  for t in /proc/$pid/task/*; do
    read -r comm <"$t/comm" 2>/dev/null || continue
    read -r cpu _ _ <"$t/schedstat" 2>/dev/null || continue
    total=$((total + cpu))
    case "$comm" in
      mqt_v_js) js=$((js + cpu)) ;;
      RenderThread) render=$((render + cpu)) ;;
    esac
  done
  read -r main _ _ <"/proc/$pid/task/$pid/schedstat" 2>/dev/null || break
  echo "$(now) main $main js $js render $render total $total"
  sleep "$interval"
done
