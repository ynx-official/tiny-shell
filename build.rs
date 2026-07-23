fn main() {
    #[cfg(windows)]
    {
        let mut res = winres::WindowsResource::new();
        res.set_icon("assets/icons/tiny-shell.ico");
        res.set("FileDescription", "TinyShell");
        res.set("ProductName", "TinyShell");
        res.compile().unwrap();
    }
}
