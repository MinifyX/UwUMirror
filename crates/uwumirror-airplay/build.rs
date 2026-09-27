// Compiles playfair, the FairPlay key decryption (see playfair/README.md).
fn main() {
    let files = [
        "hand_garble.c",
        "modified_md5.c",
        "omg_hax.c",
        "playfair.c",
        "sap_hash.c",
    ];
    let mut build = cc::Build::new();
    for file in files {
        build.file(format!("playfair/{file}"));
        println!("cargo:rerun-if-changed=playfair/{file}");
    }
    // Reverse-engineered C: its warnings are not ours to fix,
    // and fixing them would mean diverging from the code everyone else uses.
    build
        .include("playfair")
        .warnings(false)
        .compile("playfair");
}
