fn main() {
    #[cfg(windows)]
    embed_resource::compile("assets/icon.rc", embed_resource::NONE)
        .manifest_required()
        .expect("Windows 图标资源编译失败");
}
