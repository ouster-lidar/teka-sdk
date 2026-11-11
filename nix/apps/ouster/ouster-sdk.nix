# Ouster Sensor SDK Python distribution built from source against nixpkgs
# cmake / nanobind / GLFW so Wayland viz uses the native GLFW stack.
#
# `src` is the public ouster-sdk flake input (github.com/ouster-lidar/ouster-sdk).
{
  lib,
  stdenv,
  python312Packages,
  autoPatchelfHook,
  libglvnd,
  src,
}:

let
  version = "1.0.1";
  pkgs = python312Packages.pkgs;
  # SDK python/src/cpp/nb_func.cpp is a fork of nanobind 2.9.2 (setup.py pin).
  # nixpkgs nanobind 2.12 breaks that patched source.
  nanobindCompat = python312Packages.nanobind.overridePythonAttrs (_: rec {
    version = "2.9.2";
    src = pkgs.fetchPypi {
      pname = "nanobind";
      inherit version;
      hash = "sha256-52CEct6Z03V1mBTKs+LJSro/nsgOYs/vjO1JXKXCfW4=";
    };
  });
  pyTag = "cp${
    lib.replaceStrings [ "." ] [ "" ] (lib.versions.majorMinor python312Packages.python.version)
  }";
in
assert lib.assertMsg (pyTag == "cp312") "ouster-sdk.nix: python must be 3.12.";

python312Packages.buildPythonPackage rec {
  pname = "ouster_sdk";
  inherit version src;
  format = "setuptools";

  dontUseCmakeConfigure = true;
  postUnpack = ''
    sourceRoot="$sourceRoot/python"
  '';

  nativeBuildInputs = with python312Packages; [
    autoPatchelfHook
    nanobindCompat
    numpy
    setuptools
    wheel
    pkgs.cmake
    pkgs.ninja
    pkgs."pkg-config"
  ];

  pythonRelaxDeps = [
    "psutil"
    "click"
    "prettytable"
    "requests"
    "more-itertools"
    "numpy"
    "laspy"
    "threadpoolctl"
    "pillow"
    "packaging"
    "rosbags"
    "zeroconf"
    "flask"
    "waitress"
    "setuptools"
    "nanobind"
    "pytest-asyncio"
  ];

  buildInputs = [
    stdenv.cc.cc
    libglvnd
    pkgs.curl
    pkgs.eigen
    pkgs.flatbuffers
    pkgs.glfw
    pkgs.libnl
    pkgs.libpcap
    pkgs.libpng
    pkgs.libtins
    pkgs.libzip
    pkgs.openssl
    pkgs."ceres-solver"
    pkgs.robin-map
    pkgs.tbb
    pkgs.zlib
    pkgs.zstd
  ];

  dontStrip = true;

  postPatch = ''
    # Upstream tree runs version_manager (needs git / _private). Use VERSION.
    rm -f ../_private/scripts/dev_script_library/dev_version_manager.py
    ${pkgs.buildPackages.python3.interpreter} ${./patch-ouster-sdk-setup.py}
  '';

  preBuild = ''
    export VCPKG_MANIFEST_MODE=OFF
    export USE_OPENMP=OFF
    export BUILD_ML_INFERENCE=OFF
    export OUSTER_SDK_CMAKE_ARGS="-DBUILD_PERCEPTION=OFF -DBUILD_ML_INFERENCE=OFF -DSKIP_NB_STUBGEN=${
      if stdenv.buildPlatform == stdenv.hostPlatform then "OFF" else "ON"
    } -DFLATBUFFERS_FLATC_EXECUTABLE=${lib.getExe pkgs.buildPackages.flatbuffers}"
  '';

  propagatedBuildInputs = with python312Packages; [
    click
    flask
    glfw
    laspy
    more-itertools
    nanobindCompat
    numpy
    packaging
    pillow
    prettytable
    psutil
    requests
    rosbags
    setuptools
    threadpoolctl
    waitress
    zeroconf
  ];

  doCheck = false;

  meta = {
    description = "Ouster Sensor SDK built from source against nixpkgs GLFW";
    homepage = "https://github.com/ouster-lidar/ouster-sdk/";
    license = lib.licenses.bsd3;
    mainProgram = "ouster-cli";
  };
}
