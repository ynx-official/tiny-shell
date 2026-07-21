fn main() {
    #[cfg(windows)]
    {
        let mut res = winres::WindowsResource::new();
        res.set_icon("assets/icons/tiny-shell.ico");
        res.compile().unwrap();
    }
}
