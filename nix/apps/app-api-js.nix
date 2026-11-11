# Shared app web helper (prepended into app *.js at image build time).
{ runCommand }:

runCommand "app-api-js" { } ''
  mkdir -p $out
  cp ${../web/app-api.js} $out/app-api.js
''
