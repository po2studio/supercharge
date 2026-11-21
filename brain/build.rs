//! Fetches the model files that get compiled into the binary (see `src/embed.rs`),
//! pinned by revision and sha256, into `assets/` (git-ignored). Needs `curl` once.

use sha2::{Digest, Sha256};
use std::{fs, path::Path, process::Command};

const REVISION: &str = "761b726dd34fb83930e26aab4e9ac3899aa1fa78";
const REPO: &str = "Xenova/multilingual-e5-small";

const ASSETS: &[(&str, &str, &str)] = &[
    (
        "onnx/model_quantized.onnx",
        "model.onnx",
        "f80102d3f2a1229f387d3c81909990d8945513e347b0eab049f7de3c6f98c193",
    ),
    (
        "tokenizer.json",
        "tokenizer.json",
        "0b44a9d7b51c3c62626640cda0e2c2f70fdacdc25bbbd68038369d14ebdf4c39",
    ),
];

fn sha256(path: &Path) -> String {
    let bytes = fs::read(path).unwrap();
    format!("{:x}", Sha256::digest(bytes))
}

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=src/glibc_compat.c");
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let target_env = std::env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
    if target_os == "linux" && target_env == "gnu" {
        cc::Build::new()
            .file("src/glibc_compat.c")
            .compile("glibc_compat");
    }
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("assets");
    fs::create_dir_all(&dir).unwrap();
    for (remote, local, hash) in ASSETS {
        let path = dir.join(local);
        if path.exists() && sha256(&path) == *hash {
            continue;
        }
        let url = format!("https://huggingface.co/{REPO}/resolve/{REVISION}/{remote}");
        let tmp = dir.join(format!("{local}.part"));
        let status = Command::new("curl")
            .args([
                "--fail",
                "--location",
                "--silent",
                "--show-error",
                "--retry",
                "8",
                "--retry-delay",
                "30",
                "--retry-all-errors",
                "-o",
            ])
            .arg(&tmp)
            .arg(&url)
            .status()
            .expect("curl is needed once to fetch the embedding model");
        assert!(status.success(), "downloading {url} failed");
        let got = sha256(&tmp);
        assert_eq!(got, *hash, "{url}: sha256 mismatch");
        fs::rename(&tmp, &path).unwrap();
    }
}
