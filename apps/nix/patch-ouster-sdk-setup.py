from pathlib import Path

p = Path("setup.py")
t = p.read_text()
start = t.find("    def install_deps(self, ext):")
end = t.find("    def cmake_config(self, ext):")
if start < 0 or end < 0:
    raise SystemExit("ouster-sdk.nix: install_deps/cmake_config not found in setup.py")
replacement = """    def install_deps(self, ext):
        self.cmake_log(
            f"CMake: Extension: {ext.name} Install Deps: Using nix-provided cmake/nanobind"
        )
        self._cmake_path = shutil.which("cmake")
        if self._cmake_path is None:
            raise RuntimeError("cmake not found on PATH")

"""
p.write_text(t[:start] + replacement + t[end:])

cmake = Path("CMakeLists.txt")
ct = cmake.read_text()
needle = "nanobind_add_stub2(\n  _bindings_stub"
if needle not in ct:
    raise SystemExit("ouster-sdk.nix: nanobind_add_stub2 block not found")
cmake.write_text(
    ct.replace(
        needle,
        "if(NOT SKIP_NB_STUBGEN)\n" + needle,
        1,
    ).replace(
        """  DEPENDS _bindings
  RECURSIVE
  INCLUDE_PRIVATE
)
""",
        """  DEPENDS _bindings
  RECURSIVE
  INCLUDE_PRIVATE
)
endif()
""",
        1,
    )
)
