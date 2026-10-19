//! Builds every composite template of the application.
//!
//! A template that does not match the GTK and libadwaita it runs on (an
//! unknown property, signal or object type) is only reported as a critical
//! warning when a widget is created, and the widget then panics on the first
//! access of a template child. This test builds the template of every template
//! class, so such errors are found without logging in.
//!
//! It needs a display and the compiled resources of the build directory:
//! `PAPER_PLANE_RESOURCES` and `PAPER_PLANE_UI_RESOURCES` point to the
//! `.gresource` files (meson test sets them), otherwise the installed ones are
//! used.

use std::sync::Mutex;

use gtk::gio;
use gtk::glib;
use gtk::glib::translate::IntoGlib;

use crate::config;

const UI_PREFIX: &str = "/app/drey/paper-plane/ui";

/// Classes that need a running application (a client manager with TDLib
/// clients) to be created. The application creates them on start-up, so a
/// plain start of the application checks their templates.
const NEED_APPLICATION: &[&str] = &["PaplClientManagerView", "PaplWindow"];

static MESSAGES: Mutex<Vec<String>> = Mutex::new(Vec::new());

fn register_resources(var: &str, default: &str) {
    let path = std::env::var(var).unwrap_or_else(|_| default.to_owned());
    let resource = gio::Resource::load(&path)
        .unwrap_or_else(|e| panic!("Could not load the resources {path}: {e}"));
    gio::resources_register(&resource);
}

fn ui_files(path: &str, files: &mut Vec<String>) {
    let children =
        gio::resources_enumerate_children(path, gio::ResourceLookupFlags::NONE).unwrap();

    for child in children {
        let child_path = format!("{path}/{child}");
        if child.ends_with('/') {
            ui_files(child_path.trim_end_matches('/'), files);
        } else if child.ends_with(".ui") {
            files.push(child_path);
        }
    }
}

fn template_classes() -> Vec<(String, String)> {
    let mut files = vec![];
    ui_files(UI_PREFIX, &mut files);
    files.sort();

    let mut classes = vec![];
    for file in files {
        let bytes = gio::resources_lookup_data(&file, gio::ResourceLookupFlags::NONE).unwrap();
        let text = String::from_utf8_lossy(&bytes);

        if let Some(start) = text.find("<template class=\"") {
            let rest = &text[start + "<template class=\"".len()..];
            let class = rest[..rest.find('"').unwrap()].to_owned();
            classes.push((file, class));
        }
    }

    classes
}

#[test]
fn templates_build_without_warnings() {
    if std::env::var_os("WAYLAND_DISPLAY").is_none() && std::env::var_os("DISPLAY").is_none() {
        eprintln!("No display: skipping the template test");
        return;
    }

    glib::log_set_writer_func(|level, fields| {
        let field = |key: &str| {
            fields
                .iter()
                .find(|f| f.key() == key)
                .and_then(|f| f.value_str())
                .unwrap_or_default()
                .to_owned()
        };
        let line = format!("{}: {:?}: {}", field("GLIB_DOMAIN"), level, field("MESSAGE"));
        eprintln!("{line}");

        if matches!(
            level,
            glib::LogLevel::Error | glib::LogLevel::Critical | glib::LogLevel::Warning
        ) && matches!(field("GLIB_DOMAIN").as_str(), "Gtk" | "Adwaita" | "GLib-GObject")
        {
            MESSAGES.lock().unwrap().push(line);
        }

        glib::LogWriterOutput::Handled
    });

    adw::init().expect("Failed to init GTK/libadwaita");
    super::init();

    register_resources("PAPER_PLANE_RESOURCES", config::RESOURCES_FILE);
    register_resources("PAPER_PLANE_UI_RESOURCES", config::UI_RESOURCES_FILE);

    let classes = template_classes();
    assert!(
        classes.len() > 40,
        "Only {} templates found under {UI_PREFIX}",
        classes.len()
    );

    let mut failures = vec![];
    for (file, class) in classes {
        if NEED_APPLICATION.contains(&class.as_str()) {
            eprintln!("{file}: {class}: skipped, needs a running application");
            continue;
        }
        eprintln!("{file}: {class}");

        let Some(type_) = glib::Type::from_name(&class) else {
            failures.push(format!("{file}: template class {class} is not registered"));
            continue;
        };

        let is_abstract = unsafe {
            glib::gobject_ffi::g_type_test_flags(
                type_.into_glib(),
                glib::gobject_ffi::G_TYPE_FLAG_ABSTRACT,
            ) != glib::ffi::GFALSE
        };
        if is_abstract {
            // built as the parent template of its subclasses
            eprintln!("  abstract, skipped");
            continue;
        }

        MESSAGES.lock().unwrap().clear();

        // Only the instance initialization, which builds the template, runs:
        // most widgets need objects of a logged-in session as construct
        // properties. The half-made instance is leaked on purpose.
        unsafe {
            glib::gobject_ffi::g_type_create_instance(type_.into_glib());
        }

        for message in MESSAGES.lock().unwrap().drain(..) {
            failures.push(format!("{file} ({class}): {message}"));
        }
    }

    assert!(
        failures.is_empty(),
        "Templates with errors:\n{}",
        failures.join("\n")
    );
}
