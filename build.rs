fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        println!("cargo:rerun-if-changed=assets/objectlift.ico");
        println!("cargo:rerun-if-changed=assets/objectlift.rc");
        embed_resource::compile("assets/objectlift.rc", embed_resource::NONE)
            .manifest_required()
            .expect("compile ObjectLift Windows resources");
    }
}
