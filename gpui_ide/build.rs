use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

const LUA_SOURCES: &[&str] = &[
    "lapi.c",
    "lauxlib.c",
    "lcode.c",
    "lctype.c",
    "ldebug.c",
    "ldo.c",
    "ldump.c",
    "lfunc.c",
    "lgc.c",
    "llex.c",
    "lmem.c",
    "lobject.c",
    "lopcodes.c",
    "lparser.c",
    "lstate.c",
    "lstring.c",
    "ltable.c",
    "ltm.c",
    "lundump.c",
    "lvm.c",
    "lzio.c",
];

fn main() {
    let target = env::var("TARGET").unwrap_or_default();
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());

    // --- VERSIONINFO resource ---
    if target.contains("windows") {
        let rc = manifest_dir.join("resources").join("app.rc");
        let res = out_dir.join("app_res.o");
        println!("cargo:rerun-if-changed={}", rc.display());
        match Command::new("windres")
            .args([
                "-i",
                rc.to_str().unwrap(),
                "-o",
                res.to_str().unwrap(),
                "--input-format=rc",
                "--output-format=coff",
                "-F",
                "pe-x86-64",
            ])
            .status()
        {
            Ok(s) if s.success() => {
                println!("cargo:rustc-link-arg={}", res.display());
            }
            Ok(s) => println!("cargo:warning=windres failed with {s}"),
            Err(e) => println!("cargo:warning=windres not found ({e})"),
        }

        // Static libunwind from LLVM-MinGW (LLVM_MINGW_LIB is set by the
        // build scripts; without it the standard import library is used).
        if let Ok(llvm_lib) = env::var("LLVM_MINGW_LIB") {
            let lib_dir = PathBuf::from(llvm_lib);
            let static_unwind = lib_dir.join("libunwind.a");
            if static_unwind.is_file() {
                println!("cargo:rustc-link-search=native={}", lib_dir.display());
                println!("cargo:rustc-link-arg={}", static_unwind.display());
            }
        }
    }

    // --- In-process luac (Lua 5.5.1 / LUA_32BITS) ---
    let lua_dir = manifest_dir.join("../mspm0_lua/third_party/lua");
    let api_c = manifest_dir.join("native/mspm0_luac_api.c");
    let api_h = manifest_dir.join("native/mspm0_luac_api.h");
    println!("cargo:rerun-if-changed={}", api_c.display());
    println!("cargo:rerun-if-changed={}", api_h.display());
    for name in LUA_SOURCES {
        println!("cargo:rerun-if-changed={}", lua_dir.join(name).display());
    }
    println!(
        "cargo:rerun-if-changed={}",
        lua_dir.join("luaconf.h").display()
    );
    println!("cargo:rerun-if-changed={}", lua_dir.join("lua.h").display());

    let cc = find_c_compiler();
    let mut objs = Vec::new();
    for name in LUA_SOURCES {
        let src = lua_dir.join(name);
        let obj = out_dir.join(format!("{name}.o"));
        compile_c(&cc, &src, &obj, &lua_dir);
        objs.push(obj);
    }
    {
        let obj = out_dir.join("mspm0_luac_api.o");
        compile_c(&cc, &api_c, &obj, &lua_dir);
        objs.push(obj);
    }

    let lib = out_dir.join("libmspm0_luac.a");
    let ar = find_ar();
    let mut cmd = Command::new(&ar);
    cmd.arg("rcs").arg(&lib);
    for o in &objs {
        cmd.arg(o);
    }
    run(&mut cmd, "ar");

    println!("cargo:rustc-link-search=native={}", out_dir.display());
    println!("cargo:rustc-link-lib=static=mspm0_luac");
}

fn find_c_compiler() -> PathBuf {
    if let Ok(cc) = env::var("CC") {
        return PathBuf::from(cc);
    }
    for name in ["x86_64-w64-mingw32-clang", "clang", "gcc"] {
        if which(name) {
            return PathBuf::from(name);
        }
    }
    panic!("no C compiler found for in-process luac");
}

fn find_ar() -> PathBuf {
    for name in ["llvm-ar", "x86_64-w64-mingw32-llvm-ar", "ar"] {
        if which(name) {
            return PathBuf::from(name);
        }
    }
    panic!("no ar found");
}

fn which(name: &str) -> bool {
    Command::new("where.exe")
        .arg(name)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn compile_c(cc: &Path, src: &Path, obj: &Path, include: &Path) {
    let mut cmd = Command::new(cc);
    cmd.args([
        "-c",
        "-std=c99",
        "-O2",
        "-DLUA_32BITS",
        "-DNDEBUG",
        "-fno-stack-protector",
    ]);
    cmd.arg(format!("-I{}", include.display()));
    cmd.arg(src);
    cmd.arg("-o");
    cmd.arg(obj);
    run(&mut cmd, "cc");
}

fn run(cmd: &mut Command, label: &str) {
    let status = cmd
        .status()
        .unwrap_or_else(|e| panic!("{label} failed to start: {e}"));
    if !status.success() {
        panic!("{label} failed: {status:?} ({cmd:?})");
    }
}
