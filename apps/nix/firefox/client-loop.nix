{
  writeText,
  runCommand,
  writeShellScript,
  caBundlePath,
}:

let
  firefoxAppPrefs = writeText "jetstream-portable-firefox-user.js" ''
    user_pref("network.trr.mode", 0);
    user_pref("network.captive-portal-service.enabled", false);
    user_pref("network.connectivity-service.enabled", false);
    user_pref("security.enterprise_roots.enabled", true);
    user_pref("datareporting.policy.dataSubmissionEnabled", false);
    user_pref("toolkit.telemetry.unified", false);
    user_pref("intl.locale.requested", "en-US");
    user_pref("gfx.webrender.all", true);
    user_pref("layers.acceleration.force-enabled", true);
  '';

  firefoxAppProfileSeed = runCommand "jetstream-portable-firefox-profile-seed" { } ''
    mkdir -p "$out"
    install -m444 ${firefoxAppPrefs} "$out/user.js"
  '';

  mkFirefoxClientLoop =
    {
      name,
      firefoxCliTail,
      profileRel,
      nssTools,
      p11-kit,
      firefox-bin,
    }:
    writeShellScript name ''
      set +e
      hostCa=/etc/ssl/certs/ca-bundle.crt
      export SSL_CERT_FILE="${caBundlePath}"
      [[ -r "$hostCa" ]] && export SSL_CERT_FILE="$hostCa"
      export MOZ_ENABLE_WAYLAND=1
      export MOZ_DISABLE_TELEMETRY=1
      export MOZ_LOG="''${MOZ_LOG:-}"
      predir="${firefoxAppProfileSeed}"
      profile="''${HOME}/${profileRel}"
      while true; do
        mkdir -p "$profile"
        install -m444 "$predir/user.js" "$profile/user.js"
        rm -f "$profile/.parentlock" "$profile/lock" 2>/dev/null || true
        if [[ ! -e "$profile/pkcs11.txt" ]]; then
          ${nssTools}/bin/certutil -N --empty-password -d sql:"$profile" >/dev/null 2>&1 || true
        fi
        if ! ${nssTools}/bin/modutil -dbdir sql:"$profile" -list "p11-kit-trust" >/dev/null 2>&1; then
          ${nssTools}/bin/modutil \
            -dbdir sql:"$profile" \
            -force \
            -add "p11-kit-trust" \
            -libfile "${p11-kit}/lib/pkcs11/p11-kit-trust.so" \
            >/dev/null 2>&1 || true
        fi
        ${firefox-bin}/bin/firefox \
          --no-remote \
          --profile "$profile" \
          ${firefoxCliTail} || true
        sleep 1
      done
    '';

in
mkFirefoxClientLoop
