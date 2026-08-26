# Ouster marketing images from the public Sanity CDN — pinned in the Nix store
# for offline Slidev builds (no runtime network on edge devices).
{
  fetchurl,
}:

let
  mk =
    name: url: hash:
    fetchurl {
      inherit
        name
        url
        hash
        ;
    };
in
{
  hero =
    mk "hero-physical-ai.png"
      "https://cdn.sanity.io/images/zqlkz0u8/production/5ed3076707a7f2bef3511a0701f3746e09028cee-2480x1242.png?w=1400&auto=format"
      "sha256-sULFg2grKFiCVUJ1MB2sKmiex1Ch9IX0R84f/XO++Ig=";

  logo =
    mk "logo.svg"
      "https://cdn.sanity.io/images/zqlkz0u8/production/0b8e8323bace28e6f159031729d6938f15b12b39-72x16.svg"
      "sha256-f6dG3F2mBLo5Bq6Qn8rgHj1x3XKQJeih0KIp0iHlbF8=";

  sensorLineup =
    mk "sensor-lineup.png"
      "https://cdn.sanity.io/images/zqlkz0u8/production/bf7b292c2c2d6a3d2a04c14a6bf911c47e135051-2538x1458.png?w=1400&auto=format"
      "sha256-pPQX00+hgwzlbm6iwIHiEa2SPzDuWFwxLLYKvJc5K7o=";

  robotics =
    mk "robotics.png"
      "https://cdn.sanity.io/images/zqlkz0u8/production/47c794582217418e653aac21b5137420e2beea18-1920x1080.png?w=1280&auto=format"
      "sha256-XUYb093YT4RICkAxKOoGPvNhYl5cI10s5cFMRGQITr0=";

  smartInfrastructure =
    mk "smart-infrastructure.png"
      "https://cdn.sanity.io/images/zqlkz0u8/production/e78a422edf6cda967cefc1094cdcc805288583bc-1900x1069.png?w=1280&auto=format"
      "sha256-8PqAYp/Xc4lss8QxqRLf7Dqaayp4wlWnCm1vp94ypo4=";

  marketsHero =
    mk "markets-hero.png"
      "https://cdn.sanity.io/images/zqlkz0u8/production/816739902183316829be5ca5cdce207d298fa461-1728x1080.png?w=1280&auto=format"
      "sha256-d2VBf5Hdx7p74QUu6Eam04zwz5hJXAooRhESGJr/9R4=";

  sensorOs1 =
    mk "sensor-os1.png"
      "https://cdn.sanity.io/images/zqlkz0u8/production/c0d6ed2fada6ae637dcc2f0d6b48c90286714aeb-1159x1141.png?w=900&auto=format"
      "sha256-HzlwRYMFdDk6V6okyenWwP2j6pheLwm1IgHbRR//jbU=";
}
