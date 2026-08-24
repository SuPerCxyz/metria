fn main() {
    // 迁移通过 rust-embed 编译进二进制；显式声明目录依赖，
    // 确保新增迁移不会被 Cargo 的增量缓存遗漏。
    println!("cargo:rerun-if-changed=../../migrations");
}
