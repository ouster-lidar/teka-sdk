{
  pkgs,
  teka,
}:
pkgs.callPackage ../hello-ouster-world/viewer.nix {
  inherit teka;
  src = ./.;
  envPrefix = "HELLO_OUSTER_DYNAMO";
  extraPythonPath = [
    "dynamo-utils"
    "osf-socket"
  ];
  extraEnvironment = {
    HELLO_OUSTER_DYNAMO_CWD = "/var/lib/hello-ouster-dynamo";
    HELLO_OUSTER_DYNAMO_SHELL = "${pkgs.bash}/bin/bash";
  };
  extraBindPaths = [ "-/run/ouster-dynamo" ];
  extraPythonFiles = map (file: ./. + "/${file}") [
    "osf-socket/osf_socket.py"
    "dynamo-utils/detect_utils.py"
    "dynamo-utils/dynamo_viz_utils.py"
    "dynamo-utils/ouster_dynamo_api.py"
    "dynamo-utils/configs/__init__.py"
    "dynamo-utils/configs/viz_config.py"
    "dynamo-utils/configs/viz_configs.json"
    "dynamo-utils/configs/detect_configs.json"
  ];
}
