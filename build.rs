fn main() {
    #[cfg(windows)]
    winresource::WindowsResource::new().set_icon("assets/navify.ico").compile().expect("the Windows resources compile");
}
