// A lot of code from https://github.com/TomBebb/jit.rs/blob/master/sys/build.rs
use std::{env, fs, io, path::Path, process::Command};

use std::process::Stdio;

const GEN_BINDINGS: bool = false;

#[cfg(windows)]
static FINAL_LIB: &'static str = "libjit.dll";

#[cfg(not(windows))]
static FINAL_LIB: &'static str = "libjit.a";

static MINGW: &'static str = "c:/mingw";

static INSTALL_AUTOTOOLS_MSG:&'static str = "Failed to generate configuration script. Did you forget to install autotools, bison, flex, and libtool?";

static USE_CARGO_MSG: &'static str =
    "Build script should be ran with Cargo, run `cargo build` instead";

#[cfg(windows)]
static INSTALL_COMPILER_MSG: &'static str =
    "Failed to configure the library for your platform. Did you forget to install MinGW and MSYS? (Searched in c:/mingw)";
#[cfg(not(windows))]
static INSTALL_COMPILER_MSG: &'static str =
    "Failed to configure the library for your platform. Did you forget to install a C compiler?";

// PathExt::exists isn't stable, so fake it by querying file metadata.
fn exists<P: AsRef<Path>>(path: P) -> io::Result<bool> {
    match fs::metadata(path) {
        Ok(_) => Ok(true),
        Err(ref err) if err.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(err) => Err(err),
    }
}

fn main() {
    println!("cargo:rerun-if-changed=wrapper.h");

    if cfg!(windows) && !exists(&Path::new(MINGW)).unwrap() {
        panic!("{}", INSTALL_COMPILER_MSG);
    }
    let out_dir = env::var("OUT_DIR").ok().expect(USE_CARGO_MSG);
    let num_jobs = env::var("NUM_JOBS").ok().expect(USE_CARGO_MSG);
    let target = env::var("TARGET").ok().expect(USE_CARGO_MSG);
    let out_dir = Path::new(&*out_dir);
    let submod_path = out_dir.join("libjit");
    let final_lib_dir = submod_path.join("jit/.libs");

    if !exists(&final_lib_dir.join(FINAL_LIB)).unwrap() {
        run(
            Command::new("git")
                .args(&["clone", "git://git.savannah.gnu.org/libjit.git"])
                .arg(submod_path.as_os_str()),
            None,
        );
        run(
            Command::new("sh")
                .current_dir(&submod_path)
                .arg("bootstrap"),
            Some(INSTALL_AUTOTOOLS_MSG),
        );
        run(
            Command::new("sh")
                .current_dir(&submod_path)
                .env("CFLAGS", "-fPIC")
                .args(&[
                    "configure",
                    "--enable-static",
                    "--disable-shared",
                    &format!("--host={}", target),
                ]),
            Some(INSTALL_COMPILER_MSG),
        );
        run(
            Command::new("make")
                .arg(&format!("-j{}", num_jobs))
                .current_dir(&submod_path),
            None,
        );
    }
    let from = final_lib_dir.join(FINAL_LIB);
    let to = out_dir.join(FINAL_LIB);
    if let Err(error) = fs::copy(&from, &to) {
        panic!(
            "Failed to copy library from {:?} to {:?} due to {}",
            from, to, error
        )
    }
    println!(
        "cargo:rustc-link-search=native={}",
        out_dir
            .to_str()
            .expect("non-unicode characters in compiled libjit path")
    );
    println!("cargo:rustc-link-lib=static=jit");

    // CODE TO AUTO GEN THE BINDINGS:

    if GEN_BINDINGS {
        let mut builder = bindgen::Builder::default()
            .header("wrapper.h")
            .clang_arg(format!(
                "-I{}",
                submod_path
                    .join("include")
                    .to_str()
                    .expect("non-unicode characters in libjit path")
            ));

        if cfg!(unix) {
            builder = builder.clang_args(["-I/usr/include", "-I/usr/local/include"]);
            if let Ok(child) = Command::new("clang")
                .args(["-E", "-x", "c", "-", "-v"])
                .stdin(Stdio::piped())
                .stderr(Stdio::piped())
                .stdout(Stdio::piped())
                .spawn()
            {
                if let Ok(output) = child.wait_with_output() {
                    if output.status.success() {
                        let string_out = String::from_utf8(output.stderr).unwrap();
                        let start = string_out
                            .find("#include <...> search starts here:\n")
                            .unwrap();
                        let end = string_out.find("End of search list.").unwrap();
                        let include_paths: Vec<String> = string_out[start..end]
                            .split("\n")
                            .map(|s| format!("-I{}", s.trim()))
                            .collect();
                        builder = builder.clang_args(&include_paths);
                    }
                }
            }
        }

        let bindings = builder
            .layout_tests(false)
            .parse_callbacks(Box::new(bindgen::CargoCallbacks))
            .generate()
            .expect("Unable to generate bindings");
        bindings
            .write_to_file("src/lib.rs")
            .expect("Couldn't write bindings!");
    }
}

fn run(cmd: &mut Command, text: Option<&str>) {
    if !cmd.status().unwrap().success() {
        let text = text
            .map(|text| format!(" - {}", text))
            .unwrap_or(String::new());
        panic!("{:?} failed{}", cmd, text)
    }
}
