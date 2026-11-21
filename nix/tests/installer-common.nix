{
  validateReleaseTag = ''
    if (( $# > 1 )) || [[ -n "''${1:-}" && ! "$1" =~ ^v[0-9][A-Za-z0-9._-]*$ ]]; then
      echo 'Usage: test-published-install [RELEASE_TAG]' >&2
      exit 1
    fi
  '';

  stageInstaller = directory: ''
    cp ${../../install.sh} "${directory}/install.sh"
    printf '%s\n' "''${1:-}" > "${directory}/release-tag"
  '';

  installPublished = directory: ''
    release_tag=$(cat ${directory}/release-tag)
    if [ -n "$release_tag" ]; then
      cat ${directory}/install.sh | NIX_INSTALLER_NO_CONFIRM=true sh -s -- "$release_tag"
    else
      curl -fsSL https://raw.githubusercontent.com/ouster-lidar/teka-sdk/main/install.sh | NIX_INSTALLER_NO_CONFIRM=true sh
    fi
  '';
}
