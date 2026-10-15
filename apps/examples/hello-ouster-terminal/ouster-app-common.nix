# Ouster terminal constants and viz restart loop.
{
  lib,
  writeShellScript,
  pickOusterSensorPy,
  ousterUdpFirewallRange ? "7000-8000",
  ousterLidarDataPort ? 7602,
  ousterImuDataPort ? 7603,
  logTag,
  useCustomPorts ? true,
}:

let
  colorVars = ''
    if [[ -t 2 ]]; then
      _c_info=$'\033[1;36m'
      _c_good=$'\033[1;32m'
      _c_bad=$'\033[1;31m'
      _c_warn=$'\033[1;33m'
      _c_rst=$'\033[0m'
    else
      _c_info=""
      _c_good=""
      _c_bad=""
      _c_warn=""
      _c_rst=""
    fi
  '';
in
{
  inherit ousterUdpFirewallRange ousterLidarDataPort ousterImuDataPort;

  ousterVizLoopSh =
    {
      caBundlePath,
      wayland ? false,
      swayBin ? null,
      swayLayoutReady ? false,
      vizExitFootHint ? false,
    }:
    writeShellScript "${logTag}-ouster-viz-loop" ''
      set +e
      hostCa=/etc/ssl/certs/ca-bundle.crt
      export SSL_CERT_FILE="${caBundlePath}"
      [[ -r "$hostCa" ]] && export SSL_CERT_FILE="$hostCa"
      ${lib.optionalString wayland ''
        export GLFW_PLATFORM=wayland
        export GLFW_WAYLAND_APP_ID=ouster-viz
        unset DISPLAY 2>/dev/null || true
      ''}
      ${colorVars}
      ${lib.optionalString (swayLayoutReady && swayBin != null) ''
        layout_ready="$HOME/layout-ready"
        for _ in $(seq 1 200); do
          [[ -f "$layout_ready" ]] && break
          sleep 0.05
        done
        swaymsg() { command ${swayBin}/bin/swaymsg "$@" >/dev/null || true; }
      ''}

      pick_sensor_host() {
        python3 ${pickOusterSensorPy}
      }

      msf_print_ouster_viz_cmd() {
        local sensor_host="$1"
        printf '%b[*]%b' "$_c_info" "$_c_rst" >&2
        ${lib.optionalString useCustomPorts ''
          printf ' %q' ouster-cli source \
            --lidar-port ${toString ousterLidarDataPort} \
            --imu-port ${toString ousterImuDataPort} "$sensor_host" viz
        ''}
        ${lib.optionalString (!useCustomPorts) ''
          printf ' %q' ouster-cli source "$sensor_host" viz
        ''}
        printf '\n' >&2
      }

      while true; do
        sensor_host="$(pick_sensor_host)"
        if [[ -z "$sensor_host" ]]; then
          printf '%b[!]%b No sensor picked; retrying…\n' "$_c_warn" "$_c_rst" >&2
          sleep 3
          continue
        fi
        printf '%b[*]%b ${logTag}: lidar viz @ %s' \
          "$_c_info" "$_c_rst" "$sensor_host" >&2
        ${lib.optionalString useCustomPorts ''
          printf ' (lidar %s, imu %s)' ${toString ousterLidarDataPort} ${toString ousterImuDataPort} >&2
        ''}
        printf '.\n' >&2
        ${lib.optionalString (swayBin != null) ''
          swaymsg focus parent
          swaymsg focus parent
          swaymsg focus right
        ''}
        msf_print_ouster_viz_cmd "$sensor_host"
        ${lib.optionalString useCustomPorts ''
          ouster-cli source --lidar-port ${toString ousterLidarDataPort} \
            --imu-port ${toString ousterImuDataPort} "$sensor_host" viz
        ''}
        ${lib.optionalString (!useCustomPorts) ''
          ouster-cli source "$sensor_host" viz
        ''}
        st=$?
        if [[ "$st" -eq 0 ]]; then
          printf '%b[+]%b viz exited %s — returning to sensor picker in 5s${lib.optionalString vizExitFootHint " (close this terminal to stop)"}\n' \
            "$_c_good" "$_c_rst" "$st" >&2
        else
          printf '%b[-]%b viz exited %s — returning to sensor picker in 5s${lib.optionalString vizExitFootHint " (close this terminal to stop)"}\n' \
            "$_c_bad" "$_c_rst" "$st" >&2
        fi
        sleep 5
      done
    '';
}
