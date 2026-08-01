# Firmware

Build packaging places the current base firmware, module catalog and module
images in this directory beside the IDE executable. Runtime validation uses
`release/catalog_manifest.json`; module binaries are verified by SHA-256
before upload.

Packaged layout (assembled by `gpui_ide/package_firmware.ps1` from the
firmware build outputs):

```text
firmware/
├── build_composed/firmware_core.bin   # default reflash image (core + empty slots)
├── build_modular/mspm0_lua_modular.bin
├── build_bytecode/mspm0_lua_bytecode.bin  # UART BSL menu
├── build_modules/index.json           # module catalog index
├── build_modules/<module>/slot<N>/<module>.bin
├── release/                           # catalog manifest, API metadata, docs, lua, test vectors
├── mspm0_lua_modular.bin              # root-level fallbacks
└── mspm0_lua_bytecode.bin
```

The firmware picker (`src/bsl.rs::find_default_firmware`) prefers
`build_composed/firmware_core.bin` so a reflash can never retain modules from
an older catalog; modules are then deployed into the empty slots by the IDE
or `tools/serial_module_set.py`.
