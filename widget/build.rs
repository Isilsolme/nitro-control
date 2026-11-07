fn main() {
    slint_build::compile("ui/app.slint").expect("slint compile failed");

    #[cfg(windows)]
    {
        let mut res = winres::WindowsResource::new();
        res.set_icon("assets/icon.ico");
        res.set("FileDescription", "Nitro Control");
        res.set("ProductName", "Nitro Control");
        res.set("CompanyName", "Nitro Control");
        res.set("LegalCopyright", "Nitro Control contributors");
        res.compile().expect("winres compile failed");
    }
}
